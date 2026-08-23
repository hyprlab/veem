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
    /// Whether the sender circle is drawn at all (#29).
    avatars: bool,
    /// Whether the sender's own site icon may fill it (#30).
    sender_logos: bool,
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
    /// Show or hide the sender circle (#29).
    SetAvatars(bool),
    /// Fill it with the sender's own site icon, or stop (#30).
    SetSenderLogos(bool),
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
    /// Render the message to a PDF and open it, so the layout can be checked
    /// before any paper is used.
    PrintPreview,
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
    type CommandOutput = (u32, Option<crate::ui::message_list::Face>);

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
                            set_visible: model.avatars,
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
            avatars: true,
            sender_logos: false,
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
                crate::ui::print_preview::print_html(
                    &self.print_document_html(),
                    &sanitize_filename(&self.job_name()),
                    self.webview.root().and_downcast::<gtk::Window>(),
                );
            }

            MessageViewInput::PrintPreview => {
                // Shown inside Vireo rather than exported to a PDF and handed to
                // whatever the desktop opens PDFs with: that route is a temporary
                // file, a URI, the document portal and an external viewer, each
                // able to fail without saying anything — and it did.
                let Some(parent) = self
                    .webview
                    .root()
                    .and_downcast::<adw::ApplicationWindow>()
                else {
                    tracing::warn!("no window to attach the preview to");
                    return;
                };
                let html = self.preview_html();
                crate::ui::print_preview::open(
                    &parent,
                    &html,
                    &sanitize_filename(&self.job_name()),
                );
            }

            MessageViewInput::ThemeChanged => {
                let dark = self.effective_dark();
                self.apply_webview_bg(dark);
                if self.current.is_some() && !self.loading {
                    self.render();
                }
            }
            MessageViewInput::SetAvatars(on) => {
                self.avatars = on;
            }
            MessageViewInput::SetSenderLogos(on) => {
                self.sender_logos = on;
                self.load_avatar(&sender);
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
        (message_id, face): Self::CommandOutput,
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
        let Some(m) = self.current.as_ref() else {
            return;
        };
        let addr = m.from_addr.clone();
        self.avatar_texture = match face {
            Some(crate::ui::message_list::Face::Gravatar(b)) => {
                crate::avatar::decode_and_cache(&addr, &b)
            }
            Some(crate::ui::message_list::Face::Logo(b)) => {
                crate::logo::decode_and_cache(&addr, &b)
            }
            None => {
                if self.sender_logos {
                    crate::logo::remember_missing(&addr);
                }
                None
            }
        };
    }
}

