//! Right pane: the reading view for a single message.
//!
//! Bodies are rendered in a sandboxed WebKit view: JavaScript is disabled and a
//! Content-Security-Policy blocks remote content. When remote content is
//! withheld, its URLs are also stripped from the HTML so nothing is requested;
//! the originals are only used once the user (or a trusted sender) allows them.
//! Link clicks open in the browser.

use adw::prelude::*;
use relm4::prelude::*;
use webkit6::prelude::{PolicyDecisionExt, WebViewExt};

use crate::models::Message;

pub struct MessageView {
    /// The newest message in the thread — drives the header, avatar and actions.
    current: Option<Message>,
    /// The whole conversation, newest first. One entry = a single message (shown
    /// exactly as before); more = a scrollable conversation in the body view.
    thread: Vec<Message>,
    /// Remote content is present and currently withheld.
    blocked: bool,
    /// Whether Gravatar loading is enabled.
    gravatar: bool,
    /// Decoded Gravatar for the current sender, if any.
    avatar_texture: Option<gtk::gdk::Texture>,
    /// Owning account's display name (header chip).
    account_name: Option<String>,
    /// Provider holding the header chip's per-account colours.
    chip_provider: gtk::CssProvider,
    /// True while the body is being fetched (show a spinner instead).
    loading: bool,
    /// False from when a render starts until the WebView reports it finished
    /// loading — a themed cover hides the WebView's white inter-document gap.
    webview_ready: bool,
    webview: webkit6::WebView,
    /// Bumped per render: each load gets a unique base URI so WebKit treats it
    /// as a fresh document and re-fetches resources (reusing `about:blank` does
    /// not). An https base also lets https images load without mixed-content.
    seq: std::cell::Cell<u64>,
    /// Forced dark flag for message content, or `None` to follow the system UI.
    /// This themes email content only, not the app chrome.
    content_dark: Option<bool>,
    /// Whether the current message's From: address survived its provider's
    /// authentication checks. `None` until the verdict arrives with the body.
    sender_check: Option<crate::models::SenderCheck>,
    /// Full URL of the link under the pointer, shown in a corner overlay so a
    /// link's real destination is visible before it is clicked.
    link_preview: gtk::Label,
}

impl MessageView {
    /// The current verdict, defaulting to "unverified" before one arrives.
    fn trust(&self) -> crate::models::SenderTrust {
        self.sender_check
            .as_ref()
            .map(|c| c.trust)
            .unwrap_or(crate::models::SenderTrust::Unverified)
    }
}

#[derive(Debug)]
pub enum MessageViewInput {
    Show {
        /// The conversation, newest first. A single message for a normal open;
        /// several for a threaded conversation.
        thread: Vec<Message>,
        /// The sender is trusted, so remote content may auto-load.
        allow_remote: bool,
        /// Whether Gravatar loading is enabled.
        gravatar: bool,
        /// Owning account's display name and colour, for the header chip.
        account_name: Option<String>,
        account_color: Option<String>,
        /// The body is still being fetched — show a spinner.
        loading: bool,
    },
    LoadRemoteOnce,
    AllowSenderAlways,
    /// The sender email link in the header was clicked.
    ComposeSender,
    /// The system/app light-dark preference changed; re-render to match.
    ThemeChanged,
    /// Print the message on screen (issue #16).
    Print,
    /// Set the message-content theme: `None` follows the system, `Some(dark)`
    /// forces light/dark for email content only (not the app UI).
    SetContentTheme(Option<bool>),
    /// The WebView finished loading the current document — reveal it.
    Rendered,
    /// The sender-authentication verdict for the message now on screen.
    SetSenderCheck(Box<crate::models::SenderCheck>),
    /// A conversation message header was double-clicked — open that message in
    /// its own window.
    OpenHeader { account_id: u32, id: u32 },
}

#[derive(Debug)]
pub enum MessageViewOutput {
    /// Add this sender address to the remote-content allowlist.
    AllowSender(String),
    /// Compose a new message to this address.
    ComposeTo(String),
    /// Open a conversation message in its own window (header double-clicked).
    OpenWindow(Box<Message>),
}

#[relm4::component(pub)]
impl Component for MessageView {
    type Init = ();
    type Input = MessageViewInput;
    type Output = MessageViewOutput;
    /// (message id, fetched Gravatar bytes) — id guards against stale results.
    type CommandOutput = (u32, Option<Vec<u8>>);

