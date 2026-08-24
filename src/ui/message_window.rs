//! A message popped out into its own top-level window: a standalone reader that
//! carries the exact same toolbar actions as the main window's reader pane.
//!
//! The window owns its own [`MessageView`] and attachment state, but defers the
//! real work (reply, move, load attachments, …) back to the app via outputs so
//! behaviour stays identical to the main window.

use adw::prelude::*;
use relm4::prelude::*;

use crate::models::{Attachment, Message};
use crate::ui::message_list::RowAction;
use crate::ui::message_view::{MessageView, MessageViewInput, MessageViewOutput};

/// Everything needed to open a popout reader for one message.
#[derive(Debug)]
pub struct MessageWindowInit {
    pub message: Message,
    pub gravatar: bool,
    /// Whether the sender circle is drawn (#29).
    pub avatars: bool,
    /// Whether a sender's site icon may fill it (#30).
    pub sender_logos: bool,
    pub account_name: Option<String>,
    pub account_color: Option<String>,
    pub allow_remote: bool,
    /// The body is still being fetched — show a spinner.
    pub loading: bool,
    pub attachments: Vec<Attachment>,
    /// Attachments exist on the server but aren't downloaded yet.
    pub attachments_available: bool,
    pub attachments_loading: bool,
    /// Message-content theme override (`None` follows the system).
    pub content_dark: Option<bool>,
}

pub struct MessageWindow {
    msg: Message,
    view: Controller<MessageView>,
    gravatar: bool,
    account_name: Option<String>,
    account_color: Option<String>,
    allow_remote: bool,
    loading: bool,
    attachments: Vec<Attachment>,
    attachments_available: bool,
    attachments_loading: bool,
    attach_list: gtk::Box,
}

#[derive(Debug)]
pub enum MessageWindowInput {
    /// Show or hide the sender circle.
    SetAvatars(bool),
    /// Turn sender logos on or off.
    SetSenderLogos(bool),
    /// Print this message (Ctrl+P), the same as in the main window.
    Print,
    /// Preview it as a PDF (Ctrl+Shift+P).
    PrintPreview,
    /// No-op (unreachable output mapping).
    Ignore,
    /// The message body arrived from the server.
    SetBody(String),
    /// The sender-authentication verdict for this message.
    SetSenderCheck(Box<crate::models::SenderCheck>),
    /// Reflect a star toggle that happened elsewhere (or came back from the app).
    SetStarred(bool),
    /// Downloaded attachments are now available.
    SetAttachments(Vec<Attachment>),
    /// Attachments exist but need an explicit download.
    AttachmentsPending,
    /// Update the message-content theme (`None` follows the system).
    SetContentTheme(Option<bool>),
    // ---- toolbar actions ----
    Reply,
    ReplyAll,
    Forward,
    AddToContacts,
    ToggleStar,
    Delete,
    Archive,
    Spam,
    ViewSource,
    LoadAttachmentsNow,
    OpenAttachment(usize),
    SaveAllAttachments,
    // ---- from the embedded reader ----
    AllowSender(String),
    ComposeTo(String),
}

#[derive(Debug)]
pub enum MessageWindowOutput {
    /// A per-message action handled exactly like a list/context-menu action.
    Action { action: RowAction, message: Box<Message> },
    /// Add this sender to Contacts.
    AddToContacts { name: String, email: String },
    /// Download this message's attachments from the server.
    LoadAttachments(Box<Message>),
    /// Open a single attachment.
    OpenAttachment(Box<Attachment>),
    /// Save every attachment.
    SaveAllAttachments(Vec<Attachment>),
    /// Persist a remote-content allowlist entry.
    AllowSender(String),
    /// Compose to an address.
    ComposeTo(String),
    /// The window was closed.
    Closed,
}

#[relm4::component(pub)]
impl Component for MessageWindow {
    type Init = MessageWindowInit;
    type Input = MessageWindowInput;
    type Output = MessageWindowOutput;
    type CommandOutput = ();

