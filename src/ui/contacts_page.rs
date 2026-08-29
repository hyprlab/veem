//! In-app Contacts view, shown in the main content area (like the attachments
//! gallery): a searchable list of every GNOME Contacts entry on the left, the
//! selected contact's full card on the right. Reading is straight from EDS, so
//! it shows what GNOME Contacts shows; anything Vireo can't do (editing,
//! linking, new books) is one click away via "Open GNOME Contacts".

use adw::prelude::*;
use relm4::prelude::*;

use crate::contacts::{ContactDetails, Labeled};

pub struct ContactsPage {
    contacts: Vec<ContactDetails>,
    query: String,
    /// Index into `contacts` of the contact shown in the detail pane.
    selected: Option<usize>,
    /// Filtered list-row order → index into `contacts`.
    row_map: Vec<usize>,
    /// A load is underway (the page was just opened).
    loading: bool,
}

#[derive(Debug)]
pub enum ContactsPageInput {
    /// Fresh EDS read: replace everything (keeps the selection when possible).
    SetContacts(Vec<ContactDetails>),
    /// The page was just opened; a read is on its way.
    SetLoading,
    Query(String),
    RowSelected(i32),
    Compose(String),
    OpenUrl(String),
}

#[derive(Debug)]
pub enum ContactsPageOutput {
    /// Start a message to this address.
    Compose(String),
}

#[relm4::component(pub)]
impl Component for ContactsPage {
    type Init = ();
    type Input = ContactsPageInput;
    type Output = ContactsPageOutput;
    type CommandOutput = ();

    view! {
        #[name = "page_stack"]
        gtk::Stack {
            set_transition_type: gtk::StackTransitionType::Crossfade,

            // No contacts at all (or still loading the first read).
            add_named[Some("none")] = &adw::StatusPage {
                set_icon_name: Some("co.hyprlab.Vireo-x-office-address-book-symbolic"),
                #[watch]
                set_title: if model.loading { "Loading Contacts…" } else { "No Contacts" },
                #[watch]
                set_description: Some(if model.loading {
                    ""
                } else {
                    "Contacts you add in GNOME Contacts appear here."
                }),
            },

            add_named[Some("browser")] = &gtk::Box {
                set_orientation: gtk::Orientation::Horizontal,

                gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,
                    set_width_request: 280,

                    #[name = "search"]
                    gtk::SearchEntry {
                        set_placeholder_text: Some("Search contacts"),
                        set_margin_top: 8,
                        set_margin_bottom: 8,
                        set_margin_start: 10,
                        set_margin_end: 10,
                        connect_search_changed[sender] => move |e| {
                            sender.input(ContactsPageInput::Query(e.text().to_string()));
                        },
                    },

                    gtk::ScrolledWindow {
                        set_vexpand: true,
                        set_hscrollbar_policy: gtk::PolicyType::Never,

                        #[name = "list"]
                        gtk::ListBox {
                            add_css_class: "navigation-sidebar",
                            set_selection_mode: gtk::SelectionMode::Single,
                            connect_row_selected[sender] => move |_, row| {
                                if let Some(row) = row {
                                    sender.input(ContactsPageInput::RowSelected(row.index()));
                                }
                            },
                        },
                    },
                },

                gtk::Separator {
                    set_orientation: gtk::Orientation::Vertical,
                },

                gtk::ScrolledWindow {
                    set_hexpand: true,
                    set_vexpand: true,
                    set_hscrollbar_policy: gtk::PolicyType::Never,
                    add_css_class: "contact-detail-pane",

                    adw::Clamp {
                        set_maximum_size: 560,
                        set_tightening_threshold: 420,

                        #[name = "detail_box"]
                        gtk::Box {
                            set_orientation: gtk::Orientation::Vertical,
                            set_margin_top: 24,
                            set_margin_bottom: 32,
                            set_margin_start: 18,
                            set_margin_end: 18,
                        },
                    },
                },
            },
        }
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = ContactsPage {
            contacts: Vec::new(),
            query: String::new(),
            selected: None,
            row_map: Vec::new(),
            loading: true,
        };
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update_with_view(
        &mut self,
        widgets: &mut Self::Widgets,
        message: Self::Input,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match message {
            ContactsPageInput::SetLoading => {
                self.loading = true;
                if self.contacts.is_empty() {
                    widgets.page_stack.set_visible_child_name("none");
                }
            }

            ContactsPageInput::SetContacts(contacts) => {
                // Keep the same person selected across a refresh, by identity.
                let keep = self
                    .selected
                    .and_then(|i| self.contacts.get(i))
                    .map(|c| (c.name.clone(), c.primary_email().map(str::to_string)));
                self.contacts = contacts;
                self.loading = false;
                self.selected = keep.and_then(|(name, email)| {
                    self.contacts.iter().position(|c| {
                        c.name == name && c.primary_email().map(str::to_string) == email
                    })
                });
                self.rebuild(widgets, &sender);
            }

            ContactsPageInput::Query(q) => {
                self.query = q.to_lowercase();
                self.rebuild(widgets, &sender);
            }

            ContactsPageInput::RowSelected(row) => {
                let Some(&idx) = self.row_map.get(row as usize) else { return };
                if self.selected == Some(idx) {
                    return;
                }
                self.selected = Some(idx);
                self.render_detail(widgets, &sender);
            }

            ContactsPageInput::Compose(email) => {
                let _ = sender.output(ContactsPageOutput::Compose(email));
            }

            ContactsPageInput::OpenUrl(url) => {
                // Bare "example.org" URLs are common in vCards.
                let full = if url.contains("://") { url } else { format!("https://{url}") };
                let _ = gtk::gio::AppInfo::launch_default_for_uri(
                    &full,
                    gtk::gio::AppLaunchContext::NONE,
                );
            }
        }
    }
}