    view! {
        gtk::Stack {
            set_transition_type: gtk::StackTransitionType::Crossfade,

            add_named[Some("empty")] = &adw::StatusPage {
                set_icon_name: Some("co.hyprlab.Vireo-mail-read-symbolic"),
                set_title: "No message selected",
                set_description: Some("Choose a message from the list to read it here."),
            },

            add_named[Some("message")] = &gtk::Box {
                set_orientation: gtk::Orientation::Vertical,
                add_css_class: "reader-pane",

                gtk::Revealer {
                    set_transition_type: gtk::RevealerTransitionType::SlideDown,
                    #[watch]
                    set_reveal_child: model.trust().is_alarming(),

                    gtk::Box {
                        add_css_class: "spoof-alert",
                        set_spacing: 8,

                        gtk::Image { set_icon_name: Some("co.hyprlab.Vireo-dialog-warning-symbolic") },
                        gtk::Label {
                            #[watch]
                            set_label: model
                                .sender_check
                                .as_ref()
                                .map(|c| c.summary.as_str())
                                .unwrap_or_default(),
                            set_hexpand: true,
                            set_halign: gtk::Align::Start,
                            set_wrap: true,
                            set_xalign: 0.0,
                        },
                    },
                },

                gtk::Revealer {
                    set_transition_type: gtk::RevealerTransitionType::SlideDown,
                    #[watch]
                    set_reveal_child: model.blocked,

                    gtk::Box {
                        add_css_class: "remote-alert",
                        set_spacing: 8,

                        gtk::Image { set_icon_name: Some("co.hyprlab.Vireo-security-high-symbolic") },
                        gtk::Label {
                            set_label: "Remote content (images, trackers) was blocked to protect your privacy.",
                            set_hexpand: true,
                            set_halign: gtk::Align::Start,
                            set_wrap: true,
                            set_xalign: 0.0,
                        },
                        gtk::Button {
                            set_label: "Load",
                            set_valign: gtk::Align::Center,
                            connect_clicked => MessageViewInput::LoadRemoteOnce,
                        },
                        gtk::Button {
                            set_label: "Always allow sender",
                            set_valign: gtk::Align::Center,
                            #[watch]
                            set_tooltip_text: model.current.as_ref().map(|m| m.from_addr.as_str()),
                            connect_clicked => MessageViewInput::AllowSenderAlways,
                        },
                    },
                },

                gtk::Box {
                    add_css_class: "reader-header",
                    set_orientation: gtk::Orientation::Vertical,
                    set_spacing: 12,

                    gtk::Box {
                        set_halign: gtk::Align::Start,
                        #[watch]
                        set_visible: model.account_name.is_some(),
                        gtk::Label {
                            #[watch]
                            set_label: model.account_name.as_deref().unwrap_or_default(),
                            add_css_class: "account-chip",
                            add_css_class: "vireo-account-chip",
                        },
                    },

                    gtk::Label {
                        #[watch]
                        set_label: model.current.as_ref().map(|m| m.subject.as_str()).unwrap_or_default(),
                        set_halign: gtk::Align::Start,
                        set_wrap: true,
                        // Break mid-word for unbreakable tokens (e.g. an
                        // undecodable subject or a long URL) so an extreme
                        // subject can never force the pane — and with it the
                        // window controls — wider than the screen.
                        set_wrap_mode: gtk::pango::WrapMode::WordChar,
                        set_xalign: 0.0,
                        set_selectable: true,
                        add_css_class: "reader-subject",
                    },

                    gtk::Box {
                        set_spacing: 12,
                        // For a conversation each message carries its own header in
                        // the scrollable body, so hide this single-message header.
                        #[watch]
                        set_visible: model.thread.len() <= 1,

                        adw::Avatar {
                            set_size: 44,
                            set_valign: gtk::Align::Center,
                            set_show_initials: true,
                            #[watch]
                            set_text: model.current.as_ref().map(|m| m.from_name.as_str()),
                            #[watch]
                            set_custom_image: model.avatar_texture.as_ref(),
                        },

                        gtk::Box {
                            set_orientation: gtk::Orientation::Vertical,
                            set_valign: gtk::Align::Center,
                            set_hexpand: true,

                            gtk::Label {
                                #[watch]
                                set_label: model.current.as_ref().map(|m| m.from_name.as_str()).unwrap_or_default(),
                                set_halign: gtk::Align::Start,
                                set_selectable: true,
                                add_css_class: "reader-from-name",
                            },
                            gtk::Label {
                                #[watch]
                                set_markup: &email_link_markup(model.current.as_ref()),
                                set_halign: gtk::Align::Start,
                                set_selectable: true,
                                set_tooltip_text: Some("Send a new message to this address"),
                                add_css_class: "reader-from-addr",
                                connect_activate_link[sender] => move |_, _uri| {
                                    sender.input(MessageViewInput::ComposeSender);
                                    gtk::glib::Propagation::Stop
                                },
                            },
                        },

                        gtk::Label {
                            #[watch]
                            set_label: &model.current.as_ref().map(|m| m.datetime_full()).unwrap_or_default(),
                            set_valign: gtk::Align::Start,
                            set_selectable: true,
                            add_css_class: "reader-date",
                        },
                    },

                    gtk::Label {
                        #[watch]
                        set_label: &cc_line(model.current.as_ref()),
                        #[watch]
                        set_visible: model.thread.len() <= 1
                            && model.current.as_ref().is_some_and(|m| !m.cc.trim().is_empty()),
                        set_halign: gtk::Align::Start,
                        set_wrap: true,
                        set_wrap_mode: gtk::pango::WrapMode::WordChar,
                        set_xalign: 0.0,
                        set_selectable: true,
                        add_css_class: "reader-cc",
                    },
                },

                gtk::Separator {},

                #[name = "body_stack"]
                gtk::Stack {
                    set_vexpand: true,
                    #[watch]
                    set_visible_child_name: model.body_page(),

                    // Themed cover shown while the WebView loads, so its white
                    // inter-document gap is never visible.
                    add_named[Some("blank")] = &gtk::Box {
                        set_vexpand: true,
                        set_hexpand: true,
                    },

                    add_named[Some("loading")] = &gtk::Box {
                        set_orientation: gtk::Orientation::Vertical,
                        set_halign: gtk::Align::Center,
                        set_valign: gtk::Align::Center,
                        set_spacing: 14,

                        gtk::Spinner {
                            set_spinning: true,
                            set_width_request: 36,
                            set_height_request: 36,
                        },
                        gtk::Label {
                            set_label: "Loading…",
                            add_css_class: "dim-label",
                        },
                    },
                },
            },

            #[watch]
            set_visible_child_name: if model.current.is_some() { "message" } else { "empty" },
        }
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let webview = new_webview();
        // Browser-style link preview: a small plaque in the bottom-left corner of
        // the body showing exactly where the link under the pointer goes. GTK
        // tooltips are unreliable over a WebView (WebKit handles motion events
        // itself, so GTK's hover timer often never starts), and a phishing check
        // that only sometimes appears is worse than none.
        let link_preview = gtk::Label::new(None);
        link_preview.add_css_class("link-preview");
        link_preview.set_halign(gtk::Align::Start);
        link_preview.set_valign(gtk::Align::End);
        link_preview.set_visible(false);
        link_preview.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        link_preview.set_max_width_chars(90);
        link_preview.set_can_target(false); // never intercept clicks meant for the page
        webview.connect_mouse_target_changed({
            let label = link_preview.clone();
            move |_view, hit, _modifiers| {
                let uri = hit.context_is_link().then(|| hit.link_uri()).flatten();
                match uri {
                    Some(uri) => {
                        label.set_text(&link_destination(&uri, hit.link_label().as_deref()));
                        label.set_visible(true);
                    }
                    None => label.set_visible(false),
                }
            }
        });
        let chip_provider = gtk::CssProvider::new();
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &chip_provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
        let model = MessageView {
            current: None,
            thread: Vec::new(),
            blocked: false,
            gravatar: false,
            avatar_texture: None,
            account_name: None,
            chip_provider,
            loading: false,
            webview_ready: false,
            webview,
            sender_check: None,
            link_preview: link_preview.clone(),
            seq: std::cell::Cell::new(0),
            content_dark: None,
        };

        // Reveal the WebView only once it's finished loading the document.
        let ready_sender = sender.clone();
        model.webview.connect_load_changed(move |_view, event| {
            if event == webkit6::LoadEvent::Finished {
                ready_sender.input(MessageViewInput::Rendered);
            }
        });

        // Double-click on a conversation header → open that message's window.
        if let Some(ucm) = model.webview.user_content_manager() {
            let open_sender = sender.clone();
            ucm.connect_script_message_received(Some("vireo"), move |_ucm, value| {
                let key = value.to_str().to_string();
                if let Some((a, i)) = key.split_once(':') {
                    if let (Ok(account_id), Ok(id)) = (a.parse::<u32>(), i.parse::<u32>()) {
                        open_sender.input(MessageViewInput::OpenHeader { account_id, id });
                    }
                }
            });
        }

        // Re-render the body when the light/dark preference changes so unstyled
        // content tracks the theme live.
        let style_manager = adw::StyleManager::default();
        model.apply_webview_bg(style_manager.is_dark());
        let theme_sender = sender.clone();
        style_manager.connect_dark_notify(move |_| {
            theme_sender.input(MessageViewInput::ThemeChanged);
        });

        let widgets = view_output!();
        let body_overlay = gtk::Overlay::new();
        body_overlay.set_child(Some(&model.webview));
        body_overlay.add_overlay(&link_preview);
        widgets.body_stack.add_named(&body_overlay, Some("body"));
        widgets.body_stack.set_visible_child_name("body");
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match msg {
            MessageViewInput::Show {
                thread,
                allow_remote,
                gravatar,
                account_name,
                account_color,
                loading,
            } => {
                // A new message: the previous message's verdict must not linger
                // on screen while this one's is still being fetched.
                let same_message = self.current.as_ref().zip(thread.first()).is_some_and(
                    |(a, b)| a.id == b.id && a.account_id == b.account_id,
                );
                if !same_message {
                    self.sender_check = None;
                }
                self.link_preview.set_visible(false);
                self.current = thread.first().cloned();
                self.thread = thread;
                self.gravatar = gravatar;
                self.account_name = account_name;
                self.loading = loading;
                if let Some(color) = &account_color {
                    let css = format!(
                        ".vireo-account-chip {{ background-color: {}; color: {}; }}",
                        crate::color::pale(color, 0.18),
                        color
                    );
                    self.chip_provider.load_from_data(&css);
                }
                let has_remote = self
                    .thread
                    .iter()
                    .any(|m| has_remote_resources(&m.body));
                self.blocked = has_remote && !allow_remote;
                self.load_avatar(&sender);
                // While loading, the spinner page is shown; rendering the (empty)
                // body would just flash blank, so wait for the real body.
                if !self.loading {
                    self.render();
                }
            }
            MessageViewInput::LoadRemoteOnce => {
                self.blocked = false;
                self.render();
            }
            MessageViewInput::AllowSenderAlways => {
                if let Some(m) = &self.current {
                    let _ = sender.output(MessageViewOutput::AllowSender(m.from_addr.clone()));
                }
                self.blocked = false;
                self.render();
            }
            MessageViewInput::ComposeSender => {
                if let Some(m) = &self.current {
                    if !m.from_addr.is_empty() {
                        let _ = sender.output(MessageViewOutput::ComposeTo(m.from_addr.clone()));
                    }
                }
            }
            MessageViewInput::Print => {
                // Printing is deliberately split in two: GTK's async print dialog
                // collects the settings, then WebKit prints with them.
                //
                // WebKit's own `run_dialog` would be one call, but it spins a
                // nested main loop, and polling a glib future from inside one
                // aborts the process ("Polling futures only allowed if the thread
                // is owning the MainContext") — which is exactly what happened
                // the first time this shipped. GtkPrintDialog exists because of
                // that class of bug: it returns through a callback instead.
                let print = webkit6::PrintOperation::new(&self.webview);
                let job = self
                    .current
                    .as_ref()
                    .map(|m| m.subject.clone())
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or_else(|| "Message".to_string());
                let parent = self.webview.root().and_downcast::<gtk::Window>();

                let dialog = gtk::PrintDialog::new();
                dialog.set_title("Print Message");
                // Names the job in the queue and seeds the filename when printing
                // to a file, which is otherwise "unknown".
                let settings = gtk::PrintSettings::new();
                settings.set(
                    gtk::PRINT_SETTINGS_OUTPUT_BASENAME,
                    Some(&sanitize_filename(&job)),
                );
                dialog.set_print_settings(&settings);

                dialog.setup(
                    parent.as_ref(),
                    gtk::gio::Cancellable::NONE,
                    move |result| match result {
                        Ok(setup) => {
                            print.set_print_settings(&setup.print_settings());
                            print.set_page_setup(&setup.page_setup());
                            print.connect_failed(|_, error| {
                                tracing::warn!("printing failed: {error}");
                            });
                            // Keep the operation alive until WebKit says it is
                            // done; dropping it here would cancel the job.
                            let keep = std::cell::RefCell::new(Some(print.clone()));
                            print.connect_finished(move |_| {
                                keep.borrow_mut().take();
                            });
                            print.print();
                        }
                        // Dismissing the dialog arrives here as an error; it is
                        // the ordinary way to change your mind, not a failure.
                        Err(e) => tracing::debug!("print dialog dismissed: {e}"),
                    },
                );
            }

            MessageViewInput::ThemeChanged => {
                let dark = self.effective_dark();
                self.apply_webview_bg(dark);
                if self.current.is_some() && !self.loading {
                    self.render();
                }
            }
            MessageViewInput::SetContentTheme(o) => {
                if self.content_dark != o {
                    self.content_dark = o;
                    let dark = self.effective_dark();
                    self.apply_webview_bg(dark);
                    if self.current.is_some() && !self.loading {
                        self.render();
                    }
                }
            }
            MessageViewInput::SetSenderCheck(check) => {
                self.sender_check = Some(*check);
            }

            MessageViewInput::Rendered => {
                self.webview_ready = true;
            }
            MessageViewInput::OpenHeader { account_id, id } => {
                if let Some(m) = self
                    .thread
                    .iter()
                    .find(|m| m.account_id == account_id && m.id == id)
                {
                    let _ = sender.output(MessageViewOutput::OpenWindow(Box::new(m.clone())));
                }
            }
        }
    }