    view! {
        adw::Window {
            set_title: Some(&title_text(&model.msg)),
            set_default_width: 720,
            set_default_height: 820,

            connect_close_request[sender] => move |_| {
                let _ = sender.output(MessageWindowOutput::Closed);
                gtk::glib::Propagation::Proceed
            },

            #[wrap(Some)]
            set_content = &adw::ToolbarView {
                add_top_bar = &adw::HeaderBar {
                    add_css_class: "flat",
                    #[wrap(Some)]
                    set_title_widget = &gtk::Label {
                        #[watch]
                        set_label: &title_text(&model.msg),
                        add_css_class: "pane-title",
                        set_ellipsize: gtk::pango::EllipsizeMode::End,
                    },
                    pack_start = &gtk::Button {
                        set_icon_name: "co.hyprlab.Vireo-mail-reply-sender-symbolic",
                        set_tooltip_text: Some("Reply"),
                        add_css_class: "flat",
                        connect_clicked => MessageWindowInput::Reply,
                    },
                    pack_start = &gtk::Button {
                        set_icon_name: "co.hyprlab.Vireo-mail-reply-all-symbolic",
                        set_tooltip_text: Some("Reply All"),
                        add_css_class: "flat",
                        connect_clicked => MessageWindowInput::ReplyAll,
                    },
                    pack_start = &gtk::Button {
                        set_icon_name: "co.hyprlab.Vireo-mail-forward-symbolic",
                        set_tooltip_text: Some("Forward"),
                        add_css_class: "flat",
                        connect_clicked => MessageWindowInput::Forward,
                    },
                    pack_start = &gtk::Button {
                        set_icon_name: "co.hyprlab.Vireo-contact-new-symbolic",
                        set_tooltip_text: Some("Add sender to Contacts"),
                        add_css_class: "flat",
                        connect_clicked => MessageWindowInput::AddToContacts,
                    },
                    pack_start = &gtk::Button {
                        set_tooltip_text: Some("Flag"),
                        add_css_class: "flat",
                        #[watch]
                        set_icon_name: if model.msg.starred {
                            "co.hyprlab.Vireo-starred-symbolic"
                        } else {
                            "co.hyprlab.Vireo-non-starred-symbolic"
                        },
                        connect_clicked => MessageWindowInput::ToggleStar,
                    },
                    pack_end = &gtk::Button {
                        set_icon_name: "co.hyprlab.Vireo-user-trash-symbolic",
                        set_tooltip_text: Some("Delete"),
                        add_css_class: "flat",
                        connect_clicked => MessageWindowInput::Delete,
                    },
                    pack_end = &gtk::Button {
                        set_icon_name: "co.hyprlab.Vireo-background-app-ghost-symbolic",
                        set_tooltip_text: Some("View Source"),
                        add_css_class: "flat",
                        connect_clicked => MessageWindowInput::ViewSource,
                    },
                    pack_end = &gtk::Button {
                        set_icon_name: "co.hyprlab.Vireo-mail-mark-junk-symbolic",
                        set_tooltip_text: Some("Mark as Spam"),
                        add_css_class: "flat",
                        connect_clicked => MessageWindowInput::Spam,
                    },
                    pack_end = &gtk::Button {
                        set_icon_name: "co.hyprlab.Vireo-mail-archive-symbolic",
                        set_tooltip_text: Some("Archive"),
                        add_css_class: "flat",
                        connect_clicked => MessageWindowInput::Archive,
                    },
                    pack_end = &gtk::Spinner {
                        set_valign: gtk::Align::Center,
                        set_tooltip_text: Some("Downloading attachments…"),
                        #[watch]
                        set_spinning: model.attachments_loading,
                        #[watch]
                        set_visible: model.attachments_loading,
                    },
                    pack_end = &gtk::Button {
                        set_icon_name: "co.hyprlab.Vireo-folder-download-symbolic",
                        set_tooltip_text: Some("Load attachments from server"),
                        add_css_class: "flat",
                        add_css_class: "attach-present",
                        #[watch]
                        set_visible: model.attachments_available && !model.attachments_loading,
                        connect_clicked => MessageWindowInput::LoadAttachmentsNow,
                    },
                    pack_end = &gtk::MenuButton {
                        set_icon_name: "co.hyprlab.Vireo-mail-attachment-symbolic",
                        set_tooltip_text: Some("Attachments"),
                        add_css_class: "flat",
                        add_css_class: "attach-present",
                        #[watch]
                        set_visible: !model.attachments.is_empty(),
                        #[wrap(Some)]
                        set_popover = &gtk::Popover {
                            #[local_ref]
                            attach_list -> gtk::Box {
                                set_orientation: gtk::Orientation::Vertical,
                                set_spacing: 4,
                                set_width_request: 260,
                            },
                        },
                    },
                },
                #[wrap(Some)]
                set_content = model.view.widget(),
            },
        }
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let view = MessageView::builder()
            .launch(())
            .forward(sender.input_sender(), |out| match out {
                MessageViewOutput::AllowSender(addr) => MessageWindowInput::AllowSender(addr),
                MessageViewOutput::ComposeTo(addr) => MessageWindowInput::ComposeTo(addr),
                // A popout shows a single message (no conversation headers), so
                // neither of these fires; map them to a no-op to keep the match
                // total. The window's own toolbar carries Reply and Forward.
                MessageViewOutput::OpenWindow(_) => MessageWindowInput::Ignore,
                MessageViewOutput::CardAction { .. } => MessageWindowInput::Ignore,
            });
        // Apply the message-content theme before the first render.
        view.emit(MessageViewInput::SetContentTheme(init.content_dark));

        view.emit(MessageViewInput::SetAvatars(init.avatars));
        view.emit(MessageViewInput::SetSenderLogos(init.sender_logos));

        let model = MessageWindow {
            msg: init.message,
            view,
            gravatar: init.gravatar,
            account_name: init.account_name,
            account_color: init.account_color,
            allow_remote: init.allow_remote,
            loading: init.loading,
            attachments: init.attachments,
            attachments_available: init.attachments_available,
            attachments_loading: init.attachments_loading,
            attach_list: gtk::Box::new(gtk::Orientation::Vertical, 0),
        };

        let attach_list = model.attach_list.clone();
        let widgets = view_output!();

        // Ctrl+P prints, matching the main window. This window has no menu bar to
        // hang an action off, so the accelerator is wired directly.
        {
            let keys = gtk::EventControllerKey::new();
            let s = sender.clone();
            keys.connect_key_pressed(move |_, keyval, _, state| {
                if state.contains(gtk::gdk::ModifierType::CONTROL_MASK) {
                    // Shift+P is the preview; the keyval arrives capitalised.
                    if keyval == gtk::gdk::Key::P {
                        s.input(MessageWindowInput::PrintPreview);
                        return gtk::glib::Propagation::Stop;
                    }
                    if keyval == gtk::gdk::Key::p {
                        s.input(MessageWindowInput::Print);
                        return gtk::glib::Propagation::Stop;
                    }
                }
                gtk::glib::Propagation::Proceed
            });
            root.add_controller(keys);
        }

        model.render_body();
        model.rebuild_attach_popover(&sender);

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>, root: &Self::Root) {
        match msg {
            MessageWindowInput::Ignore => {}
            MessageWindowInput::SetAvatars(on) => {
                self.view.emit(MessageViewInput::SetAvatars(on));
            }
            MessageWindowInput::SetSenderLogos(on) => {
                self.view.emit(MessageViewInput::SetSenderLogos(on));
            }
            MessageWindowInput::SetContentTheme(o) => {
                self.view.emit(MessageViewInput::SetContentTheme(o));
            }
            MessageWindowInput::SetSenderCheck(check) => {
                self.view.emit(MessageViewInput::SetSenderCheck(check));
            }
            MessageWindowInput::SetBody(body) => {
                self.msg.body = body;
                self.loading = false;
                self.render_body();
            }
            MessageWindowInput::SetStarred(starred) => {
                self.msg.starred = starred;
            }
            MessageWindowInput::SetAttachments(items) => {
                self.attachments = items;
                self.attachments_available = false;
                self.attachments_loading = false;
                self.rebuild_attach_popover(&sender);
            }
            MessageWindowInput::AttachmentsPending => {
                self.attachments_available = true;
                self.attachments_loading = false;
            }
            MessageWindowInput::Reply => self.emit_action(RowAction::Reply, &sender),
            MessageWindowInput::ReplyAll => self.emit_action(RowAction::ReplyAll, &sender),
            MessageWindowInput::Forward => self.emit_action(RowAction::Forward, &sender),
            MessageWindowInput::ToggleStar => self.emit_action(RowAction::ToggleStar, &sender),
            MessageWindowInput::ViewSource => self.emit_action(RowAction::ViewSource, &sender),
            // Moving the message away — let the app handle it, then close.
            MessageWindowInput::Print => self.view.emit(MessageViewInput::Print),

            MessageWindowInput::PrintPreview => self.view.emit(MessageViewInput::PrintPreview),

            MessageWindowInput::Delete => {
                self.emit_action(RowAction::Delete, &sender);
                root.close();
            }
            MessageWindowInput::Archive => {
                self.emit_action(RowAction::Archive, &sender);
                root.close();
            }
            MessageWindowInput::Spam => {
                self.emit_action(RowAction::Spam, &sender);
                root.close();
            }
            MessageWindowInput::AddToContacts => {
                let _ = sender.output(MessageWindowOutput::AddToContacts {
                    name: self.msg.from_name.clone(),
                    email: self.msg.from_addr.clone(),
                });
            }
            MessageWindowInput::LoadAttachmentsNow => {
                self.attachments_available = false;
                self.attachments_loading = true;
                let _ = sender.output(MessageWindowOutput::LoadAttachments(Box::new(self.msg.clone())));
            }
            MessageWindowInput::OpenAttachment(i) => {
                if let Some(att) = self.attachments.get(i) {
                    let _ = sender.output(MessageWindowOutput::OpenAttachment(Box::new(att.clone())));
                }
            }
            MessageWindowInput::SaveAllAttachments => {
                let _ = sender.output(MessageWindowOutput::SaveAllAttachments(self.attachments.clone()));
            }
            MessageWindowInput::AllowSender(addr) => {
                let _ = sender.output(MessageWindowOutput::AllowSender(addr));
            }
            MessageWindowInput::ComposeTo(addr) => {
                let _ = sender.output(MessageWindowOutput::ComposeTo(addr));
            }
        }
    }
}

