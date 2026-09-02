//! Compose window: write a new message, reply, or forward.

use adw::prelude::*;
use relm4::prelude::*;

use crate::contacts::Suggestion;
use crate::models::DraftOrigin;
use crate::ui::rich_editor::{self, RichEditor};
use crate::worker::OutgoingMessage;

/// Which recipient field a suggestion is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    To,
    Cc,
    Bcc,
}

/// A signature block as HTML (`-- ` delimiter). The stored signature is HTML
/// (legacy plain-text signatures are converted).
fn sig_html(sig: &str) -> String {
    let body = rich_editor::signature_to_html(sig);
    format!("<div class=\"vireo-sig\"><br>-- <br>{body}</div>")
}

/// Size the pane for its host. Both hosts impose a definite height now — the
/// reader-covering overlay inline (it fills the whole pane), the window itself
/// popped out — so the editor always expands to fill whatever it is given.
/// Inline, the compose header also stands in for the reader's (which it
/// covers), so it takes over the GNOME window decorations.
fn size_for_host(
    root: &adw::ToolbarView,
    header: &adw::HeaderBar,
    editor_holder: &gtk::Box,
    windowed: bool,
) {
    root.set_vexpand(true);
    editor_holder.set_vexpand(true);
    editor_holder.set_height_request(-1);
    header.set_show_end_title_buttons(!windowed);
}

/// Set the inline/window toggle button's icon + tooltip for the current host.
fn set_toggle_icon(btn: &gtk::Button, windowed: bool) {
    if windowed {
        btn.set_icon_name("co.hyprlab.Vireo-view-restore-symbolic");
        btn.set_tooltip_text(Some("Collapse into reader"));
    } else {
        btn.set_icon_name("co.hyprlab.Vireo-view-fullscreen-symbolic");
        btn.set_tooltip_text(Some("Open in window"));
    }
}

/// Replace the recipient currently being typed (after the last comma) with the
/// chosen suggestion, leaving a trailing ", " ready for the next recipient.
fn complete_field(row: &adw::EntryRow, sug: &Suggestion) {
    let text = row.text().to_string();
    let prefix_len = text.rfind(',').map(|i| i + 1).unwrap_or(0);
    let prefix = &text[..prefix_len];
    row.set_text(&format!("{}{}, ", prefix, sug.display()));
    row.set_position(-1);
}

/// One selectable "from" account in the compose window.
#[derive(Debug, Clone)]
pub struct ComposeAccount {
    pub id: u32,
    pub label: String,
    /// Signature text appended to the body when this account is selected.
    pub signature: String,
    /// The identity's sending address (the account's own, or an alias's).
    pub email: String,
    /// Set for a send-as alias (#34): the full From to put on the wire
    /// ("Name <alias@host>"). `None` sends as the account itself.
    pub alias_from: Option<String>,
}

/// Initial field contents (empty for a new message; populated for reply/forward).
#[derive(Debug, Default)]
pub struct ComposePrefill {
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub subject: String,
    /// HTML prefill placed into the rich editor (e.g. a quoted reply/forward).
    pub body_html: String,
    /// Files to attach on open, already on disk (a queued message's attachments
    /// are written out before its composer opens).
    pub attachments: Vec<std::path::PathBuf>,
    /// Threading headers for a reply: the parent's Message-ID, and the thread's
    /// id chain. Both stored bare (no angle brackets).
    pub in_reply_to: String,
    pub references: String,
    /// When editing an existing draft, its origin (so saving/sending replaces it).
    pub draft_origin: Option<DraftOrigin>,
    /// When editing a queued Outbox message, the row this replaces.
    pub outbox_origin: Option<u32>,
    /// For a reply: the original's To+Cc, so the composer can answer from the
    /// alias the mail was addressed to (#34). Empty otherwise.
    pub reply_addressed_to: String,
}

/// Everything the compose pane needs to open.
#[derive(Debug)]
pub struct ComposeInit {
    /// Stable id so the app can track this composer across inline/window moves.
    pub compose_id: u32,
    pub prefill: ComposePrefill,
    pub accounts: Vec<ComposeAccount>,
    /// Index into `accounts` of the account to send from by default.
    pub selected: usize,
    /// Recipient autocomplete suggestions (Contacts + mail history).
    pub suggestions: Vec<Suggestion>,
    /// Whether the pane starts hosted in a standalone window (vs. inline).
    pub windowed: bool,
    /// Whether the inline/window toggle button is offered (reply/forward only).
    pub can_toggle: bool,
    /// Compact reply (#86 follow-up): the split reply hides every address/
    /// subject row and shows just the editor — popping out to a window brings
    /// the full fields back.
    pub compact: bool,
}

