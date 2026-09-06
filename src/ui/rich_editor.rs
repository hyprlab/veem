//! A small reusable WYSIWYG rich-text editor: a `contentEditable` WebView with a
//! formatting toolbar. JavaScript runs only in this (our own) document, to drive
//! editing commands. Used for the compose body and the account signature.

use adw::prelude::*;
use webkit6::prelude::WebViewExt;
use crate::i18n::{i18n, i18n_noop};

/// A rich-text editor widget. Add `widget` to a container; read the content back
/// with [`RichEditor::extract_html`] (asynchronous, since it queries the WebView).
/// Clones share the same underlying widgets (GObject references).
#[derive(Clone)]
pub struct RichEditor {
    /// The toolbar + editor, ready to be placed in a container.
    pub widget: gtk::Box,
    webview: webkit6::WebView,
    /// Where "Send as Attachment Instead" delivers the lifted image, as a
    /// temp-file path the host adds to its attachment list. Set by the host
    /// via [`RichEditor::connect_send_as_attachment`].
    attach_cb: std::rc::Rc<std::cell::RefCell<Option<Box<dyn Fn(std::path::PathBuf)>>>>,
}

/// Push the spell-checking preference onto the shared web context (#114).
/// Checking only happens in editable views, so the reader — which shares the
/// context — is unaffected.
///
/// The language is ALWAYS set when checking is on: enchant is given no
/// language at all until told, so "enabled" without this call underlines
/// nothing. The configured language wins; blank follows the session locale;
/// and either is swapped for the closest installed dictionary when its own
/// is missing, since a language without a dictionary silently checks
/// nothing.
pub fn apply_spellcheck() {
    let ctx = super::message_view::shared_web_context();
    let on = crate::config::load_spellcheck();
    ctx.set_spell_checking_enabled(on);
    if !on {
        return;
    }
    let lang = resolved_spell_language();
    ctx.set_spell_checking_languages(&[&lang]);
}

/// The language checking actually runs with: the configured one, else the
/// session locale, either mapped onto an installed dictionary.
pub fn resolved_spell_language() -> String {
    let configured = crate::config::load_spellcheck_langs();
    let want = configured
        .split([',', ';', ' '])
        .map(str::trim)
        .find(|s| !s.is_empty())
        .map(String::from)
        .or_else(locale_language)
        .unwrap_or_else(|| "en_US".to_string());
    let dicts = installed_dictionaries();
    if dicts.is_empty() || dicts.iter().any(|d| *d == want) {
        return want;
    }
    // No dictionary for the exact code: any same-language variant beats
    // checking nothing (en_GB for en_US), and any dictionary beats none.
    let prefix = want.split('_').next().unwrap_or(&want).to_string();
    dicts
        .iter()
        .find(|d| d.starts_with(&prefix))
        .or_else(|| dicts.first())
        .cloned()
        .unwrap_or(want)
}

/// The session locale as a dictionary-style code ("en_US.UTF-8" → "en_US").
fn locale_language() -> Option<String> {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|v| std::env::var(v).ok())
        .map(|l| l.split('.').next().unwrap_or_default().to_string())
        .find(|l| !l.is_empty() && l != "C" && l != "POSIX")
}

/// The hunspell dictionaries that resolve to real files. Inside the Flatpak
/// most of `/usr/share/hunspell` is dangling symlinks until the locale
/// extension carries the language; `metadata()` follows symlinks, so a
/// dangling one errs out and is rightly skipped.
pub fn installed_dictionaries() -> Vec<String> {
    let mut codes: std::collections::BTreeSet<String> = Default::default();
    // Exactly the places enchant's hunspell provider reads (verified
    // empirically — a listed dictionary the checker can't use is the silent
    // failure this feature avoids): the classic locations, every SYSTEM XDG
    // data dir (which is how the Flatpak's bundled /app/share/hunspell set
    // is found), and enchant's per-user drop-in under the config dir. The
    // user DATA dir is deliberately absent: enchant does not search it.
    let mut dirs = vec![
        std::path::PathBuf::from("/usr/share/hunspell"),
        std::path::PathBuf::from("/usr/share/myspell"),
    ];
    for d in gtk::glib::system_data_dirs() {
        dirs.push(d.join("hunspell"));
    }
    dirs.push(gtk::glib::user_config_dir().join("enchant").join("hunspell"));
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            let is_dic = p.extension().is_some_and(|x| x == "dic");
            if is_dic && p.metadata().map(|m| m.is_file()).unwrap_or(false) {
                if let Some(s) = p.file_stem().and_then(|s| s.to_str()) {
                    codes.insert(s.to_string());
                }
            }
        }
    }
    // The runtime ships English many times over (eighteen region variants,
    // all hardlinks to the same data); the list keeps the five anyone looks
    // for. Other languages are shown as installed.
    const KEPT_ENGLISH: &[&str] = &["en_US", "en_GB", "en_CA", "en_AU", "en_NZ"];
    codes
        .into_iter()
        .filter(|c| {
            c.split('_').next() != Some("en") || KEPT_ENGLISH.contains(&c.as_str())
        })
        .collect()
}

impl RichEditor {
    pub fn new(initial_html: &str) -> Self {
        // The shared document-viewer context (issue #106): a default-context
        // view would bring up a second web process with browser-sized caches.
        // Spell checking rides the same context; refreshing it here keeps a
        // just-changed preference honored without a restart.
        apply_spellcheck();
        // The message handler carries the word under the caret out for a
        // mid-word spelling verdict (see the paste script's caretWord).
        let ucm = webkit6::UserContentManager::new();
        ucm.register_script_message_handler("vireoSpell", None);
        let webview = webkit6::WebView::builder()
            .web_context(&super::message_view::shared_web_context())
            .user_content_manager(&ucm)
            .build();
        {
            let v = webview.clone();
            ucm.connect_script_message_received(Some("vireoSpell"), move |_, value| {
                let word = value.to_str().to_string();
                let bad = crate::spell::word_is_misspelled(&word);
                exec(&v, &format!("window.__vireoSpellMark({bad})"));
            });
        }
        let settings = webkit6::Settings::new();
        settings.set_enable_javascript(true);
        settings.set_enable_developer_extras(false);
        // A script error in the editor document (all our own code) is
        // otherwise completely silent.
        settings.set_enable_write_console_messages_to_stdout(true);
        webview.set_settings(&settings);
        // Transparent until the document paints: a fresh WebView otherwise
        // renders an opaque default surface (black) for its first frames,
        // which flashes while the inline composer slides over the reader.
        // The document itself paints `Canvas` below, so the editor's normal
        // ground appears with the first real frame.
        webview.set_background_color(&gtk::gdk::RGBA::new(0.0, 0.0, 0.0, 0.0));
        // The document paints asynchronously: the host panel slides in
        // smoothly and the editor's content would land a beat later as a
        // pop. Start the view invisible and fade it in when its load
        // finishes — content arrives as a fade into the settled panel.
        webview.set_opacity(0.0);
        webview.connect_load_changed(|v, ev| {
            if ev == webkit6::LoadEvent::Finished {
                fade_in(v);
            }
        });
        {
            // Failsafe: whatever happens to the load, the editor must never
            // stay invisible.
            let v = webview.clone();
            gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(1200), move || {
                if v.opacity() < 1.0 {
                    fade_in(&v);
                }
            });
        }
        let dark = adw::StyleManager::default().is_dark();
        webview.load_html(&document(initial_html, dark), Some("https://vireo.localhost/editor"));