impl MessageWindow {
    fn emit_action(&self, action: RowAction, sender: &ComponentSender<Self>) {
        let _ = sender.output(MessageWindowOutput::Action {
            action,
            message: Box::new(self.msg.clone()),
        });
    }

    fn render_body(&self) {
        self.view.emit(MessageViewInput::Show {
            thread: vec![self.msg.clone()],
            allow_remote: self.allow_remote,
            gravatar: self.gravatar,
            account_name: self.account_name.clone(),
            account_color: self.account_color.clone(),
            primary: None,
            folder_labels: std::collections::HashMap::new(),
            loading: self.loading,
        });
    }

    /// Rebuild the attachments popover (a row per attachment + "Save All").
    fn rebuild_attach_popover(&self, sender: &ComponentSender<Self>) {
        while let Some(child) = self.attach_list.first_child() {
            self.attach_list.remove(&child);
        }
        for (i, att) in self.attachments.iter().enumerate() {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            row.add_css_class("attach-row");

            let info = gtk::Box::new(gtk::Orientation::Vertical, 0);
            info.set_hexpand(true);
            let name = gtk::Label::new(Some(&att.name));
            name.set_halign(gtk::Align::Start);
            name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
            name.set_max_width_chars(28);
            let size = gtk::Label::new(Some(&att.human_size()));
            size.set_halign(gtk::Align::Start);
            size.add_css_class("dim-label");
            size.add_css_class("caption");
            info.append(&name);
            info.append(&size);
            row.append(&info);

            let open = gtk::Button::with_label("Open");
            open.add_css_class("flat");
            open.set_valign(gtk::Align::Center);
            let s = sender.input_sender().clone();
            open.connect_clicked(move |_| {
                let _ = s.send(MessageWindowInput::OpenAttachment(i));
            });
            row.append(&open);
            self.attach_list.append(&row);
        }
        if !self.attachments.is_empty() {
            self.attach_list
                .append(&gtk::Separator::new(gtk::Orientation::Horizontal));
            let save = gtk::Button::with_label("Save All…");
            save.add_css_class("flat");
            let s = sender.input_sender().clone();
            save.connect_clicked(move |_| {
                let _ = s.send(MessageWindowInput::SaveAllAttachments);
            });
            self.attach_list.append(&save);
        }
    }
}

/// The window/title text for a message (its subject, or a placeholder).
fn title_text(m: &Message) -> String {
    if m.subject.trim().is_empty() {
        "(No subject)".to_string()
    } else {
        m.subject.clone()
    }
}