pub struct Compose {
    accounts: Vec<ComposeAccount>,
    /// The rich-text (HTML) body editor.
    editor: RichEditor,
    /// Signature currently appended to the body (so it can be swapped out).
    current_sig: String,
    /// Files to attach.
    attachments: Vec<std::path::PathBuf>,
    /// When editing a queued Outbox message, the row this replaces once sent.
    outbox_origin: Option<u32>,
    /// Recipient suggestions, filtered as the user types.
    suggestions: Vec<Suggestion>,
    /// Shared autocomplete popover and which field it's currently attached to.
    completion: gtk::Popover,
    completion_field: Option<Field>,
    /// The list inside the popover + the keyboard-highlighted row, for arrow-key nav.
    completion_list: Option<gtk::ListBox>,
    completion_selected: usize,
    completion_count: usize,
    /// Whether the popover is showing (read synchronously by the key handler).
    completion_open: std::rc::Rc<std::cell::Cell<bool>>,
    /// Threading headers carried from the message being replied to.
    in_reply_to: String,
    references: String,
    /// When editing an existing draft, its origin (replaced on save/send).
    draft_origin: Option<DraftOrigin>,
    /// Stable id the app uses to track this composer across host moves.
    compose_id: u32,
    /// Currently shown as a standalone window (drives the toggle-button icon).
    windowed: bool,
    /// Whether this composer offers the inline/window toggle at all.
    can_toggle: bool,
    compact: bool,
    /// A recipient/subject field was edited since open (body edits are tracked
    /// separately by the editor itself). Used for save-if-dirty.
    fields_dirty: bool,
}

#[derive(Debug)]
pub enum ComposeInput {
    Send,
    /// The editor's HTML + plain text came back asynchronously — finish sending.
    SendBody { html: String, text: String, to: String, cc: String, bcc: String, reply_to: String, subject: String, from_account_id: u32, from_alias: Option<String> },
    /// Save the current message to Drafts.
    SaveDraft,
    /// The editor content came back — finish saving the draft.
    SaveDraftBody { html: String, text: String, to: String, cc: String, bcc: String, reply_to: String, subject: String, from_account_id: u32, from_alias: Option<String> },
    Cancel,
    /// The user clicked the inline/window toggle button.
    ToggleWindowed,
    /// The app moved this pane between inline and window; sync the button icon.
    SetWindowed(bool),
    /// Re-grab keyboard focus into the editor (after a host move).
    FocusEditor,
    /// A recipient/subject field changed — mark dirty.
    MarkFieldsDirty,
    /// Save to Drafts only if edited, then close (used when superseded / on nav).
    SaveDraftIfDirty,
    AccountChanged,
    AttachFiles,
    AddAttachments(Vec<std::path::PathBuf>),
    RemoveAttachment(usize),
    OpenContacts,
    /// The given recipient field changed — refresh autocomplete.
    Suggest(Field),
    /// Arrow-key move of the autocomplete highlight (+1 down, -1 up).
    CompletionMove(i32),
    /// Accept the highlighted suggestion into the active field.
    CompletionAccept,
    /// Dismiss the autocomplete popover.
    CompletionClose,
}

#[derive(Debug)]
pub enum ComposeOutput {
    Send(Box<OutgoingMessage>),
    /// Save the message to the Drafts folder (no send).
    SaveDraft(Box<OutgoingMessage>),
    /// Ask the app to promote/demote this pane (inline ↔ window). Carries the id.
    ToggleWindow(u32),
    /// This pane is done (cancelled / sent / draft-saved / superseded). Carries
    /// the id so the app tears down the right host.
    Close(u32),
}

#[relm4::component(pub)]
impl Component for Compose {
    type Init = ComposeInit;
    type Input = ComposeInput;
    type Output = ComposeOutput;
    type CommandOutput = ();