        // The stock editable menu's single "Paste" hides the plain/rich choice
        // behind the preference; the menu offers both, always, in its place.
        // Over an image the menu leads with image actions — cut, copy, and
        // demoting an inline picture to an ordinary attachment — acting on
        // the exact node the right-click landed on (`__vireoCtxImg`).
        let attach_cb: std::rc::Rc<std::cell::RefCell<Option<Box<dyn Fn(std::path::PathBuf)>>>> =
            std::rc::Rc::new(std::cell::RefCell::new(None));
        let menu_attach_cb = attach_cb.clone();
        webview.connect_context_menu(move |view, menu, hit| {
            if hit.context_is_image() {
                // Stock image entries (copy/save/open variants) are replaced
                // by ours, which also know about the editable document.
                for item in menu.items().iter().filter(|i| {
                    matches!(
                        i.stock_action(),
                        webkit6::ContextMenuAction::CopyImageToClipboard
                            | webkit6::ContextMenuAction::CopyImageUrlToClipboard
                            | webkit6::ContextMenuAction::DownloadImageToDisk
                            | webkit6::ContextMenuAction::OpenImageInNewWindow
                    )
                }) {
                    menu.remove(item);
                }
                // The stock Cut/Copy already act on the image — the
                // right-click selected it. The one image-specific verb is
                // demoting it to an ordinary attachment; the script that
                // lifts it declines non-embedded (remote) images itself,
                // since the hit test doesn't surface data: URIs to check
                // against here.
                let action = gtk::gio::SimpleAction::new("vireo-img-attach", None);
                let v = view.clone();
                let cb = menu_attach_cb.clone();
                action.connect_activate(move |_, _| detach_ctx_image(&v, cb.clone()));
                menu.insert(
                    &webkit6::ContextMenuItem::from_gaction(
                        &action,
                        &i18n("Send as Attachment Instead"),
                        None,
                    ),
                    0,
                );
                menu.insert(&webkit6::ContextMenuItem::new_separator(), 1);
                // Sizes lead the menu: the fractions are of the writing
                // width, which is the width the recipient reads at, so
                // "Large" means the full column rather than some pixel count
                // that means nothing on the other end. Dragging the corner
                // handles does the same thing by hand.
                let mut at = 0;
                for (label, kind) in [
                    (i18n("Small"), "small"),
                    (i18n("Medium"), "medium"),
                    (i18n("Large"), "large"),
                    (i18n("Original Size"), "original"),
                ] {
                    let action =
                        gtk::gio::SimpleAction::new(&format!("vireo-img-{kind}"), None);
                    let v = view.clone();
                    action.connect_activate(move |_, _| {
                        exec(&v, &format!("window.__vireoSetImageSize('{kind}')"));
                    });
                    menu.insert(
                        &webkit6::ContextMenuItem::from_gaction(&action, &label, None),
                        at,
                    );
                    at += 1;
                }
                {
                    let action = gtk::gio::SimpleAction::new("vireo-img-fit", None);
                    let v = view.clone();
                    let hint = js_escape(&i18n(
                        "Will be recompressed to the size shown when this is sent",
                    ));
                    action.connect_activate(move |_, _| {
                        exec(&v, &format!("window.__vireoToggleFit('{hint}')"));
                    });
                    menu.insert(
                        &webkit6::ContextMenuItem::from_gaction(
                            &action,
                            &i18n("Recompress to This Size on Send"),
                            None,
                        ),
                        at,
                    );
                    at += 1;
                }
                menu.insert(&webkit6::ContextMenuItem::new_separator(), at);
            }
            let items = menu.items();
            let Some(pos) = items
                .iter()
                .position(|i| i.stock_action() == webkit6::ContextMenuAction::Paste)
            else {
                return false; // no paste here (nothing editable hit): stock menu as-is
            };
            for item in items.iter().filter(|i| {
                matches!(
                    i.stock_action(),
                    webkit6::ContextMenuAction::Paste | webkit6::ContextMenuAction::PasteAsPlainText
                )
            }) {
                menu.remove(item);
            }
            let mut at = pos as i32;
            for (label, rich) in [(i18n("Paste with Formatting"), true), (i18n("Paste as Plain Text"), false)] {
                let action =
                    gtk::gio::SimpleAction::new(if rich { "vireo-paste-rich" } else { "vireo-paste-plain" }, None);
                let v = view.clone();
                let cb = menu_attach_cb.clone();
                action.connect_activate(move |_, _| paste_into(&v, rich, &cb));
                menu.insert(&webkit6::ContextMenuItem::from_gaction(&action, &label, None), at);
                at += 1;
            }
            false
        });

        // Files dropped from a file manager never reach the document: WebKitGTK
        // hands the page a uri-list it then refuses to serve, so the drop
        // handler in PASTE_SCRIPT sees an empty `DataTransfer.files` and lets
        // WebKit insert the path as a link. Only the widget can see them, so
        // the widget takes them — declaring just `GdkFileList` leaves every
        // other drag (text, an image dragged out of the reader) to WebKit.
        {
            let drop =
                gtk::DropTarget::new(gtk::gdk::FileList::static_type(), gtk::gdk::DragAction::COPY);
            drop.set_propagation_phase(gtk::PropagationPhase::Capture);
            let v = webview.clone();
            let cb = attach_cb.clone();
            drop.connect_drop(move |_, value, x, y| {
                let Ok(list) = value.get::<gtk::gdk::FileList>() else { return false };
                deliver_files(&v, &list.files(), Some((x, y)), &cb)
            });
            webview.add_controller(drop);
        }

        let toolbar = build_toolbar(&webview);

        let frame = gtk::Frame::new(None);
        frame.set_vexpand(true);
        // Same card treatment as the address fields' boxed-list: shadow and
        // radius from libadwaita, with the WebView clipped to the corners.
        frame.add_css_class("card");
        frame.set_overflow(gtk::Overflow::Hidden);
        frame.set_child(Some(&webview));

        let bx = gtk::Box::new(gtk::Orientation::Vertical, 6);
        bx.append(&toolbar);
        bx.append(&frame);

