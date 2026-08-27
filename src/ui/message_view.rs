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
    /// Which of the conversation's messages came from another folder, and what to
    /// call that folder.
    folder_labels: std::collections::HashMap<(u32, u32), String>,
    /// Remote content was detected and is currently withheld — this drives the
    /// "blocked" banner, and nothing else.
    blocked: bool,
    /// Messages the user deliberately marked unread while this conversation was
    /// on screen. They keep their mark but get no scroll sentinel, so simply
    /// having them in view can't undo the thing the user just asked for. Cleared
    /// whenever a conversation is opened afresh.
    no_autoread: std::collections::HashSet<(u32, u32)>,
    /// Whether the blocked-content banner is shown at all. It gates only the notice: `blocked`
    /// still governs what is withheld, so hiding it never loads anything.
    show_banner: bool,
    /// Whether the user has actually permitted remote content for what is on
    /// screen (settings, "Load once", or "Always allow sender").
    ///
    /// Kept separate from `blocked` on purpose. `blocked` depends on a detector
    /// guessing whether a message references remote resources; this does not.
    /// Stripping and the CSP key off *this*, so a detector miss costs a banner
    /// rather than the protection itself.
    remote_allowed: bool,
    /// Owning account's display name (header chip).
    account_name: Option<String>,
    /// Provider holding the header chip's per-account colours.
    chip_provider: gtk::CssProvider,
    /// Paints the reader's spinner and its inter-document cover in the *message*
    /// theme rather than the app's. Reading a light message in a dark app used
    /// to mean a dark spinner giving way to a white page.
    cover_provider: gtk::CssProvider,
    /// Fingerprint of what the WebView is currently showing. Re-selecting a
    /// message that renders to the same document skips the load entirely —
    /// every load blanks the view for an instant, however briefly.
    shown_fingerprint: Option<u64>,
    /// What each message's frame measured last time it was shown, so reopening a
    /// conversation lays out right away instead of settling into place.
    frame_heights: std::collections::HashMap<(u32, u32), u32>,
    /// This conversation came ready-made, so there is nothing to cover.
    instant: bool,
    /// The messages the list has selected, outlined in the reader.
    selected_cards: Vec<(u32, u32)>,
    /// True while the body is being fetched (show a spinner instead).
    loading: bool,
    /// False from when a render starts until the WebView reports it finished
    /// loading — a themed cover hides the WebView's white inter-document gap.
    webview_ready: bool,
    /// Which message the document currently on screen was rendered for, so an
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
    /// Whether the blocked-remote-content banner is shown. It doesn't change
    /// what is blocked — only what the reader says about it.
    SetBannerShown(bool),
    Show {
        /// The conversation, newest first. A single message for a normal open;
        /// several for a threaded conversation.
        thread: Vec<Message>,
        /// The sender is trusted, so remote content may auto-load.
        allow_remote: bool,
        /// Owning account's display name and colour, for the header chip.
        account_name: Option<String>,
        account_color: Option<String>,
        /// The body is still being fetched — show a spinner.
        loading: bool,
        /// The message the user actually selected, which drives the header,
        /// avatar and sender check. Without it the newest message would, and a
        /// conversation can now open with a reply of your own on top (#21).
        /// Boxed: a `Message` inline here would double the size of every message
        /// this component sends.
        primary: Option<Box<Message>>,
        /// The app already had this conversation assembled, so the reader has
        /// nothing to wait for: it swaps the document in place rather than
        /// covering the view, which is what put a spinner on every return to a
        /// thread it had shown minutes before.
        instant: bool,
        /// Labels for conversation messages that live in another folder, keyed by
        /// (account, message id) — "Sent" beside a reply of yours read from the
        /// Inbox. Messages from the folder on screen aren't in here.
        folder_labels: std::collections::HashMap<(u32, u32), String>,
    },
    LoadRemoteOnce,
    AllowSenderAlways,
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
    /// The user marked this conversation message unread: keep the mark until the
    /// conversation is opened again, rather than clearing it the moment the
    /// message happens to be in view.
    SuppressAutoRead { account_id: u32, id: u32 },
    /// The page around the cards was clicked — nothing is selected any more.
    ClearCards,
    /// A card was clicked, with whatever modifier was held.
    CardClicked { account_id: u32, id: u32, mode: SelectMode },
    /// Which messages the list has selected. Drawn as an accent outline on the
    /// matching cards, applied to the live document rather than by rendering it
    /// again — a re-render would lose the reader's scroll position.
    SetSelectedCards(Vec<(u32, u32)>),
    /// A message's frame measured this tall, so reopening it can lay out at that
    /// height instead of settling into it.
    FrameSized { account_id: u32, id: u32, height: u32 },
    /// One conversation message has been scrolled all the way through, so it
    /// has been read.
    MarkSeen { account_id: u32, id: u32 },
    /// Reply / Reply all / Forward chosen on one card's header, so the action
    /// applies to that message rather than to whichever the reader calls primary.
    CardAction {
        action: crate::ui::message_list::RowAction,
        account_id: u32,
        id: u32,
    },
}

/// How a click on a conversation card changes the selection, mirroring what the
/// message list does with the same modifiers.
#[derive(Debug, Clone, Copy)]
pub enum SelectMode {
    /// Plain click: this message alone.
    Plain,
    /// Ctrl: add or remove this one, leaving the rest.
    Toggle,
    /// Shift: everything between the last selection and this one.
    Range,
}

#[derive(Debug)]
pub enum MessageViewOutput {
    /// Add this sender address to the remote-content allowlist.
    AllowSender(String),
    /// Open a conversation message in its own window (header double-clicked).
    OpenWindow(Box<Message>),
    /// The reader's selection changed. It owns this: a conversation can hold
    /// messages the list has no row for — a reply of yours read in from Sent —
    /// and those must still be selectable. The list mirrors what it can.
    SelectCards(Vec<(u32, u32)>),
    /// A conversation message has been read (scrolled through). The reader has
    /// already dropped its mark; the app makes it stick.
    MarkSeen { account_id: u32, id: u32 },
    /// An action chosen on one message's card in a conversation.
    CardAction {
        action: crate::ui::message_list::RowAction,
        message: Box<Message>,
    },
}

#[relm4::component(pub)]
impl Component for MessageView {
    type Init = ();
    type Input = MessageViewInput;
    type Output = MessageViewOutput;
    type CommandOutput = ();

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
                    set_reveal_child: model.blocked && model.show_banner,