    view! {
        // Host-agnostic root: the same pane is shown inline (in a reader Revealer)
        // or set as the content of an app-owned window. Hosting/close is the app's
        // job (see ComposeOutput::ToggleWindow / Close).
        adw::ToolbarView {
                #[name = "header"]
                add_top_bar = &adw::HeaderBar {
                    set_show_start_title_buttons: false,
                    set_show_end_title_buttons: false,
                    // No "Vireo" branding on the compose bar.
                    #[wrap(Some)]
                    set_title_widget = &gtk::Label {
                        set_label: "",
                    },

                    pack_start = &gtk::Button {
                        set_label: "Cancel",
                        connect_clicked => ComposeInput::Cancel,
                    },
                    pack_start = &gtk::Button {
                        set_label: "Save Draft",
                        set_tooltip_text: Some("Save to Drafts"),
                        connect_clicked => ComposeInput::SaveDraft,
                    },
                    pack_end = &gtk::Button {
                        set_label: "Send",
                        add_css_class: "suggested-action",
                        connect_clicked => ComposeInput::Send,
                    },
                    pack_end = &gtk::Button {
                        set_icon_name: "co.hyprlab.Vireo-mail-attachment-symbolic",
                        set_tooltip_text: Some("Attach files"),
                        connect_clicked => ComposeInput::AttachFiles,
                    },
                    pack_end = &gtk::Button {
                        set_icon_name: "co.hyprlab.Vireo-x-office-address-book-symbolic",
                        set_tooltip_text: Some("Open Contacts"),
                        connect_clicked => ComposeInput::OpenContacts,
                    },
                    // Promote inline reply → window, or collapse window → inline.
                    // Icon/visibility set in `init` and on SetWindowed.
                    #[name = "toggle_btn"]
                    pack_end = &gtk::Button {
                        set_tooltip_text: Some("Open in window"),
                        connect_clicked => ComposeInput::ToggleWindowed,
                    },
                },

                #[wrap(Some)]
                set_content = &gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,
                    set_spacing: 12,
                    add_css_class: "compose-pane",

                    // From and To are always offered, inline included — a
                    // forward is unaddressable without To (#25, #52). Cc, Bcc,
                    // and (for replies/forwards) the prefilled Subject wait
                    // behind the To row's "More" button; per-row visibility is
                    // set in `init`.
                    #[name = "fields_list"]
                    gtk::ListBox {
                        add_css_class: "boxed-list",
                        add_css_class: "compose-fields",
                        set_selection_mode: gtk::SelectionMode::None,

                        #[name = "from_row"]
                        adw::ComboRow {
                            set_title: "From",
                            connect_selected_notify => ComposeInput::AccountChanged,
                        },
                        #[name = "to_row"]
                        adw::EntryRow {
                            set_title: "To",
                            set_input_purpose: gtk::InputPurpose::Email,
                        },
                        #[name = "cc_row"]
                        adw::EntryRow {
                            set_title: "Cc",
                            set_input_purpose: gtk::InputPurpose::Email,
                        },
                        #[name = "bcc_row"]
                        adw::EntryRow {
                            set_title: "Bcc",
                            set_input_purpose: gtk::InputPurpose::Email,
                        },
                        #[name = "reply_to_row"]
                        adw::EntryRow {
                            set_title: "Reply-To",
                            set_input_purpose: gtk::InputPurpose::Email,
                        },
                        #[name = "subject_row"]
                        adw::EntryRow {
                            set_title: "Subject",
                        },
                    },

                    #[name = "attach_box"]
                    gtk::FlowBox {
                        set_selection_mode: gtk::SelectionMode::None,
                        set_column_spacing: 6,
                        set_row_spacing: 6,
                        set_max_children_per_line: 4,
                        set_visible: false,
                    },

                    // Holder for the shared rich-text editor (toolbar + body),
                    // appended in `init`.
                    #[name = "editor_holder"]
                    gtk::Box {
                        set_orientation: gtk::Orientation::Vertical,
                        set_vexpand: true,
                    },
                },
        }
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let ComposeInit {
            compose_id,
            prefill,
            accounts,
            selected,
            suggestions,
            windowed,
            can_toggle,
            compact,
        } = init;
        let in_reply_to = prefill.in_reply_to.clone();
        let references = prefill.references.clone();
        let draft_origin = prefill.draft_origin.clone();
        let outbox_origin = prefill.outbox_origin;
        let prefill_attachments = prefill.attachments.clone();
        let current_sig = accounts.get(selected).map(|a| a.signature.clone()).unwrap_or_default();

        let completion = gtk::Popover::new();
        completion.set_autohide(false); // don't steal focus from the entry
        completion.set_can_focus(false);
        completion.set_position(gtk::PositionType::Bottom);
        completion.add_css_class("menu");

        // Initial editor content: a blank line to type on, the quoted
        // reply/forward (if any), then the signature.
        let mut content = String::from("<div><br></div>");
        if !prefill.body_html.is_empty() {
            content.push_str(&prefill.body_html);
        }
        // A draft already contains its signature; don't add another.
        if draft_origin.is_none() && !current_sig.is_empty() {
            content.push_str(&sig_html(&current_sig));
        }
        let editor = RichEditor::new(&content);