        RichEditor { widget: bx, webview, attach_cb }
    }

    /// What "Send as Attachment Instead" does with the lifted image: the
    /// host receives the temp file's path and adds it to its attachments.
    pub fn connect_send_as_attachment(&self, f: impl Fn(std::path::PathBuf) + 'static) {
        *self.attach_cb.borrow_mut() = Some(Box::new(f));
    }

    /// Replace the editor contents with `content` (HTML).
    pub fn set_html(&self, content: &str) {
        let dark = adw::StyleManager::default().is_dark();
        self.webview
            .load_html(&document(content, dark), Some("https://vireo.localhost/editor"));
    }

    pub fn grab_focus(&self) {
        self.webview.grab_focus();
    }

    /// Run a JavaScript snippet against the editor document (e.g. to swap the
    /// signature block when the From account changes).
    pub fn run_js(&self, js: &str) {
        exec(&self.webview, js);
    }

    /// Whether keyboard focus is currently inside the editor's WebView — the
    /// guard a host's paste shortcut uses so it never hijacks Ctrl+V aimed at
    /// an address entry.
    pub fn has_focus(&self) -> bool {
        self.webview
            .root()
            .and_then(|r| r.focus())
            .is_some_and(|f| f == *self.webview.upcast_ref::<gtk::Widget>() || f.is_ancestor(&self.webview))
    }

    /// Paste the clipboard into the editor, keeping (`rich`) or stripping the
    /// clipboard's formatting for this one paste, whatever the standing
    /// preference says.
    pub fn paste(&self, rich: bool) {
        paste_into(&self.webview, rich, &self.attach_cb);
    }

    /// Whether the body has been edited since it was loaded (async read of a JS
    /// flag set on the first `input` event). Used to avoid saving a pristine,
    /// quote-only reply to Drafts when the reader navigates away.
    pub fn is_dirty(&self, cb: impl FnOnce(bool) + 'static) {
        self.webview.evaluate_javascript(
            "String(!!window.__vireoDirty)",
            None,
            None,
            gtk::gio::Cancellable::NONE,
            move |res| cb(res.map(|v| v.to_str() == "true").unwrap_or(false)),
        );
    }

    /// Read the current body HTML asynchronously.
    pub fn extract_html(&self, cb: impl FnOnce(String) + 'static) {
        self.webview.evaluate_javascript(
            "window.__vireoBodyHtml()",
            None,
            None,
            gtk::gio::Cancellable::NONE,
            move |res| cb(res.map(|v| v.to_str().to_string()).unwrap_or_default()),
        );
    }

    /// Read the body for *sending*: any picture armed with "Recompress to
    /// This Size on Send" is recut to the size it is drawn at first. Drafts
    /// go through [`RichEditor::extract`], which never touches the pixels —
    /// a save must not cost quality.
    pub fn extract_for_send(&self, cb: impl FnOnce(String, String) + 'static) {
        self.read_body("window.__vireoBodyHtmlForSend()", cb);
    }

    /// Read the current body HTML and a plain-text rendering asynchronously.
    pub fn extract(&self, cb: impl FnOnce(String, String) + 'static) {
        self.read_body("window.__vireoBodyHtml()", cb);
    }

    fn read_body(&self, reader: &str, cb: impl FnOnce(String, String) + 'static) {
        self.webview.evaluate_javascript(
            &format!("{reader} + '\\u0000' + document.body.innerText"),
            None,
            None,
            gtk::gio::Cancellable::NONE,
            move |res| {
                let combined = res.map(|v| v.to_str().to_string()).unwrap_or_default();
                let (html, text) = combined
                    .split_once('\u{0}')
                    .map(|(h, t)| (h.to_string(), t.to_string()))
                    .unwrap_or_else(|| (combined.clone(), String::new()));
                cb(html, text);
            },
        );
    }
}

fn exec(webview: &webkit6::WebView, js: &str) {
    webview.evaluate_javascript(js, None, None, gtk::gio::Cancellable::NONE, |_| {});
}

/// Fade the editor view to full opacity. The animation object is parked on
/// the view itself so it stays alive for its 200ms (a dropped animation
/// stops); a repeat call replaces the parked one harmlessly.
fn fade_in(webview: &webkit6::WebView) {
    let anim = adw::TimedAnimation::new(
        webview,
        webview.opacity(),
        1.0,
        200,
        adw::CallbackAnimationTarget::new({
            let v = webview.clone();
            move |o| v.set_opacity(o)
        }),
    );
    anim.set_easing(adw::Easing::EaseOutCubic);
    anim.play();
    unsafe {
        webview.set_data("vireo-fade-anim", anim);
    }
}

/// "Send as Attachment Instead": pull the right-clicked image out of the
/// document, decode its data: URI to a temp file, and hand the path to the
/// host's callback (which adds it to the attachment list). The node is
/// removed in the same script that reads it, so the demotion is atomic even
/// if the same picture was pasted twice.
fn detach_ctx_image(
    webview: &webkit6::WebView,
    cb: std::rc::Rc<std::cell::RefCell<Option<Box<dyn Fn(std::path::PathBuf)>>>>,
) {
    webview.evaluate_javascript(
        "(function(){var n=window.__vireoCtxImg;\
          if(!n){var sel=getSelection();\
            if(sel.rangeCount===1){var r=sel.getRangeAt(0);\
              if(r.startContainer===r.endContainer&&r.endOffset-r.startOffset===1){\
                var c=r.startContainer.childNodes[r.startOffset];\
                if(c&&c.tagName==='IMG')n=c;}}}\
          if(!n||n.tagName!=='IMG')return 'E:none';\
          var s=n.src;if(s.indexOf('data:image/')!==0)return 'E:src:'+s.slice(0,40);\
          n.remove();window.__vireoCtxImg=null;return s;})()",
        None,
        None,
        gtk::gio::Cancellable::NONE,
        move |res| {
            let v = match res {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!("img-attach: script failed: {e}");
                    return;
                }
            };
            let src = v.to_str();
            if let Some(err) = src.strip_prefix("E:") {
                tracing::warn!("img-attach: script declined: {err}");
                return;
            }
            let Some(path) = data_uri_to_temp_file(&src) else {
                tracing::warn!("img-attach: not a liftable data: image");
                return;
            };
            match cb.borrow().as_ref() {
                Some(f) => f(path),
                None => tracing::warn!("img-attach: no attach callback set on this editor"),
            }
        },
    );
}

/// Write a `data:image/...;base64,...` URI's bytes to a temp file the send
/// path can read like any chosen attachment. The name it gets is the name
/// that goes out on the wire.
fn data_uri_to_temp_file(uri: &str) -> Option<std::path::PathBuf> {
    let rest = uri.strip_prefix("data:")?;
    let (meta, payload) = rest.split_once(',')?;
    let mime = meta.strip_suffix(";base64")?;
    if !mime.starts_with("image/") {
        return None;
    }
    let data = crate::oauth::base64_decode(payload)?;
    let ext = match mime {
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        _ => "png",
    };
    let dir = std::env::temp_dir().join("vireo-inline-images");
    std::fs::create_dir_all(&dir).ok()?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let path = dir.join(format!("image-{stamp}.{ext}"));
    std::fs::write(&path, &data).ok()?;
    Some(path)
}

/// One paste in an explicit mode: arm the document's one-shot flag, then run
/// the editing command once the flag is in place (the callback orders the two,
/// since both travel to the web process asynchronously). The DOM paste handler
/// consumes the flag — see [`PASTE_SCRIPT`].
///
/// A file copied in a file manager is taken first, for the same reason the
/// drop target exists: the clipboard offers `GdkFileList`, but by the time
/// WebKit has turned it into a paste the document sees an empty
/// `clipboardData` and the bare path arrives as text. The formats are checked
/// synchronously so an ordinary text paste is not delayed by a read that was
/// always going to fail.
fn paste_into(
    webview: &webkit6::WebView,
    rich: bool,
    cb: &std::rc::Rc<std::cell::RefCell<Option<Box<dyn Fn(std::path::PathBuf)>>>>,
) {
    if webview.clipboard().formats().contains_type(gtk::gdk::FileList::static_type()) {
        let v = webview.clone();
        let cb = cb.clone();
        webview.clipboard().read_value_async(
            gtk::gdk::FileList::static_type(),
            gtk::glib::Priority::DEFAULT,
            gtk::gio::Cancellable::NONE,
            move |res| {
                let taken = res
                    .ok()
                    .and_then(|val| val.get::<gtk::gdk::FileList>().ok())
                    .is_some_and(|list| deliver_files(&v, &list.files(), None, &cb));
                // Nothing usable on the clipboard after all: the ordinary
                // paste still owes the user whatever else is on it.
                if !taken {
                    paste_via_webkit(&v, rich);
                }
            },
        );
        return;
    }
    paste_via_webkit(webview, rich);
}