    fn update_cmd(
        &mut self,
        (message_id, bytes): Self::CommandOutput,
        _sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        // Ignore results for a message that's no longer shown.
        let still_current = self
            .current
            .as_ref()
            .is_some_and(|m| m.id == message_id);
        if !still_current {
            return;
        }
        if let (Some(bytes), Some(m)) = (bytes, self.current.as_ref()) {
            self.avatar_texture = crate::avatar::decode_and_cache(&m.from_addr, &bytes);
        }
    }
}

impl MessageView {
    /// Set the sender avatar: cached texture if available, otherwise (when
    /// Gravatar is enabled) kick off a background fetch keyed to this message.
    fn load_avatar(&mut self, sender: &ComponentSender<Self>) {
        self.avatar_texture = None;
        if !self.gravatar {
            return;
        }
        let Some(m) = self.current.as_ref() else {
            return;
        };
        let email = m.from_addr.clone();
        if email.is_empty() {
            return;
        }
        if let Some(tex) = crate::avatar::cached(&email) {
            self.avatar_texture = Some(tex);
            return;
        }
        let id = m.id;
        sender.oneshot_command(async move {
            let bytes = tokio::task::spawn_blocking(move || crate::avatar::fetch(&email))
                .await
                .ok()
                .flatten();
            (id, bytes)
        });
    }