        let model = Compose {
            accounts,
            editor,
            current_sig,
            attachments: prefill_attachments,
            suggestions,
            completion,
            completion_field: None,
            completion_list: None,
            completion_selected: 0,
            completion_count: 0,
            completion_open: std::rc::Rc::new(std::cell::Cell::new(false)),
            in_reply_to,
            references,
            draft_origin,
            outbox_origin,
            compose_id,
            windowed,
            can_toggle,
            compact,
            fields_dirty: false,
        };
        let widgets = view_output!();
        widgets.editor_holder.append(&model.editor.widget);

        // The inline/window toggle: only reply/forward panes can toggle. Its icon
        // reflects the current host (fullscreen = "expand to window", restore =
        // "collapse back inline").
        widgets.toggle_btn.set_visible(model.can_toggle);
        set_toggle_icon(&widgets.toggle_btn, model.windowed);
        size_for_host(&root, &widgets.header, &widgets.editor_holder, model.windowed);

        // Per-row visibility (#25): To always; Cc/Bcc only when prefilled (a
        // reply-all carries Cc). The Subject is always shown — replies and
        // forwards arrive with it prefilled, but it stays the user's to see
        // and change (2026-08-31).
        let cc_shown = !prefill.cc.trim().is_empty();
        let bcc_shown = !prefill.bcc.trim().is_empty();
        widgets.cc_row.set_visible(cc_shown);
        widgets.bcc_row.set_visible(bcc_shown);
        // Reply-To (#58) is rare enough to always start hidden behind "More".
        widgets.reply_to_row.set_visible(false);
        widgets.subject_row.set_visible(true);
        // Compact split reply: only the editor shows; the full field rows
        // return when the composer pops out to a window.
        widgets.fields_list.set_visible(!model.compact);
        {
            let more = gtk::Button::with_label("More");
            more.add_css_class("flat");
            more.set_valign(gtk::Align::Center);
            more.set_tooltip_text(Some("Show Cc, Bcc and Reply-To"));
            let cc = widgets.cc_row.clone();
            let bcc = widgets.bcc_row.clone();
            let reply_to = widgets.reply_to_row.clone();
            let btn = more.clone();
            more.connect_clicked(move |_| {
                cc.set_visible(true);
                bcc.set_visible(true);
                reply_to.set_visible(true);
                btn.set_visible(false);
            });
            widgets.to_row.add_suffix(&more);
        }

        // Populate the From dropdown.
        let labels: Vec<&str> = model.accounts.iter().map(|a| a.label.as_str()).collect();
        let strings = gtk::StringList::new(&labels);
        widgets.from_row.set_model(Some(&strings));
        // Custom factory so the selected account isn't needlessly ellipsized.
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(|_, item| {
            if let Some(item) = item.downcast_ref::<gtk::ListItem>() {
                let label = gtk::Label::new(None);
                label.set_xalign(0.0);
                label.set_ellipsize(gtk::pango::EllipsizeMode::None);
                item.set_child(Some(&label));
            }
        });
        factory.connect_bind(|_, item| {
            if let Some(item) = item.downcast_ref::<gtk::ListItem>() {
                let text = item
                    .item()
                    .and_downcast::<gtk::StringObject>()
                    .map(|s| s.string().to_string())
                    .unwrap_or_default();
                if let Some(label) = item.child().and_downcast::<gtk::Label>() {
                    label.set_label(&text);
                }
            }
        });
        widgets.from_row.set_factory(Some(&factory));
        widgets.from_row.set_selected(selected as u32);
        widgets.from_row.set_visible(model.accounts.len() > 1);

        widgets.to_row.set_text(&prefill.to);
        widgets.cc_row.set_text(&prefill.cc);
        widgets.bcc_row.set_text(&prefill.bcc);
        widgets.subject_row.set_text(&prefill.subject);
        if !model.attachments.is_empty() {
            model.rebuild_attachments(&widgets.attach_box, &sender);
        }

        // Wire autocomplete *after* prefilling, so the initial text doesn't pop it.
        for (row, field) in [
            (&widgets.to_row, Field::To),
            (&widgets.cc_row, Field::Cc),
            (&widgets.bcc_row, Field::Bcc),
        ] {
            let s = sender.clone();
            row.connect_changed(move |_| {
                s.input(ComposeInput::Suggest(field));
                s.input(ComposeInput::MarkFieldsDirty);
            });

            // Close the popover when the field loses focus.
            let focus = gtk::EventControllerFocus::new();
            let s = sender.clone();
            focus.connect_leave(move |_| s.input(ComposeInput::CompletionClose));
            row.add_controller(focus);
        }
        // Subject edits also count as dirtying the draft.
        let s = sender.clone();
        widgets
            .subject_row
            .connect_changed(move |_| s.input(ComposeInput::MarkFieldsDirty));

