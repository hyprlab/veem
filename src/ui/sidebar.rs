//! Left pane: an optional "All Inboxes" (unified) row, then one section per
//! account. Each account has a coloured avatar circle + header button (chevron
//! on the right) and an animated `gtk::Revealer` holding its folder list, so
//! expanding/collapsing slides smoothly. Exactly one thing is selected across
//! the unified row and all account folder lists.
//!
//! Account order is managed in the Accounts window, not here. Collapse state is
//! owned by the app (persisted); collapse is animated locally and reported for
//! persistence WITHOUT a full rebuild (which would interrupt the animation).

use std::collections::HashMap;

use gtk::prelude::*;
use relm4::prelude::*;

use crate::models::{Account, Folder, FolderKind};

/// A per-account inbox shown in the expandable "All Inboxes" sub-list.
#[derive(Clone)]
struct InboxRef {
    account_id: u32,
    folder_id: u32,
    name: String,
    path: String,
}

/// One account's section data, as handed to the sidebar.
#[derive(Debug, Clone)]
pub struct SectionData {
    pub account: Account,
    pub folders: Vec<Folder>,
    pub collapsed: bool,
    /// Resolved avatar background colour ("#rrggbb").
    pub color: String,
    /// Avatar emoji; when absent, account-name initials are shown.
    pub emoji: Option<String>,
}

/// What is currently selected in the sidebar.
#[derive(Clone, PartialEq, Debug)]
enum Sel {
    None,
    Unified,
    /// The attachments gallery (all inboxes).
    Attachments,
    Folder(u32, String),
    /// An account's inbox selected via the "All Inboxes" sub-list.
    UnifiedInbox(u32),
}

pub struct Sidebar {
    /// Last sections received, in display order.
    sections: Vec<SectionData>,
    /// Whether to show the unified "All Inboxes" row.
    show_unified: bool,
    /// Per-account widgets, rebuilt on each SetContents.
    revealers: HashMap<u32, gtk::Revealer>,
    chevrons: HashMap<u32, gtk::Image>,
    folder_lists: HashMap<u32, gtk::ListBox>,
    /// The unified-row list box (one row), when shown.
    unified_list: Option<gtk::ListBox>,
    /// The "Attachments" row list box (one row), when shown.
    attachments_list: Option<gtk::ListBox>,
    /// Display-wide provider holding each account's avatar colour rules.
    color_provider: gtk::CssProvider,
    selected: Sel,
    /// Icon-only mode: hide all text, show just icons and account pills.
    collapsed: bool,
    /// Total unread across all inboxes, for the "All Inboxes" badge.
    unified_unread: u32,
    /// Unread badge labels by (account_id, folder_id), updated in place.
    folder_badges: HashMap<(u32, u32), gtk::Label>,
    /// The "All Inboxes" unread badge label, when shown.
    unified_badge: Option<gtk::Label>,
    /// Whether the "All Inboxes" per-account inbox sub-list is expanded.
    unified_expanded: bool,
    unified_revealer: Option<gtk::Revealer>,
    unified_chevron: Option<gtk::Image>,
    unified_inbox_list: Option<gtk::ListBox>,
    /// Per-account inboxes shown under "All Inboxes", in sub-list row order.
    unified_inboxes: Vec<InboxRef>,
    /// Unread badges for the sub-list rows, by (account_id, inbox folder_id).
    unified_inbox_badges: HashMap<(u32, u32), gtk::Label>,
}

#[derive(Debug)]
pub enum SidebarInput {
    SetContents {
        sections: Vec<SectionData>,
        show_unified: bool,
        unified_unread: u32,
    },
    UnifiedRowSelected,
    /// The "Attachments" row was chosen.
    AttachmentsRowSelected,
    FolderRowSelected { account_id: u32, index: i32 },
    /// A per-account inbox row under "All Inboxes" was chosen.
    UnifiedInboxRowSelected(i32),
    /// Toggle the "All Inboxes" per-account inbox sub-list.
    ToggleUnifiedExpand,
    ToggleCollapseLocal(u32),
    ToggleCollapsed,
    /// Update unread badges in place without rebuilding the sidebar.
    SetUnread {
        folders: HashMap<(u32, u32), u32>,
        unified: u32,
    },
    /// A message drag was dropped on a folder row.
    DropOnFolder { account_id: u32, path: String, payload: String },
    /// A message drag has hovered a collapsed account long enough — expand it.
    ExpandForDrop(u32),
}

#[derive(Debug)]
pub enum SidebarOutput {
    UnifiedSelected,
    /// The attachments gallery was selected.
    AttachmentsSelected,
    FolderSelected {
        account_id: u32,
        folder_id: u32,
        name: String,
        path: String,
    },
    ToggleCollapse(u32),
    /// The user toggled icon-only mode; `true` means collapsed.
    CollapsedChanged(bool),
    /// The empty-state "Add first account" button was clicked.
    AddAccount,
    /// A right-click context-menu action from a sidebar item.
    Context(CtxAction),
    /// A message (identified by ids) was dropped on a folder to move it there.
    MoveMessage {
        account_id: u32,
        folder_id: u32,
        uid: u32,
        id: u32,
        dest: String,
    },
}