    /// Whether message content should render dark: the user's forced choice, or
    /// the system UI theme when following it.
    fn effective_dark(&self) -> bool {
        self.content_dark
            .unwrap_or_else(|| adw::StyleManager::default().is_dark())
    }

    fn render(&mut self) {
        let dark = self.effective_dark();
        self.apply_webview_bg(dark);
        // Hide the WebView behind the themed cover until this load finishes.
        self.webview_ready = false;
        let html = self.document_html(dark);
        let n = self.seq.get().wrapping_add(1);
        self.seq.set(n);
        self.webview
            .load_html(&html, Some(&format!("https://vireo.localhost/message/{n}")));
    }

    /// Which body-stack page to show: spinner while fetching, themed cover while
    /// the WebView loads, then the rendered message(s).
    fn body_page(&self) -> &'static str {
        if self.loading {
            "loading"
        } else if !self.webview_ready {
            "blank"
        } else {
            "body"
        }
    }

    /// The wrapper document: one sandboxed iframe per message (so each email's CSS
    /// is fully isolated and its scripts can't run), with per-message headers in
    /// conversation mode. A small script sizes each iframe to its content.
    fn document_html(&self, dark: bool) -> String {
        let conversation = self.thread.len() > 1;
        let mut sections = String::new();
        for m in &self.thread {
            let body = if m.body.trim().is_empty() {
                "<div class=\"vireo-loading\">Loading…</div>".to_string()
            } else {
                message_frame(&m.body, self.blocked, dark)
            };
            if conversation {
                sections.push_str(&format!(
                    "<section class=\"vireo-msg\">\
                       <header class=\"vireo-msg-hdr\" data-key=\"{aid}:{id}\" \
                         title=\"Double-click to open in a new window\">\
                         <span class=\"vireo-from\">{from}</span>{addr}\
                         <span class=\"vireo-date\">{date}</span>\
                       </header>{body}</section>",
                    aid = m.account_id,
                    id = m.id,
                    from = attr_escape(&m.from_name),
                    addr = if m.from_addr.is_empty() {
                        String::new()
                    } else {
                        format!("<span class=\"vireo-addr\">&lt;{}&gt;</span>", attr_escape(&m.from_addr))
                    },
                    date = attr_escape(&m.datetime_full()),
                    body = body,
                ));
            } else {
                sections.push_str(&body);
            }
        }
        let scheme = if dark { "dark" } else { "light" };
        // Paint the wrapper and the (still-loading) iframes in the theme colour so
        // there's no white flash before each message's content renders.
        let bg = if dark { "#1e1e1e" } else { "#ffffff" };
        format!(
            "<!doctype html><html><head><meta charset=\"utf-8\">\
             <meta name=\"color-scheme\" content=\"{scheme}\">\
             <style>\
               :root{{color-scheme:{scheme};}}\
               body{{margin:0;padding:0;background:{bg};font:14px/1.55 system-ui,sans-serif;}}\
               iframe.vireo-frame{{width:100%;border:0;display:block;background:{bg};}}\
               .vireo-msg{{border-bottom:1px solid rgba(128,128,128,0.25);}}\
               .vireo-msg-hdr{{display:flex;gap:8px;align-items:baseline;flex-wrap:wrap;padding:12px 16px;cursor:default;user-select:none;transition:background 120ms ease;}}\
               .vireo-msg-hdr:hover{{background:rgba(128,128,128,0.16);}}\
               .vireo-from{{font-weight:700;}}\
               .vireo-addr{{opacity:0.55;font-size:0.9em;}}\
               .vireo-date{{margin-left:auto;opacity:0.55;font-size:0.85em;}}\
               .vireo-loading{{opacity:0.5;padding:16px;}}\
             </style>\
             <script>{SIZE_SCRIPT}</script>\
             </head><body>{sections}</body></html>"
        )
    }

    /// Paint the WebView canvas in the theme colour so unstyled bodies (and the
    /// gap before a load) match light/dark mode instead of flashing white.
    fn apply_webview_bg(&self, dark: bool) {
        let rgba = if dark {
            gtk::gdk::RGBA::new(0.118, 0.118, 0.118, 1.0)
        } else {
            gtk::gdk::RGBA::new(1.0, 1.0, 1.0, 1.0)
        };
        self.webview.set_background_color(&rgba);
    }
}