                    gtk::Box {
                        add_css_class: "remote-alert",
                        set_spacing: 8,

                        gtk::Image { set_icon_name: Some("co.hyprlab.Vireo-security-high-symbolic") },
                        gtk::Label {
                            set_label: "Remote content (images, trackers) is blocked to protect your privacy.",
                            set_hexpand: true,
                            set_halign: gtk::Align::Start,
                            set_wrap: true,
                            // Ask for the full single-line width and wrap only
                            // when the pane really is too narrow — without this
                            // a wrapping label requests minimal width and folds
                            // to two lines even with room to spare.
                            set_natural_wrap_mode: gtk::NaturalWrapMode::None,
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

                },


                #[name = "body_stack"]
                gtk::Stack {
                    // The message's ground, painted on the stack itself so every
                    // page sits on it — the spinner box is centred and paints
                    // only its own few square inches.
                    add_css_class: "reader-cover",
                    // Both pages sit on that same ground, so a short dissolve
                    // between them reads as the message arriving rather than as
                    // the hard cut a stack does by default.
                    set_transition_type: gtk::StackTransitionType::Crossfade,
                    set_transition_duration: 120,
                    set_vexpand: true,
                    #[watch]
                    set_visible_child_name: model.body_page(),

                    add_named[Some("loading")] = &gtk::Box {
                        add_css_class: "reader-loading",
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
                            // Dimming comes from the cover's own foreground, which
                            // is picked for the message theme — `dim-label` would
                            // fade it against the app's instead.
                            set_label: "Loading…",
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
        let cover_provider = gtk::CssProvider::new();
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &chip_provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
            gtk::style_context_add_provider_for_display(
                &display,
                &cover_provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
        let model = MessageView {
            current: None,
            thread: Vec::new(),
            folder_labels: std::collections::HashMap::new(),
            blocked: false,
            no_autoread: std::collections::HashSet::new(),
            show_banner: crate::config::load_show_remote_banner(),
            remote_allowed: false,
            account_name: None,
            chip_provider,
            cover_provider,
            shown_fingerprint: None,
            frame_heights: std::collections::HashMap::new(),
            instant: false,
            selected_cards: Vec::new(),
            loading: false,
            webview_ready: false,
            webview,
            sender_check: None,
            link_preview: link_preview.clone(),
            seq: std::cell::Cell::new(0),
            content_dark: None,
        };

        // The document being loaded is not the same as it being ready to look at:
        // each message body is a frame that loads afterwards and is then sized
        // from its placeholder height, so revealing here shows every card at the
        // wrong size and then jumping. The page says when its frames have
        // settled; this is only the backstop for a page that never does.
        let ready_sender = sender.clone();
        model.webview.connect_load_changed(move |_view, event| {
            if event == webkit6::LoadEvent::Finished {
                let s = ready_sender.clone();
                gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(600), move || {
                    s.input(MessageViewInput::Rendered);
                });
            }
        });

        // Double-click on a conversation header → open that message's window.
        if let Some(ucm) = model.webview.user_content_manager() {
            let open_sender = sender.clone();
            ucm.connect_script_message_received(Some("vireo"), move |_ucm, value| {
                // "verb:account:id" — the page only ever posts what this document
                // put there, but it is still parsed strictly.
                use crate::ui::message_list::RowAction;
                let msg = value.to_str().to_string();
                let mut parts = msg.splitn(4, ':');
                let (Some(verb), Some(a), Some(i)) =
                    (parts.next(), parts.next(), parts.next())
                else {
                    return;
                };
                let extra = parts.next();
                let (Ok(account_id), Ok(id)) = (a.parse::<u32>(), i.parse::<u32>()) else {
                    return;
                };
                match verb {
                    // Every message frame has loaded and been sized: the layout
                    // has settled, so there is something worth revealing.
                    "ready" => open_sender.input(MessageViewInput::Rendered),
                    "desel" => open_sender.input(MessageViewInput::ClearCards),
                    "sel" => {
                        let mode = match extra {
                            Some("t") => SelectMode::Toggle,
                            Some("r") => SelectMode::Range,
                            _ => SelectMode::Plain,
                        };
                        open_sender.input(MessageViewInput::CardClicked {
                            account_id,
                            id,
                            mode,
                        });
                    }
                    "size" => {
                        if let Some(Ok(height)) = extra.map(|h| h.parse::<u32>()) {
                            open_sender.input(MessageViewInput::FrameSized {
                                account_id,
                                id,
                                height,
                            });
                        }
                    }
                    "open" => open_sender.input(MessageViewInput::OpenHeader { account_id, id }),
                    "seen" => open_sender.input(MessageViewInput::MarkSeen { account_id, id }),
                    "reply" => open_sender.input(MessageViewInput::CardAction {
                        action: RowAction::Reply,
                        account_id,
                        id,
                    }),
                    "replyall" => open_sender.input(MessageViewInput::CardAction {
                        action: RowAction::ReplyAll,
                        account_id,
                        id,
                    }),
                    "forward" => open_sender.input(MessageViewInput::CardAction {
                        action: RowAction::Forward,
                        account_id,
                        id,
                    }),
                    _ => {}
                }
            });
        }

        // Re-render the body when the light/dark preference changes so unstyled
        // content tracks the theme live.
        let style_manager = adw::StyleManager::default();
        model.apply_webview_bg(model.effective_dark());
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
                account_name,
                account_color,
                loading,
                primary,
                folder_labels,
                instant,
            } => {
                let shown = primary.map(|p| *p).or_else(|| thread.first().cloned());
                // A new message: the previous message's verdict must not linger
                // on screen while this one's is still being fetched.
                let same_message = self.current.as_ref().zip(shown.as_ref()).is_some_and(
                    |(a, b)| a.id == b.id && a.account_id == b.account_id,
                );
                if !same_message {
                    self.sender_check = None;
                }
                self.link_preview.set_visible(false);
                self.current = shown;
                // A fresh open: anything the user marked unread earlier can be
                // read by scrolling again.
                if !same_message {
                    self.no_autoread.clear();
                }
                self.thread = thread;
                self.folder_labels = folder_labels;
                self.account_name = account_name;
                self.loading = loading;
                self.instant = instant;
                if let Some(color) = &account_color {
                    let css = format!(
                        ".vireo-account-chip {{ background-color: {}; color: {}; }}",
                        crate::color::pale(color, 0.18),
                        color
                    );
                    self.chip_provider.load_from_data(&css);
                }
                self.remote_allowed = allow_remote;
                let has_remote = self
                    .thread
                    .iter()
                    .any(|m| has_remote_resources(&m.body));
                self.blocked = has_remote && !allow_remote;
                // Repaint the cover for what is arriving, even when the spinner
                // is about to be shown instead of a document: the whole point is
                // that the spinner already sits on the right colour.
                self.apply_webview_bg(self.effective_dark());
                // While loading, the spinner page is shown; rendering the (empty)
                // body would just flash blank, so wait for the real body.
                if !self.loading {
                    self.render();
                }
            }
            MessageViewInput::LoadRemoteOnce => {
                self.remote_allowed = true;
                self.blocked = false;
                self.render();
            }
            MessageViewInput::AllowSenderAlways => {
                if let Some(m) = &self.current {
                    let _ = sender.output(MessageViewOutput::AllowSender(m.from_addr.clone()));
                }
                self.remote_allowed = true;
                self.blocked = false;
                self.render();
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
            MessageViewInput::SetBannerShown(show) => {
                self.show_banner = show;
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
            MessageViewInput::SuppressAutoRead { account_id, id } => {
                self.no_autoread.insert((account_id, id));
                for m in self.thread.iter_mut() {
                    if m.account_id == account_id && m.id == id {
                        m.unread = true;
                    }
                }
                if !self.loading {
                    self.render();
                }
            }
            MessageViewInput::ClearCards => {
                if !self.selected_cards.is_empty() {
                    self.selected_cards.clear();
                    self.apply_card_selection();
                    let _ = sender.output(MessageViewOutput::SelectCards(Vec::new()));
                }
            }
            MessageViewInput::CardClicked { account_id, id, mode } => {
                // Only meaningful in a conversation: a lone message is already
                // the selection.
                if self.thread.len() <= 1 {
                    return;
                }
                let key = (account_id, id);
                let order: Vec<(u32, u32)> =
                    self.thread.iter().map(|m| (m.account_id, m.id)).collect();
                let mut keys = match mode {
                    SelectMode::Plain => vec![key],
                    SelectMode::Toggle => {
                        let mut k = self.selected_cards.clone();
                        if let Some(pos) = k.iter().position(|x| *x == key) {
                            k.remove(pos);
                        } else {
                            k.push(key);
                        }
                        k
                    }
                    SelectMode::Range => {
                        // From the first thing already selected to this one, in
                        // the order the conversation is shown.
                        let anchor = self
                            .selected_cards
                            .first()
                            .and_then(|a| order.iter().position(|x| x == a))
                            .unwrap_or_else(|| {
                                order.iter().position(|x| *x == key).unwrap_or(0)
                            });
                        let here = order.iter().position(|x| *x == key).unwrap_or(anchor);
                        let (lo, hi) = (anchor.min(here), anchor.max(here));
                        order[lo..=hi].to_vec()
                    }
                };
                keys.retain(|k| order.contains(k));
                self.selected_cards = keys.clone();
                self.apply_card_selection();
                let _ = sender.output(MessageViewOutput::SelectCards(keys));
            }
            MessageViewInput::SetSelectedCards(keys) => {
                // The list mirrors every selection change here — including the
                // plain single "this row is open" state that comes with merely
                // opening a message or thread, which must not outline anything.
                // A lone mirrored key is therefore dropped: the accent border
                // appears only for a deliberate selection — a card header
                // clicked in the reader (CardClicked, which never routes
                // through here), or a multi-selection made in the list.
                let keys = if keys.len() == 1 { Vec::new() } else { keys };
                if self.selected_cards != keys {
                    self.selected_cards = keys;
                    self.apply_card_selection();
                }
            }
            MessageViewInput::FrameSized { account_id, id, height } => {
                // A few hundred numbers at most, and only for messages actually
                // opened; the store is trimmed rather than allowed to creep.
                if self.frame_heights.len() > 512 {
                    self.frame_heights.clear();
                }
                self.frame_heights.insert((account_id, id), height);
            }
            MessageViewInput::MarkSeen { account_id, id } => {
                // Keep the local copy in step so a later re-render doesn't put
                // the mark back on a message already read.
                let mut found = false;
                for m in self.thread.iter_mut() {
                    if m.account_id == account_id && m.id == id && m.unread {
                        m.unread = false;
                        found = true;
                    }
                }
                if found {
                    let _ = sender.output(MessageViewOutput::MarkSeen { account_id, id });
                }
            }
            MessageViewInput::CardAction { action, account_id, id } => {
                if let Some(m) = self
                    .thread
                    .iter()
                    .find(|m| m.account_id == account_id && m.id == id)
                {
                    let _ = sender.output(MessageViewOutput::CardAction {
                        action,
                        message: Box::new(m.clone()),
                    });
                }
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

}

impl MessageView {
    /// The desktop accent colour, as the document can use it. Read from the
    /// widget's style so it follows the user's choice; GNOME's own blue is the
    /// fallback when the theme doesn't define it.
    fn accent_hex(&self) -> String {
        #[allow(deprecated)]
        self.webview
            .style_context()
            .lookup_color("accent_bg_color")
            .map(|c| crate::color::to_hex(&c))
            .unwrap_or_else(|| "#3584e4".to_string())
    }

    /// Outline the selected cards in the document already on screen. Rendering
    /// it again would reload every frame and lose the reader's place.
    fn apply_card_selection(&self) {
        // A lone card never wears the outline (see `conversation_document`).
        if self.thread.len() <= 1 {
            return;
        }
        let keys: Vec<String> = self
            .selected_cards
            .iter()
            .map(|(a, i)| format!("'{a}:{i}'"))
            .collect();
        let js = format!(
            "(function(){{var s=[{}];             var els=document.querySelectorAll('.vireo-msg');             for(var i=0;i<els.length;i++){{var k=els[i].dataset.key;             els[i].classList.toggle('selected', s.indexOf(k)>=0);}}}})()",
            keys.join(",")
        );
        self.webview
            .evaluate_javascript(&js, None, None, None::<&gtk::gio::Cancellable>, |_| {});
    }

    fn effective_dark(&self) -> bool {
        self.content_dark
            .unwrap_or_else(|| adw::StyleManager::default().is_dark())
    }

    fn render(&mut self) {
        let dark = self.effective_dark();
        self.apply_webview_bg(dark);
        // Already showing exactly this — returning to a conversation, or a
        // re-render nothing changed. Loading it again would only blank the view
        // and paint the same pixels back.
        let fingerprint = self.render_fingerprint(dark);
        if self.webview_ready && self.shown_fingerprint == Some(fingerprint) {
            return;
        }
        self.shown_fingerprint = Some(fingerprint);
        // A conversation is covered until its new document has painted: the
        // spinner is already up from the moment the thread was opened, so it
        // simply stays until there is something to replace it — one transition,
        // not three.
        //
        // A single message is not. It is one small frame that paints in a few
        // milliseconds, and covering that is how a plain message ended up
        // showing a spinner every time it was opened; the view keeps what it has
        // until the new document replaces it.
        if self.thread.len() > 1 && !self.instant {
            self.webview_ready = false;
        }
        let html = self.document_html(dark);
        let n = self.seq.get().wrapping_add(1);
        self.seq.set(n);
        self.webview
            .load_html(&html, Some(&format!("https://vireo.localhost/message/{n}")));
    }

    /// Which body-stack page to show: spinner while fetching, themed cover while
    /// the WebView loads, then the rendered message(s).
    fn body_page(&self) -> &'static str {
        // A document that has been handed over but hasn't painted yet is still
        // loading as far as the reader is concerned. Showing the previous
        // message in the meantime, or a bare cover between two spinners, is the
        // flash this avoids.
        if self.loading || !self.webview_ready {
            "loading"
        } else {
            "body"
        }
    }

    /// The wrapper document for what is currently on screen.
    /// Everything the rendered document depends on, in one number. Ordered by
    /// the thread so it is stable — the sets it consults are read through it
    /// rather than iterated, whose order is not.
    fn render_fingerprint(&self, dark: bool) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        dark.hash(&mut h);
        // A theme change can move the grounds without flipping `dark` (a
        // different GTK theme variant, say) — the document must follow.
        self.theme_grounds(dark).hash(&mut h);
        self.remote_allowed.hash(&mut h);
        self.thread.len().hash(&mut h);
        for m in &self.thread {
            let key = (m.account_id, m.id);
            key.hash(&mut h);
            m.unread.hash(&mut h);
            m.from_name.hash(&mut h);
            m.from_addr.hash(&mut h);
            m.to.hash(&mut h);
            m.cc.hash(&mut h);
            m.body.hash(&mut h);
            m.datetime_full().hash(&mut h);
            self.no_autoread.contains(&key).hash(&mut h);
            self.folder_labels.get(&key).hash(&mut h);
        }
        h.finish()
    }

    fn document_html(&self, dark: bool) -> String {
        // Hand the live theme grounds to the (display-free, testable) document
        // builder without widening its signature — see LIVE_GROUNDS.
        LIVE_GROUNDS.with(|g| *g.borrow_mut() = Some(self.theme_grounds(dark)));
        Self::conversation_document(
            &self.thread,
            &self.folder_labels,
            &self.no_autoread,
            &self.frame_heights,
            &self.selected_cards,
            &self.accent_hex(),
            !self.remote_allowed,
            dark,
        )
    }

    /// The wrapper document: one sandboxed iframe per message (so each email's
    /// CSS is fully isolated and its scripts can't run), with per-message
    /// headers in conversation mode. A small script sizes each iframe to its
    /// content.
    ///
    /// Takes the thread rather than `&self` so it can be exercised without a GTK
    /// display: what it emits is a security boundary, and one that needs
    /// regression cover.
    fn conversation_document(
        thread: &[Message],
        folder_labels: &std::collections::HashMap<(u32, u32), String>,
        no_autoread: &std::collections::HashSet<(u32, u32)>,
        heights: &std::collections::HashMap<(u32, u32), u32>,
        selected: &[(u32, u32)],
        accent: &str,
        restrict: bool,
        dark: bool,
    ) -> String {
        // Every message renders with the conversation chrome — a thread of one
        // gets the same in-document header. But only a real conversation is
        // *carded*: inset rounded cards on the deeper page. A lone message
        // goes full-bleed — no gutter, no radius, its ground filling the whole
        // view — so it reads as a message rather than a card in a margin.
        let conversation = !thread.is_empty();
        let carded = thread.len() > 1;
        // Card selection only means something between cards: a lone message is
        // already the selection, so it never wears the accent outline.
        let mark_selection = thread.len() > 1;
        let mut sections = String::new();
        for m in thread {
            let body = if m.body.trim().is_empty() {
                "<div class=\"vireo-loading\">Loading…</div>".to_string()
            } else {
                message_frame(
                    &m.body,
                    restrict,
                    dark,
                    (m.account_id, m.id),
                    heights.get(&(m.account_id, m.id)).copied(),
                )
            };
            if conversation {
                sections.push_str(&format!(
                    "<section class=\"vireo-msg{sel}\" data-key=\"{aid}:{id}\">\
                       <header class=\"vireo-msg-hdr\" data-key=\"{aid}:{id}\" \
                         title=\"Double-click to open in a new window\">\
                         {ava}{dot}<span class=\"vireo-from\">{from}</span>{addr}{folder}{rcpt_toggle}\
                         <span class=\"vireo-date\">{date}</span>\
                         {acts}{rcpt}\
                       </header>{body}{seen_mark}</section>",
                    aid = m.account_id,
                    id = m.id,
                    // Per-card Reply/Reply all/Forward, only where they earn
                    // their keep: a real conversation, where the toolbar can't
                    // say which message it means. A single message answers to
                    // the toolbar alone — its card carries no action pills.
                    acts = if thread.len() > 1 {
                        format!(
                            "<span class=\"vireo-acts\">\
                               <button type=\"button\" class=\"vireo-act\" data-act=\"reply\" \
                                 data-key=\"{aid}:{id}\" title=\"Reply to this message\">Reply</button>\
                               <button type=\"button\" class=\"vireo-act\" data-act=\"replyall\" \
                                 data-key=\"{aid}:{id}\" title=\"Reply to everyone on this message\">Reply all</button>\
                               <button type=\"button\" class=\"vireo-act\" data-act=\"forward\" \
                                 data-key=\"{aid}:{id}\" title=\"Forward this message\">Forward</button>\
                             </span>",
                            aid = m.account_id,
                            id = m.id,
                        )
                    } else {
                        String::new()
                    },
                    sel = if mark_selection && selected.contains(&(m.account_id, m.id)) {
                        " selected"
                    } else {
                        ""
                    },
                    // `escape_text`, not `attr_escape`: these land in element
                    // text content, where `<` and `>` are structural. A `From:`
                    // display name is attacker-controlled (and RFC 2047-decoded,
                    // so any byte sequence can be delivered), and this document
                    // is the trusted wrapper — not a sandboxed message frame.
                    from = escape_text(&m.from_name),
                    // An initials circle, tinted per sender address, so who
                    // wrote each card — and which cards are your own replies —
                    // reads at a glance (#22). Pure markup: no texture crosses
                    // into this document, and the initial is escaped like every
                    // other header field.
                    ava = {
                        let initial = m
                            .from_name
                            .trim()
                            .chars()
                            .next()
                            .or_else(|| m.from_addr.trim().chars().next())
                            .map(|c| c.to_uppercase().to_string())
                            .unwrap_or_else(|| "?".to_string());
                        let hue = m
                            .from_addr
                            .to_ascii_lowercase()
                            .bytes()
                            .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32))
                            % 360;
                        format!(
                            "<span class=\"vireo-ava\" style=\"background:hsl({hue},52%,{l}%)\">{}</span>",
                            escape_text(&initial),
                            l = if dark { 38 } else { 45 },
                        )
                    },
                    addr = if m.from_addr.is_empty() {
                        String::new()
                    } else {
                        format!("<span class=\"vireo-addr\">&lt;{}&gt;</span>", escape_text(&m.from_addr))
                    },
                    date = escape_text(&m.datetime_full()),
                    // Everyone the message went to, tucked behind a small chip so
                    // a card's header stays one line tall until asked. Escaped
                    // like the sender: recipient headers are attacker-controlled.
                    rcpt_toggle = match recipient_count(m) {
                        0 => String::new(),
                        n => format!(
                            "<button type=\"button\" class=\"vireo-rcpt-toggle\" \
                             title=\"Show recipients\">{n} recipient{s}</button>",
                            s = if n == 1 { "" } else { "s" },
                        ),
                    },
                    rcpt = recipients_html(m),
                    // Where this message was read from, when that isn't the
                    // folder on screen — the reply you sent, pulled in from Sent.
                    folder = match folder_labels.get(&(m.account_id, m.id)) {
                        Some(label) => format!(
                            "<span class=\"vireo-folder\">{}</span>",
                            escape_text(label)
                        ),
                        None => String::new(),
                    },
                    body = body,
                    // Unread messages in a conversation are marked, and the mark
                    // is cleared by reading them: the sentinel below the body
                    // reports when this message has been scrolled all the way
                    // through, which is the moment it has actually been read.
                    dot = if m.unread {
                        format!("<span class=\"vireo-dot\" data-key=\"{}:{}\"></span>", m.account_id, m.id)
                    } else {
                        String::new()
                    },
                    seen_mark = if m.unread && !no_autoread.contains(&(m.account_id, m.id)) {
                        format!("<div class=\"vireo-end\" data-key=\"{}:{}\"></div>", m.account_id, m.id)
                    } else {
                        String::new()
                    },
                ));
            } else {
                sections.push_str(&body);
            }
        }
        let scheme = if dark { "dark" } else { "light" };
        // Paint the wrapper and the (still-loading) iframes in the theme colour so
        // there's no white flash before each message's content renders. The live
        // theme's grounds when the reader set them (issue #62); the stock GNOME
        // values otherwise (tests).
        let (bg, deep, chrome) = LIVE_GROUNDS.with(|g| g.borrow().clone()).unwrap_or_else(|| {
            let (g, p, c) = if dark {
                (GROUND.1, PAGE.1, CHROME.1)
            } else {
                (GROUND.0, PAGE.0, CHROME.0)
            };
            (g.to_string(), p.to_string(), c.to_string())
        });
        // Each message card only reads as a card against a slightly deeper
        // ground than its own; a full-bleed single message sits on its own
        // ground, so the whole view is one colour.
        let page = if carded { deep } else { bg.clone() };
        let body_class = if carded { " class=\"vireo-conv\"" } else { "" };
        // Defence in depth for the wrapper: the only script allowed to run is the
        // one carrying this render's nonce, which is ours. Anything a message
        // manages to smuggle into this document — an injected `<script>`, an
        // `onerror=` handler — is refused by the engine even if the escaping
        // above is ever wrong again.
        //
        // Deliberately *only* `script-src`/`object-src`/`base-uri`: no
        // `default-src`. A wrapper policy is inherited by the `srcdoc` frames, so
        // restricting images or styles here would silently re-block the remote
        // content the user has chosen to load. Each frame carries its own
        // `default-src 'none'` policy for that.
        let nonce = crate::rng::nonce(24).ok();
        let csp = match &nonce {
            Some(n) => format!(
                "<meta http-equiv=\"Content-Security-Policy\" content=\"\
                 script-src 'nonce-{n}'; object-src 'none'; base-uri 'none'\">"
            ),
            // No entropy, no nonce we can trust — allow no script at all. The
            // iframes render at their default height instead of resizing, which
            // is a visual degradation, not a security one.
            None => "<meta http-equiv=\"Content-Security-Policy\" content=\"\
                     script-src 'none'; object-src 'none'; base-uri 'none'\">"
                .to_string(),
        };
        let sizer = match &nonce {
            Some(n) => format!("<script nonce=\"{n}\">{SIZE_SCRIPT}</script>"),
            None => String::new(),
        };
        format!(
            "<!doctype html><html><head><meta charset=\"utf-8\">{csp}\
             <meta name=\"color-scheme\" content=\"{scheme}\">\
             <style>\
               :root{{color-scheme:{scheme};}}\
               body{{margin:0;padding:0;background:{page};font:14px/1.55 system-ui,sans-serif;}}\
               body.vireo-conv{{padding:14px;}}\
               iframe.vireo-frame{{width:100%;border:0;display:block;background:{bg};}}\
               iframe.vireo-frame.anim{{transition:height 240ms cubic-bezier(0.4,0,0.2,1);}}\
               @media (prefers-reduced-motion:reduce){{iframe.vireo-frame.anim{{transition:none;}}}}\
               .vireo-msg{{background:{bg};\
                 border-radius:12px;overflow:hidden;margin:0 0 14px;}}\
               .vireo-msg:last-child{{margin-bottom:0;}}\
               .vireo-msg{{user-select:none;}}\
               body:not(.vireo-conv) .vireo-msg{{border-radius:0;margin:0;}}\
               .vireo-msg.selected{{box-shadow:0 0 0 2px {accent};}}\
               .vireo-msg-hdr{{cursor:pointer;}}\
               .vireo-msg-hdr{{display:flex;gap:8px;align-items:baseline;flex-wrap:wrap;padding:12px 16px;cursor:default;user-select:none;\
                 position:sticky;top:0;z-index:1;background-color:{bg};}}\
               body:not(.vireo-conv) .vireo-msg-hdr{{background-color:{chrome};}}\
               .vireo-ava{{width:26px;height:26px;border-radius:50%;flex:none;align-self:center;\
                 display:flex;align-items:center;justify-content:center;color:#fff;\
                 font-size:0.8em;font-weight:700;}}\
               .vireo-from{{font-weight:700;}}\
               .vireo-addr{{opacity:0.55;font-size:0.9em;}}\
               .vireo-date{{margin-left:auto;opacity:0.55;font-size:0.85em;}}\
               .vireo-dot{{width:8px;height:8px;border-radius:50%;background:#3584e4;\
                 flex:none;align-self:center;}}\
               .vireo-end{{height:1px;}}\
               .vireo-quote{{display:block;margin:0 16px 12px;padding:0 7px;\
                 font:inherit;font-size:0.7em;line-height:1.45;letter-spacing:0.06em;\
                 color:inherit;opacity:0.6;background:rgba(128,128,128,0.16);\
                 border:0;border-radius:999px;cursor:pointer;}}\
               .vireo-quote:hover{{opacity:0.95;background:rgba(128,128,128,0.28);}}\
               .vireo-quote.open{{opacity:0.95;}}\
               .vireo-acts{{display:flex;gap:6px;}}\
               .vireo-act{{font:inherit;font-size:0.8em;color:inherit;background:none;\
                 border:1px solid rgba(128,128,128,0.45);border-radius:999px;\
                 padding:2px 10px;cursor:pointer;opacity:0.75;\
                 transition:opacity 120ms ease,background 120ms ease;}}\
               .vireo-act:hover{{opacity:1;background:rgba(128,128,128,0.18);}}\
               .vireo-act:active{{background:rgba(128,128,128,0.3);}}\
               .vireo-folder{{margin-left:0.5em;padding:0.05em 0.45em;border-radius:0.7em;\
                 font-size:0.78em;opacity:0.75;border:1px solid currentColor;}}\
               .vireo-rcpt-toggle{{font:inherit;font-size:0.78em;color:inherit;background:none;\
                 border:1px solid rgba(128,128,128,0.45);border-radius:999px;\
                 padding:0.05em 0.6em;cursor:pointer;opacity:0.7;\
                 transition:opacity 120ms ease,background 120ms ease;}}\
               .vireo-rcpt-toggle:hover{{opacity:1;background:rgba(128,128,128,0.18);}}\
               .vireo-rcpt-toggle.open{{opacity:1;background:rgba(128,128,128,0.18);}}\
               .vireo-rcpt{{flex-basis:100%;font-size:0.85em;opacity:0.75;\
                 user-select:text;overflow-wrap:anywhere;margin:0;}}\
               .vireo-rcpt div{{margin-top:2px;}}\
               .vireo-loading{{opacity:0.5;padding:16px;}}\
             </style>{sizer}\
             </head><body{body_class}>{sections}</body></html>"
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
                let doc = if self.remote_allowed { doc } else { strip_remote(&doc) };
                let head = if conversation {
                    print_message_header_html(m)
                } else {
                    String::new()
                };
                (head, doc)
            })
            .collect();
        print_document(&self.print_header_html(), &messages, self.remote_allowed)
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

    /// The reader's two grounds for `dark`, resolved from the live libadwaita
    /// theme instead of hard-coded values (issue #62): GROUND is the theme's
    /// own view background, and PAGE shades it a step deeper so conversation
    /// cards keep reading as cards under any theme variant. When the reader is
    /// forcing a scheme the app isn't currently in, the theme can't answer for
    /// that mode, so the stock GNOME values stand in.
    #[allow(deprecated)] // lookup_color: named theme colours have no successor yet
    fn theme_grounds(&self, dark: bool) -> (String, String, String) {
        let style = self.webview.style_context();
        if dark == adw::StyleManager::default().is_dark() {
            if let Some(c) = style.lookup_color("view_bg_color") {
                let hex = |r: f32, g: f32, b: f32| {
                    format!(
                        "#{:02x}{:02x}{:02x}",
                        (r * 255.0).round() as u8,
                        (g * 255.0).round() as u8,
                        (b * 255.0).round() as u8,
                    )
                };
                // The stock pairs' own ratios: #1e1e1e→#141414 and #fff→#f1f1f1.
                let f = if dark { 0.667 } else { 0.945 };
                let ground = hex(c.red(), c.green(), c.blue());
                let page = hex(c.red() * f, c.green() * f, c.blue() * f);
                // The window's own ground — what the GTK reader header (the
                // subject block) sits on. A full-bleed single message paints
                // its in-document header this colour so subject and header
                // read as one surface.
                let chrome = style
                    .lookup_color("window_bg_color")
                    .map(|w| hex(w.red(), w.green(), w.blue()))
                    .unwrap_or_else(|| {
                        (if dark { CHROME.1 } else { CHROME.0 }).to_string()
                    });
                return (ground, page, chrome);
            }
        }
        let (g, p, c) = if dark {
            (GROUND.1, PAGE.1, CHROME.1)
        } else {
            (GROUND.0, PAGE.0, CHROME.0)
        };
        (g.to_string(), p.to_string(), c.to_string())
    }

    /// Paint the WebView canvas in the theme colour so unstyled bodies (and the
    /// gap before a load) match light/dark mode instead of flashing white.
    fn apply_webview_bg(&self, dark: bool) {
        // Whatever is about to be shown: a conversation's cards sit on the
        // deeper page ground, a full-bleed single message on the plain ground.
        // The cover matches it so the spinner gives way to the document without
        // a change of colour.
        let (ground, page, _chrome) = self.theme_grounds(dark);
        let ground = if self.thread.len() > 1 { page } else { ground };
        self.webview.set_background_color(&ground_rgba(&ground));
        let bg = ground;
        // The spinner and the cover stand in for the message, so they answer to
        // the message's theme: #1e1e1e matches the document's own dark ground,
        // white its light one. The label and spinner take a dimmed foreground
        // from the same side, so neither disappears into the ground.
        let fg = if dark {
            "rgba(255,255,255,0.55)"
        } else {
            "rgba(0,0,0,0.45)"
        };
        self.cover_provider.load_from_data(&format!(
            ".reader-cover {{ background-color: {bg}; }}\
             .reader-loading label, .reader-loading spinner {{ color: {fg}; }}"
        ));
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
                            // Only web and mail links reach the desktop's handlers.
                            // An HTML body keeps its own `href` values verbatim, so
                            // without this a message could hand `file://`, `smb://`
                            // or any scheme a third-party app has registered to that
                            // app on a single click.
                            if is_launchable_uri(&uri) {
                                let _ = gtk::gio::AppInfo::launch_default_for_uri(
                                    &uri,
                                    None::<&gtk::gio::AppLaunchContext>,
                                );
                            } else {
                                tracing::warn!(
                                    "refused to open a link with an unsupported scheme: {}",
                                    uri.split(':').next().unwrap_or("?")
                                );
                            }
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

/// Whether a clicked link may be handed to the desktop's default handler.
///
/// An allowlist, not a blocklist: every scheme a desktop registers is a program
/// that would be started with a sender-controlled argument, and there is no way
/// to enumerate the dangerous ones ahead of time.
fn is_launchable_uri(uri: &str) -> bool {
    match uri.split_once(':') {
        Some((scheme, rest)) => {
            matches!(
                scheme.to_ascii_lowercase().as_str(),
                "http" | "https" | "mailto"
            ) && !rest.is_empty()
        }
        None => false,
    }
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

/// How many addresses a message's To + Cc headers name.
fn recipient_count(m: &Message) -> usize {
    [m.to.as_str(), m.cc.as_str()]
        .iter()
        .flat_map(|s| s.split(','))
        .filter(|a| !a.trim().is_empty())
        .count()
}

/// The collapsible To/Cc block inside a conversation card's header. Starts
/// hidden; the header's recipients chip toggles it. Empty when the message
/// names no recipients at all.
fn recipients_html(m: &Message) -> String {
    let mut lines = String::new();
    for (label, list) in [("To", m.to.trim()), ("Cc", m.cc.trim())] {
        if !list.is_empty() {
            lines.push_str(&format!("<div><b>{label}:</b> {}</div>", escape_text(list)));
        }
    }
    if lines.is_empty() {
        return String::new();
    }
    format!("<div class=\"vireo-rcpt\" hidden>{lines}</div>")
}

/// Every URL in `html` that would cause a fetch from a remote host, as byte
/// ranges into `html`.
///
/// One walk feeds both the detector and the stripper, so the banner and the
/// blocking can no longer disagree about what counts as remote. The pair of
/// substring lists this replaces had already drifted: `SRC="HTTP://…"` was
/// detected but not stripped, and `src = "http://…"` (whitespace around `=`,
/// which HTML permits) was neither.
fn remote_url_spans(html: &str) -> Vec<(usize, usize)> {
    let b = html.as_bytes();
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        let closing = j < b.len() && b[j] == b'/';
        if closing {
            j += 1;
        }
        let name_start = j;
        while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'-' || b[j] == b':') {
            j += 1;
        }
        if j == name_start {
            // Not a tag (a comment, a stray `<`, an entity) — nothing to parse.
            i += 1;
            continue;
        }
        let tag = html[name_start..j].to_ascii_lowercase();

        // Attributes, up to the closing `>`.
        while j < b.len() && b[j] != b'>' {
            if b[j].is_ascii_whitespace() || b[j] == b'/' {
                j += 1;
                continue;
            }
            let an_start = j;
            while j < b.len()
                && !matches!(b[j], b'=' | b'>' | b'/')
                && !b[j].is_ascii_whitespace()
            {
                j += 1;
            }
            if j == an_start {
                j += 1;
                continue;
            }
            let attr = String::from_utf8_lossy(&b[an_start..j]).to_ascii_lowercase();

            // `=` may be surrounded by whitespace; an attribute may have no value.
            let mut k = j;
            while k < b.len() && b[k].is_ascii_whitespace() {
                k += 1;
            }
            if k >= b.len() || b[k] != b'=' {
                continue;
            }
            k += 1;
            while k < b.len() && b[k].is_ascii_whitespace() {
                k += 1;
            }
            let (vs, ve) = if k < b.len() && (b[k] == b'"' || b[k] == b'\'') {
                let q = b[k];
                k += 1;
                let start = k;
                while k < b.len() && b[k] != q {
                    k += 1;
                }
                let end = k;
                if k < b.len() {
                    k += 1;
                }
                (start, end)
            } else {
                let start = k;
                while k < b.len() && b[k] != b'>' && !b[k].is_ascii_whitespace() {
                    k += 1;
                }
                (start, k)
            };
            j = k;

            match attr.as_str() {
                // "url 1x, url 2x, …" — the URL is each candidate's first token.
                "srcset" | "imagesrcset" => {
                    let mut at = vs;
                    while at < ve {
                        let mut end = at;
                        while end < ve && b[end] != b',' {
                            end += 1;
                        }
                        push_url(b, at, end, &mut spans);
                        at = end + 1;
                    }
                }
                "style" => push_css(b, vs, ve, &mut spans),
                _ if fetches(&tag, &attr) => push_url(b, vs, ve, &mut spans),
                _ => {}
            }
        }

        // A `<style>` element's body carries `url()` and `@import`. Only the
        // opening tag starts one — treating `</style>` as another would take the
        // rest of the document for stylesheet text and skip past everything in
        // it, which is how the `<img>` after a `<style>` block went unstripped.
        if tag == "style" && !closing {
            let start = (j + 1).min(b.len());
            let end = find_ci(b, b"</style", start).unwrap_or(b.len());
            push_css(b, start, end, &mut spans);
            j = end;
        }

        i = if j > i { j } else { i + 1 };
    }

    // Only spans that land on character boundaries can be sliced or replaced.
    spans.retain(|(s, e)| s < e && html.is_char_boundary(*s) && html.is_char_boundary(*e));
    spans.sort_unstable();
    spans.dedup();
    spans
}

/// Whether an attribute on this element causes a fetch on its own.
fn fetches(tag: &str, attr: &str) -> bool {
    match attr {
        "src" | "poster" | "background" | "lowsrc" | "dynsrc" | "codebase" | "xlink:href" => true,
        "data" => tag == "object",
        // `href` fetches on `<link>` (stylesheets, preloads, icons) and on SVG
        // `<image>`/`<use>`. On `<a>` it is a destination the user must click,
        // and treating those as remote content would put the banner on nearly
        // every message and so teach people to ignore it.
        "href" => matches!(tag, "link" | "image" | "use"),
        _ => false,
    }
}

/// Record `b[start..end]` (trimmed) if it points at a remote host.
fn push_url(b: &[u8], start: usize, end: usize, spans: &mut Vec<(usize, usize)>) {
    let mut s = start;
    let mut e = end.min(b.len());
    while s < e && b[s].is_ascii_whitespace() {
        s += 1;
    }
    // For `srcset` the candidate is "<url> <descriptor>"; the URL ends at the
    // first space. For a plain attribute there is no descriptor to trim.
    let mut u = s;
    while u < e && !b[u].is_ascii_whitespace() {
        u += 1;
    }
    e = u;
    if s < e && is_remote_url(&b[s..e]) {
        spans.push((s, e));
    }
}

/// Record every remote `url(…)` and `@import "…"` in a stylesheet or `style=`.
fn push_css(b: &[u8], start: usize, end: usize, spans: &mut Vec<(usize, usize)>) {
    let end = end.min(b.len());
    let mut at = start;
    while let Some(p) = find_ci(&b[..end], b"url(", at) {
        if p >= end {
            break;
        }
        let mut k = p + 4;
        while k < end && b[k].is_ascii_whitespace() {
            k += 1;
        }
        let quote = if k < end && (b[k] == b'"' || b[k] == b'\'') {
            let q = b[k];
            k += 1;
            Some(q)
        } else {
            None
        };
        let term = quote.unwrap_or(b')');
        let vs = k;
        while k < end && b[k] != term && !(quote.is_none() && b[k].is_ascii_whitespace()) {
            k += 1;
        }
        if vs < k && is_remote_url(&b[vs..k]) {
            spans.push((vs, k));
        }
        at = (p + 4).max(k);
    }
    let mut at = start;
    while let Some(p) = find_ci(&b[..end], b"@import", at) {
        if p >= end {
            break;
        }
        let mut k = p + 7;
        while k < end && b[k].is_ascii_whitespace() {
            k += 1;
        }
        // The `url(…)` form is already covered by the loop above; this is the
        // bare-string form, `@import "//host/x.css"`.
        if k < end && (b[k] == b'"' || b[k] == b'\'') {
            let q = b[k];
            k += 1;
            let vs = k;
            while k < end && b[k] != q {
                k += 1;
            }
            if vs < k && is_remote_url(&b[vs..k]) {
                spans.push((vs, k));
            }
        }
        at = p + 7;
    }
}

/// Whether a URL reaches a remote host.
///
/// `data:` and `cid:` carry their own bytes, and a path-relative or root-relative
/// URL resolves against `vireo.localhost`, which serves nothing. A
/// protocol-relative `//host/x` does reach the network — that one is the bypass
/// the old substring list missed.
fn is_remote_url(u: &[u8]) -> bool {
    let u = std::str::from_utf8(u).unwrap_or("").trim();
    if u.starts_with("//") {
        return true;
    }
    match u.split_once(':') {
        Some((scheme, _)) if !scheme.is_empty() && scheme.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')
        }) => !matches!(
            scheme.to_ascii_lowercase().as_str(),
            "data" | "cid" | "blocked" | "mailto" | "tel" | "about" | "javascript"
        ),
        _ => false,
    }
}