/// Actions offered by sidebar right-click menus.
#[derive(Debug, Clone)]
pub enum CtxAction {
    MarkFolderRead { account_id: u32, folder_id: u32 },
    RefreshFolder { account_id: u32, folder_id: u32 },
    MarkAllInboxesRead,
    RefreshAllInboxes,
    OpenAccountSettings,
    RemoveAccount(u32),
    /// Create a new custom folder under this account.
    NewFolder(u32),
    /// Delete a custom folder (its contents are moved to Trash first).
    DeleteFolder { account_id: u32, name: String, path: String },
}

#[relm4::component(pub)]
impl Component for Sidebar {
    type Init = bool;
    type Input = SidebarInput;
    type Output = SidebarOutput;
    type CommandOutput = ();

    view! {
        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,

            gtk::ScrolledWindow {
                set_vexpand: true,
                set_hscrollbar_policy: gtk::PolicyType::Never,

                #[name = "normal_box"]
                gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,
                },
            },

            gtk::Box {
                add_css_class: "sidebar-footer",
                set_orientation: gtk::Orientation::Horizontal,

                #[name = "collapse_btn"]
                gtk::Button {
                    set_icon_name: "go-previous-symbolic",
                    set_tooltip_text: Some("Collapse sidebar"),
                    set_hexpand: true,
                    add_css_class: "flat",
                    connect_clicked => SidebarInput::ToggleCollapsed,
                },
            },
        }
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let color_provider = gtk::CssProvider::new();
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &color_provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }

        let model = Sidebar {
            sections: Vec::new(),
            show_unified: false,
            revealers: HashMap::new(),
            chevrons: HashMap::new(),
            folder_lists: HashMap::new(),
            unified_list: None,
            attachments_list: None,
            color_provider,
            selected: Sel::None,
            collapsed: init,
            unified_unread: 0,
            folder_badges: HashMap::new(),
            unified_badge: None,
            unified_expanded: true,
            unified_revealer: None,
            unified_chevron: None,
            unified_inbox_list: None,
            unified_inboxes: Vec::new(),
            unified_inbox_badges: HashMap::new(),
        };

        let widgets = view_output!();
        if init {
            widgets.collapse_btn.set_icon_name("go-next-symbolic");
            widgets.collapse_btn.set_tooltip_text(Some("Expand sidebar"));
        }

        ComponentParts { model, widgets }
    }

    fn update_with_view(
        &mut self,
        widgets: &mut Self::Widgets,
        msg: Self::Input,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match msg {
            SidebarInput::SetContents { sections, show_unified, unified_unread } => {
                self.sections = sections;
                self.show_unified = show_unified;
                self.unified_unread = unified_unread;
                self.rebuild_normal(&widgets.normal_box, &sender);
                self.restore_selection();
            }

            SidebarInput::UnifiedRowSelected => {
                if self.selected == Sel::Unified {
                    return;
                }
                self.selected = Sel::Unified;
                self.clear_other_selections(Sel::Unified);
                let _ = sender.output(SidebarOutput::UnifiedSelected);
            }

            SidebarInput::AttachmentsRowSelected => {
                if self.selected == Sel::Attachments {
                    return;
                }
                self.selected = Sel::Attachments;
                self.clear_other_selections(Sel::Attachments);
                let _ = sender.output(SidebarOutput::AttachmentsSelected);
            }

            SidebarInput::UnifiedInboxRowSelected(index) => {
                if let Some(r) = self.unified_inboxes.get(index as usize).cloned() {
                    let key = Sel::UnifiedInbox(r.account_id);
                    if self.selected == key {
                        return;
                    }
                    self.selected = key.clone();
                    self.clear_other_selections(key);
                    let _ = sender.output(SidebarOutput::FolderSelected {
                        account_id: r.account_id,
                        folder_id: r.folder_id,
                        name: r.name,
                        path: r.path,
                    });
                }
            }

            SidebarInput::ToggleUnifiedExpand => {
                self.unified_expanded = !self.unified_expanded;
                if let Some(rev) = &self.unified_revealer {
                    rev.set_reveal_child(self.unified_expanded);
                }
                if let Some(ch) = &self.unified_chevron {
                    ch.set_icon_name(Some(if self.unified_expanded {
                        "pan-down-symbolic"
                    } else {
                        "pan-end-symbolic"
                    }));
                }
            }

            SidebarInput::FolderRowSelected { account_id, index } => {
                let folder = self
                    .sections
                    .iter()
                    .find(|s| s.account.id == account_id)
                    .and_then(|s| s.folders.get(index as usize))
                    .cloned();
                if let Some(folder) = folder {
                    let key = Sel::Folder(account_id, folder.path.clone());
                    if self.selected == key {
                        return;
                    }
                    self.selected = key.clone();
                    self.clear_other_selections(key);
                    let _ = sender.output(SidebarOutput::FolderSelected {
                        account_id,
                        folder_id: folder.id,
                        name: folder.name.clone(),
                        path: folder.path.clone(),
                    });
                }
            }

            SidebarInput::SetUnread { folders, unified } => {
                for ((aid, fid), label) in self.folder_badges.iter().chain(&self.unified_inbox_badges) {
                    let n = folders.get(&(*aid, *fid)).copied().unwrap_or(0);
                    label.set_text(&n.to_string());
                    label.set_visible(n > 0);
                }
                if let Some(label) = &self.unified_badge {
                    label.set_text(&unified.to_string());
                    label.set_visible(unified > 0);
                }
                self.unified_unread = unified;
                // Persist the fresh counts into `sections` as well. Otherwise the
                // next rebuild_normal (e.g. toggling the sidebar collapse) recreates
                // every badge from the folder unread values captured at the last
                // SetContents, reverting in-place updates — so a read inbox's chip
                // reappears on collapse. Keep `sections` a faithful mirror.
                for section in &mut self.sections {
                    for folder in &mut section.folders {
                        folder.unread = folders
                            .get(&(section.account.id, folder.id))
                            .copied()
                            .unwrap_or(0);
                    }
                }
            }

            SidebarInput::ToggleCollapsed => {
                self.collapsed = !self.collapsed;
                widgets.collapse_btn.set_icon_name(if self.collapsed {
                    "go-next-symbolic"
                } else {
                    "go-previous-symbolic"
                });
                widgets.collapse_btn.set_tooltip_text(Some(if self.collapsed {
                    "Expand sidebar"
                } else {
                    "Collapse sidebar"
                }));
                self.rebuild_normal(&widgets.normal_box, &sender);
                self.restore_selection();
                let _ = sender.output(SidebarOutput::CollapsedChanged(self.collapsed));
            }

            SidebarInput::ToggleCollapseLocal(id) => {
                if let Some(rev) = self.revealers.get(&id) {
                    let expanded = !rev.reveals_child();
                    rev.set_reveal_child(expanded);
                    if let Some(ch) = self.chevrons.get(&id) {
                        ch.set_icon_name(Some(if expanded {
                            "pan-down-symbolic"
                        } else {
                            "pan-end-symbolic"
                        }));
                    }
                    if let Some(s) = self.sections.iter_mut().find(|s| s.account.id == id) {
                        s.collapsed = !expanded;
                    }
                    let _ = sender.output(SidebarOutput::ToggleCollapse(id));
                }
            }

            SidebarInput::ExpandForDrop(id) => {
                // Expand a collapsed account so its folders become drop targets.
                if let Some(rev) = self.revealers.get(&id) {
                    if !rev.reveals_child() {
                        rev.set_reveal_child(true);
                        if let Some(ch) = self.chevrons.get(&id) {
                            ch.set_icon_name(Some("pan-down-symbolic"));
                        }
                        if let Some(s) = self.sections.iter_mut().find(|s| s.account.id == id) {
                            s.collapsed = false;
                        }
                        let _ = sender.output(SidebarOutput::ToggleCollapse(id));
                    }
                }
            }

            SidebarInput::DropOnFolder { account_id: dest_account, path: dest, payload } => {
                let parts: Vec<&str> = payload.split('\t').collect();
                if parts.len() == 5 && parts[0] == "veem-move" {
                    if let (Ok(aid), Ok(fid), Ok(uid), Ok(id)) = (
                        parts[1].parse::<u32>(),
                        parts[2].parse::<u32>(),
                        parts[3].parse::<u32>(),
                        parts[4].parse::<u32>(),
                    ) {
                        // Only move within the same account (cross-account IMAP
                        // moves aren't supported), and not onto its own folder.
                        if aid == dest_account {
                            let _ = sender.output(SidebarOutput::MoveMessage {
                                account_id: aid,
                                folder_id: fid,
                                uid,
                                id,
                                dest,
                            });
                        }
                    }
                }
            }
        }
    }
}