/// Create a sandboxed WebView: no JavaScript or dev tools, smooth scrolling, and
/// links routed to the external browser.
fn new_webview() -> webkit6::WebView {
    // A user-content manager with a script message handler lets the wrapper
    // document notify us (e.g. a double-clicked conversation header).
    let ucm = webkit6::UserContentManager::new();
    ucm.register_script_message_handler("vireo", None);
    let webview = webkit6::WebView::builder()
        .user_content_manager(&ucm)
        .build();

    let settings = webkit6::Settings::new();
    // JavaScript runs only in our own (trusted) wrapper document — it sizes each
    // message's iframe to its content. Every email body is embedded in a
    // `sandbox`ed iframe WITHOUT `allow-scripts`, so message scripts never run.
    settings.set_enable_javascript(true);
    settings.set_enable_developer_extras(false);
    webview.set_settings(&settings);

    // "Save Image As…" on a right-clicked image. WebKit's own item hands the
    // image to a download, which needs a `WebKitNetworkSession` destination
    // handler we don't have — so it silently did nothing. Every image the reader
    // shows inline (attached photos, and `cid:` images since 1.7.1) is a `data:`
    // URI whose bytes are already in the document, so swap in our own item that
    // decodes them and opens a real save dialog.
    webview.connect_context_menu(|view, menu, hit| {
        if !hit.context_is_image() {
            return false; // not an image — leave the default menu alone
        }
        let Some(uri) = hit.image_uri() else {
            return false;
        };
        let Some((mime, data)) = decode_data_uri(&uri) else {
            // A remote image: WebKit's own download-backed item is the only way
            // to save it, so leave the menu untouched.
            return false;
        };

        // Replace the stock item in place so the menu keeps its familiar order.
        let Some(stock) = menu
            .items()
            .into_iter()
            .find(|i| i.stock_action() == webkit6::ContextMenuAction::DownloadImageToDisk)
        else {
            return false;
        };
        let position = menu.items().iter().position(|i| i == &stock).unwrap_or(0);

        let action = gtk::gio::SimpleAction::new("vireo-save-image", None);
        let window = view.root().and_downcast::<gtk::Window>();
        action.connect_activate(move |_, _| {
            let dialog = gtk::FileDialog::builder()
                .initial_name(default_image_name(&mime))
                .title("Save Image")
                .build();
            let data = data.clone();
            dialog.save(window.as_ref(), gtk::gio::Cancellable::NONE, move |res| {
                if let Ok(file) = res {
                    if let Some(path) = file.path() {
                        let _ = std::fs::write(path, &data);
                    }
                }
            });
        });
        menu.remove(&stock);
        menu.insert(
            &webkit6::ContextMenuItem::from_gaction(&action, "Save Image As…", None),
            position as i32,
        );
        false // show the (edited) menu
    });

    webview.connect_decide_policy(|_view, decision, decision_type| {
        // Links (including ones inside sandboxed message iframes, and `_blank`
        // links that request a new window) open in the external browser.
        let is_nav = decision_type == webkit6::PolicyDecisionType::NavigationAction;
        let is_new_window = decision_type == webkit6::PolicyDecisionType::NewWindowAction;
        if is_nav || is_new_window {
            if let Some(nav) = decision.downcast_ref::<webkit6::NavigationPolicyDecision>() {
                if let Some(mut action) = nav.navigation_action() {
                    let clicked = is_new_window
                        || action.navigation_type() == webkit6::NavigationType::LinkClicked;
                    if clicked {
                        if let Some(uri) = action.request().and_then(|r| r.uri()) {
                            let _ = gtk::gio::AppInfo::launch_default_for_uri(
                                &uri,
                                None::<&gtk::gio::AppLaunchContext>,
                            );
                        }
                        decision.ignore();
                        return true;
                    }
                }
            }
        }
        false
    });

    // Show the target URL as a tooltip while hovering a link. WebKit doesn't do
    // this itself; we track the hovered link and answer GTK's query-tooltip,
    // re-querying whenever the hovered link changes so it updates immediately.
    let hovered: std::rc::Rc<std::cell::RefCell<Option<String>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    webview.set_has_tooltip(true);

    let hq = hovered.clone();
    webview.connect_query_tooltip(move |_view, _x, _y, _keyboard, tooltip| {
        match hq.borrow().as_deref() {
            Some(uri) => {
                tooltip.set_text(Some(uri));
                true
            }
            None => false,
        }
    });

    let hm = hovered.clone();
    webview.connect_mouse_target_changed(move |view, hit, _modifiers| {
        let uri = if hit.context_is_link() {
            hit.link_uri().map(|s| s.to_string())
        } else {
            None
        };
        if *hm.borrow() != uri {
            *hm.borrow_mut() = uri;
            view.trigger_tooltip_query();
        }
    });

    webview
}