/// Case-insensitive byte search from `from`.
fn find_ci(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from >= hay.len() || needle.is_empty() {
        return None;
    }
    hay[from..]
        .windows(needle.len())
        .position(|w| w.eq_ignore_ascii_case(needle))
        .map(|p| p + from)
}

/// Neutralize remote resource references so nothing is fetched while blocked.
/// Targets resource-loading attributes only; `<a href>` links are left intact.
fn strip_remote(html: &str) -> String {
    let spans = remote_url_spans(html);
    if spans.is_empty() {
        return html.to_string();
    }
    let mut out = String::with_capacity(html.len());
    let mut at = 0usize;
    for (s, e) in spans {
        if s < at {
            continue; // overlapping match already rewritten
        }
        out.push_str(&html[at..s]);
        out.push_str("blocked://");
        at = e;
    }
    out.push_str(&html[at..]);
    out
}

/// Does the HTML reference remote (network-reachable) resources?
///
/// This decides whether the "remote content was blocked" banner appears, and
/// nothing more — the blocking itself follows the user's setting, not this. A
/// miss here is a missing banner, not a leaked request.
fn has_remote_resources(html: &str) -> bool {
    !remote_url_spans(html).is_empty()
}

/// Inject a Content-Security-Policy `<meta>` into the document head. When remote
/// content is disallowed only inline styles and `data:` URIs are permitted.
///
/// `allow_remote` is the user's own choice, never the output of
/// [`has_remote_resources`]. That is what makes this an independent second line
/// of defence: if the detector fails to spot a reference, the engine still
/// refuses the fetch.
// ===== Dark-mode colour adaptation (issue #35) =====
//
// Emails are designed for light rendering: dark text, light (or absent)
// backgrounds. In dark mode an email that sets `color:#333` but no background
// paints near-black text on the reader's dark ground. The sandboxed frames run
// no JavaScript, so the fix happens here, on the document text, at render
// time (never in the on-disk body cache): every colour the message declares is
// checked and, when its lightness is wrong for a dark ground, flipped in HSL —
// hue and saturation kept, lightness mirrored. Text colours darker than
// mid-grey become light; backgrounds lighter than mid-grey become dark;
// everything already suited to a dark ground is left untouched, so mail
// designed dark passes through unchanged.

