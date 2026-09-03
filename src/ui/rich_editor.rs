//! A small reusable WYSIWYG rich-text editor: a `contentEditable` WebView with a
//! formatting toolbar. JavaScript runs only in this (our own) document, to drive
//! editing commands. Used for the compose body and the account signature.

use adw::prelude::*;
use webkit6::prelude::WebViewExt;

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
    for dir in ["/usr/share/hunspell", "/usr/share/myspell"] {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
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
    codes.into_iter().collect()
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
                        "Send as Attachment Instead",
                        None,
                    ),
                    0,
                );
                menu.insert(&webkit6::ContextMenuItem::new_separator(), 1);
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
            for (label, rich) in [("Paste with Formatting", true), ("Paste as Plain Text", false)] {
                let action =
                    gtk::gio::SimpleAction::new(if rich { "vireo-paste-rich" } else { "vireo-paste-plain" }, None);
                let v = view.clone();
                action.connect_activate(move |_, _| paste_into(&v, rich));
                menu.insert(&webkit6::ContextMenuItem::from_gaction(&action, label, None), at);
                at += 1;
            }
            false
        });

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
        paste_into(&self.webview, rich);
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
            "document.body.innerHTML",
            None,
            None,
            gtk::gio::Cancellable::NONE,
            move |res| cb(res.map(|v| v.to_str().to_string()).unwrap_or_default()),
        );
    }

    /// Read the current body HTML and a plain-text rendering asynchronously.
    pub fn extract(&self, cb: impl FnOnce(String, String) + 'static) {
        self.webview.evaluate_javascript(
            "document.body.innerHTML + '\\u0000' + document.body.innerText",
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
fn paste_into(webview: &webkit6::WebView, rich: bool) {
    let v = webview.clone();
    webview.evaluate_javascript(
        &format!("window.__vireoPasteOnce={rich};"),
        None,
        None,
        gtk::gio::Cancellable::NONE,
        move |_| v.execute_editing_command("Paste"),
    );
}

fn build_toolbar(webview: &webkit6::WebView) -> gtk::Box {
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    bar.add_css_class("toolbar");
    bar.add_css_class("format-bar");

    // (icon, tooltip, execCommand snippet)
    let commands: &[(&str, &str, &str)] = &[
        ("co.hyprlab.Vireo-format-text-bold-symbolic", "Bold", "document.execCommand('bold')"),
        ("co.hyprlab.Vireo-format-text-italic-symbolic", "Italic", "document.execCommand('italic')"),
        ("co.hyprlab.Vireo-format-text-underline-symbolic", "Underline", "document.execCommand('underline')"),
        ("co.hyprlab.Vireo-format-text-strikethrough-symbolic", "Strikethrough", "document.execCommand('strikeThrough')"),
        ("SEP", "", ""),
        ("co.hyprlab.Vireo-view-list-bullet-symbolic", "Bulleted list", "document.execCommand('insertUnorderedList')"),
        ("co.hyprlab.Vireo-view-list-ordered-symbolic", "Numbered list", "document.execCommand('insertOrderedList')"),
        // Adwaita has no blockquote glyph; the indent icon reads as "quote".
        ("co.hyprlab.Vireo-format-indent-more-symbolic", "Quote", "document.execCommand('formatBlock',false,'blockquote')"),
        // `LINK` is a sentinel command (handled specially); the icon is real.
        ("co.hyprlab.Vireo-insert-link-symbolic", "Insert link", "LINK"),
        ("SEP", "", ""),
        ("co.hyprlab.Vireo-edit-clear-symbolic", "Clear formatting", "document.execCommand('removeFormat')"),
    ];

    for (icon, tip, cmd) in commands {
        if *icon == "SEP" {
            bar.append(&gtk::Separator::new(gtk::Orientation::Vertical));
            continue;
        }
        let btn = gtk::Button::from_icon_name(icon);
        btn.set_tooltip_text(Some(tip));
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
    let dialog = adw::MessageDialog::new(parent.as_ref(), Some("Insert Link"), None);
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("ok", "Insert");
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
    im.src = url;
  }
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
  /* Clicking an image selects it whole, so it can be deleted, cut, or
     copied like any other selection. */
  document.addEventListener('click', function(e){
    var t = e.target;
    if(t && t.tagName === 'IMG'){
      var r = document.createRange(); r.selectNode(t);
      var s = getSelection(); s.removeAllRanges(); s.addRange(r);
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