impl Sidebar {
    /// Rebuild the list: optional unified row, then per-account headers with
    /// animated folder revealers, and refresh the per-account colour rules.
    fn rebuild_normal(&mut self, container: &gtk::Box, sender: &ComponentSender<Self>) {
        while let Some(child) = container.first_child() {
            container.remove(&child);
        }
        if self.collapsed {
            container.add_css_class("rail-collapsed");
        } else {
            container.remove_css_class("rail-collapsed");
        }
        self.revealers.clear();
        self.chevrons.clear();
        self.folder_lists.clear();
        self.unified_list = None;
        self.attachments_list = None;
        self.folder_badges.clear();
        self.unified_badge = None;
        self.unified_revealer = None;
        self.unified_chevron = None;
        self.unified_inbox_list = None;
        self.unified_inboxes.clear();
        self.unified_inbox_badges.clear();

        // No accounts yet: show a prompt to add the first one instead of an empty
        // sidebar (the app is blank in this state).
        if self.sections.is_empty() {
            let s = sender.clone();
            let add = gtk::Button::new();
            add.add_css_class("suggested-action");
            add.add_css_class("pill");
            add.set_valign(gtk::Align::Center);
            add.set_halign(gtk::Align::Center);
            add.connect_clicked(move |_| {
                let _ = s.output(SidebarOutput::AddAccount);
            });
            if self.collapsed {
                add.set_icon_name("list-add-symbolic");
                add.set_tooltip_text(Some("Add account"));
                add.set_margin_top(12);
                container.append(&add);
            } else {
                let label_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                label_box.append(&gtk::Image::from_icon_name("list-add-symbolic"));
                label_box.append(&gtk::Label::new(Some("Add first account")));
                add.set_child(Some(&label_box));
                let empty = gtk::Box::new(gtk::Orientation::Vertical, 12);
                empty.set_valign(gtk::Align::Start);
                empty.set_margin_top(36);
                empty.set_margin_start(16);
                empty.set_margin_end(16);
                let hint = gtk::Label::new(Some("No accounts yet"));
                hint.add_css_class("dim-label");
                empty.append(&hint);
                empty.append(&add);
                container.append(&empty);
            }
            return;
        }

        let sections = self.sections.clone();

        // Unified "All Inboxes" row, with an expandable per-account inbox list.
        if self.show_unified {
            let list = gtk::ListBox::new();
            list.set_selection_mode(gtk::SelectionMode::Single);
            list.add_css_class("navigation-sidebar");

            let row = gtk::ListBoxRow::new();
            let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            hbox.add_css_class("folder-row");
            let img = gtk::Image::from_icon_name("mail-inbox-symbolic");
            img.add_css_class("folder-icon");
            if self.collapsed {
                hbox.set_halign(gtk::Align::Center);
                row.set_tooltip_text(Some(&if self.unified_unread > 0 {
                    format!("All Inboxes ({})", self.unified_unread)
                } else {
                    "All Inboxes".to_string()
                }));
                // Total-unread chip overlaid on the inbox icon so the count stays
                // visible in the icon-only rail.
                let (overlay, badge) = with_unread_overlay(&img, self.unified_unread);
                hbox.append(&overlay);
                self.unified_badge = Some(badge);
            } else {
                hbox.append(&img);
                let label = gtk::Label::new(Some("All Inboxes"));
                label.set_hexpand(true);
                label.set_halign(gtk::Align::Start);
                label.add_css_class("account-name");
                hbox.append(&label);
                let badge = gtk::Label::new(Some(&self.unified_unread.to_string()));
                badge.add_css_class("unread-badge");
                badge.set_valign(gtk::Align::Center);
                badge.set_visible(self.unified_unread > 0);
                hbox.append(&badge);
                self.unified_badge = Some(badge);

                // Disclosure chevron toggling the per-account inbox sub-list.
                let chevron = gtk::Image::from_icon_name(if self.unified_expanded {
                    "pan-down-symbolic"
                } else {
                    "pan-end-symbolic"
                });
                let chev_btn = gtk::Button::new();
                chev_btn.set_child(Some(&chevron));
                chev_btn.add_css_class("flat");
                chev_btn.add_css_class("chevron-btn");
                chev_btn.set_valign(gtk::Align::Center);
                chev_btn.set_tooltip_text(Some("Show each inbox"));
                let cs = sender.input_sender().clone();
                chev_btn.connect_clicked(move |_| {
                    let _ = cs.send(SidebarInput::ToggleUnifiedExpand);
                });
                hbox.append(&chev_btn);
                self.unified_chevron = Some(chevron);
            }
            row.set_child(Some(&hbox));
            list.append(&row);

            let s = sender.input_sender().clone();
            list.connect_row_selected(move |_, row| {
                if row.is_some() {
                    let _ = s.send(SidebarInput::UnifiedRowSelected);
                }
            });
            // Right-click "All Inboxes": act on every inbox at once.
            let click = gtk::GestureClick::new();
            click.set_button(gtk::gdk::BUTTON_SECONDARY);
            let cs = sender.clone();
            let list_w = list.clone();
            click.connect_pressed(move |_, _, x, y| {
                show_sidebar_menu(
                    &list_w,
                    x,
                    y,
                    vec![
                        ("Mark All as Read", CtxAction::MarkAllInboxesRead),
                        ("Refresh", CtxAction::RefreshAllInboxes),
                    ],
                    &cs,
                );
            });
            list.add_controller(click);
            container.append(&list);
            self.unified_list = Some(list);

            // Per-account inbox sub-list, in both layouts. In the icon-only rail a
            // small toggle button under the "All Inboxes" icon stands in for the
            // in-row chevron and expands/collapses these account pills.
            {
                if self.collapsed {
                    let toggle = gtk::Button::new();
                    toggle.add_css_class("flat");
                    toggle.add_css_class("chevron-btn");
                    toggle.set_halign(gtk::Align::Center);
                    toggle.set_tooltip_text(Some("Show each inbox"));
                    let chevron = gtk::Image::from_icon_name(if self.unified_expanded {
                        "pan-down-symbolic"
                    } else {
                        "pan-end-symbolic"
                    });
                    toggle.set_child(Some(&chevron));
                    let cs = sender.input_sender().clone();
                    toggle.connect_clicked(move |_| {
                        let _ = cs.send(SidebarInput::ToggleUnifiedExpand);
                    });
                    container.append(&toggle);
                    self.unified_chevron = Some(chevron);
                }

                let sub = gtk::ListBox::new();
                sub.set_selection_mode(gtk::SelectionMode::Single);
                sub.add_css_class("navigation-sidebar");
                for section in &sections {
                    let Some(inbox) =
                        section.folders.iter().find(|f| f.kind == FolderKind::Inbox)
                    else {
                        continue;
                    };
                    let aid = section.account.id;
                    let (row, badge) = build_unified_inbox_row(section, inbox, self.collapsed);
                    sub.append(&row);
                    self.unified_inbox_badges.insert((aid, inbox.id), badge);
                    self.unified_inboxes.push(InboxRef {
                        account_id: aid,
                        folder_id: inbox.id,
                        name: inbox.name.clone(),
                        path: inbox.path.clone(),
                    });
                }
                let ss = sender.input_sender().clone();
                sub.connect_row_selected(move |_, row| {
                    if let Some(row) = row {
                        let _ = ss.send(SidebarInput::UnifiedInboxRowSelected(row.index()));
                    }
                });
                // Right-click an inbox sub-row: act on that account's inbox.
                let click = gtk::GestureClick::new();
                click.set_button(gtk::gdk::BUTTON_SECONDARY);
                let cs = sender.clone();
                let sub_w = sub.clone();
                let refs = self.unified_inboxes.clone();
                click.connect_pressed(move |_, _, x, y| {
                    if let Some(r) = sub_w
                        .row_at_y(y as i32)
                        .and_then(|row| refs.get(row.index() as usize))
                    {
                        show_sidebar_menu(
                            &sub_w,
                            x,
                            y,
                            vec![
                                ("Mark as Read", CtxAction::MarkFolderRead {
                                    account_id: r.account_id,
                                    folder_id: r.folder_id,
                                }),
                                ("Refresh", CtxAction::RefreshFolder {
                                    account_id: r.account_id,
                                    folder_id: r.folder_id,
                                }),
                                ("Account Settings…", CtxAction::OpenAccountSettings),
                            ],
                            &cs,
                        );
                    }
                });
                sub.add_controller(click);

                let revealer = gtk::Revealer::new();
                revealer.set_transition_type(gtk::RevealerTransitionType::SlideDown);
                revealer.set_transition_duration(200);
                revealer.set_reveal_child(self.unified_expanded);
                revealer.set_child(Some(&sub));
                container.append(&revealer);
                self.unified_revealer = Some(revealer);
                self.unified_inbox_list = Some(sub);
            }
        }

        // "Attachments" row — a gallery of every inbox attachment. Sits under the
        // All Inboxes block and above the per-account sections.
        {
            let list = gtk::ListBox::new();
            list.set_selection_mode(gtk::SelectionMode::Single);
            list.add_css_class("navigation-sidebar");

            let row = gtk::ListBoxRow::new();
            let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            hbox.add_css_class("folder-row");
            let img = gtk::Image::from_icon_name("mail-attachment-symbolic");
            img.add_css_class("folder-icon");
            if self.collapsed {
                hbox.set_halign(gtk::Align::Center);
                row.set_tooltip_text(Some("Attachments"));
                hbox.append(&img);
            } else {
                hbox.append(&img);
                let label = gtk::Label::new(Some("Attachments"));
                label.set_hexpand(true);
                label.set_halign(gtk::Align::Start);
                label.add_css_class("account-name");
                hbox.append(&label);
            }
            row.set_child(Some(&hbox));
            list.append(&row);

            let s = sender.clone();
            list.connect_row_selected(move |_, row| {
                if row.is_some() {
                    s.input(SidebarInput::AttachmentsRowSelected);
                }
            });
            container.append(&list);
            self.attachments_list = Some(list);
        }

        for section in &sections {
            let id = section.account.id;

            // Header: avatar circle + name/email on the left, chevron on the right.
            let header = gtk::Button::new();
            header.add_css_class("flat");
            header.add_css_class("account-header");
            let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 10);

            let name_str = if section.account.name.trim().is_empty() {
                section.account.email.clone()
            } else {
                section.account.name.clone()
            };

            let circle = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            circle.add_css_class("account-circle");
            circle.add_css_class(&format!("acct-color-{id}"));
            circle.set_valign(gtk::Align::Center);
            // Keep it a perfect circle: a fixed square that never stretches with
            // the row. (Without this the glyph's hexpand propagates up and the
            // circle widens into an oval whenever a name column sits beside it.)
            circle.set_halign(gtk::Align::Center);
            circle.set_hexpand(false);
            circle.set_size_request(34, 34);
            let glyph = gtk::Label::new(None);
            glyph.set_hexpand(true);
            glyph.set_halign(gtk::Align::Center);
            glyph.set_valign(gtk::Align::Center);
            match &section.emoji {
                Some(em) if !em.is_empty() => {
                    glyph.set_text(em);
                    glyph.add_css_class("account-emoji");
                }
                _ => glyph.set_text(&account_initials(&name_str, &section.account.email)),
            }
            circle.append(&glyph);
            hbox.append(&circle);

            // Chevron is tracked even when collapsed so per-account toggles still
            // update an icon; it's only shown in the expanded layout.
            let chevron = gtk::Image::from_icon_name(if section.collapsed {
                "pan-end-symbolic"
            } else {
                "pan-down-symbolic"
            });
            chevron.set_valign(gtk::Align::Center);

            if self.collapsed {
                hbox.set_halign(gtk::Align::Center);
                header.set_tooltip_text(Some(&name_str));
            } else {
                let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
                vbox.set_hexpand(true);
                vbox.set_valign(gtk::Align::Center);
                let name = gtk::Label::new(Some(&name_str));
                name.set_halign(gtk::Align::Start);
                name.set_ellipsize(gtk::pango::EllipsizeMode::End);
                name.add_css_class("account-name");
                let email = gtk::Label::new(Some(&section.account.email));
                email.set_halign(gtk::Align::Start);
                email.set_ellipsize(gtk::pango::EllipsizeMode::End);
                email.add_css_class("account-email");
                vbox.append(&name);
                vbox.append(&email);
                hbox.append(&vbox);
                hbox.append(&chevron);
            }

            header.set_child(Some(&hbox));
            let s = sender.input_sender().clone();
            header.connect_clicked(move |_| {
                let _ = s.send(SidebarInput::ToggleCollapseLocal(id));
            });
            // Right-click an account: act on the account / its inbox.
            let inbox_id = section.folders.iter().find(|f| f.kind == FolderKind::Inbox).map(|f| f.id);
            let click = gtk::GestureClick::new();
            click.set_button(gtk::gdk::BUTTON_SECONDARY);
            let cs = sender.clone();
            let header_w = header.clone();
            click.connect_pressed(move |_, _, x, y| {
                let mut items: Vec<(&str, CtxAction)> = Vec::new();
                if let Some(fid) = inbox_id {
                    items.push(("Mark Inbox as Read", CtxAction::MarkFolderRead {
                        account_id: id,
                        folder_id: fid,
                    }));
                    items.push(("Refresh", CtxAction::RefreshFolder {
                        account_id: id,
                        folder_id: fid,
                    }));
                }
                items.push(("New Folder…", CtxAction::NewFolder(id)));
                items.push(("Account Settings…", CtxAction::OpenAccountSettings));
                items.push(("Remove Account…", CtxAction::RemoveAccount(id)));
                show_sidebar_menu(&header_w, x, y, items, &cs);
            });
            header.add_controller(click);

            // While dragging a message, hovering the account header for 500ms
            // expands a collapsed account so its folders become drop targets.
            let motion = gtk::DropControllerMotion::new();
            let ms = sender.input_sender().clone();
            let timer: std::rc::Rc<std::cell::RefCell<Option<gtk::glib::SourceId>>> =
                std::rc::Rc::new(std::cell::RefCell::new(None));
            let t_enter = timer.clone();
            motion.connect_enter(move |_, _, _| {
                if t_enter.borrow().is_some() {
                    return;
                }
                let ms = ms.clone();
                let t = t_enter.clone();
                let src = gtk::glib::timeout_add_local_once(
                    std::time::Duration::from_millis(500),
                    move || {
                        *t.borrow_mut() = None;
                        let _ = ms.send(SidebarInput::ExpandForDrop(id));
                    },
                );
                *t_enter.borrow_mut() = Some(src);
            });
            let t_leave = timer.clone();
            motion.connect_leave(move |_| {
                if let Some(src) = t_leave.borrow_mut().take() {
                    src.remove();
                }
            });
            header.add_controller(motion);
            container.append(&header);

            // Animated folder list.
            let revealer = gtk::Revealer::new();
            revealer.set_transition_type(gtk::RevealerTransitionType::SlideDown);
            revealer.set_transition_duration(200);
            revealer.set_reveal_child(!section.collapsed);

            let list = gtk::ListBox::new();
            list.set_selection_mode(gtk::SelectionMode::Single);
            list.add_css_class("navigation-sidebar");
            for folder in &section.folders {
                let (row, badge) = build_folder_row(folder, self.collapsed);
                // Accept a dropped message → move it into this folder.
                let drop = gtk::DropTarget::new(
                    gtk::glib::types::Type::STRING,
                    gtk::gdk::DragAction::MOVE,
                );
                let ds = sender.input_sender().clone();
                let dest_path = folder.path.clone();
                drop.connect_drop(move |_, value, _, _| {
                    if let Ok(payload) = value.get::<String>() {
                        let _ = ds.send(SidebarInput::DropOnFolder {
                            account_id: id,
                            path: dest_path.clone(),
                            payload,
                        });
                        return true;
                    }
                    false
                });
                row.add_controller(drop);
                list.append(&row);
                if let Some(badge) = badge {
                    self.folder_badges.insert((id, folder.id), badge);
                }
            }
            let s2 = sender.input_sender().clone();
            list.connect_row_selected(move |_, row| {
                if let Some(row) = row {
                    let _ = s2.send(SidebarInput::FolderRowSelected {
                        account_id: id,
                        index: row.index(),
                    });
                }
            });
            // Right-click a folder: act on that folder.
            let click = gtk::GestureClick::new();
            click.set_button(gtk::gdk::BUTTON_SECONDARY);
            let cs = sender.clone();
            let list_w = list.clone();
            let folders = section.folders.clone();
            click.connect_pressed(move |_, _, x, y| {
                if let Some(f) = list_w
                    .row_at_y(y as i32)
                    .and_then(|row| folders.get(row.index() as usize))
                {
                    let mut items = vec![
                        ("Mark as Read", CtxAction::MarkFolderRead {
                            account_id: id,
                            folder_id: f.id,
                        }),
                        ("Refresh", CtxAction::RefreshFolder {
                            account_id: id,
                            folder_id: f.id,
                        }),
                    ];
                    // Only user-created folders can be deleted.
                    if f.kind == FolderKind::Custom {
                        items.push(("Delete Folder…", CtxAction::DeleteFolder {
                            account_id: id,
                            name: f.name.clone(),
                            path: f.path.clone(),
                        }));
                    }
                    show_sidebar_menu(&list_w, x, y, items, &cs);
                }
            });
            list.add_controller(click);

            // "+ Add Folder" button at the bottom of the list for quick creation.
            let add_btn = gtk::Button::new();
            add_btn.add_css_class("flat");
            add_btn.add_css_class("add-folder-btn");
            let add_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            add_box.add_css_class("folder-row");
            add_box.append(&gtk::Image::from_icon_name("list-add-symbolic"));
            if self.collapsed {
                add_box.set_halign(gtk::Align::Center);
                add_btn.set_tooltip_text(Some("Add Folder"));
            } else {
                let lbl = gtk::Label::new(Some("Add Folder"));
                lbl.set_halign(gtk::Align::Start);
                lbl.set_hexpand(true);
                add_box.append(&lbl);
            }
            add_btn.set_child(Some(&add_box));
            let cs = sender.clone();
            add_btn.connect_clicked(move |_| {
                let _ = cs.output(SidebarOutput::Context(CtxAction::NewFolder(id)));
            });

            let wrap = gtk::Box::new(gtk::Orientation::Vertical, 0);
            wrap.append(&list);
            wrap.append(&add_btn);
            revealer.set_child(Some(&wrap));
            container.append(&revealer);

            self.revealers.insert(id, revealer);
            self.chevrons.insert(id, chevron);
            self.folder_lists.insert(id, list);
        }