/// What to show in the link preview: the destination, plus an explicit warning
/// when the link's visible text claims a different site than it goes to — the
/// oldest phishing trick there is (`click here: paypal.com` pointing elsewhere).
fn link_destination(uri: &str, label: Option<&str>) -> String {
    match label.and_then(|l| mismatched_host(uri, l)) {
        Some(claimed) => format!("{uri}  ⚠ looks like \"{claimed}\" but goes to {}", host_of(uri).unwrap_or_default()),
        None => uri.to_string(),
    }
}

/// The host a link's visible text claims, when that text is itself a URL or bare
/// hostname pointing somewhere other than the link's real target.
fn mismatched_host(uri: &str, label: &str) -> Option<String> {
    let claimed = host_of(label.trim())?;
    let actual = host_of(uri)?;
    // Compare from the right so `mail.example.com` matches `example.com`.
    let same = actual == claimed
        || actual.ends_with(&format!(".{claimed}"))
        || claimed.ends_with(&format!(".{actual}"));
    (!same).then_some(claimed)
}

/// The hostname of a URL, or of a bare hostname like `paypal.com/login`.
fn host_of(text: &str) -> Option<String> {
    let text = text.trim();
    // A scheme has no dots, which is what separates `mailto:` from a bare
    // `example.com:8080`. Only web links have a host worth comparing.
    let rest = match text.split_once(':') {
        Some((scheme, rest))
            if !scheme.is_empty()
                && !scheme.contains('.')
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-')) =>
        {
            if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
                return None;
            }
            rest.trim_start_matches("//")
        }
        _ => text,
    };
    let host = rest
        .split(['/', '?', '#'])
        .next()?
        .rsplit('@') // strip any userinfo
        .next()?
        .split(':')
        .next()?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    let looks_like_host = host.contains('.')
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    looks_like_host.then_some(host)
}

/// Split a `data:<mime>;base64,<payload>` URI into its MIME type and bytes.
/// Returns `None` for any other scheme, or for a `data:` URI that isn't base64
/// (the reader only ever emits base64 ones).
fn decode_data_uri(uri: &str) -> Option<(String, Vec<u8>)> {
    let rest = uri.strip_prefix("data:").or_else(|| uri.strip_prefix("DATA:"))?;
    let (meta, payload) = rest.split_once(',')?;
    let mime = meta.strip_suffix(";base64")?;
    let data = gtk::glib::base64_decode(payload);
    (!data.is_empty()).then(|| (mime.to_ascii_lowercase(), data))
}

/// A sensible filename to pre-fill the save dialog with. The original name isn't
/// recoverable from a `data:` URI — the properly named copy is in the attachment
/// drawer — so offer `image.<ext>` derived from the MIME type.
fn default_image_name(mime: &str) -> String {
    let ext = match mime {
        "image/jpeg" => "jpg",
        "image/svg+xml" => "svg",
        other => other.rsplit('/').next().unwrap_or("img"),
    };
    format!("image.{ext}")
}

/// Markup for the sender address as a clickable mailto link.
fn email_link_markup(m: Option<&Message>) -> String {
    match m {
        Some(m) if !m.from_addr.is_empty() => {
            let esc = gtk::glib::markup_escape_text(&m.from_addr);
            format!("<a href=\"mailto:{esc}\">{esc}</a>")
        }
        _ => String::new(),
    }
}

/// "Cc: a@b, c@d" for the header, or empty when there are no Cc recipients.
fn cc_line(m: Option<&Message>) -> String {
    match m {
        Some(m) if !m.cc.trim().is_empty() => format!("Cc: {}", m.cc.trim()),
        _ => String::new(),
    }
}