/// Parse a CSS colour token to linear [r, g, b, a] in 0..=1. Handles hex
/// (#rgb/#rgba/#rrggbb/#rrggbbaa), rgb()/rgba() with numbers or percentages,
/// and the common named colours. `bare_hex` additionally accepts legacy
/// attribute values like `bgcolor=ffffff` with no `#`.
fn parse_css_color(token: &str, bare_hex: bool) -> Option<[f32; 4]> {
    let t = token.trim();
    let hex = |s: &str| -> Option<[f32; 4]> {
        let v = |i: usize, n: usize| {
            u8::from_str_radix(&s[i..i + n], 16)
                .ok()
                .map(|b| if n == 1 { (b * 17) as f32 / 255.0 } else { b as f32 / 255.0 })
        };
        match s.len() {
            3 => Some([v(0, 1)?, v(1, 1)?, v(2, 1)?, 1.0]),
            4 => Some([v(0, 1)?, v(1, 1)?, v(2, 1)?, v(3, 1)?]),
            6 => Some([v(0, 2)?, v(2, 2)?, v(4, 2)?, 1.0]),
            8 => Some([v(0, 2)?, v(2, 2)?, v(4, 2)?, v(6, 2)?]),
            _ => None,
        }
    };
    if let Some(rest) = t.strip_prefix('#') {
        return hex(rest);
    }
    let lower = t.to_ascii_lowercase();
    if lower.starts_with("rgb(") || lower.starts_with("rgba(") {
        let inner = t[t.find('(')? + 1..].strip_suffix(')')?;
        let parts: Vec<&str> = inner
            .split(|c| c == ',' || c == '/' || char::is_whitespace(c))
            .filter(|p| !p.is_empty())
            .collect();
        if parts.len() < 3 {
            return None;
        }
        let chan = |s: &str| -> Option<f32> {
            if let Some(p) = s.strip_suffix('%') {
                p.trim().parse::<f32>().ok().map(|v| v / 100.0)
            } else {
                s.trim().parse::<f32>().ok().map(|v| v / 255.0)
            }
        };
        let alpha = |s: &str| -> Option<f32> {
            if let Some(p) = s.strip_suffix('%') {
                p.trim().parse::<f32>().ok().map(|v| v / 100.0)
            } else {
                s.trim().parse::<f32>().ok()
            }
        };
        return Some([
            chan(parts[0])?.clamp(0.0, 1.0),
            chan(parts[1])?.clamp(0.0, 1.0),
            chan(parts[2])?.clamp(0.0, 1.0),
            parts.get(3).and_then(|s| alpha(s)).unwrap_or(1.0).clamp(0.0, 1.0),
        ]);
    }
    let named: Option<u32> = match lower.as_str() {
        "black" => Some(0x000000),
        "white" => Some(0xffffff),
        "gray" | "grey" => Some(0x808080),
        "dimgray" | "dimgrey" => Some(0x696969),
        "darkgray" | "darkgrey" => Some(0xa9a9a9),
        "lightgray" | "lightgrey" => Some(0xd3d3d3),
        "gainsboro" => Some(0xdcdcdc),
        "whitesmoke" => Some(0xf5f5f5),
        "silver" => Some(0xc0c0c0),
        "red" => Some(0xff0000),
        "darkred" | "maroon" => Some(0x800000),
        "green" => Some(0x008000),
        "darkgreen" => Some(0x006400),
        "blue" => Some(0x0000ff),
        "navy" | "darkblue" => Some(0x000080),
        "midnightblue" => Some(0x191970),
        "purple" => Some(0x800080),
        "indigo" => Some(0x4b0082),
        "brown" => Some(0xa52a2a),
        "orange" => Some(0xffa500),
        "yellow" => Some(0xffff00),
        "teal" => Some(0x008080),
        "olive" => Some(0x808000),
        _ => None,
    };
    if let Some(rgb) = named {
        return Some([
            ((rgb >> 16) & 0xff) as f32 / 255.0,
            ((rgb >> 8) & 0xff) as f32 / 255.0,
            (rgb & 0xff) as f32 / 255.0,
            1.0,
        ]);
    }
    if bare_hex && t.len() == 6 && t.bytes().all(|b| b.is_ascii_hexdigit()) {
        return hex(t);
    }
    None
}