        // Per-account avatar colours (background + readable text).
        let mut css = String::new();
        for s in &sections {
            let text = crate::color::readable_text(&s.color);
            css.push_str(&format!(
                ".acct-color-{0} {{ background-color: {1}; }} \
                 .acct-color-{0} label {{ color: {2}; }}\n",
                s.account.id, s.color, text
            ));
        }
        self.color_provider.load_from_data(&css);
    }

    /// Re-apply the current selection after a rebuild; on first populate (no
    /// prior selection) default to the unified row if shown, else the first folder.
    /// Deselect every list except the one owning `keep` (whose own list keeps its
    /// selection). Used when a selection moves between sections.
    fn clear_other_selections(&self, keep: Sel) {
        if keep != Sel::Unified {
            if let Some(l) = &self.unified_list {
                l.unselect_all();
            }
        }
        if keep != Sel::Attachments {
            if let Some(l) = &self.attachments_list {
                l.unselect_all();
            }
        }
        if !matches!(keep, Sel::UnifiedInbox(_)) {
            if let Some(l) = &self.unified_inbox_list {
                l.unselect_all();
            }
        }
        for (aid, lb) in &self.folder_lists {
            if !matches!(&keep, Sel::Folder(kaid, _) if kaid == aid) {
                lb.unselect_all();
            }
        }
    }

    fn select_attachments(&self) {
        if let Some(list) = &self.attachments_list {
            if let Some(row) = list.row_at_index(0) {
                list.select_row(Some(&row));
            }
        }
    }

    fn restore_selection(&mut self) {
        match self.selected.clone() {
            Sel::Unified => self.select_unified(),
            Sel::Attachments => self.select_attachments(),
            Sel::Folder(acc, path) => self.select_folder(acc, &path),
            Sel::UnifiedInbox(acc) => self.select_unified_inbox(acc),
            Sel::None => {
                if self.show_unified {
                    self.select_unified();
                } else if let Some(acc) = self
                    .sections
                    .iter()
                    .find(|s| !s.folders.is_empty())
                    .map(|s| s.account.id)
                {
                    self.select_folder_index(acc, 0);
                }
            }
        }
    }

    fn select_unified(&self) {
        if let Some(list) = &self.unified_list {
            if let Some(row) = list.row_at_index(0) {
                list.select_row(Some(&row));
            }
        }
    }

    fn select_unified_inbox(&self, account_id: u32) {
        if let Some(list) = &self.unified_inbox_list {
            if let Some(idx) = self
                .unified_inboxes
                .iter()
                .position(|r| r.account_id == account_id)
            {
                if let Some(row) = list.row_at_index(idx as i32) {
                    list.select_row(Some(&row));
                }
            }
        }
    }

    fn select_folder(&self, account_id: u32, path: &str) {
        let idx = self
            .sections
            .iter()
            .find(|s| s.account.id == account_id)
            .and_then(|s| s.folders.iter().position(|f| f.path == path));
        if let Some(idx) = idx {
            self.select_folder_index(account_id, idx);
        }
    }

    fn select_folder_index(&self, account_id: u32, idx: usize) {
        if let Some(list) = self.folder_lists.get(&account_id) {
            if let Some(row) = list.row_at_index(idx as i32) {
                list.select_row(Some(&row));
            }
        }
    }
}

