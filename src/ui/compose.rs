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
    format!("<div class=\"veem-sig\"><br>-- <br>{body}</div>")
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
}

/// Initial field contents (empty for a new message; populated for reply/forward).
#[derive(Debug, Default)]
pub struct ComposePrefill {
    pub to: String,
    pub cc: String,
    pub subject: String,
    /// HTML prefill placed into the rich editor (e.g. a quoted reply/forward).
    pub body_html: String,
    /// When editing an existing draft, its origin (so saving/sending replaces it).
    pub draft_origin: Option<DraftOrigin>,
}

/// Everything the compose window needs to open.
#[derive(Debug)]
pub struct ComposeInit {
    pub prefill: ComposePrefill,
    pub accounts: Vec<ComposeAccount>,
    /// Index into `accounts` of the account to send from by default.
    pub selected: usize,
    /// Recipient autocomplete suggestions (Contacts + mail history).
    pub suggestions: Vec<Suggestion>,
}

pub struct Compose {
    accounts: Vec<ComposeAccount>,
    /// The rich-text (HTML) body editor.
    editor: RichEditor,
    /// Signature currently appended to the body (so it can be swapped out).
    current_sig: String,
    /// Files to attach.
    attachments: Vec<std::path::PathBuf>,
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
    /// When editing an existing draft, its origin (replaced on save/send).
    draft_origin: Option<DraftOrigin>,
}

#[derive(Debug)]
pub enum ComposeInput {
    Send,
    /// The editor's HTML + plain text came back asynchronously — finish sending.
    SendBody { html: String, text: String, to: String, cc: String, bcc: String, subject: String, from_account_id: u32 },
    /// Save the current message to Drafts.
    SaveDraft,
    /// The editor content came back — finish saving the draft.
    SaveDraftBody { html: String, text: String, to: String, cc: String, bcc: String, subject: String, from_account_id: u32 },
    Cancel,
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
    Closed,
}

#[relm4::component(pub)]
impl Component for Compose {
    type Init = ComposeInit;
    type Input = ComposeInput;
    type Output = ComposeOutput;
    type CommandOutput = ();