        // Drive the suggestion list from a single capture-phase key handler on
        // the window — the toplevel sees every key first, regardless of focus.
        let key = gtk::EventControllerKey::new();
        key.set_propagation_phase(gtk::PropagationPhase::Capture);
        let s = sender.clone();
        let open = model.completion_open.clone();
        let editor = model.editor.clone();
        key.connect_key_pressed(move |_, keyval, _, state| {
            use gtk::glib::Propagation;
            // Ctrl+V pastes in the preferred mode, Ctrl+Shift+V in the
            // opposite one (a shifted V arrives capitalised). Only over the
            // body: the address and subject entries are plain text by nature,
            // and the focus guard leaves their Ctrl+V untouched.
            if state.contains(gtk::gdk::ModifierType::CONTROL_MASK) && editor.has_focus() {
                let rich = crate::config::load_paste_rich();
                if keyval == gtk::gdk::Key::v {
                    editor.paste(rich);
                    return Propagation::Stop;
                }
                if keyval == gtk::gdk::Key::V {
                    editor.paste(!rich);
                    return Propagation::Stop;
                }
            }
            if !open.get() {
                // Escape backs out of the whole composer — the same as Cancel,
                // so an accidental reply is one key away from being undone. Only
                // once the suggestion list is closed, which Escape dismisses
                // first (below), so one press never does both.
                if keyval == gtk::gdk::Key::Escape {
                    s.input(ComposeInput::Cancel);
                    return Propagation::Stop;
                }
                return Propagation::Proceed;
            }
            // Compare by value (the const-as-pattern match wasn't matching).
            if keyval == gtk::gdk::Key::Down {
                s.input(ComposeInput::CompletionMove(1));
                Propagation::Stop
            } else if keyval == gtk::gdk::Key::Up {
                s.input(ComposeInput::CompletionMove(-1));
                Propagation::Stop
            } else if keyval == gtk::gdk::Key::Return || keyval == gtk::gdk::Key::KP_Enter {
                s.input(ComposeInput::CompletionAccept);
                Propagation::Stop
            } else if keyval == gtk::gdk::Key::Escape {
                s.input(ComposeInput::CompletionClose);
                Propagation::Stop
            } else {
                Propagation::Proceed
            }
        });
        root.add_controller(key);

        if prefill.to.is_empty() {
            widgets.to_row.grab_focus();
        } else {
            model.editor.grab_focus();
        }