impl MessageView {
    /// Set the sender's face: a cached one if known, otherwise a background look
    /// keyed to this message — the sender's Gravatar, or the icon their domain
    /// publishes (#30), whichever is enabled and answers.
    fn load_avatar(&mut self, sender: &ComponentSender<Self>) {
        self.avatar_texture = None;
        // Nothing to fetch when the circle isn't drawn (#29).
        if !self.avatars {
            return;
        }
        let Some(m) = self.current.as_ref() else {
            return;
        };
        let email = m.from_addr.clone();
        if email.is_empty() {
            return;
        }
        if let Some(tex) = crate::avatar::cached(&email).or_else(|| crate::logo::cached(&email)) {
            self.avatar_texture = Some(tex);
            return;
        }
        let want_logo = self.sender_logos && !crate::logo::known_missing(&email);
        if !self.gravatar && !want_logo {
            return;
        }
        let id = m.id;
        let gravatar = self.gravatar;
        sender.oneshot_command(async move {
            (
                id,
                crate::ui::message_list::find_face(email, gravatar, want_logo).await,
            )
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

    /// What to call the print job — the subject, or something rather than nothing.
    fn job_name(&self) -> String {
        self.current
            .as_ref()
            .map(|m| m.subject.clone())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "Message".to_string())
    }

    /// The document that gets printed: the header, then each message inlined
    /// into the page.
    ///
    /// Not the reader's document. That one puts every message in a sandboxed
    /// iframe, which is right on screen — an email's CSS cannot escape it — but
    /// wrong on paper, where a print engine draws the frame at its on-screen size
    /// with its scrollbars and clips the rest.
    fn print_document_html(&self) -> String {
        // In a conversation the top header describes the first message only, so
        // each one says who sent it and when — as the reader's own per-message
        // headers do on screen.
        let conversation = self.thread.len() > 1;
        let messages: Vec<(String, String)> = self
            .thread
            .iter()
            .map(|m| {
                let doc = body_html(&m.body);
                let doc = if self.blocked { strip_remote(&doc) } else { doc };
                let head = if conversation {
                    print_message_header_html(m)
                } else {
                    String::new()
                };
                (head, doc)
            })
            .collect();
        print_document(&self.print_header_html(), &messages, !self.blocked)
    }

    /// That same document, dressed as a page for the preview window.
    fn preview_html(&self) -> String {
        let doc = self.print_document_html();
        let extra = format!(
            "<style>{}</style></head>",
            crate::ui::print_preview::PREVIEW_STYLES
        );
        let doc = doc.replacen("</head>", &extra, 1);
        // Wrap the content in the sheet the styles above draw.
        doc.replacen("<body>", "<body><div class=\"vireo-print-sheet\">", 1)
            .replacen("</body>", "</div></body>", 1)
    }

    /// The header block that only appears on paper (see [`print_header_html`]).
    fn print_header_html(&self) -> String {
        print_header_html(self.thread.first().or(self.current.as_ref()))
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
/// A web view for the print preview: same sandboxing as the reader's, since it
/// shows the same message.
pub fn new_preview_webview() -> webkit6::WebView {
    new_webview()
}

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
         <style>:root{{color-scheme:{scheme};}}{body_pad}\
         @media print{{:root{{color-scheme:light;}}html,body{{background:#fff !important;}}}}\
         </style>"
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

/// Remove a document's structural tags, keeping everything inside them.
///
/// Printing cannot use the reader's sandboxed iframes: a print engine draws an
/// iframe at its on-screen size — scrollbars included — and clips whatever does
/// not fit, so a long message came out as one cropped page. Inlining each
/// message into the printed document instead lets it flow across pages.
///
/// `<style>` and `<meta>` are deliberately kept: a message's own CSS is what
/// makes it look like itself on paper too.
fn inline_body(doc: &str) -> String {
    let mut out = String::with_capacity(doc.len());
    let lower = doc.to_ascii_lowercase();
    let mut i = 0;
    while i < doc.len() {
        if lower[i..].starts_with("<!doctype") {
            match doc[i..].find('>') {
                Some(end) => {
                    i += end + 1;
                    continue;
                }
                None => break,
            }
        }
        let structural = ["<html", "</html", "<head", "</head", "<body", "</body"];
        if let Some(tag) = structural.iter().find(|t| lower[i..].starts_with(**t)) {
            // Only a real tag: "<bodyguard" is content, "<body class=…" is not.
            let after = i + tag.len();
            let next = doc.as_bytes().get(after).copied().unwrap_or(b'>');
            if next == b'>' || next.is_ascii_whitespace() || next == b'/' {
                match doc[i..].find('>') {
                    Some(end) => {
                        i += end + 1;
                        continue;
                    }
                    None => break,
                }
            }
        }
        let ch = doc[i..].chars().next().unwrap_or('\u{fffd}');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Point a message's own `html`/`body` rules at the block it prints in.
///
/// Inlining costs the isolation an iframe gave: with every message in one
/// document, one sender's `body{font-family:monospace}` would restyle the whole
/// printout, including the next message in the conversation. Redirecting those
/// selectors to `.vireo-print-msg` keeps the rule working where it is meant to
/// and nowhere else. Bare type selectors (`p`, `a`) still reach across, which is
/// the remaining price of printing a thread as one page.
fn scope_styles(doc: &str, block: &str) -> String {
    let lower = doc.to_ascii_lowercase();
    let mut out = String::with_capacity(doc.len());
    let mut rest = 0;
    while let Some(open) = lower[rest..].find("<style").map(|i| i + rest) {
        let Some(body_start) = lower[open..].find('>').map(|i| open + i + 1) else {
            break;
        };
        let end = lower[body_start..]
            .find("</style")
            .map(|i| body_start + i)
            .unwrap_or(doc.len());
        out.push_str(&doc[rest..body_start]);
        out.push_str(&scope_css(&doc[body_start..end], block));
        rest = end;
    }
    out.push_str(&doc[rest..]);
    out
}

/// Rewrite `html`/`body` in every selector of a stylesheet.
fn scope_css(css: &str, block: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut prelude = String::new();
    // What each open brace holds: rules (an at-rule such as `@media`) or
    // declarations. Only the former contains further selectors.
    let mut stack: Vec<bool> = Vec::new();
    let in_rules = |stack: &Vec<bool>| stack.last().copied().unwrap_or(true);
    for ch in css.chars() {
        match ch {
            '{' => {
                let holds_rules = prelude.trim_start().starts_with('@');
                if in_rules(&stack) {
                    out.push_str(&scope_selectors(&prelude, block));
                } else {
                    out.push_str(&prelude);
                }
                prelude.clear();
                stack.push(holds_rules);
                out.push('{');
            }
            '}' => {
                out.push_str(&prelude);
                prelude.clear();
                stack.pop();
                out.push('}');
            }
            _ => prelude.push(ch),
        }
    }
    out.push_str(&prelude);
    out
}

/// Replace whole-word `html`/`body` in a selector list with the message's block.
fn scope_selectors(prelude: &str, block: &str) -> String {
    let bytes = prelude.as_bytes();
    let mut out = String::with_capacity(prelude.len());
    let mut i = 0;
    while i < prelude.len() {
        let word = ["html", "body"]
            .into_iter()
            .find(|w| prelude[i..].to_ascii_lowercase().starts_with(w));
        // A tag name, not part of `.body`, `#body` or `bodyguard`.
        let boundary_before = i == 0 || !matches!(bytes[i - 1], b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'#' | b'%');
        if let Some(word) = word {
            let after = bytes.get(i + word.len()).copied();
            let boundary_after = !matches!(after, Some(b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_'));
            if boundary_before && boundary_after {
                out.push_str(block);
                i += word.len();
                continue;
            }
        }
        let ch = prelude[i..].chars().next().unwrap_or('\u{fffd}');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Rules for the printed document, which is a plain page rather than a stack of
/// frames: long URLs wrap instead of running off the sheet, images shrink to the
/// page, and each message after the first starts a new one.
const PRINT_DOCUMENT_STYLES: &str = "\
    html,body{background:#fff;color:#000;margin:0;padding:0;}\
    body{font:11pt/1.5 system-ui,sans-serif;overflow-wrap:anywhere;}\
    img,table,pre{max-width:100% !important;}\
    img{height:auto !important;}\
    pre{white-space:pre-wrap;}\
    .vireo-print-msg + .vireo-print-msg{border-top:1pt solid #999;margin-top:12pt;padding-top:12pt;}\
    .vireo-print-msghdr{font:bold 10pt/1.45 system-ui,sans-serif;color:#000;margin:0 0 8pt;}\
    .vireo-print-hdr{display:block;padding:0 0 10pt;margin:0 0 12pt;\
      border-bottom:1pt solid #999;font:10pt/1.45 system-ui,sans-serif;color:#000;}\
    .vireo-print-subject{font-size:14pt;font-weight:700;margin:0 0 8pt;}\
    .vireo-print-row{margin:0 0 2pt;}\
    .vireo-print-label{font-weight:700;}";

/// The header block that only appears on paper: subject, who it is from and to,
/// and when.
///
/// On screen these facts are in the pane above the message, which is a GTK widget
/// and therefore cannot be printed — WebKit prints the document it is showing,
/// and that document is the body alone. Printed mail without a sender or a date
/// is close to useless (issue #16), so the same facts go into the document and
/// are hidden with `@media`.
/// Assemble the printed page: the header, then every message inlined into it.
fn print_document(
    header: &str,
    messages: &[(String, String)],
    allow_remote: bool,
) -> String {
    let mut body = header.to_string();
    for (n, (head, doc)) in messages.iter().enumerate() {
        let block = format!("vireo-print-m{n}");
        body.push_str(&format!("<article class=\"vireo-print-msg {block}\">"));
        body.push_str(head);
        body.push_str(&scope_styles(&inline_body(doc), &format!(".{block}")));
        body.push_str("</article>");
    }
    // The same content policy the reader's frames get: no scripts, and remote
    // content only when the sender is trusted.
    inject_csp(
        &format!(
            "<!doctype html><html><head><meta charset=\"utf-8\">\
             <style>{PRINT_DOCUMENT_STYLES}</style></head><body>{body}</body></html>"
        ),
        allow_remote,
        false,
    )
}

/// Who sent one message of a printed conversation, and when.
fn print_message_header_html(m: &Message) -> String {
    let from = if m.from_addr.is_empty() {
        attr_escape(&m.from_name)
    } else if m.from_name.trim().is_empty() {
        attr_escape(&m.from_addr)
    } else {
        format!(
            "{} &lt;{}&gt;",
            attr_escape(&m.from_name),
            attr_escape(&m.from_addr)
        )
    };
    format!(
        "<div class=\"vireo-print-msghdr\">{from} — {date}</div>",
        date = attr_escape(&m.datetime_full())
    )
}

fn print_header_html(message: Option<&Message>) -> String {
    // Newest first, so a conversation is described by the message on top.
    let Some(m) = message else {
        return String::new();
    };
    let row = |label: &str, value: &str| -> String {
        if value.trim().is_empty() {
            return String::new();
        }
        format!(
            "<div class=\"vireo-print-row\"><span class=\"vireo-print-label\">{}</span> {}</div>",
            escape_text(label),
            escape_text(value)
        )
    };
    let from = if m.from_addr.trim().is_empty() {
        m.from_name.clone()
    } else if m.from_name.trim().is_empty() || m.from_name == m.from_addr {
        m.from_addr.clone()
    } else {
        format!("{} <{}>", m.from_name, m.from_addr)
    };
    let subject = if m.subject.trim().is_empty() {
        "(no subject)".to_string()
    } else {
        m.subject.clone()
    };
    format!(
        "<div class=\"vireo-print-hdr\">\
           <div class=\"vireo-print-subject\">{subject}</div>{from}{to}{cc}{date}\
         </div>",
        subject = escape_text(&subject),
        from = row("From:", &from),
        to = row("To:", &m.to),
        cc = row("Cc:", &m.cc),
        date = row("Date:", &m.datetime_full()),
    )
}

/// Escape text for HTML content: a subject or an address that contains `<` must
/// not become a tag.
fn escape_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
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

    fn msg_for_print() -> Message {
        Message {
            id: 1,
            account_id: 1,
            folder_id: 1,
            uid: 1,
            from_name: "Ada Lovelace".into(),
            from_addr: "ada@example.com".into(),
            to: "me@example.com".into(),
            cc: "carol@example.com".into(),
            subject: "Quarterly numbers".into(),
            preview: String::new(),
            body: "<p>hi</p>".into(),
            date: "09:14".into(),
            timestamp: 0,
            unread: false,
            starred: false,
            has_attachment: false,
            message_id: String::new(),
            references: String::new(),
        }
    }

    #[test]
    fn the_printed_page_carries_the_header() {
        // On screen these facts live in a GTK pane, which WebKit cannot print —
        // so they have to be in the document itself (issue #16).
        let m = msg_for_print();
        let header = print_header_html(Some(&m));
        assert!(header.contains("Quarterly numbers"), "{header}");
        assert!(header.contains("Ada Lovelace &lt;ada@example.com&gt;"), "{header}");
        assert!(header.contains("me@example.com"), "{header}");
        assert!(header.contains("carol@example.com"), "{header}");
        assert!(header.contains("From:") && header.contains("To:") && header.contains("Date:"));
    }

    #[test]
    fn the_printed_header_is_text_not_markup() {
        let mut m = msg_for_print();
        m.subject = "<script>alert(1)</script>".into();
        m.cc = String::new();
        let header = print_header_html(Some(&m));
        assert!(!header.contains("<script>"), "{header}");
        assert!(header.contains("&lt;script&gt;"), "{header}");
        // An empty field is left out rather than printed as a blank line.
        assert!(!header.contains("Cc:"), "{header}");
        // No message at all: nothing to describe.
        assert_eq!(print_header_html(None), "");
    }

    #[test]
    fn printing_inlines_the_message_instead_of_framing_it() {
        // The bug: printed pages carried the reader's iframe scrollbars and were
        // cut off at the frame's on-screen height, because a print engine draws an
        // iframe as a box rather than paginating what is inside it.
        let doc = print_document(
            "<div class=\"vireo-print-hdr\">HEADER</div>",
            &[(
                String::new(),
                "<!DOCTYPE html><html><head><style>p{color:red}</style></head>\
                 <body class=\"x\"><p>Hello</p></body></html>"
                    .to_string(),
            )],
            false,
        );
        assert!(!doc.contains("<iframe"), "{doc}");
        assert!(doc.contains("HEADER"));
        assert!(doc.contains("<p>Hello</p>"));
        // The message keeps its own styling on paper.
        assert!(doc.contains("p{color:red}"));
        // Long URLs wrap rather than running off the sheet, and wide content is
        // scaled down instead of being clipped.
        assert!(doc.contains("overflow-wrap:anywhere"));
        assert!(doc.contains("img,table,pre{max-width:100% !important;}"));
        // Still no scripts, and blocked remote content stays blocked.
        assert!(doc.contains("Content-Security-Policy"));
    }

    #[test]
    fn inlining_keeps_everything_but_the_structure() {
        assert_eq!(
            inline_body("<!doctype html><html><head><meta charset=\"utf-8\"></head><body id=\"a\">hi</body></html>"),
            "<meta charset=\"utf-8\">hi"
        );
        // A tag that merely starts like one of them is content, not structure.
        assert_eq!(inline_body("<bodyguard>x</bodyguard>"), "<bodyguard>x</bodyguard>");
        assert_eq!(inline_body("<p>plain fragment</p>"), "<p>plain fragment</p>");
        // Unterminated tags must not swallow the rest of the message.
        assert_eq!(inline_body("text < 5 and > 2"), "text < 5 and > 2");
    }

    #[test]
    fn one_message_cannot_restyle_the_whole_printout() {
        // A conversation prints as one document, so a sender's body rules have to
        // land on that sender's block and nothing else.
        let out = scope_styles(
            "<style>body{font-family:monospace}\
             @media print{html,body>p{color:red}}\
             .bodyguard{margin:0}</style><p>hi</p>",
            ".vireo-print-m1",
        );
        assert!(out.contains(".vireo-print-m1{font-family:monospace}"), "{out}");
        assert!(
            out.contains("@media print{.vireo-print-m1,.vireo-print-m1>p{color:red}}"),
            "{out}"
        );
        // Only whole tag names: a class that merely contains "body" is untouched,
        // and so are declarations that mention one.
        assert!(out.contains(".bodyguard{margin:0}"), "{out}");
        assert!(out.contains("<p>hi</p>"));
        assert!(!scope_styles("<p>body{x}</p>", ".vireo-print-m0").contains("vireo-print-m0"));
        // Each message gets a class of its own, so the first sender's rules stop
        // at the first message.
        let doc = print_document(
            "",
            &[
                (String::new(), "<style>body{color:red}</style>one".into()),
                (String::new(), "two".into()),
            ],
            false,
        );
        assert!(doc.contains(".vireo-print-m0{color:red}"), "{doc}");
        assert!(doc.contains("vireo-print-msg vireo-print-m1"), "{doc}");
    }

    #[test]
    fn a_printed_conversation_says_who_sent_what() {
        let mut a = msg_for_print();
        a.from_name = "Alfonso".into();
        a.from_addr = "a@example.com".into();
        let head = print_message_header_html(&a);
        assert!(head.contains("Alfonso &lt;a@example.com&gt;"), "{head}");
        assert!(head.contains(&attr_escape(&a.datetime_full())));
    }

    #[test]
    fn a_preview_uri_is_escaped() {
        // Subjects become filenames, and mail subjects are full of spaces and
        // brackets: "file://" + path is not a URI, and GIO opens nothing.
        let path = std::path::Path::new("/tmp/vireo-print/[hyprlab] Sync 1.11.0.pdf");
        let uri = gtk::gio::File::for_path(path).uri().to_string();
        assert!(!uri.contains(' '), "{uri}");
        assert!(uri.contains("%20"), "{uri}");
        assert!(uri.starts_with("file:///tmp/vireo-print/"), "{uri}");
    }

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