/// The stock paste: arm the one-shot mode flag, then let WebKit paste.
fn paste_via_webkit(webview: &webkit6::WebView, rich: bool) {
    let v = webview.clone();
    webview.evaluate_javascript(
        &format!("window.__vireoPasteOnce={rich};"),
        None,
        None,
        gtk::gio::Cancellable::NONE,
        move |_| v.execute_editing_command("Paste"),
    );
}

/// Images inline, everything else attached. `at` is the drop point in widget
/// coordinates (which are the document's viewport coordinates), placing the
/// caret where the picture was let go; a paste keeps the caret it has.
/// Returns whether any file was taken, so a caller can fall back.
///
/// Reading and encoding happen here rather than in the document because the
/// document is never given the file in the first place — see [`paste_into`].
fn deliver_files(
    webview: &webkit6::WebView,
    files: &[gtk::gio::File],
    at: Option<(f64, f64)>,
    cb: &std::rc::Rc<std::cell::RefCell<Option<Box<dyn Fn(std::path::PathBuf)>>>>,
) -> bool {
    let mut took = false;
    let mut point = at;
    for file in files {
        let Some(path) = file.path() else { continue };
        match read_image_for_insert(file, &path) {
            Some((b64, mime)) => {
                let (x, y) = point.take().map_or(("null".into(), "null".into()), |(x, y)| {
                    (format!("{x}"), format!("{y}"))
                });
                // The base64 goes in unescaped on purpose: its alphabet has
                // nothing a JS string literal cares about, and running the
                // escape over tens of megabytes of it costs real time. Only
                // the MIME type comes from outside.
                let mime = js_escape(&mime);
                let name = js_escape(
                    &path.file_name().unwrap_or_default().to_string_lossy(),
                );
                exec(
                    webview,
                    &format!(
                        "window.__vireoInsertImageURL(\
                         'data:{mime};base64,{b64}','{mime}','{name}',{x},{y})"
                    ),
                );
                took = true;
            }
            // Not a picture, or too big to carry in the body: the composer's
            // attachment list is where it belongs. Inline images are lifted
            // into `cid:` parts at send time either way, so an attachment is
            // the same journey with a different disposition.
            None => match cb.borrow().as_ref() {
                Some(f) => {
                    f(path);
                    took = true;
                }
                None => tracing::warn!("editor drop: no attach callback set on this editor"),
            },
        }
    }
    took
}

/// An image small enough to inline, as base64 of its bytes plus its MIME type.
/// Anything else — a document, an unreadable file, or a picture so large that
/// base64 of it would be a burden to ferry into the web process — returns
/// `None` and becomes an attachment instead. The cap is deliberately generous:
/// the document downscales to 1600px on insert, so the size that matters to
/// the recipient is decided there, not here.
fn read_image_for_insert(
    file: &gtk::gio::File,
    path: &std::path::Path,
) -> Option<(String, String)> {
    const MAX_INLINE_BYTES: u64 = 32 * 1024 * 1024;
    let info = file
        .query_info(
            "standard::content-type,standard::size",
            gtk::gio::FileQueryInfoFlags::NONE,
            gtk::gio::Cancellable::NONE,
        )
        .ok()?;
    let mime = info.content_type()?.to_string();
    // Deliberately a list, not `image/*`: a type the engine cannot decode
    // would insert a broken <img> carrying the whole file as base64 — a RAW
    // photo or an HEIC off a phone is tens of megabytes of nothing. A
    // renderable format missing from this list merely becomes an attachment,
    // which is the harmless way to be wrong.
    const INLINE_MIMES: &[&str] = &[
        "image/png",
        "image/jpeg",
        "image/gif",
        "image/webp",
        "image/bmp",
        "image/svg+xml",
        "image/avif",
    ];
    if !INLINE_MIMES.contains(&mime.as_str()) || info.size() as u64 > MAX_INLINE_BYTES {
        return None;
    }
    let data = std::fs::read(path).ok()?;
    Some((crate::oauth::base64_encode(&data), mime))
}

fn build_toolbar(webview: &webkit6::WebView) -> gtk::Box {
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    bar.add_css_class("toolbar");
    bar.add_css_class("format-bar");

    // (icon, tooltip, execCommand snippet)
    let commands: &[(&str, &str, &str)] = &[
        ("co.hyprlab.Vireo-format-text-bold-symbolic", i18n_noop("Bold"), "document.execCommand('bold')"),
        ("co.hyprlab.Vireo-format-text-italic-symbolic", i18n_noop("Italic"), "document.execCommand('italic')"),
        ("co.hyprlab.Vireo-format-text-underline-symbolic", i18n_noop("Underline"), "document.execCommand('underline')"),
        ("co.hyprlab.Vireo-format-text-strikethrough-symbolic", i18n_noop("Strikethrough"), "document.execCommand('strikeThrough')"),
        ("SEP", "", ""),
        ("co.hyprlab.Vireo-view-list-bullet-symbolic", i18n_noop("Bulleted list"), "document.execCommand('insertUnorderedList')"),
        ("co.hyprlab.Vireo-view-list-ordered-symbolic", i18n_noop("Numbered list"), "document.execCommand('insertOrderedList')"),
        // Adwaita has no blockquote glyph; the indent icon reads as "quote".
        ("co.hyprlab.Vireo-format-indent-more-symbolic", i18n_noop("Quote"), "document.execCommand('formatBlock',false,'blockquote')"),
        // `LINK` is a sentinel command (handled specially); the icon is real.
        ("co.hyprlab.Vireo-insert-link-symbolic", i18n_noop("Insert link"), "LINK"),
        ("SEP", "", ""),
        ("co.hyprlab.Vireo-edit-clear-symbolic", i18n_noop("Clear formatting"), "document.execCommand('removeFormat')"),
    ];

    for (icon, tip, cmd) in commands {
        if *icon == "SEP" {
            bar.append(&gtk::Separator::new(gtk::Orientation::Vertical));
            continue;
        }
        let btn = gtk::Button::from_icon_name(icon);
        btn.set_tooltip_text(Some(i18n(tip).as_str()));
        // Don't take focus, so the editor keeps its selection.
        btn.set_can_focus(false);
        btn.add_css_class("flat");
        if *cmd == "LINK" {
            let wv = webview.clone();
            btn.connect_clicked(move |b| prompt_link(&wv, b));
        } else {
            let wv = webview.clone();
            let cmd = cmd.to_string();
            btn.connect_clicked(move |_| exec(&wv, &cmd));
        }
        bar.append(&btn);
    }
    bar
}