impl ContactsPage {
    /// Whether a contact matches the active search.
    fn matches(&self, c: &ContactDetails) -> bool {
        if self.query.is_empty() {
            return true;
        }
        let q = self.query.as_str();
        c.name.to_lowercase().contains(q)
            || c.nickname.to_lowercase().contains(q)
            || c.org.to_lowercase().contains(q)
            || c.emails.iter().any(|e| e.value.to_lowercase().contains(q))
            || c.phones.iter().any(|p| p.value.to_lowercase().contains(q))
    }

    /// Rebuild the list from `contacts` + `query`, keep (or default) the
    /// selection, and render its detail card.
    fn rebuild(&mut self, widgets: &ContactsPageWidgets, sender: &ComponentSender<Self>) {
        widgets
            .page_stack
            .set_visible_child_name(if self.contacts.is_empty() { "none" } else { "browser" });

        while let Some(child) = widgets.list.first_child() {
            widgets.list.remove(&child);
        }
        self.row_map.clear();
        for (idx, c) in self.contacts.iter().enumerate() {
            if !self.matches(c) {
                continue;
            }
            self.row_map.push(idx);
            let row = adw::ActionRow::new();
            row.set_title(&gtk::glib::markup_escape_text(&c.name));
            if let Some(email) = c.primary_email() {
                if email != c.name {
                    row.set_subtitle(&gtk::glib::markup_escape_text(email));
                }
            }
            row.set_activatable(true);
            row.add_prefix(&avatar_for(c, 34));
            widgets.list.append(&row);
        }

        // Selection: keep it if it survived the filter, else the first match.
        let shown_pos = self
            .selected
            .and_then(|sel| self.row_map.iter().position(|&i| i == sel))
            .or((!self.row_map.is_empty()).then_some(0));
        match shown_pos {
            Some(pos) => {
                self.selected = Some(self.row_map[pos]);
                if let Some(row) = widgets.list.row_at_index(pos as i32) {
                    widgets.list.select_row(Some(&row));
                }
            }
            None => self.selected = None,
        }
        self.render_detail(widgets, sender);
    }