fn account_initials(name: &str, email: &str) -> String {
    let mut it = name.split_whitespace();
    let a = it.next().and_then(|w| w.chars().next());
    let b = it.next().and_then(|w| w.chars().next());
    match (a, b) {
        (Some(a), Some(b)) => format!("{a}{b}").to_uppercase(),
        (Some(a), None) => a.to_uppercase().to_string(),
        _ => email
            .chars()
            .next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_else(|| "?".to_string()),
    }
}

/// Pop up a right-click context menu of `items` anchored at (x, y) in `parent`.
/// The popover unparents itself when dismissed (weak refs avoid a leak cycle).
fn show_sidebar_menu(
    parent: &impl IsA<gtk::Widget>,
    x: f64,
    y: f64,
    items: Vec<(&str, CtxAction)>,
    sender: &ComponentSender<Sidebar>,
) {
    let popover = gtk::Popover::new();
    popover.set_has_arrow(false);
    popover.set_position(gtk::PositionType::Bottom);
    popover.add_css_class("menu");

    let menu = gtk::Box::new(gtk::Orientation::Vertical, 0);
    menu.add_css_class("context-menu");
    for (label, action) in items {
        let btn = gtk::Button::with_label(label);
        btn.add_css_class("flat");
        btn.set_halign(gtk::Align::Fill);
        if let Some(lbl) = btn.child().and_downcast::<gtk::Label>() {
            lbl.set_xalign(0.0);
        }
        let s = sender.clone();
        let weak = popover.downgrade();
        btn.connect_clicked(move |_| {
            let _ = s.output(SidebarOutput::Context(action.clone()));
            if let Some(p) = weak.upgrade() {
                p.popdown();
            }
        });
        menu.append(&btn);
    }
    popover.set_child(Some(&menu));
    popover.set_parent(parent);
    popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    popover.connect_closed(|p| p.unparent());
    popover.popup();
}