fn rgb_to_hsl(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if (max - min).abs() < f32::EPSILON {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if (max - r).abs() < f32::EPSILON {
        ((g - b) / d).rem_euclid(6.0)
    } else if (max - g).abs() < f32::EPSILON {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } / 6.0;
    (h, s, l)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    if s <= 0.0 {
        return (l, l, l);
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let f = |mut t: f32| {
        t = t.rem_euclid(1.0);
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    (f(h + 1.0 / 3.0), f(h), f(h - 1.0 / 3.0))
}

/// Flip a colour for the dark ground when its lightness calls for it: text
/// darker than mid-grey mirrors up (floored so it stays clearly readable),
/// backgrounds lighter than mid-grey mirror down (floored above pure black so
/// they read as surfaces, like the reader's own grounds). `None` = keep as is.
fn adapt_color(token: &str, background: bool, bare_hex: bool) -> Option<String> {
    let [r, g, b, a] = parse_css_color(token, bare_hex)?;
    if a <= 0.01 {
        return None; // effectively transparent either way
    }
    let (h, s, l) = rgb_to_hsl(r, g, b);
    let flipped = if background {
        if l <= 0.6 {
            return None;
        }
        (1.0 - l).max(0.08)
    } else {
        if l >= 0.5 {
            return None;
        }
        (1.0 - l).max(0.72)
    };
    let (nr, ng, nb) = hsl_to_rgb(h, s, flipped);
    let to8 = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
    Some(if a < 1.0 {
        format!("rgba({},{},{},{:.2})", to8(nr), to8(ng), to8(nb), a)
    } else {
        format!("#{:02x}{:02x}{:02x}", to8(nr), to8(ng), to8(nb))
    })
}

/// Rewrite one CSS declaration list (an inline `style` attribute's content or
/// a rule body). Declarations are split at `;` outside parentheses and quotes
/// — data: URLs contain semicolons — and only `color`, `background-color`,
/// and `background`'s colour tokens are touched.
fn rewrite_declarations(decls: &str) -> String {
    let mut out = String::with_capacity(decls.len());
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut start = 0;
    let mut i = 0;
    while i < decls.len() {
        let c = decls[i..].chars().next().unwrap();
        // Comments may contain `;` — `/* background: #fff; */` must not split
        // the declaration after it in half (seen in the wild, issue #35).
        if quote.is_none() && decls[i..].starts_with("/*") {
            i += decls[i..].find("*/").map(|r| r + 2).unwrap_or(decls.len() - i);
            continue;
        }
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"') | (None, '\'') => quote = Some(c),
            (None, '(') => depth += 1,
            (None, ')') => depth = depth.saturating_sub(1),
            (None, ';') if depth == 0 => {
                out.push_str(&rewrite_one_declaration(&decls[start..i]));
                out.push(';');
                start = i + 1;
            }
            _ => {}
        }
        i += c.len_utf8();
    }
    out.push_str(&rewrite_one_declaration(&decls[start..]));
    out
}

/// One `prop: value` declaration, colour-adapted when the property carries a
/// colour whose direction we know. Anything unrecognised passes through
/// byte-for-byte.
fn rewrite_one_declaration(decl: &str) -> String {
    // Step over any leading comments so `/* old */ background: #fff` still
    // parses to a property we recognise; the comment is kept verbatim.
    let mut p = 0;
    loop {
        let rest = &decl[p..];
        let trimmed = rest.trim_start();
        p += rest.len() - trimmed.len();
        if trimmed.starts_with("/*") {
            match trimmed.find("*/") {
                Some(e) => p += e + 2,
                None => return decl.to_string(),
            }
        } else {
            break;
        }
    }
    let (head, decl_body) = decl.split_at(p);
    let Some(colon) = decl_body.find(':') else { return decl.to_string() };
    let prop = decl_body[..colon].trim().to_ascii_lowercase();
    let value = &decl_body[colon + 1..];
    let background = match prop.as_str() {
        "color" => false,
        "background-color" | "background" => true,
        _ => return decl.to_string(),
    };
    // Keep any !important, transform the value's colour tokens.
    let (value_body, important) = match value.to_ascii_lowercase().find("!important") {
        Some(at) => (&value[..at], &value[at..]),
        None => (value, ""),
    };
    let mut rewritten = String::with_capacity(value_body.len());
    for piece in split_value_tokens(value_body) {
        match &piece {
            ValuePiece::Token(t) => match adapt_color(t, background, false) {
                Some(new) => rewritten.push_str(&new),
                None => rewritten.push_str(t),
            },
            ValuePiece::Raw(r) => rewritten.push_str(r),
        }
    }
    format!("{head}{}:{rewritten}{important}", &decl_body[..colon])
}

enum ValuePiece<'a> {
    /// A candidate colour token (word or function call).
    Token(&'a str),
    /// Whitespace, url(...), strings — copied verbatim.
    Raw(&'a str),
}

/// Split a CSS value into colour-candidate tokens and verbatim runs, keeping
/// `url(...)` and quoted strings intact (their contents are not colours, and
/// data: URLs may contain anything).
fn split_value_tokens(value: &str) -> Vec<ValuePiece<'_>> {
    let mut pieces = Vec::new();
    let lower = value.to_ascii_lowercase();
    let mut i = 0;
    while i < value.len() {
        let c = value[i..].chars().next().unwrap();
        if c.is_whitespace() || c == ',' {
            let start = i;
            while i < value.len() {
                let c = value[i..].chars().next().unwrap();
                if c.is_whitespace() || c == ',' {
                    i += c.len_utf8();
                } else {
                    break;
                }
            }
            pieces.push(ValuePiece::Raw(&value[start..i]));
        } else if lower[i..].starts_with("url(") {
            let start = i;
            i += 4;
            let mut depth = 1;
            while i < value.len() && depth > 0 {
                let c = value[i..].chars().next().unwrap();
                if c == '(' {
                    depth += 1;
                } else if c == ')' {
                    depth -= 1;
                }
                i += c.len_utf8();
            }
            pieces.push(ValuePiece::Raw(&value[start..i]));
        } else if c == '"' || c == '\'' {
            let start = i;
            i += 1;
            while i < value.len() {
                let ch = value[i..].chars().next().unwrap();
                i += ch.len_utf8();
                if ch == c {
                    break;
                }
            }
            pieces.push(ValuePiece::Raw(&value[start..i]));
        } else {
            // A word, possibly a function like rgb(...): take through balanced
            // parens if one opens immediately after the name.
            let start = i;
            while i < value.len() {
                let ch = value[i..].chars().next().unwrap();
                if ch.is_whitespace() || ch == ',' {
                    break;
                }
                i += ch.len_utf8();
                if ch == '(' {
                    let mut depth = 1;
                    while i < value.len() && depth > 0 {
                        let c2 = value[i..].chars().next().unwrap();
                        if c2 == '(' {
                            depth += 1;
                        } else if c2 == ')' {
                            depth -= 1;
                        }
                        i += c2.len_utf8();
                    }
                    break;
                }
            }
            pieces.push(ValuePiece::Token(&value[start..i]));
        }
    }
    pieces
}

/// Rewrite the declaration bodies inside a `<style>` block, leaving selectors,
/// at-rules, comments, and strings untouched. Brace-nesting (`@media { sel {
/// … } }`) is handled by only treating brace-content that closes without
/// opening another brace as declarations.
fn rewrite_css(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut i = 0;
    let mut seg_start = 0;
    let mut in_comment = false;
    let mut quote: Option<char> = None;
    let mut decl_start: Option<usize> = None;
    while i < css.len() {
        let c = css[i..].chars().next().unwrap();
        if in_comment {
            if css[i..].starts_with("*/") {
                in_comment = false;
                i += 2;
                continue;
            }
        } else if let Some(q) = quote {
            if c == q {
                quote = None;
            }
        } else if css[i..].starts_with("/*") {
            in_comment = true;
            i += 2;
            continue;
        } else if c == '"' || c == '\'' {
            quote = Some(c);
        } else if c == '{' {
            out.push_str(&css[seg_start..i + 1]);
            seg_start = i + 1;
            decl_start = Some(i + 1);
        } else if c == '}' {
            match decl_start.take() {
                // The segment closed without opening a nested brace: it is a
                // declaration list.
                Some(ds) => {
                    out.push_str(&rewrite_declarations(&css[ds..i]));
                    out.push('}');
                    seg_start = i + 1;
                }
                None => {
                    out.push_str(&css[seg_start..i + 1]);
                    seg_start = i + 1;
                }
            }
        }
        i += c.len_utf8();
    }
    out.push_str(&css[seg_start..]);
    out
}

/// Rewrite one tag's colour-bearing attributes: `style` (declarations),
/// `color`/`text` (text direction), `bgcolor` (background direction). `text`
/// only means a colour on `<body>`.
fn rewrite_tag_attrs(tag: &str) -> String {
    let lower = tag.to_ascii_lowercase();
    let is_body = lower.starts_with("<body");
    let mut out = String::with_capacity(tag.len());
    let mut i = 0;
    while i < tag.len() {
        let rest_lower = &lower[i..];
        let attr = ["style", "color", "bgcolor", "text"].iter().find(|a| {
            rest_lower.starts_with(**a)
                && i > 0
                && lower.as_bytes()[i - 1].is_ascii_whitespace()
                && rest_lower[a.len()..].trim_start().starts_with('=')
        });
        let Some(attr) = attr else {
            let c = tag[i..].chars().next().unwrap();
            out.push(c);
            i += c.len_utf8();
            continue;
        };
        if *attr == "text" && !is_body {
            let c = tag[i..].chars().next().unwrap();
            out.push(c);
            i += c.len_utf8();
            continue;
        }
        // name, '=', then a quoted or bare value.
        let eq = i + rest_lower.find('=').unwrap();
        let mut v = eq + 1;
        while v < tag.len() && tag.as_bytes()[v].is_ascii_whitespace() {
            v += 1;
        }
        let (val_start, val_end, quoted) = if v < tag.len()
            && (tag.as_bytes()[v] == b'"' || tag.as_bytes()[v] == b'\'')
        {
            let q = tag.as_bytes()[v] as char;
            let end = tag[v + 1..].find(q).map(|r| v + 1 + r).unwrap_or(tag.len());
            (v + 1, end, true)
        } else {
            let end = tag[v..]
                .find(|c: char| c.is_ascii_whitespace() || c == '>' || c == '/')
                .map(|r| v + r)
                .unwrap_or(tag.len());
            (v, end, false)
        };
        let value = &tag[val_start..val_end];
        let new = match *attr {
            "style" => rewrite_declarations(value),
            "bgcolor" => adapt_color(value, true, true).unwrap_or_else(|| value.to_string()),
            _ => adapt_color(value, false, true).unwrap_or_else(|| value.to_string()),
        };
        out.push_str(&tag[i..val_start]);
        out.push_str(&new);
        i = val_end + usize::from(quoted && val_end < tag.len());
        if quoted && val_end < tag.len() {
            out.push(tag.as_bytes()[val_end] as char);
        }
    }
    out
}

/// The whole-document pass: `<style>` blocks through the CSS rewriter, every
/// other tag through the attribute rewriter, text content untouched. Tag ends
/// are found quote-aware, since `>` inside attribute values is legal HTML.
fn adapt_colors_for_dark(doc: &str) -> String {
    let lower = doc.to_ascii_lowercase();
    let mut out = String::with_capacity(doc.len() + 64);
    let mut i = 0;
    while i < doc.len() {
        let Some(rel) = doc[i..].find('<') else {
            out.push_str(&doc[i..]);
            break;
        };
        let tag_start = i + rel;
        out.push_str(&doc[i..tag_start]);
        if lower[tag_start..].starts_with("<!--") {
            let end = doc[tag_start..]
                .find("-->")
                .map(|r| tag_start + r + 3)
                .unwrap_or(doc.len());
            out.push_str(&doc[tag_start..end]);
            i = end;
            continue;
        }
        if lower[tag_start..].starts_with("<style") {
            let Some(open) = doc[tag_start..].find('>') else {
                out.push_str(&doc[tag_start..]);
                break;
            };
            let css_start = tag_start + open + 1;
            out.push_str(&doc[tag_start..css_start]);
            let css_end = lower[css_start..]
                .find("</style")
                .map(|r| css_start + r)
                .unwrap_or(doc.len());
            out.push_str(&rewrite_css(&doc[css_start..css_end]));
            i = css_end;
            continue;
        }
        // Quote-aware scan for the tag's closing '>'.
        let mut j = tag_start;
        let mut quote: Option<u8> = None;
        let mut end = doc.len();
        while j < doc.len() {
            let b = doc.as_bytes()[j];
            match quote {
                Some(q) if b == q => quote = None,
                Some(_) => {}
                None if b == b'"' || b == b'\'' => quote = Some(b),
                None if b == b'>' => {
                    end = j + 1;
                    break;
                }
                None => {}
            }
            j += 1;
        }
        out.push_str(&rewrite_tag_attrs(&doc[tag_start..end]));
        i = end;
    }
    out
}

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
    // Every message gets a comfortable default inset: the UA's 8px body margin
    // is reset so content sits at exactly 20px of breathing room. Injected
    // ahead of the email's own CSS, so a message that styles its body (a
    // full-bleed design, say) still wins.
    let body_pad = "body{margin:0;padding:20px;box-sizing:border-box;}";
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

/// Wrapper-document script: size each message iframe to its content height so the
/// whole conversation scrolls as one page (the iframes have no inner scrollbars).
/// Re-measures as images load and as content reflows.
/// The reader's two grounds. A message sits on [`GROUND`]; in a conversation the
/// cards sit on the slightly deeper [`PAGE`], which is what makes them read as
/// cards. The spinner and the cover behind the WebView use the same pair, so
/// handing over to the document changes nothing on screen.
const GROUND: (&str, &str) = ("#ffffff", "#1e1e1e");
const PAGE: (&str, &str) = ("#f1f1f1", "#141414");
/// The window chrome's ground (stock GNOME `window_bg_color`): what the GTK
/// reader header — the subject block — sits on. A full-bleed single message
/// paints its in-document header this colour so the two read as one surface.
const CHROME: (&str, &str) = ("#fafafa", "#242424");

thread_local! {
    /// The grounds as resolved from the live libadwaita theme, refreshed by
    /// `document_html` just before each build (issue #62). The document
    /// builder is a static fn so tests can exercise it without a display —
    /// this hands it the theme without widening that signature. `None` (as in
    /// tests) falls back to the stock GNOME values above.
    static LIVE_GROUNDS: std::cell::RefCell<Option<(String, String, String)>> =
        const { std::cell::RefCell::new(None) };
}

/// That ground as a colour the WebView itself can be painted with.
fn ground_rgba(hex: &str) -> gtk::gdk::RGBA {
    let v = |i: usize| {
        u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0) as f32 / 255.0
    };
    gtk::gdk::RGBA::new(v(1), v(3), v(5), 1.0)
}

const SIZE_SCRIPT: &str = "\
function s(f){if(f._s)return;f._s=1;try{var d=f.contentDocument;if(!d)return;\
var b=d.body,e=d.documentElement;\
var prev=f.style.height;f.style.height='0px';void f.offsetHeight;\
var h=Math.max(b?b.scrollHeight:0,e?e.scrollHeight:0,b?b.offsetHeight:0);\
if(h>0){f.style.height=h+'px';\
if(f.dataset.key&&f._h!==h){f._h=h;\
try{window.webkit.messageHandlers.vireo.postMessage('size:'+f.dataset.key+':'+h);}catch(_){}}}\
else{f.style.height=prev;}}catch(_){}finally{f._s=0;}}\
function pick(k,e){var mo=e.shiftKey?'r':((e.ctrlKey||e.metaKey)?'t':'p');\
if(mo!=='p'){try{e.preventDefault();\
var g=(e.view&&e.view.getSelection)?e.view.getSelection():null;if(g)g.removeAllRanges();}catch(_){}}\
else{try{var t=(e.view&&e.view.getSelection)?e.view.getSelection():null;\
if(t&&String(t).length)return;}catch(_){}}\
try{window.webkit.messageHandlers.vireo.postMessage('sel:'+k+':'+mo);}catch(_){}}\
function quote(f){try{var d=f.contentDocument;if(!d||!d.body||f._q)return;\
var sel=['.vireo-quote-attr','.gmail_quote','blockquote[type=\"cite\"]','#divRplyFwdMsg','blockquote'];\
var q=null;for(var i=0;i<sel.length&&!q;i++)q=d.querySelector(sel[i]);\
if(!q)return;\
var top=q;while(top.parentNode&&top.parentNode!==d.body)top=top.parentNode;\
if(!top.parentNode)return;\
var before=false;\
for(var n=d.body.firstChild;n&&n!==top;n=n.nextSibling){\
if(n.nodeType===1||(n.nodeType===3&&n.textContent.trim()))before=true;}\
if(!before)return;\
f._q=1;\
var box=d.createElement('div');top.parentNode.insertBefore(box,top);\
while(box.nextSibling)box.appendChild(box.nextSibling);\
box.style.display='none';\
if(!f.parentNode)return;\
var b=document.createElement('button');b.className='vireo-quote';\
b.type='button';b.textContent='\u{2022}\u{2022}\u{2022}';\
b.setAttribute('title','Show quoted text');\
f.parentNode.insertBefore(b,f.nextSibling);\
b.addEventListener('click',function(e){e.stopPropagation();e.preventDefault();\
var on=box.style.display==='none';\
var from=f.getBoundingClientRect().height;\
box.style.display=on?'':'none';\
b.classList.toggle('open',on);\
b.setAttribute('title',on?'Hide quoted text':'Show quoted text');\
var to=0;try{var dd=f.contentDocument,bb=dd.body,ee=dd.documentElement;\
var prev=f.style.height;f.style.height='0px';void f.offsetHeight;\
to=Math.max(bb?bb.scrollHeight:0,ee?ee.scrollHeight:0,bb?bb.offsetHeight:0);\
f.style.height=prev;void f.offsetHeight;}catch(_){}\
if(!to){s(f);return;}\
f.style.height=from+'px';f.classList.add('anim');\
void f.offsetHeight;\
f.style.height=to+'px';\
setTimeout(function(){f.classList.remove('anim');s(f);},280);});}catch(_){}}\
function init(f){quote(f);s(f);try{var d=f.contentDocument;if(d){if(window.ResizeObserver&&d.body){new ResizeObserver(function(){s(f);}).observe(d.body);}\
if(f.dataset.key&&!f._c){f._c=1;d.addEventListener('click',function(e){\
if(e.target&&e.target.closest&&e.target.closest('a'))return;pick(f.dataset.key,e);});}\
var im=d.images||[];for(var i=0;i<im.length;i++){if(!im[i].complete){im[i].addEventListener('load',function(){s(f);});im[i].addEventListener('error',function(){s(f);});}}}}catch(_){}\
setTimeout(function(){s(f);},250);setTimeout(function(){s(f);},1000);}\
function all(){return document.querySelectorAll('iframe.vireo-frame');}\
document.addEventListener('DOMContentLoaded',function(){\
var fs=all();var pend=fs.length,rdy=false;\
function ready(){if(rdy)return;rdy=true;\
try{window.webkit.messageHandlers.vireo.postMessage('ready:0:0');}catch(_){}}\
if(!pend)ready();\
for(var i=0;i<fs.length;i++){(function(f){var counted=false;\
function tick(){if(counted)return;counted=true;if(--pend<=0)ready();}\
if(f.contentDocument&&f.contentDocument.readyState==='complete'){init(f);tick();}\
f.addEventListener('load',function(){init(f);tick();});})(fs[i]);}\
setTimeout(ready,450);\
var hs=document.querySelectorAll('.vireo-msg-hdr');\
for(var j=0;j<hs.length;j++){hs[j].addEventListener('dblclick',function(){\
try{window.webkit.messageHandlers.vireo.postMessage('open:'+this.dataset.key);}catch(_){}});}\
var rs=document.querySelectorAll('.vireo-rcpt-toggle');\
for(var r=0;r<rs.length;r++){rs[r].addEventListener('click',function(e){\
e.stopPropagation();e.preventDefault();\
var h=this.closest('.vireo-msg-hdr');var b=h?h.querySelector('.vireo-rcpt'):null;if(!b)return;\
var on=b.hasAttribute('hidden');\
if(on)b.removeAttribute('hidden');else b.setAttribute('hidden','');\
this.classList.toggle('open',on);\
this.setAttribute('title',on?'Hide recipients':'Show recipients');});\
rs[r].addEventListener('dblclick',function(e){e.stopPropagation();});}\
document.addEventListener('click',function(e){\
if(e.target&&e.target.closest&&e.target.closest('.vireo-msg'))return;\
try{window.webkit.messageHandlers.vireo.postMessage('desel:0:0');}catch(_){}});\
var ms=document.querySelectorAll('.vireo-msg');\
for(var q=0;q<ms.length;q++){ms[q].addEventListener('click',function(e){\
var k=this.dataset.key;if(k)pick(k,e);});}\
var as=document.querySelectorAll('.vireo-act');\
for(var k=0;k<as.length;k++){as[k].addEventListener('click',function(e){\
e.stopPropagation();e.preventDefault();\
try{window.webkit.messageHandlers.vireo.postMessage(this.dataset.act+':'+this.dataset.key);}catch(_){}});\
as[k].addEventListener('dblclick',function(e){e.stopPropagation();});}\
var es=document.querySelectorAll('.vireo-end');\
if(es.length&&window.IntersectionObserver){\
var io=new IntersectionObserver(function(en,ob){\
for(var n=0;n<en.length;n++){if(!en[n].isIntersecting)continue;\
var t=en[n].target,k=t.dataset.key;ob.unobserve(t);\
var d=document.querySelector('.vireo-dot[data-key=\"'+k+'\"]');if(d)d.remove();\
try{window.webkit.messageHandlers.vireo.postMessage('seen:'+k);}catch(_){}}});\
for(var m=0;m<es.length;m++)io.observe(es[m]);}});\
window.addEventListener('resize',function(){var fs=all();for(var i=0;i<fs.length;i++)s(fs[i]);});";

/// One message body as a sandboxed iframe: its own document (so CSS can't leak to
/// other messages) with no `allow-scripts` (so the email can't run JavaScript).
/// `allow-same-origin` lets the wrapper script measure its height.
fn message_frame(body: &str, restrict: bool, dark: bool, key: (u32, u32), height: Option<u32>) -> String {
    let doc = body_html(body);
    let doc = if restrict { strip_remote(&doc) } else { doc };
    // Dark mode: adapt the message's own colours so dark-on-dark text can't
    // happen (issue #35). `color-scheme` only helps unstyled mail; anything
    // that sets explicit dark text without a background needs its colours
    // transformed, and the sandboxed frames run no JS to do it live.
    let doc = if dark { adapt_colors_for_dark(&doc) } else { doc };
    let doc = inject_csp(&doc, !restrict, dark);
    format!(
        // `allow-same-origin` lets our wrapper script measure the frame height;
        // `allow-popups` lets `_blank` links reach the policy handler (which opens
        // them externally). No `allow-scripts`, so the email's own JS never runs.
        //
        // Opening at the height it had last time means the conversation lays out
        // correctly on its first frame. Without it every card starts at the
        // browser's default and jumps once its content is measured, which is a
        // visible lurch on a thread that has simply been reopened.
        "<iframe class=\"vireo-frame\" data-key=\"{aid}:{id}\"{style} \
         sandbox=\"allow-same-origin allow-popups\" srcdoc=\"{doc}\"></iframe>",
        aid = key.0,
        id = key.1,
        style = match height {
            Some(h) => format!(" style=\"height:{h}px\""),
            None => String::new(),
        },
        doc = attr_escape(&doc)
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
        escape_text(&m.from_name)
    } else if m.from_name.trim().is_empty() {
        escape_text(&m.from_addr)
    } else {
        format!(
            "{} &lt;{}&gt;",
            escape_text(&m.from_name),
            escape_text(&m.from_addr)
        )
    };
    format!(
        "<div class=\"vireo-print-msghdr\">{from} — {date}</div>",
        date = escape_text(&m.datetime_full())
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

/// Escape a string for use inside a double-quoted HTML **attribute** value
/// (e.g. `srcdoc`), and nothing else.
///
/// This deliberately leaves `<` and `>` alone — inside a quoted attribute they
/// are ordinary characters, and the `srcdoc` payload is a whole HTML document
/// that must survive intact. It is therefore **not** safe for element text
/// content: use [`escape_text`] there.
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
             body{{margin:0;padding:20px;font:14px/1.5 system-ui,sans-serif;\
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

    // ===== Dark-mode colour adaptation (issue #35) =====

    /// The core failure: near-black text with no background of its own must
    /// come out light, or it vanishes on the dark ground.
    #[test]
    fn dark_text_is_lightened() {
        let out = adapt_colors_for_dark(r#"<p style="color:#000">x</p>"#);
        assert_eq!(out, r#"<p style="color:#ffffff">x</p>"#);
        let out = adapt_colors_for_dark(r#"<p style="color: #333333;">x</p>"#);
        assert_eq!(out, r#"<p style="color: #cccccc;">x</p>"#);
        let out = adapt_colors_for_dark(r#"<p style="color:rgb(20, 20, 20)">x</p>"#);
        assert!(out.contains("color:#ebebeb"), "{out}");
    }

    /// Light backgrounds mirror down to surfaces; the floor keeps them above
    /// pure black so they still read as cards.
    #[test]
    fn light_backgrounds_are_darkened() {
        let out = adapt_colors_for_dark(r#"<td style="background-color:#ffffff">x</td>"#);
        assert_eq!(out, r#"<td style="background-color:#141414">x</td>"#);
        let out = adapt_colors_for_dark(r#"<td style="background:#f6f6f6 no-repeat">x</td>"#);
        assert!(out.contains("background:#141414 no-repeat"), "{out}");
    }

    /// Mail already designed for a dark ground passes through unchanged.
    #[test]
    fn dark_designed_mail_is_untouched() {
        let doc = r#"<div style="color:#eeeeee;background-color:#222222">x</div>"#;
        assert_eq!(adapt_colors_for_dark(doc), doc);
    }

    /// Legacy attributes carry colours too — `<font color>` and `bgcolor`,
    /// with or without the leading `#`.
    #[test]
    fn legacy_color_attributes_are_adapted() {
        let out = adapt_colors_for_dark(r##"<font color="#111111">x</font>"##);
        assert_eq!(out, r##"<font color="#eeeeee">x</font>"##);
        let out = adapt_colors_for_dark(r#"<table bgcolor="ffffff"><tr></tr></table>"#);
        assert_eq!(out, r##"<table bgcolor="#141414"><tr></tr></table>"##);
        let out = adapt_colors_for_dark(r#"<body text="black" bgcolor="white">x</body>"#);
        assert_eq!(out, r##"<body text="#ffffff" bgcolor="#141414">x</body>"##);
    }

    /// `<style>` blocks are rewritten rule by rule — selectors untouched,
    /// declarations adapted, nesting (`@media`) survives.
    #[test]
    fn style_blocks_are_adapted() {
        let doc = "<style>p{color:black}@media screen{.x{background:white}}</style><p>x</p>";
        let out = adapt_colors_for_dark(doc);
        assert!(out.contains("p{color:#ffffff}"), "{out}");
        assert!(out.contains(".x{background:#141414}"), "{out}");
        assert!(out.contains("@media screen"), "{out}");
    }

    /// A data: URL inside a background shorthand contains semicolons and
    /// base64 — it must pass through byte-for-byte while the colour beside it
    /// is still adapted.
    #[test]
    fn urls_survive_color_adaptation() {
        let doc = r#"<div style="background:url(data:image/png;base64,AAAA//12) #fff">x</div>"#;
        let out = adapt_colors_for_dark(doc);
        assert!(out.contains("url(data:image/png;base64,AAAA//12)"), "{out}");
        assert!(out.contains("#141414"), "{out}");
    }

    /// !important must survive, and colours in properties we don't understand
    /// must be left alone rather than guessed at.
    #[test]
    fn important_kept_and_unknown_props_untouched() {
        let out = adapt_colors_for_dark(r#"<p style="color:#000 !important">x</p>"#);
        assert!(out.contains("color:#ffffff !important"), "{out}");
        let doc = r#"<p style="border-color:#000;box-shadow:0 0 2px #000">x</p>"#;
        assert_eq!(adapt_colors_for_dark(doc), doc);
    }

    /// A CSS comment carrying a semicolon must not split the declaration
    /// after it — a real newsletter commented out one background and declared
    /// another right behind it, and the white slipped through (issue #35).
    #[test]
    fn comments_with_semicolons_do_not_hide_declarations() {
        let doc = "<style>.t{ /* background-color: #f0f2f5; */ background-color: #fff; }</style>";
        let out = adapt_colors_for_dark(doc);
        assert!(out.contains("background-color: #141414;"), "{out}");
        assert!(out.contains("/* background-color: #f0f2f5; */"), "comment kept: {out}");
        let out = adapt_colors_for_dark(r#"<p style="/* x; */ color: #000">x</p>"#);
        assert!(out.contains("/* x; */ color: #ffffff"), "{out}");
    }

    /// Mid-lightness brand colours sit fine on either ground: leave them.
    #[test]
    fn mid_tones_are_left_alone() {
        let doc = r#"<a style="color:#3584e4;background-color:#26a269">x</a>"#;
        assert_eq!(adapt_colors_for_dark(doc), doc);
    }

    /// The light path never rewrites anything.
    #[test]
    fn light_mode_frames_are_untouched() {
        let body = r#"<p style="color:#000">x</p>"#;
        let frame = message_frame(body, true, false, (1, 1), None);
        assert!(frame.contains("color:#000"), "{frame}");
        let frame = message_frame(body, true, true, (1, 1), None);
        assert!(!frame.contains("color:#000"), "{frame}");
    }

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

    /// A conversation is a stack of cards in the order it is handed, so the
    /// reader shows the message that started it first and the newest last.
    #[test]
    fn a_conversation_renders_one_card_per_message_in_order() {
        let mut first = msg_for_print();
        first.body = "<p>opening</p>".into();
        let mut second = msg_for_print();
        second.id = 2;
        second.from_name = "Grace Hopper".into();
        second.body = "<p>reply</p>".into();

        let doc = MessageView::conversation_document(
            &[first, second],
            &std::collections::HashMap::new(),
            &Default::default(),
            &Default::default(),
            &[],
            "#3584e4",
            true,
            false,
        );
        assert_eq!(
            doc.matches("<section class=\"vireo-msg\"").count(),
            2,
            "each message gets its own card: {doc}"
        );
        let ada = doc.find("Ada Lovelace").expect("first sender present");
        let grace = doc.find("Grace Hopper").expect("second sender present");
        assert!(ada < grace, "cards must keep the order they were handed");
        assert!(doc.contains("<body class=\"vireo-conv\">"), "conversation padding");
        // The cards sit on the deeper page — the same colour the spinner and the
        // cover behind the WebView are painted, so the handover is invisible.
        assert!(doc.contains(&format!("background:{}", PAGE.0)), "page ground: {doc}");
    }

    /// Each card names everyone the message went to, collapsed behind a chip so
    /// the header stays one line tall — and a recipient list is header text an
    /// attacker controls, so it must land escaped in the trusted wrapper.
    #[test]
    fn every_card_carries_its_recipients_escaped_and_collapsed() {
        let mut first = msg_for_print();
        first.to = "me@example.com, <script>alert(1)</script>@evil.test".into();
        let mut second = msg_for_print();
        second.id = 2;
        second.cc = String::new();

        let doc = MessageView::conversation_document(
            &[first, second],
            &std::collections::HashMap::new(),
            &Default::default(),
            &Default::default(),
            &[],
            "#3584e4",
            true,
            false,
        );
        assert_eq!(
            doc.matches("class=\"vireo-rcpt\" hidden").count(),
            2,
            "each card gets a collapsed To/Cc block: {doc}"
        );
        assert_eq!(
            doc.matches("class=\"vireo-rcpt-toggle\"").count(),
            2,
            "each card gets a recipients chip: {doc}"
        );
        // 3 addresses on the first card (2 To + 1 Cc), 1 on the second.
        assert!(doc.contains(">3 recipients<"), "recipient count on the chip: {doc}");
        assert!(doc.contains(">1 recipient<"), "singular for one recipient: {doc}");
        assert!(!doc.contains("<script>alert(1)</script>"), "recipients must be escaped: {doc}");
        assert!(doc.contains("&lt;script&gt;"), "escaped form present: {doc}");
    }

    /// A message that names no recipients at all gets neither chip nor block.
    #[test]
    fn a_card_with_no_recipients_shows_no_chip() {
        let mut only = msg_for_print();
        only.to = String::new();
        only.cc = " ".into();
        let mut second = msg_for_print();
        second.id = 2;
        let doc = MessageView::conversation_document(
            &[only, second],
            &std::collections::HashMap::new(),
            &Default::default(),
            &Default::default(),
            &[],
            "#3584e4",
            true,
            false,
        );
        assert_eq!(
            doc.matches("<button type=\"button\" class=\"vireo-rcpt-toggle\"").count(),
            1,
            "only the second card: {doc}"
        );
    }

    /// Each card carries its own Reply/Reply all/Forward, keyed to that message
    /// — the toolbar's buttons are disabled in a conversation precisely because
    /// they could not say which message they meant.
    #[test]
    fn every_card_carries_its_own_actions_keyed_to_that_message() {
        let first = msg_for_print();
        let mut second = msg_for_print();
        second.id = 2;
        let doc = MessageView::conversation_document(
            &[first, second],
            &std::collections::HashMap::new(),
            &Default::default(),
            &Default::default(),
            &[],
            "#3584e4",
            true,
            false,
        );
        for act in ["reply", "replyall", "forward"] {
            assert_eq!(
                doc.matches(&format!("data-act=\"{act}\"")).count(),
                2,
                "one {act} button per card: {doc}"
            );
        }
        // Keyed per message, so the action can't land on the wrong one.
        assert!(doc.contains("data-act=\"reply\" data-key=\"1:1\""), "{doc}");
        assert!(doc.contains("data-act=\"reply\" data-key=\"1:2\""), "{doc}");
    }

    /// An unread message in a conversation is marked, and carries the sentinel
    /// that reports when it has been scrolled through. A message already read
    /// carries neither — otherwise every re-render would re-mark it.
    #[test]
    fn only_unread_conversation_messages_are_marked() {
        let read = msg_for_print();
        let mut unread = msg_for_print();
        unread.id = 2;
        unread.unread = true;

        let doc = MessageView::conversation_document(
            &[read, unread],
            &std::collections::HashMap::new(),
            &Default::default(),
            &Default::default(),
            &[],
            "#3584e4",
            true,
            false,
        );
        assert_eq!(doc.matches("class=\"vireo-dot\"").count(), 1, "one dot: {doc}");
        assert_eq!(doc.matches("class=\"vireo-end\"").count(), 1, "one sentinel: {doc}");
        // Both keyed to the unread message, so reading it can't clear the other's.
        assert!(doc.contains("class=\"vireo-dot\" data-key=\"1:2\""), "{doc}");
        assert!(doc.contains("class=\"vireo-end\" data-key=\"1:2\""), "{doc}");
    }

    /// Marking a message unread while its conversation is open must survive the
    /// message being in view: it keeps its dot, but no sentinel, so nothing
    /// reads it back until the conversation is opened afresh.
    #[test]
    fn a_deliberately_unread_message_keeps_its_mark() {
        let read = msg_for_print();
        let mut unread = msg_for_print();
        unread.id = 2;
        unread.unread = true;
        let suppressed: std::collections::HashSet<(u32, u32)> = [(1u32, 2u32)].into();

        let doc = MessageView::conversation_document(
            &[read, unread],
            &std::collections::HashMap::new(),
            &suppressed,
            &Default::default(),
            &[],
            "#3584e4",
            true,
            false,
        );
        assert_eq!(doc.matches("class=\"vireo-dot\"").count(), 1, "still marked: {doc}");
        assert_eq!(doc.matches("class=\"vireo-end\"").count(), 0, "no sentinel: {doc}");
    }

    /// A frame opens at the height it had last time, so a reopened conversation
    /// lays out on its first frame rather than every card jumping once measured.
    #[test]
    fn a_known_frame_height_is_used_on_reopen() {
        let a = msg_for_print();
        let mut b = msg_for_print();
        b.id = 2;
        let heights: std::collections::HashMap<(u32, u32), u32> = [((1u32, 2u32), 640u32)].into();

        let doc = MessageView::conversation_document(
            &[a, b],
            &Default::default(),
            &Default::default(),
            &heights,
            &[],
            "#3584e4",
            true,
            false,
        );
        assert!(doc.contains("style=\"height:640px\""), "known height used: {doc}");
        assert_eq!(doc.matches("style=\"height:").count(), 1, "only the known one");
        // Keyed per message, so a height can't be applied to the wrong frame.
        assert!(doc.contains("data-key=\"1:2\""), "{doc}");
    }

    /// A selected message is outlined in the accent colour, and only that one.
    #[test]
    fn a_selected_card_is_outlined_in_the_accent() {
        let a = msg_for_print();
        let mut b = msg_for_print();
        b.id = 2;
        let doc = MessageView::conversation_document(
            &[a, b],
            &Default::default(),
            &Default::default(),
            &Default::default(),
            &[(1, 2)],
            "#ff8800",
            true,
            false,
        );
        assert!(doc.contains("class=\"vireo-msg selected\" data-key=\"1:2\""), "{doc}");
        assert_eq!(doc.matches("vireo-msg selected").count(), 1, "only the selected one");
        assert!(doc.contains("box-shadow:0 0 0 2px #ff8800"), "outlined in the accent: {doc}");
    }

    /// A lone message gets the same in-document header a conversation card
    /// does, but goes full-bleed: no card gutter (the vireo-conv padding), and
    /// no per-card action pills — it reads as a message, not a card.
    #[test]
    fn a_single_message_keeps_the_header_but_goes_full_bleed() {
        let doc = MessageView::conversation_document(
            &[msg_for_print()],
            &std::collections::HashMap::new(),
            &Default::default(),
            &Default::default(),
            &[],
            "#3584e4",
            true,
            false,
        );
        assert!(doc.contains("<section class=\"vireo-msg\""), "message chrome: {doc}");
        assert!(doc.contains("class=\"vireo-msg-hdr\""), "in-document header: {doc}");
        assert!(!doc.contains("<body class=\"vireo-conv\">"), "no card gutter: {doc}");
        assert!(!doc.contains("data-act=\"reply\""), "no card actions: {doc}");
        // Full-bleed also means the plain ground, not the cards' deeper page.
        assert!(doc.contains(&format!("background:{}", GROUND.0)), "plain ground: {doc}");
    }

    #[test]
    fn a_message_from_another_folder_is_labelled_with_it() {
        let a = msg_for_print(); // the message on screen
        let mut b = msg_for_print();
        b.id = 2;
        b.folder_id = 3; // pulled in from Sent
        let labels = std::collections::HashMap::from([((1u32, 2u32), "Sent".to_string())]);
        let doc = MessageView::conversation_document(&[a, b], &labels, &Default::default(), &Default::default(), &[], "#3584e4", true, false);
        assert_eq!(
            doc.matches("vireo-folder").count(),
            // once in the stylesheet, once on the message that came from Sent —
            // and never on the one the reader is already showing the folder of.
            2,
            "only the message from another folder should carry a folder badge"
        );
        assert!(doc.contains(">Sent</span>"), "the badge should name the folder");
    }

    #[test]
    fn a_folder_label_cannot_smuggle_markup_into_the_header() {
        // Folder names come from the server (a mailbox can be called anything),
        // and this document is the trusted wrapper around the sandboxed frames.
        let a = msg_for_print();
        let mut b = msg_for_print();
        b.id = 2;
        let labels =
            std::collections::HashMap::from([((1u32, 2u32), "<img src=x onerror=alert(1)>".into())]);
        let doc = MessageView::conversation_document(&[a, b], &labels, &Default::default(), &Default::default(), &[], "#3584e4", true, false);
        assert!(!doc.contains("<img src=x"), "the label must be escaped, not rendered");
        assert!(doc.contains("&lt;img src=x"));
    }

    #[test]
    fn a_sender_cannot_put_markup_in_the_conversation_header() {
        // The wrapper document has JavaScript enabled (it sizes the message
        // iframes) and the iframes are same-origin, so anything that executes
        // here can read every message body in the thread. A `From:` display name
        // is attacker-controlled and RFC 2047-decoded, so it can carry any bytes
        // at all — it has to reach the page as text, never as markup.
        let mut a = msg_for_print();
        a.from_name = "<script>x=1</script>".into();
        a.from_addr = "<img src=y onerror=z>@example.com".into();
        let mut b = msg_for_print();
        b.id = 2;
        // Two messages: the per-message headers only render in conversation mode.
        let doc = MessageView::conversation_document(&[a, b], &Default::default(), &Default::default(), &Default::default(),
            &[], "#3584e4", true, false);

        assert!(!doc.contains("<script>x=1"), "{doc}");
        assert!(!doc.contains("<img src=y"), "{doc}");
        assert!(doc.contains("&lt;script&gt;x=1&lt;/script&gt;"), "{doc}");
        assert!(doc.contains("onerror=z&gt;"), "{doc}");
    }

    #[test]
    fn the_wrapper_only_runs_its_own_script() {
        let mut a = msg_for_print();
        a.from_name = "Ada".into();
        let mut b = msg_for_print();
        b.id = 2;
        let doc = MessageView::conversation_document(&[a, b], &Default::default(), &Default::default(), &Default::default(),
            &[], "#3584e4", true, false);

        // A nonce'd CSP, so an injected `<script>` or `onerror=` is refused by
        // the engine even if the escaping above ever regresses.
        // The wrapper's own policy — not one of the frames', which are embedded
        // in this same string as escaped `srcdoc` payloads.
        let policy = doc
            .split("<meta http-equiv=\"Content-Security-Policy\" content=\"")
            .nth(1)
            .and_then(|r| r.split('"').next())
            .expect("the wrapper declares a CSP");
        let nonce = policy
            .split("script-src 'nonce-")
            .nth(1)
            .and_then(|r| r.split('\'').next())
            .expect("wrapper CSP declares a nonce");
        assert!(nonce.len() >= 16, "nonce is {} chars", nonce.len());
        assert!(doc.contains(&format!("<script nonce=\"{nonce}\">")), "{doc}");
        // The sizing script is the *only* script, and it carries the nonce.
        assert_eq!(doc.matches("<script").count(), 1, "{doc}");
        // No `default-src` here: a wrapper policy is inherited by the srcdoc
        // frames, and one would re-block content the user has allowed. Each
        // frame brings its own `default-src 'none'`.
        assert!(!policy.contains("default-src"), "{policy}");
        // Two renders never share a nonce.
        let again =
            MessageView::conversation_document(
                &[msg_for_print(), msg_for_print()],
                &Default::default(),
                &Default::default(),
                &Default::default(),
                &[],
                "#3584e4",
                true,
                false,
            );
        assert!(!again.contains(nonce), "nonce was reused across renders");
    }

    #[test]
    fn the_remote_detector_sees_past_the_obvious_spellings() {
        // Every row here loaded a tracking pixel with no banner shown, because
        // the old detector matched fixed substrings like `src="http`.
        for html in [
            // Protocol-relative: resolves against https://vireo.localhost.
            r#"<img src="//tracker.example/p.gif">"#,
            // HTML permits whitespace around `=`.
            r#"<img src = "http://tracker.example/p.gif">"#,
            // Neither `<svg><image href>` nor `poster` was in the attribute list.
            r#"<svg><image href="http://tracker.example/p.gif"/></svg>"#,
            r#"<video poster="http://tracker.example/p.gif">"#,
            // `@import` was only matched in its `url(http…)` spelling.
            r#"<style>@import url(//tracker.example/x.css)</style>"#,
            r#"<style>@import "https://tracker.example/x.css"</style>"#,
            // Case: the detector lowercased, the stripper did not.
            r#"<IMG SRC="HTTP://tracker.example/p.gif">"#,
            // Unquoted, and quoted with single quotes.
            r#"<img src=http://tracker.example/p.gif>"#,
            r#"<img src='//tracker.example/p.gif'>"#,
            r#"<div style="background:url('//tracker.example/p.gif')">"#,
            r#"<img srcset="//tracker.example/p.gif 1x, /local.gif 2x">"#,
            r#"<link rel="stylesheet" href="//tracker.example/x.css">"#,
        ] {
            assert!(has_remote_resources(html), "not detected: {html}");
            let stripped = strip_remote(html);
            assert!(
                !stripped.contains("tracker.example"),
                "not stripped: {html} -> {stripped}"
            );
        }
    }

    #[test]
    fn a_stylesheet_does_not_hide_what_follows_it() {
        // The walk has to resume after `</style>`, not treat the closing tag as
        // opening another stylesheet and take the rest of the document for CSS.
        let html = "<html><head><style>.a{background:url(https://cdn.example/b.png)}</style>\
                    </head><body><img src=\"https://cdn.example/i.png\"></body></html>";
        let out = strip_remote(html);
        assert!(!out.contains("cdn.example"), "{out}");
        assert_eq!(out.matches("blocked://").count(), 2, "{out}");
    }

    #[test]
    fn the_detector_leaves_self_contained_messages_alone() {
        // A false banner on ordinary mail teaches people to ignore the real one.
        for html in [
            r#"<img src="data:image/png;base64,iVBOR">"#,
            r#"<img src="cid:part1@example.com">"#,
            r#"<p>plain text, no resources at all</p>"#,
            // A link is a destination the user must click, not a fetch.
            r#"<a href="https://example.com/read-more">more</a>"#,
            r#"<div style="color:#333;font-weight:bold">styled</div>"#,
        ] {
            assert!(!has_remote_resources(html), "false positive: {html}");
            assert_eq!(strip_remote(html), html, "needlessly rewritten: {html}");
        }
    }

    #[test]
    fn blocking_follows_the_users_choice_not_the_detector() {
        // The point of the split: even for a body the detector says nothing
        // about, a frame built while remote content is disallowed carries the
        // restrictive policy. A detector miss costs a banner, not the blocking.
        let sneaky = r#"<img data-x="y" src="//tracker.example/p.gif">"#;
        let frame = message_frame(sneaky, true, false, (1, 1), None);
        assert!(frame.contains("img-src data: cid:"), "{frame}");
        assert!(!frame.contains("img-src http:"), "{frame}");
        assert!(!frame.contains("tracker.example"), "{frame}");

        // And once the user allows it, the same body renders untouched.
        let allowed = message_frame(sneaky, false, false, (1, 1), None);
        assert!(allowed.contains("img-src http: https:"), "{allowed}");
        assert!(allowed.contains("tracker.example"), "{allowed}");
    }

    #[test]
    fn only_web_and_mail_links_reach_the_desktop() {
        // An HTML body keeps its own `href` values, so a message can name any
        // scheme a third-party application has registered.
        for uri in ["http://example.com/", "https://example.com/", "MAILTO:a@b.c"] {
            assert!(is_launchable_uri(uri), "{uri}");
        }
        for uri in [
            "file:///etc/passwd",
            "smb://host/share",
            "nfs://host/export",
            "javascript:alert(1)",
            "data:text/html,<script>x</script>",
            "vscode://file/etc/passwd",
            "https:",
            "no-scheme-at-all",
        ] {
            assert!(!is_launchable_uri(uri), "{uri}");
        }
    }

    /// Renders the real wrapper document in a real WebView and reports what
    /// actually happened in the engine.
    ///
    /// Ignored by default: it needs a display and a WebKit process, which a CI
    /// runner has no business requiring. Run it by hand after touching the
    /// wrapper's CSP or its sizing script:
    ///
    /// ```text
    /// cargo test -- --ignored the_wrapper_in_a_real_engine
    /// ```
    #[test]
    #[ignore = "needs a display and a WebKit process"]
    fn the_wrapper_in_a_real_engine_sizes_frames_and_refuses_injected_script() {
        use gtk::prelude::*;
        use std::cell::RefCell;
        use std::rc::Rc;

        gtk::init().expect("a display");

        let mut a = msg_for_print();
        // If this ever executes, the assertions below see it.
        a.from_name = "<script>window.__pwned = 1</script>".into();
        a.body = "<p style=\"height:400px\">first</p>".into();
        let mut b = msg_for_print();
        b.id = 2;
        b.body = "<p style=\"height:300px\">second</p>".into();
        let html = MessageView::conversation_document(&[a, b], &Default::default(), &Default::default(), &Default::default(),
            &[], "#3584e4", true, false);

        let view = webkit6::WebView::new();
        let settings = webkit6::Settings::new();
        settings.set_enable_javascript(true);
        view.set_settings(&settings);
        // Off-screen is enough; WebKit still lays out and runs scripts.
        let win = gtk::Window::new();
        win.set_child(Some(&view));
        win.set_default_size(800, 600);
        win.present();
        view.load_html(&html, Some("https://vireo.localhost/message/1"));

        let answer: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let ctx = gtk::glib::MainContext::default();
        // Run the main loop until `done`, yielding when there is nothing to
        // dispatch. Spinning on a non-blocking iteration instead starves the
        // WebKit web process on a busy machine, which made this fail at random.
        let pump = |done: &dyn Fn() -> bool| {
            while !done() && std::time::Instant::now() < deadline {
                if !ctx.iteration(false) {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
            }
        };

        // Give the load and the two deferred `s(f)` passes time to run.
        let ready = Rc::new(std::cell::Cell::new(false));
        let r = ready.clone();
        view.connect_load_changed(move |_, ev| {
            if ev == webkit6::LoadEvent::Finished {
                r.set(true);
            }
        });
        pump(&|| ready.get());
        assert!(ready.get(), "the document never finished loading");
        // The sizing script measures on DOMContentLoaded and again on deferred
        // passes, so poll for the result rather than sampling once — sampling
        // made this fail under load, which is a flaky test, not a finding.
        // `ours` — not the frame heights — is what says the nonce'd script ran.
        // Heights come from `scrollHeight`, which is 0 until the window is
        // mapped and laid out, and whether a compositor has got round to that
        // has nothing to do with the CSP. The sizing script's top-level
        // functions become properties of `window` the moment it executes.
        let probe = "JSON.stringify({\
               pwned: typeof window.__pwned !== 'undefined',\
               ours: ['s','init','all'].every(f => typeof window[f] === 'function'),\
               heights: [...document.querySelectorAll('iframe.vireo-frame')]\
                          .map(f => f.style.height),\
               readable: (function(){ try { \
                 return document.querySelector('iframe.vireo-frame')\
                          .contentDocument.body.innerText.trim(); \
               } catch (e) { return 'blocked'; } })(),\
               from: document.querySelector('.vireo-from').textContent\
             })";
        let mut json = String::new();
        while std::time::Instant::now() < deadline {
            answer.replace(None);
            let out = answer.clone();
            view.evaluate_javascript(
                probe,
                None,
                None,
                gtk::gio::Cancellable::NONE,
                move |res| {
                    *out.borrow_mut() = Some(match res {
                        Ok(v) => v.to_str().to_string(),
                        Err(e) => format!("ERROR {e}"),
                    });
                },
            );
            pump(&|| answer.borrow().is_some());
            json = answer.borrow().clone().expect("the engine answered");
            if json.contains("\"ours\":true") {
                break;
            }
            let pause = std::time::Instant::now() + std::time::Duration::from_millis(250);
            pump(&|| std::time::Instant::now() >= pause);
        }

        // The injected script is inert: it survives as text in the header and
        // never runs.
        assert!(json.contains("\"pwned\":false"), "injected script ran: {json}");
        assert!(
            json.contains("<script>window.__pwned = 1</script>"),
            "the display name should read back as text: {json}"
        );
        // And our own nonce'd script did run.
        assert!(
            json.contains("\"ours\":true"),
            "the sizing script did not run, so the CSP nonce is not working: {json}"
        );
        // The frames are still same-origin, which is what the sizing relies on.
        assert!(json.contains("first"), "frame content unreadable: {json}");
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

#[cfg(test)]
mod scan_perf {
    use super::*;
    #[test]
    #[ignore = "timing-sensitive"]
    fn scanning_a_large_message_is_quick() {
        // A big marketing email: lots of tags, a large stylesheet, many images.
        let mut html = String::from("<html><head><style>");
        for i in 0..2000 {
            html.push_str(&format!(
                ".c{i}{{background:url(https://cdn.example/bg{i}.png);color:#333}}"
            ));
        }
        html.push_str("</style></head><body>");
        for i in 0..5000 {
            html.push_str(&format!(
                "<div class=\"c{i}\" style=\"padding:2px\"><img src=\"https://cdn.example/i{i}.png\"><a href=\"https://example.com/{i}\">x</a></div>"
            ));
        }
        html.push_str("</body></html>");
        eprintln!("body is {} KB", html.len() / 1024);

        let t = std::time::Instant::now();
        assert!(has_remote_resources(&html));
        let detect = t.elapsed();
        let t = std::time::Instant::now();
        let stripped = strip_remote(&html);
        let strip = t.elapsed();
        eprintln!("detect {detect:?}, strip {strip:?}");
        assert!(!stripped.contains("cdn.example"));
        // Links are left alone.
        assert!(stripped.contains("https://example.com/4999"));
        assert!(detect.as_millis() < 250, "detection took {detect:?}");
        assert!(strip.as_millis() < 250, "stripping took {strip:?}");
    }
}