    /// Fill the right-hand pane with the selected contact's card.
    fn render_detail(&self, widgets: &ContactsPageWidgets, sender: &ComponentSender<Self>) {
        let detail = &widgets.detail_box;
        while let Some(child) = detail.first_child() {
            detail.remove(&child);
        }
        let Some(c) = self.selected.and_then(|i| self.contacts.get(i)) else {
            return;
        };

        // ---- identity header: photo, name, who they are ----
        let avatar = avatar_for(c, 96);
        avatar.set_halign(gtk::Align::Center);
        detail.append(&avatar);

        let name = gtk::Label::new(Some(&c.name));
        name.add_css_class("title-1");
        name.set_wrap(true);
        name.set_justify(gtk::Justification::Center);
        name.set_margin_top(12);
        detail.append(&name);

        let mut byline = Vec::new();
        if !c.nickname.is_empty() && c.nickname != c.name {
            byline.push(format!("“{}”", c.nickname));
        }
        match (!c.title.is_empty(), !c.org.is_empty()) {
            (true, true) => byline.push(format!("{} · {}", c.title, c.org)),
            (true, false) => byline.push(c.title.clone()),
            (false, true) => byline.push(c.org.clone()),
            (false, false) => {}
        }
        if !byline.is_empty() {
            let sub = gtk::Label::new(Some(&byline.join("  —  ")));
            sub.add_css_class("dim-label");
            sub.set_wrap(true);
            sub.set_justify(gtk::Justification::Center);
            sub.set_margin_top(4);
            detail.append(&sub);
        }

        // ---- field groups ----
        if !c.emails.is_empty() {
            let list = group(detail, "Email");
            for e in &c.emails {
                let row = labeled_row(e);
                let compose = flat_button(
                    "co.hyprlab.Vireo-mail-message-new-symbolic",
                    "New message",
                );
                let s = sender.input_sender().clone();
                let addr = e.value.clone();
                compose.connect_clicked(move |_| {
                    let _ = s.send(ContactsPageInput::Compose(addr.clone()));
                });
                row.add_suffix(&copy_button(&e.value));
                row.add_suffix(&compose);
                row.set_activatable(true);
                let s = sender.input_sender().clone();
                let addr = e.value.clone();
                row.connect_activated(move |_| {
                    let _ = s.send(ContactsPageInput::Compose(addr.clone()));
                });
                list.append(&row);
            }
        }

        if !c.phones.is_empty() {
            let list = group(detail, "Phone");
            for p in &c.phones {
                let row = labeled_row(p);
                row.add_suffix(&copy_button(&p.value));
                list.append(&row);
            }
        }

        if !c.addresses.is_empty() {
            let list = group(detail, "Address");
            for a in &c.addresses {
                let row = labeled_row(a);
                row.add_suffix(&copy_button(&a.value));
                list.append(&row);
            }
        }

        if !c.urls.is_empty() {
            let list = group(detail, "Website");
            for url in &c.urls {
                let row = adw::ActionRow::new();
                row.set_title(&gtk::glib::markup_escape_text(url));
                row.set_activatable(true);
                let icon =
                    gtk::Image::from_icon_name("co.hyprlab.Vireo-adw-external-link-symbolic");
                icon.add_css_class("dim-label");
                row.add_suffix(&icon);
                let s = sender.input_sender().clone();
                let url = url.clone();
                row.connect_activated(move |_| {
                    let _ = s.send(ContactsPageInput::OpenUrl(url.clone()));
                });
                list.append(&row);
            }
        }

        let mut extras: Vec<Labeled> = Vec::new();
        if !c.birthday.is_empty() {
            extras.push(Labeled { label: "Birthday".into(), value: c.birthday.clone() });
        }
        if !c.note.is_empty() {
            extras.push(Labeled { label: "Note".into(), value: c.note.clone() });
        }
        if !extras.is_empty() {
            let list = group(detail, "Details");
            for x in &extras {
                let row = adw::ActionRow::new();
                row.set_title(&gtk::glib::markup_escape_text(&x.label));
                row.set_subtitle(&gtk::glib::markup_escape_text(&x.value));
                row.add_css_class("property");
                list.append(&row);
            }
        }
    }
}

/// The contact's avatar: their GNOME Contacts photo when they have one, the
/// usual initials disc when not.
fn avatar_for(c: &ContactDetails, size: i32) -> adw::Avatar {
    let avatar = adw::Avatar::new(size, Some(&c.name), true);
    if let Some(photo) = &c.photo {
        let bytes = gtk::glib::Bytes::from(photo.as_slice());
        if let Ok(texture) = gtk::gdk::Texture::from_bytes(&bytes) {
            avatar.set_custom_image(Some(&texture));
        }
    }
    avatar
}

/// A titled boxed list appended to the detail column; returns the list.
fn group(detail: &gtk::Box, title: &str) -> gtk::ListBox {
    let heading = gtk::Label::new(Some(title));
    heading.add_css_class("heading");
    heading.set_halign(gtk::Align::Start);
    heading.set_margin_top(20);
    heading.set_margin_bottom(6);
    detail.append(&heading);
    let list = gtk::ListBox::new();
    list.add_css_class("boxed-list");
    list.set_selection_mode(gtk::SelectionMode::None);
    detail.append(&list);
    list
}

/// A value row with its (optional) "Home"/"Work" label as the subtitle.
fn labeled_row(l: &Labeled) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_title(&gtk::glib::markup_escape_text(&l.value));
    if !l.label.is_empty() {
        row.set_subtitle(&gtk::glib::markup_escape_text(&l.label));
    }
    row
}

fn flat_button(icon: &str, tooltip: &str) -> gtk::Button {
    let b = gtk::Button::from_icon_name(icon);
    b.set_tooltip_text(Some(tooltip));
    b.set_valign(gtk::Align::Center);
    b.add_css_class("flat");
    b
}

/// A suffix button that puts `text` on the clipboard, confirming by briefly
/// swapping its icon for a checkmark.
fn copy_button(text: &str) -> gtk::Button {
    let b = flat_button("co.hyprlab.Vireo-edit-copy-symbolic", "Copy");
    let text = text.to_string();
    b.connect_clicked(move |b| {
        b.clipboard().set_text(&text);
        b.set_icon_name("co.hyprlab.Vireo-verified-checkmark-symbolic");
        let b = b.clone();
        gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(1200), move || {
            b.set_icon_name("co.hyprlab.Vireo-edit-copy-symbolic");
        });
    });
    b
}