        ComponentParts { model, widgets }
    }

    fn update_with_view(
        &mut self,
        widgets: &mut Self::Widgets,
        message: Self::Input,
        sender: ComponentSender<Self>,
        root: &Self::Root,
    ) {
        match message {
            ComposeInput::Cancel => {
                let _ = sender.output(ComposeOutput::Close(self.compose_id));
            }

            ComposeInput::ToggleWindowed => {
                let _ = sender.output(ComposeOutput::ToggleWindow(self.compose_id));
            }

            ComposeInput::SetWindowed(windowed) => {
                self.windowed = windowed;
                set_toggle_icon(&widgets.toggle_btn, windowed);
                size_for_host(root, &widgets.header, &widgets.editor_holder, windowed);
                // A compact reply grows its field rows back in a window (and
                // sheds them again if it returns inline).
                widgets.fields_list.set_visible(!(self.compact && !windowed));
            }

            ComposeInput::FocusEditor => self.editor.grab_focus(),

            ComposeInput::MarkFieldsDirty => self.fields_dirty = true,

            ComposeInput::SaveDraftIfDirty => {
                // Save only if the user actually edited something, so navigating
                // away from a pristine quote-only reply doesn't litter Drafts.
                if self.fields_dirty {
                    sender.input(ComposeInput::SaveDraft);
                } else {
                    let s = sender.clone();
                    let id = self.compose_id;
                    self.editor.is_dirty(move |body_dirty| {
                        if body_dirty {
                            s.input(ComposeInput::SaveDraft);
                        } else {
                            let _ = s.output(ComposeOutput::Close(id));
                        }
                    });
                }
            }

            ComposeInput::OpenContacts => {
                // Browse contacts; the chosen one is appended to the To field.
                let Some(win) = root.root().and_downcast::<gtk::Window>() else {
                    return;
                };
                let to_row = widgets.to_row.clone();
                crate::ui::contacts_browser::present(&win, move |contact| {
                    let display = if contact.name.trim().is_empty()
                        || contact.name == contact.email
                    {
                        contact.email.clone()
                    } else {
                        format!("{} <{}>", contact.name, contact.email)
                    };
                    let cur = to_row.text().to_string();
                    let trimmed = cur.trim_end();
                    let sep = if trimmed.is_empty() {
                        ""
                    } else if trimmed.ends_with(',') {
                        " "
                    } else {
                        ", "
                    };
                    to_row.set_text(&format!("{cur}{sep}{display}, "));
                    to_row.set_position(-1);
                });
            }

            ComposeInput::AttachFiles => {
                let dialog = gtk::FileDialog::new();
                dialog.set_title("Attach Files");
                let parent = root.root().and_downcast::<gtk::Window>();
                let s = sender.input_sender().clone();
                dialog.open_multiple(
                    parent.as_ref(),
                    gtk::gio::Cancellable::NONE,
                    move |res| {
                        if let Ok(model) = res {
                            let mut paths = Vec::new();
                            for i in 0..model.n_items() {
                                if let Some(file) =
                                    model.item(i).and_downcast::<gtk::gio::File>()
                                {
                                    if let Some(p) = file.path() {
                                        paths.push(p);
                                    }
                                }
                            }
                            if !paths.is_empty() {
                                let _ = s.send(ComposeInput::AddAttachments(paths));
                            }
                        }
                    },
                );
            }

            ComposeInput::AddAttachments(paths) => {
                self.attachments.extend(paths);
                self.rebuild_attachments(&widgets.attach_box, &sender);
            }

            ComposeInput::RemoveAttachment(i) => {
                if i < self.attachments.len() {
                    self.attachments.remove(i);
                    self.rebuild_attachments(&widgets.attach_box, &sender);
                }
            }

            ComposeInput::AccountChanged => {
                // Swap the editor's signature block for the new account's.
                let idx = widgets.from_row.selected() as usize;
                let new_sig = self.accounts.get(idx).map(|a| a.signature.clone()).unwrap_or_default();
                if new_sig != self.current_sig {
                    let replacement = if new_sig.is_empty() {
                        String::new()
                    } else {
                        sig_html(&new_sig)
                    };
                    let js = format!(
                        "(function(){{var s=document.querySelector('.vireo-sig');\
                         var h='{}';\
                         if(s){{if(h){{s.outerHTML=h;}}else{{s.remove();}}}}\
                         else if(h){{document.body.insertAdjacentHTML('beforeend',h);}}}})()",
                        rich_editor::js_escape(&replacement)
                    );
                    self.editor.run_js(&js);
                    self.current_sig = new_sig;
                }
            }

            ComposeInput::Suggest(field) => {
                let row = match field {
                    Field::To => &widgets.to_row,
                    Field::Cc => &widgets.cc_row,
                    Field::Bcc => &widgets.bcc_row,
                };
                self.show_completion(field, row);
            }

            ComposeInput::CompletionMove(delta) => {
                if self.completion_count == 0 {
                    return;
                }
                let max = self.completion_count as i32 - 1;
                let new = (self.completion_selected as i32 + delta).clamp(0, max) as usize;
                self.completion_selected = new;
                if let Some(list) = &self.completion_list {
                    if let Some(row) = list.row_at_index(new as i32) {
                        list.select_row(Some(&row));
                    }
                }
            }

            ComposeInput::CompletionAccept => {
                let row = match self.completion_field {
                    Some(Field::To) => &widgets.to_row,
                    Some(Field::Cc) => &widgets.cc_row,
                    Some(Field::Bcc) => &widgets.bcc_row,
                    None => return,
                };
                let text = row.text().to_string();
                let token = text.rsplit(',').next().unwrap_or("").trim().to_string();
                if !token.is_empty() {
                    let chosen = self.ranked_matches(&token).into_iter().nth(self.completion_selected);
                    if let Some(sug) = chosen {
                        complete_field(row, &sug);
                    }
                }
                self.completion_open.set(false);
                self.completion.popdown();
            }

            ComposeInput::CompletionClose => {
                self.completion_open.set(false);
                self.completion.popdown();
            }

            ComposeInput::Send => {
                let to = widgets.to_row.text().trim().to_string();
                if to.is_empty() {
                    widgets.to_row.add_css_class("error");
                    return;
                }
                let cc = widgets.cc_row.text().trim().to_string();
                let bcc = widgets.bcc_row.text().trim().to_string();
                let reply_to = widgets.reply_to_row.text().trim().to_string();
                let subject = widgets.subject_row.text().to_string();
                let idx = widgets.from_row.selected() as usize;
                let from_account_id = self.accounts.get(idx).map(|a| a.id).unwrap_or(1);
                let from_alias = self.accounts.get(idx).and_then(|a| a.alias_from.clone());

                // Pull the HTML and a plain-text version out of the editor (async),
                // then finish sending via SendBody.
                let s = sender.clone();
                self.editor.extract(move |html, text| {
                    s.input(ComposeInput::SendBody {
                        html,
                        text,
                        to: to.clone(),
                        cc: cc.clone(),
                        bcc: bcc.clone(),
                        reply_to: reply_to.clone(),
                        subject: subject.clone(),
                        from_account_id,
                        from_alias: from_alias.clone(),
                    });
                });
            }

            ComposeInput::SendBody { html, text, to, cc, bcc, reply_to, subject, from_account_id, from_alias } => {
                let out = self
                    .build_outgoing(from_account_id, from_alias, to, cc, bcc, reply_to, subject, text, html);
                let _ = sender.output(ComposeOutput::Send(Box::new(out)));
                let _ = sender.output(ComposeOutput::Close(self.compose_id));
            }

            ComposeInput::SaveDraft => {
                // A draft can be saved without recipients; just capture the fields.
                let to = widgets.to_row.text().trim().to_string();
                let cc = widgets.cc_row.text().trim().to_string();
                let bcc = widgets.bcc_row.text().trim().to_string();
                let reply_to = widgets.reply_to_row.text().trim().to_string();
                let subject = widgets.subject_row.text().to_string();
                let idx = widgets.from_row.selected() as usize;
                let from_account_id = self.accounts.get(idx).map(|a| a.id).unwrap_or(1);
                let from_alias = self.accounts.get(idx).and_then(|a| a.alias_from.clone());
                let s = sender.clone();
                self.editor.extract(move |html, text| {
                    s.input(ComposeInput::SaveDraftBody {
                        html,
                        text,
                        to: to.clone(),
                        cc: cc.clone(),
                        bcc: bcc.clone(),
                        reply_to: reply_to.clone(),
                        subject: subject.clone(),
                        from_account_id,
                        from_alias: from_alias.clone(),
                    });
                });
            }

            ComposeInput::SaveDraftBody { html, text, to, cc, bcc, reply_to, subject, from_account_id, from_alias } => {
                let out = self
                    .build_outgoing(from_account_id, from_alias, to, cc, bcc, reply_to, subject, text, html);
                let _ = sender.output(ComposeOutput::SaveDraft(Box::new(out)));
                let _ = sender.output(ComposeOutput::Close(self.compose_id));
            }
        }
    }
}