/// A row in the "All Inboxes" sub-list: a small account pill, the account name,
/// and that inbox's unread badge. In the compact rail only the pill is shown
/// (centred), with the unread count as a corner chip. Returns the badge for
/// in-place updates.
fn build_unified_inbox_row(
    section: &SectionData,
    inbox: &Folder,
    collapsed: bool,
) -> (gtk::ListBoxRow, gtk::Label) {
    let id = section.account.id;
    let row = gtk::ListBoxRow::new();
    let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    hbox.add_css_class("folder-row");
    if !collapsed {
        hbox.add_css_class("unified-subrow");
    }

    // Show the account's configured label (defaults to its email) so accounts are
    // easy to tell apart in the All Inboxes view.
    let name_str = section.account.label.clone();

    // Small account pill (colour + initials/emoji), like the header circle.
    let circle = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    circle.add_css_class("account-circle-sm");
    circle.add_css_class(&format!("acct-color-{id}"));
    circle.set_valign(gtk::Align::Center);
    circle.set_halign(gtk::Align::Center);
    circle.set_hexpand(false);
    circle.set_size_request(22, 22);
    let glyph = gtk::Label::new(None);
    glyph.set_hexpand(true);
    glyph.set_halign(gtk::Align::Center);
    glyph.set_valign(gtk::Align::Center);
    match &section.emoji {
        Some(em) if !em.is_empty() => {
            glyph.set_text(em);
            glyph.add_css_class("account-emoji");
        }
        _ => glyph.set_text(&account_initials(&name_str, &section.account.email)),
    }
    circle.append(&glyph);

    let badge = if collapsed {
        hbox.set_halign(gtk::Align::Center);
        let tip = if inbox.unread > 0 {
            format!("{} ({})", name_str, inbox.unread)
        } else {
            name_str.clone()
        };
        row.set_tooltip_text(Some(&tip));
        let (overlay, badge) = with_unread_overlay(&circle, inbox.unread);
        hbox.append(&overlay);
        badge
    } else {
        hbox.append(&circle);

        let name = gtk::Label::new(Some(&name_str));
        name.set_hexpand(true);
        name.set_halign(gtk::Align::Start);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        hbox.append(&name);

        let badge = gtk::Label::new(Some(&inbox.unread.to_string()));
        badge.add_css_class("unread-badge");
        badge.set_valign(gtk::Align::Center);
        badge.set_visible(inbox.unread > 0);
        hbox.append(&badge);
        badge
    };

    row.set_child(Some(&hbox));
    (row, badge)
}