/// Neutralize remote resource references so nothing is fetched while blocked.
/// Targets resource-loading attributes only; `<a href>` links are left intact.
fn strip_remote(html: &str) -> String {
    html.replace("src=\"http", "src=\"blocked://")
        .replace("src='http", "src='blocked://")
        .replace("src=http", "src=blocked://")
        .replace("srcset=", "data-blocked-srcset=")
        .replace("background=\"http", "background=\"blocked://")
        .replace("url(http", "url(blocked://")
        .replace("url('http", "url('blocked://")
        .replace("url(\"http", "url(\"blocked://")
}

/// Inject a Content-Security-Policy `<meta>` into the document head as a second
/// line of defense. When remote content is disallowed only inline styles and
/// `data:` URIs are permitted.
fn inject_csp(html: &str, allow_remote: bool, dark: bool) -> String {
    let policy = if allow_remote {
        "default-src 'none'; img-src http: https: data: cid:; \
         style-src 'unsafe-inline' http: https: data:; \
         font-src http: https: data:; media-src http: https: data:"
    } else {
        "default-src 'none'; img-src data: cid:; style-src 'unsafe-inline' data:; \
         font-src data:; media-src data:"
    };
    let lower = html.to_ascii_lowercase();
    // An "unstyled" message brings no CSS of its own; give it comfortable padding
    // so text isn't flush against the edges. Styled emails keep their own layout.
    let unstyled = !lower.contains("<style") && !lower.contains("style=");
    let body_pad = if unstyled {
        // Reset the UA's default 8px body margin so content sits at exactly 16px
        // (which lines up with the conversation headers).
        "body{margin:0;padding:16px;box-sizing:border-box;}"
    } else {
        ""
    };
    // `color-scheme` makes the browser's default colours (for content that sets
    // none of its own) follow the app's light/dark setting; styled emails keep
    // their own colours untouched.
    let scheme = if dark { "dark" } else { "light" };
    let supported = if dark { "dark light" } else { "light dark" };
    let theme = format!(
        "<meta name=\"color-scheme\" content=\"{supported}\">\
         <style>:root{{color-scheme:{scheme};}}{body_pad}</style>"
    );
    // `no-referrer` keeps the synthetic `vireo.localhost` base URI from leaking as
    // a Referer/Origin header — both for privacy and because hotlink-protected
    // servers (e.g. some DreamHost sites) return 403 to foreign referrers, which
    // otherwise blocks legitimate images even once the sender is trusted.
    let meta = format!(
        "{theme}<meta name=\"referrer\" content=\"no-referrer\">\
         <meta http-equiv=\"Content-Security-Policy\" content=\"{policy}\">"
    );

    if let Some(head) = lower.find("<head") {
        if let Some(close) = html[head..].find('>') {
            let at = head + close + 1;
            return format!("{}{meta}{}", &html[..at], &html[at..]);
        }
    }
    if let Some(htmltag) = lower.find("<html") {
        if let Some(close) = html[htmltag..].find('>') {
            let at = htmltag + close + 1;
            return format!("{}<head>{meta}</head>{}", &html[..at], &html[at..]);
        }
    }
    format!("<!doctype html><html><head>{meta}</head><body>{html}</body></html>")
}

/// Heuristic: does the HTML reference remote (http/https) resources?
fn has_remote_resources(html: &str) -> bool {
    let h = html.to_ascii_lowercase();
    h.contains("src=\"http")
        || h.contains("src='http")
        || h.contains("src=http")
        || h.contains("srcset=")
        || h.contains("url(http")
        || h.contains("url('http")
        || h.contains("url(\"http")
        || h.contains("background=\"http")
        || (h.contains("<link") && h.contains("stylesheet") && h.contains("http"))
}

/// Wrapper-document script: size each message iframe to its content height so the
/// whole conversation scrolls as one page (the iframes have no inner scrollbars).
/// Re-measures as images load and as content reflows.
const SIZE_SCRIPT: &str = "\
function s(f){try{var d=f.contentDocument;if(!d)return;var b=d.body,e=d.documentElement;\
var h=Math.max(b?b.scrollHeight:0,e?e.scrollHeight:0,b?b.offsetHeight:0);if(h>0)f.style.height=h+'px';}catch(_){}}\
function init(f){s(f);try{var d=f.contentDocument;if(d){if(window.ResizeObserver&&d.body){new ResizeObserver(function(){s(f);}).observe(d.body);}\
var im=d.images||[];for(var i=0;i<im.length;i++){if(!im[i].complete){im[i].addEventListener('load',function(){s(f);});im[i].addEventListener('error',function(){s(f);});}}}}catch(_){}\
setTimeout(function(){s(f);},250);setTimeout(function(){s(f);},1000);}\
function all(){return document.querySelectorAll('iframe.vireo-frame');}\
document.addEventListener('DOMContentLoaded',function(){\
var fs=all();\
for(var i=0;i<fs.length;i++){(function(f){\
if(f.contentDocument&&f.contentDocument.readyState==='complete'){init(f);}\
f.addEventListener('load',function(){init(f);});})(fs[i]);}\
var hs=document.querySelectorAll('.vireo-msg-hdr');\
for(var j=0;j<hs.length;j++){hs[j].addEventListener('dblclick',function(){\
try{window.webkit.messageHandlers.vireo.postMessage(this.dataset.key);}catch(_){}});}});\
window.addEventListener('resize',function(){var fs=all();for(var i=0;i<fs.length;i++)s(fs[i]);});";