impl Compose {
    /// Assemble an [`OutgoingMessage`] from the composed fields + attachments,
    /// carrying the draft origin so a saved/sent draft replaces its predecessor.
    #[allow(clippy::too_many_arguments)]
    fn build_outgoing(
        &self,
        from_account_id: u32,
        from_alias: Option<String>,
        to: String,
        cc: String,
        bcc: String,
        reply_to: String,
        subject: String,
        text: String,
        html: String,
    ) -> OutgoingMessage {
        OutgoingMessage {
            from_account_id,
            from_alias,
            to,
            cc,
            bcc,
            reply_to,
            subject,
            body: text,
            html,
            attachments: self
                .attachments
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect(),
            in_reply_to: self.in_reply_to.clone(),
            references: self.references.clone(),
            draft_origin: self.draft_origin.clone(),
            outbox_origin: self.outbox_origin,
        }
    }

    /// Suggestions matching `token`, ranked best-first (prefix match, then most
    /// frequently used), capped to a handful.
    fn ranked_matches(&self, token: &str) -> Vec<Suggestion> {
        let q = token.to_lowercase();
        let mut matches: Vec<Suggestion> =
            self.suggestions.iter().filter(|s| s.matches(token)).cloned().collect();
        matches.sort_by(|a, b| {
            let pa = a.email.to_lowercase().starts_with(&q) || a.name.to_lowercase().starts_with(&q);
            let pb = b.email.to_lowercase().starts_with(&q) || b.name.to_lowercase().starts_with(&q);
            pb.cmp(&pa)
                .then(b.score.cmp(&a.score))
                .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        matches.truncate(8);
        matches
    }

    /// Filter suggestions by the recipient fragment being typed and show them in
    /// a popover under the field; clicking one completes that recipient.
    fn show_completion(&mut self, field: Field, row: &adw::EntryRow) {
        let text = row.text().to_string();
        // The recipient currently being typed is the part after the last comma.
        let token = text.rsplit(',').next().unwrap_or("").trim().to_string();
        if token.is_empty() {
            self.completion_open.set(false);
            self.completion.popdown();
            return;
        }
        let matches = self.ranked_matches(&token);
        if matches.is_empty() {
            self.completion_open.set(false);
            self.completion.popdown();
            return;
        }

        // Attach the popover to the active field (only re-parent when it moves).
        if self.completion_field != Some(field) {
            if self.completion.parent().is_some() {
                self.completion.unparent();
            }
            self.completion.set_parent(row);
            self.completion_field = Some(field);
        }

        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Single);
        // Keep focus in the entry so its key controller drives navigation.
        list.set_can_focus(false);
        list.add_css_class("autocomplete");
        let count = matches.len();
        for sug in matches {
            let item = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            item.set_margin_start(6);
            item.set_margin_end(6);
            item.set_margin_top(3);
            item.set_margin_bottom(3);
            // Mark where the suggestion came from: address book vs. mail history.
            let icon = gtk::Image::from_icon_name(if sug.from_contacts {
                "co.hyprlab.Vireo-avatar-default-symbolic"
            } else {
                "co.hyprlab.Vireo-document-open-recent-symbolic"
            });
            icon.set_valign(gtk::Align::Center);
            icon.add_css_class("dim-label");
            item.append(&icon);
            let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
            let title = gtk::Label::new(Some(&sug.name));
            title.set_halign(gtk::Align::Start);
            title.set_xalign(0.0);
            text.append(&title);
            if sug.name != sug.email {
                let sub = gtk::Label::new(Some(&sug.email));
                sub.set_halign(gtk::Align::Start);
                sub.set_xalign(0.0);
                sub.add_css_class("dim-label");
                sub.add_css_class("caption");
                text.append(&sub);
            }
            item.append(&text);
            let lbr = gtk::ListBoxRow::new();
            lbr.set_can_focus(false);
            lbr.set_child(Some(&item));
            list.append(&lbr);

            // Complete this recipient on click.
            let row2 = row.clone();
            let pop = self.completion.downgrade();
            let sug = sug.clone();
            let gesture = gtk::GestureClick::new();
            gesture.connect_released(move |_, _, _, _| {
                complete_field(&row2, &sug);
                if let Some(p) = pop.upgrade() {
                    p.popdown();
                }
            });
            lbr.add_controller(gesture);
        }

        // Highlight the first suggestion so Enter accepts it immediately.
        if let Some(first) = list.row_at_index(0) {
            list.select_row(Some(&first));
        }
        self.completion_selected = 0;
        self.completion_count = count;
        self.completion_list = Some(list.clone());

        self.completion.set_child(Some(&list));
        self.completion.popup();
        self.completion_open.set(true);
    }

    fn rebuild_attachments(&self, flow: &gtk::FlowBox, sender: &ComponentSender<Self>) {
        while let Some(child) = flow.first_child() {
            flow.remove(&child);
        }
        for (i, path) in self.attachments.iter().enumerate() {
            let name = path
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "file".to_string());

            let chip = gtk::Box::new(gtk::Orientation::Horizontal, 4);
            chip.add_css_class("attach-chip");
            // FlowBoxChild defaults to halign: Fill, which would otherwise
            // stretch this box the full width of its cell — leaving the pill's
            // background trailing well past the remove button. Hug the content.
            chip.set_halign(gtk::Align::Start);
            chip.append(&gtk::Image::from_icon_name("co.hyprlab.Vireo-mail-attachment-symbolic"));
            let lbl = gtk::Label::new(Some(&name));
            lbl.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
            lbl.set_max_width_chars(22);
            chip.append(&lbl);
            let rm = gtk::Button::from_icon_name("co.hyprlab.Vireo-window-close-symbolic");
            rm.add_css_class("flat");
            rm.set_valign(gtk::Align::Center);
            let s = sender.input_sender().clone();
            rm.connect_clicked(move |_| {
                let _ = s.send(ComposeInput::RemoveAttachment(i));
            });
            chip.append(&rm);

            flow.append(&chip);
            // GtkFlowBox auto-wraps `chip` in a FlowBoxChild that, unlike
            // `chip` itself, has no halign we can set beforehand — it still
            // fills (and hover-highlights) the full cell. Shrink it to the
            // pill's own size and drop its own row interactivity, since the
            // remove button inside is the only real click target.
            if let Some(cell) = chip.parent().and_downcast::<gtk::FlowBoxChild>() {
                cell.set_halign(gtk::Align::Start);
                cell.set_can_focus(false);
                cell.set_focusable(false);
            }
        }
        flow.set_visible(!self.attachments.is_empty());
    }
}