/// Prompt for a URL and turn the current selection into a link.
fn prompt_link(webview: &webkit6::WebView, anchor: &gtk::Button) {
    let parent = anchor.root().and_downcast::<gtk::Window>();
    let dialog = adw::MessageDialog::new(parent.as_ref(), Some(i18n("Insert Link").as_str()), None);
    dialog.add_response("cancel", &i18n("Cancel"));
    dialog.add_response("ok", &i18n("Insert"));
    dialog.set_default_response(Some("ok"));
    dialog.set_response_appearance("ok", adw::ResponseAppearance::Suggested);
    let entry = gtk::Entry::new();
    entry.set_input_purpose(gtk::InputPurpose::Url);
    entry.set_placeholder_text(Some("https://example.com"));
    entry.set_activates_default(true);
    dialog.set_extra_child(Some(&entry));
    let wv = webview.clone();
    dialog.connect_response(None, move |_, resp| {
        if resp == "ok" {
            let url = entry.text().to_string();
            let url = url.trim();
            if !url.is_empty() {
                exec(
                    &wv,
                    &format!("document.execCommand('createLink',false,'{}')", js_escape(url)),
                );
            }
        }
    });
    dialog.present();
}

/// The paste choke point: every paste — shortcut, context menu, editing
/// command — raises a DOM `paste` event here, where the clipboard's flavours
/// can be inspected. `__vireoPasteRich` is the standing mode (the user's
/// preference, stamped at document build); `__vireoPasteOnce` overrides it for
/// exactly one paste (Ctrl+V and the context-menu items set it just before
/// running the Paste command, so a live preference change is honored).
///
/// Plain mode inserts the clipboard's text flavour with URLs linkified; rich
/// mode lets WebKit's native paste keep the formatting, linkifying only
/// pastes that carry no HTML at all. Lives in `<head>` (not the editable
/// body) so it never becomes part of the message.
const PASTE_SCRIPT: &str = r#"<script>
(function(){
  window.__vireoPasteOnce = null;
  var urlRe = /((?:https?:\/\/|www\.)[^\s<>()]+[^\s<>().,;:!?'"])/gi;
  function esc(s){return s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');}
  /* Inline images (#113): a pasted or dropped image lands in the text as an
     <img src="data:..."> at the caret; the send path lifts it into a cid:
     part. Oversized images are downscaled on insert — a phone screenshot is
     thousands of pixels the recipient's pane will never show. */
  var MAXPX = 1600;
  function insertImageFile(f){
    if(!f) return;
    var r = new FileReader();
    r.onload = function(){
      scaleUrl(String(r.result), f.type, function(url){
        document.execCommand('insertHTML', false, '<img src="' + url + '" style="max-width:100%">');
      });
    };
    r.readAsDataURL(f);
  }
  function scaleUrl(url, type, done){
    var im = new Image();
    im.onload = function(){
      var w = im.naturalWidth, h = im.naturalHeight;
      if(w > MAXPX || h > MAXPX){
        var s = Math.min(MAXPX / w, MAXPX / h);
        var c = document.createElement('canvas');
        c.width = Math.round(w * s); c.height = Math.round(h * s);
        c.getContext('2d').drawImage(im, 0, 0, c.width, c.height);
        url = type === 'image/jpeg' ? c.toDataURL('image/jpeg', 0.85) : c.toDataURL('image/png');
      }
      done(url);
    };
    /* A picture the engine can't decode still has to let go of the queue
       below, or every image after it waits forever. */
    im.onerror = function(){ done(url); };
    im.src = url;
  }
  /* The way in for a file the widget read for us: dropped or pasted from a
     file manager, which the document itself never gets to see (the app's
     drop target and paste path explain why). One queue keeps a batch in the
     order it was dropped — each insert waits on an image decode, so parallel
     calls would race and land backwards. `x`/`y` place the caret at the drop
     point and are given for the first of a batch only; the rest follow the
     caret the previous insert left behind. */
  var insertQ = Promise.resolve();
  window.__vireoInsertImageURL = function(url, type, name, x, y){
    insertQ = insertQ.then(function(){
      return new Promise(function(done){
        if(x !== null && document.caretRangeFromPoint){
          var r = document.caretRangeFromPoint(x, y);
          if(r){ var s = getSelection(); s.removeAllRanges(); s.addRange(r); }
        }
        scaleUrl(url, type, function(u){
          /* The file's own name rides along as alt, which is the only place
             left to keep it: the picture is a data: URI by now, and the send
             path reads this back to name the cid: part. It is also what an
             alt attribute is for, so a recipient whose client blocks images
             reads the filename rather than nothing. */
          var a = name ? ' alt="' + esc(name).replace(/"/g, '&quot;') + '"' : '';
          document.execCommand('insertHTML', false,
            '<img src="' + u + '"' + a + ' style="max-width:100%">');
          done();
        });
      });
    });
  };
  /* WebKit's own paste inserts images as blob: URLs — invisible to
     clipboardData.items, dead on the wire, and never downscaled. Convert
     every blob: image that appears, from any entry path, into the same
     scaled data: URI a handled paste produces. */
  function adoptBlobImage(img){
    fetch(img.src).then(function(r){ return r.blob(); }).then(function(b){
      var rd = new FileReader();
      rd.onload = function(){
        scaleUrl(String(rd.result), b.type, function(url){
          img.src = url;
          img.style.maxWidth = '100%';
        });
      };
      rd.readAsDataURL(b);
    }).catch(function(){});
  }
  new MutationObserver(function(muts){
    muts.forEach(function(m){
      Array.prototype.forEach.call(m.addedNodes || [], function(n){
        if(n.nodeType !== 1) return;
        var imgs = n.tagName === 'IMG' ? [n]
          : (n.querySelectorAll ? n.querySelectorAll('img') : []);
        Array.prototype.forEach.call(imgs, function(im){
          if(im.src && im.src.indexOf('blob:') === 0) adoptBlobImage(im);
        });
      });
    });
  }).observe(document.documentElement, {childList: true, subtree: true});
  /* WebKit only marks a word's spelling once the caret leaves it. The word
     under the caret is checked by us instead: after a typing pause it goes
     to the app (which asks the same enchant the body's checker uses), and a
     misspelling is drawn through the CSS Custom Highlight API — no DOM
     mutation, so the caret, the undo stack, and the outgoing HTML are all
     untouched. The mark clears the moment typing resumes or the caret
     leaves the word, where WebKit's own marking takes over. */
  var spellT = null, composing = false, spellRange = null;
  var spellHl = (window.Highlight && window.CSS && CSS.highlights)
    ? new Highlight() : null;
  if(spellHl) CSS.highlights.set('vireo-misspell', spellHl);
  function caretWord(){
    var sel = getSelection();
    if(!sel.rangeCount || !sel.isCollapsed) return null;
    var n = sel.focusNode, off = sel.focusOffset;
    if(!n || n.nodeType !== 3) return null;
    var s = n.textContent;
    var isW = function(c){ return /[\p{L}'’]/u.test(c); };
    var a = off; while(a > 0 && isW(s[a-1])) a--;
    var b = off; while(b < s.length && isW(s[b])) b++;
    if(b - a < 2) return null;
    var r = document.createRange();
    r.setStart(n, a); r.setEnd(n, b);
    return {range: r, word: s.slice(a, b)};
  }
  window.__vireoSpellMark = function(bad){
    if(!spellHl) return;
    spellHl.clear();
    if(bad && spellRange && spellRange.startContainer.isConnected){
      spellHl.add(spellRange);
    }
  };
  document.addEventListener('compositionstart', function(){ composing = true; });
  document.addEventListener('compositionend', function(){ composing = false; });
  document.addEventListener('input', function(){
    if(!spellHl) return;
    spellHl.clear();
    if(composing) return;
    clearTimeout(spellT);
    spellT = setTimeout(function(){
      if(composing) return;
      var w = caretWord();
      if(!w) return;
      spellRange = w.range;
      try{ window.webkit.messageHandlers.vireoSpell.postMessage(w.word); }catch(_){}
    }, 600);
  });
  document.addEventListener('selectionchange', function(){
    if(!spellHl || !spellRange) return;
    var sel = getSelection();
    if(!sel.rangeCount) return;
    var r = sel.getRangeAt(0);
    if(!sel.isCollapsed || r.startContainer !== spellRange.startContainer
       || r.startOffset < spellRange.startOffset
       || r.startOffset > spellRange.endOffset){
      spellHl.clear();
      spellRange = null;
    }
  });
  /* Resizing a picture. The frame and its corner handles are ordinary
     elements in this contenteditable document — there is nowhere else to
     put them — so they are marked `data-vireo-ui` and stripped from
     everything the document hands back (see `__vireoBodyHtml`). They are
     also contenteditable=false, so the caret cannot wander into them. */
  var rsBox = null, rsImg = null;
  /* Blue is an ordinary selection; red says the picture will be recut when
     the message is sent, which is the one thing here that cannot be undone.
     The same red the spelling underline and the tray dot use. */
  var RS_BLUE = '#3584e4', RS_RED = '#e01b24';
  function rsRemove(){ if(rsBox){ rsBox.remove(); rsBox = null; rsImg = null; } }
  function rsTint(){
    if(!rsBox || !rsImg) return;
    var c = rsImg.hasAttribute('data-vireo-fit') ? RS_RED : RS_BLUE;
    rsBox.style.outlineColor = c;
    Array.prototype.forEach.call(rsBox.children, function(h){ h.style.background = c; });
  }
  function rsPlace(){
    if(!rsBox || !rsImg) return;
    if(!rsImg.isConnected){ rsRemove(); return; }
    var r = rsImg.getBoundingClientRect();
    rsBox.style.left = (r.left + scrollX) + 'px';
    rsBox.style.top = (r.top + scrollY) + 'px';
    rsBox.style.width = r.width + 'px';
    rsBox.style.height = r.height + 'px';
  }
  /* A width in CSS *and* in the attribute: mail clients that ignore styles
     still honour width, and the height is left to follow so the aspect can
     never be lost. */
  function rsApply(img, w){
    img.style.width = w + 'px';
    img.style.height = 'auto';
    img.setAttribute('width', String(w));
    img.removeAttribute('height');
  }
  function rsDown(e){
    if(!rsImg) return;
    e.preventDefault(); e.stopPropagation();
    var handle = e.currentTarget, img = rsImg;
    var west = handle.dataset.corner[1] === 'w';
    var startX = e.clientX, startW = img.getBoundingClientRect().width;
    var maxW = document.body.clientWidth;
    function move(ev){
      var dx = ev.clientX - startX;
      var w = Math.round(west ? startW - dx : startW + dx);
      rsApply(img, Math.max(24, Math.min(w, maxW)));
      rsPlace();
    }
    function up(ev){
      handle.removeEventListener('pointermove', move);
      handle.removeEventListener('pointerup', up);
      try{ handle.releasePointerCapture(ev.pointerId); }catch(_){}
    }
    try{ handle.setPointerCapture(e.pointerId); }catch(_){}
    handle.addEventListener('pointermove', move);
    handle.addEventListener('pointerup', up);
  }
  function rsShow(img){
    rsRemove();
    rsImg = img;
    rsBox = document.createElement('div');
    rsBox.setAttribute('data-vireo-ui', 'resize');
    rsBox.setAttribute('contenteditable', 'false');
    rsBox.style.cssText = 'position:absolute;pointer-events:none;z-index:9;'
      + 'outline:1px solid #3584e4;';
    ['nw','ne','sw','se'].forEach(function(c){
      var h = document.createElement('div');
      h.dataset.corner = c;
      h.style.cssText = 'position:absolute;width:11px;height:11px;'
        + 'background:#3584e4;border:2px solid Canvas;border-radius:50%;'
        + 'pointer-events:auto;cursor:'
        + ((c === 'nw' || c === 'se') ? 'nwse-resize' : 'nesw-resize') + ';';
      h.style[c[0] === 'n' ? 'top' : 'bottom'] = '-6px';
      h.style[c[1] === 'w' ? 'left' : 'right'] = '-6px';
      h.addEventListener('pointerdown', rsDown);
      rsBox.appendChild(h);
    });
    document.body.appendChild(rsBox);
    rsPlace();
    rsTint();
  }
  /* The context menu's size entries. `original` drops the explicit width so
     the picture falls back to its natural size, still held to the writing
     width by the stylesheet. */
  window.__vireoSetImageSize = function(kind){
    var img = window.__vireoCtxImg || rsImg;
    if(!img) return;
    if(kind === 'original'){
      img.style.width = ''; img.style.height = '';
      img.removeAttribute('width'); img.removeAttribute('height');
    } else {
      var full = document.body.clientWidth;
      var f = kind === 'small' ? 0.25 : (kind === 'medium' ? 0.5 : 1);
      rsApply(img, Math.max(24, Math.round(full * f)));
    }
    if(img === rsImg) rsPlace(); else rsShow(img);
  };
  /* Resizing a picture changes how big it is drawn, not how many bytes it
     is: the full-resolution data: URI still goes out, and the recipient can
     still save the original. Arming this asks for the bytes to be recut to
     the size shown, once, as the message is sent — irreversible, so it is
     off until asked for and marked on the picture while it is on. */
  window.__vireoToggleFit = function(hint){
    var img = window.__vireoCtxImg || rsImg;
    if(!img) return;
    if(img.hasAttribute('data-vireo-fit')){
      img.removeAttribute('data-vireo-fit');
      img.removeAttribute('title');
    } else {
      img.setAttribute('data-vireo-fit', '1');
      img.title = hint;
    }
    if(img === rsImg) rsTint(); else rsShow(img);
  };
  /* Recut every armed picture to the width it is drawn at. drawImage on an
     already-loaded image and toDataURL are both synchronous, so the send
     path can do this and read the body in one go. Enlarged pictures are left
     alone: there are no extra pixels to be had. */
  function rsFlatten(){
    Array.prototype.forEach.call(
      document.querySelectorAll('img[data-vireo-fit]'), function(im){
        var w = Math.round(im.getBoundingClientRect().width);
        if(!w || !im.naturalWidth || w >= im.naturalWidth) return;
        try{
          var c = document.createElement('canvas');
          c.width = w;
          c.height = Math.max(1, Math.round(w * im.naturalHeight / im.naturalWidth));
          c.getContext('2d').drawImage(im, 0, 0, c.width, c.height);
          var m = /^data:(image\/[a-z+]+)/.exec(im.src);
          var type = m ? m[1] : 'image/png';
          im.src = type === 'image/jpeg'
            ? c.toDataURL('image/jpeg', 0.85) : c.toDataURL('image/png');
        }catch(_){}
      });
  }
  /* What the host reads instead of body.innerHTML: the same content with
     this script's own furniture taken out. The arming mark stays, so a
     draft reopened later still shows it and still honours it when sent. */
  window.__vireoBodyHtml = function(){
    var c = document.body.cloneNode(true);
    Array.prototype.forEach.call(c.querySelectorAll('[data-vireo-ui]'),
      function(n){ n.remove(); });
    return c.innerHTML;
  };
  /* Sending, and only sending: recut first, then read with the marks taken
     out. A draft is saved through the plain reader above, so quality is
     never lost to a save. */
  window.__vireoBodyHtmlForSend = function(){
    rsFlatten();
    Array.prototype.forEach.call(
      document.querySelectorAll('img[data-vireo-fit]'), function(n){
        n.removeAttribute('data-vireo-fit'); n.removeAttribute('title');
      });
    rsTint();
    return window.__vireoBodyHtml();
  };
  addEventListener('scroll', rsPlace, true);
  addEventListener('resize', rsPlace);
  document.addEventListener('input', rsPlace);
  /* Clicking an image selects it whole, so it can be deleted, cut, or
     copied like any other selection, and raises its resize frame. */
  document.addEventListener('click', function(e){
    var t = e.target;
    if(rsBox && rsBox.contains(t)) return;   // a handle, not the document
    if(t && t.tagName === 'IMG'){
      var r = document.createRange(); r.selectNode(t);
      var s = getSelection(); s.removeAllRanges(); s.addRange(r);
      rsShow(t);
    } else {
      rsRemove();
    }
  });
  /* A right-click on an image selects it (like a left click) and remembers
     the exact node: the menu's stock Cut/Copy then act on the image itself,
     and Send as Attachment Instead knows which one to lift. Captured on
     button-2 mousedown as well as contextmenu — whichever of the two WebKit
     actually delivers before it opens the menu. */
  function rememberImg(e){
    var t = e.target;
    if(t && t.tagName === 'IMG'){
      window.__vireoCtxImg = t;
      var r = document.createRange(); r.selectNode(t);
      var s = getSelection(); s.removeAllRanges(); s.addRange(r);
      rsShow(t);
    } else if(e.type === 'contextmenu'){
      window.__vireoCtxImg = null;
    }
  }
  document.addEventListener('mousedown', function(e){ if(e.button === 2) rememberImg(e); }, true);
  document.addEventListener('contextmenu', rememberImg, true);
  document.addEventListener('dragover', function(e){
    var it = (e.dataTransfer && e.dataTransfer.items) || [];
    for(var i = 0; i < it.length; i++){
      if(it[i].kind === 'file'){ e.preventDefault(); return; }
    }
  });
  /* Files from a file manager are taken by the widget's drop target before
     this runs — WebKitGTK leaves `dataTransfer.files` empty for them. What
     reaches here is a drag that really does carry file data, such as an
     image dragged out of another page. */
  document.addEventListener('drop', function(e){
    var fs = (e.dataTransfer && e.dataTransfer.files) || [];
    var imgs = [];
    for(var i = 0; i < fs.length; i++){ if(/^image\//.test(fs[i].type)) imgs.push(fs[i]); }
    if(!imgs.length) return;
    e.preventDefault();
    if(document.caretRangeFromPoint){
      var r = document.caretRangeFromPoint(e.clientX, e.clientY);
      if(r){ var sel = getSelection(); sel.removeAllRanges(); sel.addRange(r); }
    }
    imgs.forEach(insertImageFile);
  });
  function insertLinkified(text){
    urlRe.lastIndex = 0;
    var out='', last=0, m;
    while((m = urlRe.exec(text)) !== null){
      out += esc(text.slice(last, m.index));
      var url = m[0];
      var href = /^www\./i.test(url) ? 'http://'+url : url;
      out += '<a href="'+esc(href)+'">'+esc(url)+'</a>';
      last = m.index + url.length;
    }
    out += esc(text.slice(last));
    out = out.replace(/\r\n|\r|\n/g,'<br>');
    document.execCommand('insertHTML', false, out);
  }
  document.addEventListener('paste', function(e){
    var cd = e.clipboardData; if(!cd) return;
    /* An image on the clipboard beats any text flavour riding along with it,
       and pastes inline whichever paste mode is set. */
    var its = cd.items || [];
    for(var i = 0; i < its.length; i++){
      if(its[i].kind === 'file' && /^image\//.test(its[i].type)){
        e.preventDefault();
        insertImageFile(its[i].getAsFile());
        return;
      }
    }
    var rich = window.__vireoPasteRich === true;
    if(window.__vireoPasteOnce !== null){ rich = window.__vireoPasteOnce; window.__vireoPasteOnce = null; }
    var html = cd.getData('text/html');
    var text = cd.getData('text/plain');
    if(rich){
      if(html) return;                       // native paste keeps the formatting
      if(!text) return;
      urlRe.lastIndex = 0;
      if(!urlRe.test(text)) return;          // plain text with nothing to linkify
    } else {
      if(!text) return;                      // no text flavour; let WebKit try
    }
    e.preventDefault();
    insertLinkified(text);
  });
})();
</script>"#;

/// The contentEditable HTML document, themed for light/dark.
fn document(content: &str, dark: bool) -> String {
    let scheme = if dark { "dark" } else { "light" };
    let paste_rich = !crate::config::load_paste_plain();
    let script = format!(
        "<script>window.__vireoPasteRich={paste_rich};</script>{PASTE_SCRIPT}"
    );
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <meta name=\"color-scheme\" content=\"{scheme}\">\
         <style>\
           :root{{color-scheme:{scheme};}}\
           html,body{{height:100%;box-sizing:border-box;}}\
           body{{margin:0;padding:20px;font:14px/1.55 system-ui,sans-serif;outline:none;\
             background:Canvas;color:CanvasText;}}\
           /* Every image fits the writing width — pasted, dropped, or\
              quoted. The inline style on inserted images serves the\
              recipient; this rule is what the composer itself obeys,\
              whatever survives insertHTML. */\
           img{{max-width:100%;height:auto;}}\
           /* A picture armed to be recut when the message is sent. */\
           img[data-vireo-fit]{{outline:2px dashed #e01b24;outline-offset:2px;}}\
           /* The in-progress word's misspelling mark (Custom Highlight API);\
              the tint backs up engines that skip decorations in highlights. */\
           ::highlight(vireo-misspell){{\
             text-decoration:underline wavy #e01b24;\
             background-color:rgba(224,27,36,0.10);}}\
           blockquote{{margin:0 0 0 8px;padding-left:10px;\
             border-left:3px solid rgba(128,128,128,0.4);}}\
           .vireo-quote-attr{{opacity:0.7;margin:10px 0 4px;}}\
           .vireo-sig{{opacity:0.85;}}\
           a{{color:#3584e4;}}\
         </style>{script}\
         <script>window.__vireoDirty=false;\
           document.addEventListener('input',function(){{window.__vireoDirty=true;}},true);\
         </script></head>\
         <body contenteditable=\"true\">{content}</body></html>"
    )
}

/// Escape a string for inclusion inside a single-quoted JavaScript string.
pub fn js_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '<' => out.push_str("\\x3c"),
            _ => out.push(c),
        }
    }
    out
}

/// Convert a stored signature (which may be legacy plain text) to HTML suitable
/// for the editor / for inserting into a message.
pub fn signature_to_html(sig: &str) -> String {
    if sig.contains('<') {
        sig.to_string()
    } else {
        sig.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('\n', "<br>")
    }
}

/// A signature brought in from outside (#120): an HTML file, or source
/// pasted into the signature's HTML dialog. Plain text (nothing that looks
/// like a tag) is escaped line by line like a typed signature.
///
/// HTML is the user's own material, but it still goes through the
/// sanitizer: a `<script>` or an event handler in a signature would run in
/// every composer, and a `<style>` block would restyle the whole message.
/// Structure and inline styles survive, since a designed signature is
/// nothing but tables and `style` attributes. An image referenced by a local
/// path is read and embedded as a `data:` URI (a relative path resolves
/// against `base`, the file's own directory), so the send path lifts it into
/// a `cid:` part like any inline picture; a remote image stays a remote URL.
pub fn signature_from_source(source: &str, base: Option<&std::path::Path>) -> String {
    if !source.contains('<') {
        return signature_to_html(source.trim_end());
    }
    let with_images = inline_local_images(source, base);
    let mut b = ammonia::Builder::default();
    b.add_tags(["table", "thead", "tbody", "tfoot", "tr", "td", "th", "font", "center", "u", "span", "img"])
        .add_generic_attributes([
            "style", "width", "height", "align", "valign", "bgcolor", "border", "cellpadding",
            "cellspacing", "color", "face", "size",
        ])
        .add_tag_attributes("img", ["src", "alt", "width", "height"])
        .url_schemes(std::collections::HashSet::from(["http", "https", "mailto", "tel", "data"]))
        .link_rel(None);
    b.clean(&with_images).to_string().trim().to_string()
}

/// Every `src="…"` that names a local image file, replaced by a `data:` URI
/// of the file's bytes. URLs (`http:`, `data:`, `cid:`, …) and anything that
/// is not a readable image of a sensible size are left exactly as written.
fn inline_local_images(html: &str, base: Option<&std::path::Path>) -> String {
    const MAX_BYTES: u64 = 8 * 1024 * 1024;
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(at) = find_src_attr(rest) {
        let (head, tail) = rest.split_at(at);
        out.push_str(head);
        // `tail` starts at `src=`; the value is the quoted run after it.
        let Some((prefix, value)) = quoted_value(tail) else {
            out.push_str(&tail[..4]);
            rest = &tail[4..];
            continue;
        };
        out.push_str(prefix);
        match local_image_data_uri(value, base, MAX_BYTES) {
            Some(uri) => out.push_str(&uri),
            None => out.push_str(value),
        }
        // The closing quote is the first char of `rest`, kept as it was.
        rest = &tail[prefix.len() + value.len()..];
    }
    out.push_str(rest);
    out
}

/// Byte offset of the next ` src=` attribute (any case), or `None`.
fn find_src_attr(s: &str) -> Option<usize> {
    let lower = s.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find("src=") {
        let at = from + i;
        let preceded_by_space = at > 0 && lower.as_bytes()[at - 1].is_ascii_whitespace();
        if preceded_by_space {
            return Some(at);
        }
        from = at + 4;
    }
    None
}

/// Split `src="value"…` into (`src="`, `value`). `None` when the value is
/// unquoted or unterminated.
fn quoted_value(s: &str) -> Option<(&str, &str)> {
    let after = &s[4..];
    let quote = after.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let value = &after[quote.len_utf8()..];
    let end = value.find(quote)?;
    Some((&s[..4 + quote.len_utf8()], &value[..end]))
}

/// The `data:` URI for a `src` value that names a local image, else `None`.
fn local_image_data_uri(value: &str, base: Option<&std::path::Path>, max: u64) -> Option<String> {
    let v = value.trim();
    let lower = v.to_ascii_lowercase();
    let path = if let Some(rest) = lower.strip_prefix("file://") {
        std::path::PathBuf::from(percent_decode(&v[v.len() - rest.len()..]))
    } else if lower.contains(':') && !lower.starts_with('/') && !lower.starts_with('.') {
        // Some other scheme (http, https, data, cid, mailto…): not ours.
        return None;
    } else {
        let p = std::path::PathBuf::from(percent_decode(v));
        if p.is_absolute() {
            p
        } else {
            base?.join(p)
        }
    };
    let mime = match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "avif" => "image/avif",
        _ => return None,
    };
    let meta = std::fs::metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > max {
        return None;
    }
    let data = std::fs::read(&path).ok()?;
    Some(format!("data:{mime};base64,{}", crate::oauth::base64_encode(&data)))
}

/// `%20` and friends back to characters, for a path or id that came as a URL.
pub(crate) fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(h) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("zz"), 16) {
                out.push(h);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod signature_tests {
    use super::*;

    /// Plain text stays a typed signature: escaped, line breaks kept.
    #[test]
    fn plain_text_is_escaped_like_a_typed_signature() {
        assert_eq!(signature_from_source("Ada & Co\nCEO\n", None), "Ada &amp; Co<br>CEO");
    }

    /// The parts a designed signature is made of survive; what could run or
    /// restyle the message does not.
    #[test]
    fn html_keeps_structure_and_styles_but_not_scripts() {
        let src = "<html><head><style>body{color:red}</style></head><body>\
                   <table style=\"border:0\"><tr><td style=\"color:#333\" onclick=\"x()\">\
                   <b>Ada</b> <a href=\"https://example.com\">site</a></td></tr></table>\
                   <script>alert(1)</script></body></html>";
        let out = signature_from_source(src, None);
        assert!(out.contains("<table style=\"border:0\">"), "{out}");
        assert!(out.contains("<td style=\"color:#333\">"), "{out}");
        assert!(out.contains("<a href=\"https://example.com\">site</a>"), "{out}");
        assert!(!out.contains("onclick"), "{out}");
        assert!(!out.contains("script"), "{out}");
        assert!(!out.contains("color:red"), "{out}");
    }

    /// A local image next to the file is embedded; a remote one is left alone.
    #[test]
    fn local_images_are_embedded_and_remote_ones_kept() {
        let dir = std::env::temp_dir().join(format!("vireo-sig-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("logo.png"), [0x89, b'P', b'N', b'G']).unwrap();
        let src = "<p><img src=\"logo.png\" width=\"80\"> <img src='https://x.example/a.png'>\
                   <img src=\"missing.png\"></p>";
        let out = signature_from_source(src, Some(&dir));
        std::fs::remove_dir_all(&dir).ok();
        assert!(out.contains("src=\"data:image/png;base64,iVBORw==\""), "{out}");
        assert!(out.contains("width=\"80\""), "{out}");
        assert!(out.contains("src=\"https://x.example/a.png\""), "{out}");
        // A path that resolves to nothing is left as written.
        assert!(out.contains("src=\"missing.png\""), "{out}");
    }
}