    view! {
        adw::Window {
            set_modal: false,
            set_default_width: 660,
            set_default_height: 760,
            set_title: Some("New Message"),

            connect_close_request[sender] => move |_| {
                let _ = sender.output(ComposeOutput::Closed);
                gtk::glib::Propagation::Proceed
            },

            #[wrap(Some)]
            set_content = &adw::ToolbarView {
                add_top_bar = &adw::HeaderBar {
                    set_show_start_title_buttons: false,
                    set_show_end_title_buttons: false,

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
                        set_icon_name: "com.getveem.Veem-mail-attachment-symbolic",
                        set_tooltip_text: Some("Attach files"),
                        connect_clicked => ComposeInput::AttachFiles,
                    },
                    pack_end = &gtk::Button {
                        set_icon_name: "com.getveem.Veem-x-office-address-book-symbolic",
                        set_tooltip_text: Some("Open Contacts"),
                        connect_clicked => ComposeInput::OpenContacts,
                    },
                },

                #[wrap(Some)]
                set_content = &gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,
                    set_spacing: 12,
                    add_css_class: "compose-pane",

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
            },
        }
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let ComposeInit { prefill, accounts, selected, suggestions } = init;
        let draft_origin = prefill.draft_origin.clone();
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
            attachments: Vec::new(),
            suggestions,
            completion,
            completion_field: None,
            completion_list: None,
            completion_selected: 0,
            completion_count: 0,
            completion_open: std::rc::Rc::new(std::cell::Cell::new(false)),
            draft_origin,
        };
        let widgets = view_output!();
        widgets.editor_holder.append(&model.editor.widget);

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
        widgets.subject_row.set_text(&prefill.subject);

        // Wire autocomplete *after* prefilling, so the initial text doesn't pop it.
        for (row, field) in [
            (&widgets.to_row, Field::To),
            (&widgets.cc_row, Field::Cc),
            (&widgets.bcc_row, Field::Bcc),
        ] {
            let s = sender.clone();
            row.connect_changed(move |_| s.input(ComposeInput::Suggest(field)));

            // Close the popover when the field loses focus.
            let focus = gtk::EventControllerFocus::new();
            let s = sender.clone();
            focus.connect_leave(move |_| s.input(ComposeInput::CompletionClose));
            row.add_controller(focus);
        }

        // Drive the suggestion list from a single capture-phase key handler on
        // the window — the toplevel sees every key first, regardless of focus.
        let key = gtk::EventControllerKey::new();
        key.set_propagation_phase(gtk::PropagationPhase::Capture);
        let s = sender.clone();
        let open = model.completion_open.clone();
        key.connect_key_pressed(move |_, keyval, _, _| {
            use gtk::glib::Propagation;
            if !open.get() {
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
            ComposeInput::Cancel => root.close(),

            ComposeInput::OpenContacts => {
                // Browse contacts; the chosen one is appended to the To field.
                let to_row = widgets.to_row.clone();
                crate::ui::contacts_browser::present(root, move |contact| {
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
                let s = sender.input_sender().clone();
                dialog.open_multiple(
                    Some(root),
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
                        "(function(){{var s=document.querySelector('.veem-sig');\
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
                let subject = widgets.subject_row.text().to_string();
                let idx = widgets.from_row.selected() as usize;
                let from_account_id = self.accounts.get(idx).map(|a| a.id).unwrap_or(1);

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
                        subject: subject.clone(),
                        from_account_id,
                    });
                });
            }

            ComposeInput::SendBody { html, text, to, cc, bcc, subject, from_account_id } => {
                let out = self.build_outgoing(from_account_id, to, cc, bcc, subject, text, html);
                let _ = sender.output(ComposeOutput::Send(Box::new(out)));
                root.close();
            }

            ComposeInput::SaveDraft => {
                // A draft can be saved without recipients; just capture the fields.
                let to = widgets.to_row.text().trim().to_string();
                let cc = widgets.cc_row.text().trim().to_string();
                let bcc = widgets.bcc_row.text().trim().to_string();
                let subject = widgets.subject_row.text().to_string();
                let idx = widgets.from_row.selected() as usize;
                let from_account_id = self.accounts.get(idx).map(|a| a.id).unwrap_or(1);
                let s = sender.clone();
                self.editor.extract(move |html, text| {
                    s.input(ComposeInput::SaveDraftBody {
                        html,
                        text,
                        to: to.clone(),
                        cc: cc.clone(),
                        bcc: bcc.clone(),
                        subject: subject.clone(),
                        from_account_id,
                    });
                });
            }

            ComposeInput::SaveDraftBody { html, text, to, cc, bcc, subject, from_account_id } => {
                let out = self.build_outgoing(from_account_id, to, cc, bcc, subject, text, html);
                let _ = sender.output(ComposeOutput::SaveDraft(Box::new(out)));
                root.close();
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
        to: String,
        cc: String,
        bcc: String,
        subject: String,
        text: String,
        html: String,
    ) -> OutgoingMessage {
        OutgoingMessage {
            from_account_id,
            to,
            cc,
            bcc,
            subject,
            body: text,
            html,
            attachments: self
                .attachments
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect(),
            draft_origin: self.draft_origin.clone(),
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
                "com.getveem.Veem-avatar-default-symbolic"
            } else {
                "com.getveem.Veem-document-open-recent-symbolic"
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
            chip.append(&gtk::Image::from_icon_name("com.getveem.Veem-mail-attachment-symbolic"));
            let lbl = gtk::Label::new(Some(&name));
            lbl.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
            lbl.set_max_width_chars(22);
            chip.append(&lbl);
            let rm = gtk::Button::from_icon_name("com.getveem.Veem-window-close-symbolic");
            rm.add_css_class("flat");
            rm.set_valign(gtk::Align::Center);
            let s = sender.input_sender().clone();
            rm.connect_clicked(move |_| {
                let _ = s.send(ComposeInput::RemoveAttachment(i));
            });
            chip.append(&rm);

            flow.append(&chip);
        }
        flow.set_visible(!self.attachments.is_empty());
    }
}