/// Wrap `child` in an overlay with a small unread chip pinned to its top-right
/// corner — used in the compact rail, where there's no room for an inline badge.
/// Returns the overlay (to place in the tree) and the chip label (for updates).
fn with_unread_overlay(
    child: &impl IsA<gtk::Widget>,
    unread: u32,
) -> (gtk::Overlay, gtk::Label) {
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(child));
    let badge = gtk::Label::new(Some(&unread.to_string()));
    badge.add_css_class("unread-badge");
    badge.add_css_class("unread-badge-mini");
    badge.set_halign(gtk::Align::End);
    badge.set_valign(gtk::Align::Start);
    badge.set_visible(unread > 0);
    overlay.add_overlay(&badge);
    (overlay, badge)
}

fn build_folder_row(folder: &Folder, collapsed: bool) -> (gtk::ListBoxRow, Option<gtk::Label>) {
    let row = gtk::ListBoxRow::new();
    let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    hbox.add_css_class("folder-row");

    let img = gtk::Image::from_icon_name(folder.kind.icon());
    img.add_css_class("folder-icon");

    let badge = if collapsed {
        hbox.set_halign(gtk::Align::Center);
        let tip = if folder.unread > 0 {
            format!("{} ({})", folder.name, folder.unread)
        } else {
            folder.name.clone()
        };
        row.set_tooltip_text(Some(&tip));
        // Only the Inbox carries an unread chip; in the rail it rides the icon's
        // corner so new mail shows without expanding the sidebar.
        if folder.kind == FolderKind::Inbox {
            let (overlay, badge) = with_unread_overlay(&img, folder.unread);
            hbox.append(&overlay);
            Some(badge)
        } else {
            hbox.append(&img);
            None
        }
    } else {
        hbox.append(&img);
        let name = gtk::Label::new(Some(&folder.name));
        name.set_hexpand(true);
        name.set_halign(gtk::Align::Start);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        hbox.append(&name);

        // Only the Inbox shows an unread count chip; other folders (Spam, Sent,
        // Trash, …) don't. Present but hidden when zero so it can update in place.
        if folder.kind == FolderKind::Inbox {
            let badge = gtk::Label::new(Some(&folder.unread.to_string()));
            badge.add_css_class("unread-badge");
            badge.set_valign(gtk::Align::Center);
            badge.set_visible(folder.unread > 0);
            hbox.append(&badge);
            Some(badge)
        } else {
            None
        }
    };

    row.set_child(Some(&hbox));
    (row, badge)
}