/// One message body as a sandboxed iframe: its own document (so CSS can't leak to
/// other messages) with no `allow-scripts` (so the email can't run JavaScript).
/// `allow-same-origin` lets the wrapper script measure its height.
fn message_frame(body: &str, blocked: bool, dark: bool) -> String {
    let doc = body_html(body);
    let doc = if blocked { strip_remote(&doc) } else { doc };
    let doc = inject_csp(&doc, !blocked, dark);
    format!(
        // `allow-same-origin` lets our wrapper script measure the frame height;
        // `allow-popups` lets `_blank` links reach the policy handler (which opens
        // them externally). No `allow-scripts`, so the email's own JS never runs.
        "<iframe class=\"vireo-frame\" sandbox=\"allow-same-origin allow-popups\" srcdoc=\"{}\"></iframe>",
        attr_escape(&doc)
    )
}

/// Escape a string for use inside a double-quoted HTML attribute (e.g. `srcdoc`).
fn attr_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;")
}

/// The worker stores ready-to-render HTML, but cached bodies from older versions
/// (or odd messages) may be tag-less plain text — wrap those so they read well.
fn body_html(body: &str) -> String {
    if body.contains('<') {
        body.to_string()
    } else {
        format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><style>\
             body{{margin:0;padding:16px;font:14px/1.5 system-ui,sans-serif;\
             white-space:pre-wrap;word-wrap:break-word}}\
             </style></head><body>{}</body></html>",
            body.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
        )
    }
}

/// A message subject reduced to something usable as a filename: no separators,
/// no leading dots, and short enough for any filesystem.
fn sanitize_filename(subject: &str) -> String {
    let cleaned: String = subject
        .chars()
        .map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { '-' } else { c })
        .collect();
    // Leading dots would make a hidden file; leading dashes are what a stripped
    // path separator leaves behind, and look like a command-line flag.
    let cleaned = cleaned.trim().trim_start_matches(['.', '-']).trim();
    if cleaned.is_empty() {
        return "Message".to_string();
    }
    cleaned.chars().take(120).collect::<String>().trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn print_filenames_survive_real_subjects() {
        // The subject names the print job and seeds the filename when printing to
        // a file, so it must not carry path separators or other characters a
        // filesystem would refuse.
        assert_eq!(sanitize_filename("Quarterly numbers"), "Quarterly numbers");
        assert_eq!(sanitize_filename("Invoice 3/4 <urgent>"), "Invoice 3-4 -urgent-");
        assert_eq!(sanitize_filename("../../etc/passwd"), "etc-passwd");
        // Nothing usable left, or nothing to begin with.
        assert_eq!(sanitize_filename("   "), "Message");
        assert_eq!(sanitize_filename(""), "Message");
        assert_eq!(sanitize_filename("..."), "Message");
    }

    #[test]
    fn print_filenames_are_bounded() {
        let long = "word ".repeat(100);
        assert!(sanitize_filename(&long).chars().count() <= 120);
    }

    #[test]
    fn decodes_a_base64_data_uri() {
        let (mime, data) = decode_data_uri("data:image/jpeg;base64,/9j/4AAQ").expect("decodes");
        assert_eq!(mime, "image/jpeg");
        assert_eq!(&data[..3], b"\xff\xd8\xff"); // JPEG magic
    }

    #[test]
    fn rejects_uris_it_cannot_save_locally() {
        // Remote images keep WebKit's own (download-backed) menu item.
        assert!(decode_data_uri("https://example.com/a.png").is_none());
        // Non-base64 `data:` URIs aren't something the reader emits.
        assert!(decode_data_uri("data:image/png,rawbytes").is_none());
        assert!(decode_data_uri("not a uri").is_none());
    }

    #[test]
    fn link_preview_shows_the_plain_url_when_nothing_is_amiss() {
        assert_eq!(
            link_destination("https://example.com/a", Some("Read more")),
            "https://example.com/a"
        );
        // Link text that matches where it goes is not a mismatch.
        assert_eq!(
            link_destination("https://example.com/a", Some("example.com")),
            "https://example.com/a"
        );
        // Subdomains belong to the same site.
        assert_eq!(
            link_destination("https://mail.example.com/a", Some("example.com")),
            "https://mail.example.com/a"
        );
    }

    #[test]
    fn link_preview_calls_out_text_claiming_another_site() {
        let shown = link_destination("https://evil.example/login", Some("https://paypal.com"));
        assert!(shown.contains("paypal.com"), "{shown}");
        assert!(shown.contains("evil.example"), "{shown}");
        assert!(shown.contains('⚠'), "{shown}");
    }

    #[test]
    fn ordinary_link_text_is_never_mistaken_for_a_host() {
        assert_eq!(host_of("Click here to sign in"), None);
        assert_eq!(host_of("mailto:someone@example.com"), None);
        assert_eq!(host_of("https://example.com/path"), Some("example.com".into()));
        assert_eq!(host_of("example.com/path"), Some("example.com".into()));
        // Userinfo can't be used to disguise the real host.
        assert_eq!(
            host_of("https://paypal.com@evil.example/"),
            Some("evil.example".into())
        );
    }

    #[test]
    fn suggests_a_filename_from_the_mime_type() {
        assert_eq!(default_image_name("image/jpeg"), "image.jpg");
        assert_eq!(default_image_name("image/png"), "image.png");
        assert_eq!(default_image_name("image/svg+xml"), "image.svg");
    }
}
