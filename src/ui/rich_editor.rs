//! A small reusable WYSIWYG rich-text editor: a `contentEditable` WebView with a
//! formatting toolbar. JavaScript runs only in this (our own) document, to drive
//! editing commands. Used for the compose body and the account signature.

use adw::prelude::*;
use webkit6::prelude::WebViewExt;

/// A rich-text editor widget. Add `widget` to a container; read the content back
/// with [`RichEditor::extract_html`] (asynchronous, since it queries the WebView).
pub struct RichEditor {
    /// The toolbar + editor, ready to be placed in a container.
    pub widget: gtk::Box,
    webview: webkit6::WebView,
}

impl RichEditor {
    pub fn new(initial_html: &str) -> Self {
        let webview = webkit6::WebView::new();
        let settings = webkit6::Settings::new();
        settings.set_enable_javascript(true);
        settings.set_enable_developer_extras(false);
        webview.set_settings(&settings);
        let dark = adw::StyleManager::default().is_dark();
        webview.load_html(&document(initial_html, dark), Some("https://vireo.localhost/editor"));

        let toolbar = build_toolbar(&webview);

        let frame = gtk::Frame::new(None);
        frame.set_vexpand(true);
        frame.set_child(Some(&webview));

        let bx = gtk::Box::new(gtk::Orientation::Vertical, 6);
        bx.append(&toolbar);
        bx.append(&frame);

        RichEditor { widget: bx, webview }
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

/// Auto-linkify URLs in pasted plain text. Lives in `<head>` (not the editable
/// body) so it never becomes part of the message. Rich pastes that already carry
/// links are left untouched.
const PASTE_SCRIPT: &str = r#"<script>
(function(){
  var urlRe = /((?:https?:\/\/|www\.)[^\s<>()]+[^\s<>().,;:!?'"])/gi;
  function esc(s){return s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');}
  document.addEventListener('paste', function(e){
    var cd = e.clipboardData; if(!cd) return;
    var html = cd.getData('text/html');
    if(html && /<a[\s>]/i.test(html)) return; // already-linked rich paste: keep it
    var text = cd.getData('text/plain'); if(!text) return;
    urlRe.lastIndex = 0;
    if(!urlRe.test(text)) return;            // nothing to linkify
    e.preventDefault();
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
  });
})();
</script>"#;

/// The contentEditable HTML document, themed for light/dark.
fn document(content: &str, dark: bool) -> String {
    let scheme = if dark { "dark" } else { "light" };
    let script = PASTE_SCRIPT;
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <meta name=\"color-scheme\" content=\"{scheme}\">\
         <style>\
           :root{{color-scheme:{scheme};}}\
           html,body{{height:100%;box-sizing:border-box;}}\
           body{{margin:0;padding:20px;font:14px/1.55 system-ui,sans-serif;outline:none;}}\
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
