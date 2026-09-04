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
use crate::ui::context_menu::{show_context_menu, MenuEntry};

/// A per-account inbox shown in the expandable "All Inboxes" sub-list.
#[derive(Clone)]
struct InboxRef {
    account_id: u32,
    folder_id: u32,
    name: String,
    path: String,
}

/// A folder a filter rule files into, listed in the "Filtered Folders"
/// section inside All Inboxes. The app resolves the rules to these; the
/// sidebar only draws them (account pill + folder name + unread chip).
#[derive(Debug, Clone)]
pub struct UnifiedFolderRef {
    pub account_id: u32,
    pub folder: Folder,
}

/// One account's section data, as handed to the sidebar.
#[derive(Debug, Clone)]
pub struct SectionData {
    pub account: Account,
    pub folders: Vec<Folder>,
    pub collapsed: bool,
    /// Whether this account's custom-folders section is expanded (default hidden).
    pub custom_expanded: bool,
    /// Resolved avatar background colour ("#rrggbb").
    pub color: String,
    /// Avatar emoji; when absent, account-name initials are shown.
    pub emoji: Option<String>,
    /// Custom-folder paths whose tree node is collapsed (#51).
    pub tree_collapsed: Vec<String>,
}

/// Initial state for the sidebar.
pub struct SidebarInit {
    /// Icon-only mode: hide all text, show just icons and account pills.
    pub collapsed: bool,
    /// Whether the "Attachments" row is shown.
    pub show_attachments: bool,
    /// Whether the "Contacts" row is shown.
    pub show_contacts: bool,
}

/// What is currently selected in the sidebar.
#[derive(Clone, PartialEq, Debug)]
enum Sel {
    None,
    Unified,
    /// The attachments gallery (all inboxes).
    Attachments,
    /// The in-app contacts view.
    Contacts,
    Outbox,
    Folder(u32, String),
    /// An account's inbox selected via the "All Inboxes" sub-list.
    UnifiedInbox(u32),
    /// A filtered folder (account, path) selected via the "Filtered Folders"
    /// section inside All Inboxes.
    UnifiedFolder(u32, String),
}

pub struct Sidebar {
    /// Last sections received, in display order.
    sections: Vec<SectionData>,
    /// Whether to show the unified "All Inboxes" row.
    show_unified: bool,
    /// Whether the collapsed-up "All Inboxes" row wears its total-unread chip
    /// (while expanded, the per-inbox sub-list carries the counts instead).
    show_unified_chip: bool,
    /// Whether the disclosure chevrons LEAD their rows (Settings: Chevron
    /// placement). Off restores the classic trailing position.
    chevrons_left: bool,
    /// Per-account widgets, rebuilt on each SetContents.
    revealers: HashMap<u32, gtk::Revealer>,
    chevrons: HashMap<u32, gtk::Image>,
    folder_lists: HashMap<u32, gtk::ListBox>,
    /// Per-account list box holding just the custom (user-created) folders, shown
    /// under a collapsible "Folders" section. Selection indices into these are
    /// offset past the account's essential folders.
    custom_folder_lists: HashMap<u32, gtk::ListBox>,
    /// Per-account custom folders, in row order, for tree-visibility math.
    custom_folders: HashMap<u32, Vec<Folder>>,
    /// Collapsed tree nodes per account (paths), mirrored from SectionData and
    /// flipped locally as chevrons are clicked (#51).
    tree_collapsed: HashMap<u32, std::collections::HashSet<String>>,
    /// Each parent row's expander image, for flipping without a rebuild.
    tree_chevrons: HashMap<(u32, String), gtk::Image>,
    /// Per-account custom-row revealers (row order), for animated tree
    /// collapse/expand.
    tree_row_revealers: HashMap<u32, Vec<gtk::Revealer>>,
    /// The rebuild freeze-frame Picture and its pending lift timer.
    freeze_frame: Option<gtk::Picture>,
    freeze_timer: std::rc::Rc<std::cell::RefCell<Option<gtk::glib::SourceId>>>,
    /// The "Folders" section revealer and its chevron, per account.
    custom_revealers: HashMap<u32, gtk::Revealer>,
    custom_chevrons: HashMap<u32, gtk::Image>,
    /// The unified-row list box (one row), when shown.
    unified_list: Option<gtk::ListBox>,
    /// The pinned footer's single list box (Contacts + Attachments rows) and
    /// the rows themselves, for selection management.
    footer_list: Option<gtk::ListBox>,
    attachments_row: Option<gtk::ListBoxRow>,
    contacts_row: Option<gtk::ListBoxRow>,
    /// Refresh/spinner stack + spinner beside the "New Message" button.
    sync_stack: Option<gtk::Stack>,
    sync_spinner: Option<gtk::Spinner>,
    /// Whether any account is syncing (drives the refresh spinner).
    busy: bool,
    /// The "Outbox" row list box (one row), while anything is queued.
    outbox_list: Option<gtk::ListBox>,
    /// Display-wide provider holding each account's avatar colour rules.
    color_provider: gtk::CssProvider,
    selected: Sel,
    /// Icon-only mode: hide all text, show just icons and account pills.
    collapsed: bool,
    /// Whether the "Attachments" row is shown (in the pinned footer).
    show_attachments: bool,
    /// Whether the "Contacts" row is shown (in the pinned footer).
    show_contacts: bool,
    /// How many messages are waiting in the Outbox across all accounts. The row
    /// only exists while this is non-zero — an empty Outbox is the normal state
    /// and does not deserve permanent furniture.
    outbox_count: u32,
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
    /// The filtered folders listed inside All Inboxes, in row order.
    unified_folders: Vec<UnifiedFolderRef>,
    /// Whether the "Filtered Folders" section inside All Inboxes is open.
    unified_folders_expanded: bool,
    unified_folders_revealer: Option<gtk::Revealer>,
    unified_folders_chevron: Option<gtk::Image>,
    unified_folder_list: Option<gtk::ListBox>,
    /// Unread badges for the filtered-folder rows, by (account_id, folder_id).
    unified_folder_badges: HashMap<(u32, u32), gtk::Label>,
    /// The "Filtered Folders" header's own unread chip (the section's total),
    /// shown only while the section is folded up, like All Inboxes' chip.
    unified_folders_badge: Option<gtk::Label>,
    unified_folders_unread: u32,
    /// Inbox unread badge overlaid on each account's avatar circle, shown only
    /// while that account's section is collapsed (its Inbox row — and normal
    /// chip — is then hidden inside the revealer). Keyed by account_id.
    account_circle_badges: HashMap<u32, gtk::Label>,
}

#[derive(Debug)]
pub enum SidebarInput {
    SetContents {
        sections: Vec<SectionData>,
        show_unified: bool,
        unified_chip: bool,
        chevrons_left: bool,
        unified_unread: u32,
        /// Filter-rule folders to list inside All Inboxes (already
        /// narrowed to the rules that opt in and the Settings switch).
        unified_folders: Vec<UnifiedFolderRef>,
    },
    UnifiedRowSelected,
    /// Select the "All Inboxes" row programmatically (the tray menu's
    /// "View all unread"): the highlight follows, and the selection goes
    /// out like a click. State is set before the row, so the row-selected
    /// signal hits the already-selected guard.
    SelectUnifiedRow,
    /// The "Attachments" row was chosen.
    AttachmentsRowSelected,
    /// The "Contacts" row was clicked (it acts as a launcher, not a selection).
    ContactsRowClicked,
    FolderRowSelected { account_id: u32, index: i32 },
    /// Select a folder row programmatically — the app navigated there itself
    /// ("Go to Message" from the gallery, a notification click) and the
    /// highlight must follow. State is set before the row, so the resulting
    /// row-selected signal hits the already-selected guard and stops.
    SelectFolderRow { account_id: u32, path: String },
    /// A per-account inbox row under "All Inboxes" was chosen.
    UnifiedInboxRowSelected(i32),
    /// Toggle the "All Inboxes" per-account inbox sub-list.
    ToggleUnifiedExpand,
    /// A filtered-folder row inside "All Inboxes" was chosen.
    UnifiedFolderRowSelected(i32),
    /// Toggle the "Filtered Folders" section inside "All Inboxes".
    ToggleUnifiedFoldersExpand,
    ToggleCollapseLocal(u32),
    /// Set icon-only mode outright (the app's narrow-window breakpoint) —
    /// unlike ToggleCollapsed this never reports CollapsedChanged, so it can't
    /// overwrite the user's own persisted choice.
    SetCollapsed(bool),
    /// Toggle the collapsible "Folders" (custom folders) section for an account.
    ToggleCustomFoldersLocal(u32),
    /// Collapse/expand one folder-tree node (a parent folder's chevron, #51).
    ToggleFolderNode { account_id: u32, path: String },
    /// A custom-folder row was clicked (fires every click, selected or not):
    /// parents toggle their sub-tree without needing the caret.
    FolderRowActivated { account_id: u32, index: i32 },
    ToggleCollapsed,
    /// Show/hide the "Attachments" row in the pinned footer.
    SetAttachmentsRow(bool),
    /// Show/hide the "Contacts" row in the pinned footer.
    SetContactsRow(bool),
    /// Whether any account is syncing — spins the refresh button.
    SetBusy(bool),
    /// How many messages are waiting to be sent; 0 hides the Outbox row.
    SetOutboxCount(u32),
    /// The "Outbox" row was chosen.
    OutboxRowSelected,
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
    /// The "New message" row at the top of the sidebar.
    ComposeRequested,
    /// The refresh button beside it.
    RefreshRequested,
    /// Long-press on the rail's refresh button: reveal the status bar.
    StatusBarRequested,
    /// Right-click on the Contacts row: open the GNOME Contacts app.
    OpenGnomeContacts,
    /// A folder-tree node was collapsed or expanded (#51) — for persistence.
    FolderNodeCollapsed { account_id: u32, path: String, collapsed: bool },
    /// A folder was dropped onto a new parent ("" = the account's top level).
    MoveFolder { account_id: u32, path: String, dest: String },
    UnifiedSelected,
    /// The attachments gallery was selected.
    AttachmentsSelected,
    /// The "Contacts" row was clicked — open the contacts browser.
    ContactsClicked,
    OutboxSelected,
    FolderSelected {
        account_id: u32,
        folder_id: u32,
        name: String,
        path: String,
    },
    ToggleCollapse(u32),
    /// The user toggled the collapsible custom-folders section for an account.
    ToggleCustomFolders(u32),
    /// The user toggled icon-only mode; `true` means collapsed.
    CollapsedChanged(bool),
    /// The empty-state "Add first account" button was clicked.
    AddAccount,
    /// A right-click context-menu action from a sidebar item.
    Context(CtxAction),
    /// Messages (identified by ids) were dropped on a folder to move them there.
    /// `items` is the whole dragged selection as (account, folder, uid, id) — it
    /// may include messages from other accounts when the drag started in the
    /// unified inbox; the app filters and reports those (#23).
    MoveMessages {
        dest_account: u32,
        dest: String,
        items: Vec<(u32, u32, u32, u32)>,
    },
}

/// Actions offered by sidebar right-click menus.
#[derive(Debug, Clone)]
pub enum CtxAction {
    MarkFolderRead { account_id: u32, folder_id: u32 },
    RefreshFolder { account_id: u32, folder_id: u32 },
    MarkAllInboxesRead,
    RefreshAllInboxes,
    /// Open Settings → Accounts with this account's editor up.
    OpenAccountSettings(u32),
    RemoveAccount(u32),
    /// Create a new custom folder under this account.
    NewFolder(u32),
    /// Delete a custom folder (its contents are moved to Trash first).
    DeleteFolder { account_id: u32, name: String, path: String },
    /// Rename a custom folder (its leaf name; children follow via RENAME).
    RenameFolder { account_id: u32, name: String, path: String },
}

#[relm4::component(pub)]
impl Component for Sidebar {
    type Init = SidebarInit;
    type Input = SidebarInput;
    type Output = SidebarOutput;
    type CommandOutput = ();

    view! {
        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,

            gtk::Overlay {
                set_vexpand: true,

                // The top block (compose bar, All Inboxes) is pinned above
                // the scroller so it never scrolls away; only the per-account
                // sections below scroll. Contacts/Attachments pin below it,
                // in the footer.
                #[wrap(Some)]
                set_child = &gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,

                    #[name = "pinned_box"]
                    gtk::Box {
                        set_orientation: gtk::Orientation::Vertical,
                    },

                    #[name = "sidebar_scroller"]
                    gtk::ScrolledWindow {
                        set_vexpand: true,
                        // External, not Never (see the message list's scroller): row
                        // content — a deeply indented folder tree, say — must not force
                        // the window's minimum width past what edge-tiling allows. The
                        // split view's min/max sidebar widths govern instead.
                        set_hscrollbar_policy: gtk::PolicyType::External,

                        #[name = "normal_box"]
                        gtk::Box {
                            set_orientation: gtk::Orientation::Vertical,
                        },
                    },

                    // Pinned footer, below the scroller: the Contacts and
                    // Attachments rows stay put against the sidebar's bottom
                    // edge no matter how tall the account list above grows.
                    #[name = "footer_box"]
                    gtk::Box {
                        set_orientation: gtk::Orientation::Vertical,
                    },
                },

                // Freeze-frame for rebuilds: the sidebar's last-rendered pixels,
                // shown over the swap so recreating every row never shimmers.
                #[name = "freeze_frame"]
                add_overlay = &gtk::Picture {
                    set_visible: false,
                    set_can_target: false,
                    set_content_fit: gtk::ContentFit::Fill,
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

        let mut model = Sidebar {
            sections: Vec::new(),
            show_unified: false,
            show_unified_chip: true,
            chevrons_left: false,
            revealers: HashMap::new(),
            chevrons: HashMap::new(),
            folder_lists: HashMap::new(),
            custom_folder_lists: HashMap::new(),
            custom_folders: HashMap::new(),
            tree_collapsed: HashMap::new(),
            tree_chevrons: HashMap::new(),
            tree_row_revealers: HashMap::new(),
            freeze_frame: None,
            freeze_timer: std::rc::Rc::new(std::cell::RefCell::new(None)),
            custom_revealers: HashMap::new(),
            custom_chevrons: HashMap::new(),
            unified_list: None,
            footer_list: None,
            attachments_row: None,
            contacts_row: None,
            sync_stack: None,
            sync_spinner: None,
            busy: false,
            outbox_list: None,
            color_provider,
            selected: Sel::None,
            collapsed: init.collapsed,
            show_attachments: init.show_attachments,
            show_contacts: init.show_contacts,
            outbox_count: 0,
            unified_unread: 0,
            folder_badges: HashMap::new(),
            unified_badge: None,
            unified_expanded: true,
            unified_revealer: None,
            unified_chevron: None,
            unified_inbox_list: None,
            unified_inboxes: Vec::new(),
            unified_inbox_badges: HashMap::new(),
            unified_folders: Vec::new(),
            unified_folders_expanded: true,
            unified_folders_revealer: None,
            unified_folders_chevron: None,
            unified_folder_list: None,
            unified_folder_badges: HashMap::new(),
            unified_folders_badge: None,
            unified_folders_unread: 0,
            account_circle_badges: HashMap::new(),
        };

        let widgets = view_output!();
        model.freeze_frame = Some(widgets.freeze_frame.clone());
        // Never scroll-to-focus: a rebuild (folder drag-and-drop) destroys
        // the focused row, GTK hands focus to some early widget, and the
        // viewport would yank the sidebar to the top to show it — the
        // "jumps up then back down" on every drop. Sidebar scrolling is the
        // user's alone.
        if let Some(viewport) =
            widgets.sidebar_scroller.child().and_downcast::<gtk::Viewport>()
        {
            viewport.set_scroll_to_focus(false);
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
            SidebarInput::SetContents {
                mut sections,
                show_unified,
                unified_chip,
                chevrons_left,
                unified_unread,
                unified_folders,
            } => {
                // Order each account's folders essential-first, then custom, so
                // the essential/custom split lines up with row indices (the main
                // list holds indices 0..E, the custom list E..).
                for s in &mut sections {
                    s.folders.sort_by_key(|f| f.kind == FolderKind::Custom);
                }
                // Seed the tree's collapsed nodes from the persisted state;
                // later chevron clicks flip the local copy.
                self.tree_collapsed = sections
                    .iter()
                    .map(|s| {
                        (s.account.id, s.tree_collapsed.iter().cloned().collect())
                    })
                    .collect();
                self.sections = sections;
                self.show_unified = show_unified;
                self.show_unified_chip = unified_chip;
                self.chevrons_left = chevrons_left;
                self.unified_unread = unified_unread;
                self.unified_folders = unified_folders;
                self.unified_folders_unread =
                    self.unified_folders.iter().map(|r| r.folder.unread).sum();
                // A selected filtered folder that just left the section (its
                // rule opted out, or the section was switched off) is still
                // the open folder: carry the highlight to the account
                // section's own row for it.
                if let Sel::UnifiedFolder(acc, path) = &self.selected {
                    let listed = show_unified
                        && self
                            .unified_folders
                            .iter()
                            .any(|r| r.account_id == *acc && r.folder.path == *path);
                    if !listed {
                        self.selected = Sel::Folder(*acc, path.clone());
                    }
                }
                self.rebuild_normal(
                    &widgets.pinned_box,
                    &widgets.normal_box,
                    &widgets.footer_box,
                    &sender,
                );
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

            SidebarInput::SelectUnifiedRow => {
                if self.selected == Sel::Unified {
                    return;
                }
                self.selected = Sel::Unified;
                self.clear_other_selections(Sel::Unified);
                if let Some(l) = &self.unified_list {
                    l.select_row(l.row_at_index(0).as_ref());
                }
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

            SidebarInput::ContactsRowClicked => {
                if self.selected == Sel::Contacts {
                    return;
                }
                self.selected = Sel::Contacts;
                self.clear_other_selections(Sel::Contacts);
                let _ = sender.output(SidebarOutput::ContactsClicked);
            }

            SidebarInput::OutboxRowSelected => {
                if self.selected == Sel::Outbox {
                    return;
                }
                self.selected = Sel::Outbox;
                self.clear_other_selections(Sel::Outbox);
                let _ = sender.output(SidebarOutput::OutboxSelected);
            }

            SidebarInput::SetOutboxCount(count) => {
                if self.outbox_count == count {
                    return;
                }
                let appearing = (self.outbox_count == 0) != (count == 0);
                self.outbox_count = count;
                // The row itself comes and goes with the count, so the sidebar
                // only needs rebuilding when it crosses zero; otherwise just
                // refresh the badge in place.
                if appearing {
                    // Leaving the Outbox selected when its row disappears would
                    // strand the view on an empty list.
                    if count == 0 && self.selected == Sel::Outbox {
                        self.selected = Sel::None;
                    }
                    self.rebuild_normal(
                        &widgets.pinned_box,
                        &widgets.normal_box,
                        &widgets.footer_box,
                        &sender,
                    );
                    self.restore_selection();
                } else if let Some(list) = &self.outbox_list {
                    if let Some(row) = list.row_at_index(0) {
                        if let Some(badge) = row.child().and_downcast::<gtk::Box>() {
                            if let Some(label) =
                                badge.last_child().and_downcast::<gtk::Label>()
                            {
                                label.set_label(&count.to_string());
                            }
                        }
                    }
                }
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
                // Expanded: the sub-list shows each inbox's count, so the
                // total chip bows out; it returns when collapsed back up.
                let show_chip =
                    self.unified_unread > 0 && !self.unified_expanded && self.show_unified_chip;
                if let Some(label) = &self.unified_badge {
                    label.set_visible(show_chip);
                }
                if let Some(ch) = &self.unified_chevron {
                    ch.set_icon_name(Some(if self.unified_expanded {
                        "co.hyprlab.Vireo-pan-down-symbolic"
                    } else {
                        "co.hyprlab.Vireo-pan-end-symbolic"
                    }));
                }
            }

            SidebarInput::UnifiedFolderRowSelected(index) => {
                if let Some(r) = self.unified_folders.get(index as usize).cloned() {
                    let key = Sel::UnifiedFolder(r.account_id, r.folder.path.clone());
                    if self.selected == key {
                        return;
                    }
                    self.selected = key.clone();
                    self.clear_other_selections(key);
                    let _ = sender.output(SidebarOutput::FolderSelected {
                        account_id: r.account_id,
                        folder_id: r.folder.id,
                        name: r.folder.name,
                        path: r.folder.path,
                    });
                }
            }

            SidebarInput::ToggleUnifiedFoldersExpand => {
                self.unified_folders_expanded = !self.unified_folders_expanded;
                if let Some(rev) = &self.unified_folders_revealer {
                    rev.set_reveal_child(self.unified_folders_expanded);
                }
                // Folded: the header wears the section's total; open, each
                // row carries its own count and the total bows out.
                if let Some(label) = &self.unified_folders_badge {
                    label.set_visible(
                        self.unified_folders_unread > 0 && !self.unified_folders_expanded,
                    );
                }
                if let Some(ch) = &self.unified_folders_chevron {
                    ch.set_icon_name(Some(if self.unified_folders_expanded {
                        "co.hyprlab.Vireo-pan-down-symbolic"
                    } else {
                        "co.hyprlab.Vireo-pan-end-symbolic"
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

            SidebarInput::SelectFolderRow { account_id, path } => {
                // A click on a per-account inbox under "All Inboxes" echoes
                // back here as its plain folder path; that sub-row already
                // shows this exact folder, so keep its highlight instead of
                // jumping to the account section's own Inbox row.
                if self.selected == Sel::UnifiedInbox(account_id)
                    && self
                        .unified_inboxes
                        .iter()
                        .any(|r| r.account_id == account_id && r.path == path)
                {
                    return;
                }
                // Likewise a click on a filtered folder inside All Inboxes.
                if self.selected == Sel::UnifiedFolder(account_id, path.clone()) {
                    return;
                }
                let key = Sel::Folder(account_id, path.clone());
                if self.selected != key {
                    self.selected = key.clone();
                    self.clear_other_selections(key);
                    self.select_folder(account_id, &path);
                }
            }

            SidebarInput::SetUnread { folders, unified } => {
                for ((aid, fid), label) in self
                    .folder_badges
                    .iter()
                    .chain(&self.unified_inbox_badges)
                    .chain(&self.unified_folder_badges)
                {
                    let n = folders.get(&(*aid, *fid)).copied().unwrap_or(0);
                    label.set_text(&n.to_string());
                    label.set_visible(n > 0);
                }
                if let Some(label) = &self.unified_badge {
                    label.set_text(&unified.to_string());
                    label.set_visible(
                        unified > 0 && !self.unified_expanded && self.show_unified_chip,
                    );
                }
                let filtered: u32 = self
                    .unified_folder_badges
                    .keys()
                    .map(|k| folders.get(k).copied().unwrap_or(0))
                    .sum();
                self.unified_folders_unread = filtered;
                if let Some(label) = &self.unified_folders_badge {
                    label.set_text(&filtered.to_string());
                    label.set_visible(filtered > 0 && !self.unified_folders_expanded);
                }
                // Keep the avatar-circle badges in sync too. They only show while
                // the account is collapsed (toggled live in ToggleCollapseLocal),
                // so we just refresh the number and re-apply that visibility rule.
                for section in &self.sections {
                    if let Some(label) = self.account_circle_badges.get(&section.account.id) {
                        let n = section
                            .folders
                            .iter()
                            .find(|f| f.kind == FolderKind::Inbox)
                            .and_then(|f| folders.get(&(section.account.id, f.id)))
                            .copied()
                            .unwrap_or(0);
                        label.set_text(&n.to_string());
                        label.set_visible(section.collapsed && n > 0);
                    }
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

            SidebarInput::SetAttachmentsRow(show) => {
                self.show_attachments = show;
                // The row is about to disappear from under the selection; fall
                // back the same way an emptied selection does.
                if !show && self.selected == Sel::Attachments {
                    self.selected = Sel::None;
                }
                self.rebuild_normal(
                    &widgets.pinned_box,
                    &widgets.normal_box,
                    &widgets.footer_box,
                    &sender,
                );
                self.restore_selection();
            }

            SidebarInput::SetContactsRow(show) => {
                self.show_contacts = show;
                // The row is about to disappear from under the selection; fall
                // back the same way an emptied selection does.
                if !show && self.selected == Sel::Contacts {
                    self.selected = Sel::None;
                }
                self.rebuild_normal(
                    &widgets.pinned_box,
                    &widgets.normal_box,
                    &widgets.footer_box,
                    &sender,
                );
                self.restore_selection();
            }

            SidebarInput::SetBusy(busy) => {
                self.busy = busy;
                if let Some(sp) = &self.sync_spinner {
                    sp.set_spinning(busy);
                }
                if let Some(stack) = &self.sync_stack {
                    stack.set_visible_child_name(if busy { "spinner" } else { "icon" });
                }
            }


            SidebarInput::SetCollapsed(collapsed) => {
                // Driven by the app's narrow-window breakpoint: same visual
                // change as the user's own toggle, but no CollapsedChanged
                // output — automatic switches must not overwrite the user's
                // persisted preference.
                if self.collapsed != collapsed {
                    self.collapsed = collapsed;
                    self.rebuild_normal(
                        &widgets.pinned_box,
                        &widgets.normal_box,
                        &widgets.footer_box,
                        &sender,
                    );
                    self.restore_selection();
                }
            }

            SidebarInput::ToggleCollapsed => {
                self.collapsed = !self.collapsed;
                self.rebuild_normal(
                    &widgets.pinned_box,
                    &widgets.normal_box,
                    &widgets.footer_box,
                    &sender,
                );
                self.restore_selection();
                let _ = sender.output(SidebarOutput::CollapsedChanged(self.collapsed));
            }

            SidebarInput::ToggleCollapseLocal(id) => {
                if let Some(rev) = self.revealers.get(&id) {
                    let expanded = !rev.reveals_child();
                    rev.set_reveal_child(expanded);
                    if let Some(ch) = self.chevrons.get(&id) {
                        ch.set_icon_name(Some(if expanded {
                            "co.hyprlab.Vireo-pan-down-symbolic"
                        } else {
                            "co.hyprlab.Vireo-pan-end-symbolic"
                        }));
                    }
                    if let Some(s) = self.sections.iter_mut().find(|s| s.account.id == id) {
                        s.collapsed = !expanded;
                    }
                    // The Inbox chip lives inside the folder list we just hid/shown,
                    // so mirror it onto the avatar: visible only while collapsed.
                    if let Some(label) = self.account_circle_badges.get(&id) {
                        let n = self
                            .sections
                            .iter()
                            .find(|s| s.account.id == id)
                            .and_then(|s| s.folders.iter().find(|f| f.kind == FolderKind::Inbox))
                            .map(|f| f.unread)
                            .unwrap_or(0);
                        label.set_text(&n.to_string());
                        label.set_visible(!expanded && n > 0);
                    }
                    let _ = sender.output(SidebarOutput::ToggleCollapse(id));
                }
            }

            SidebarInput::ToggleCustomFoldersLocal(id) => {
                if let Some(rev) = self.custom_revealers.get(&id) {
                    let expanded = !rev.reveals_child();
                    rev.set_reveal_child(expanded);
                    if let Some(ch) = self.custom_chevrons.get(&id) {
                        ch.set_icon_name(Some(if expanded {
                            "co.hyprlab.Vireo-pan-down-symbolic"
                        } else {
                            "co.hyprlab.Vireo-pan-end-symbolic"
                        }));
                    }
                    if let Some(s) = self.sections.iter_mut().find(|s| s.account.id == id) {
                        s.custom_expanded = expanded;
                    }
                    let _ = sender.output(SidebarOutput::ToggleCustomFolders(id));
                }
            }

            SidebarInput::ToggleFolderNode { account_id, path } => {
                self.toggle_folder_node(account_id, path, &sender);
            }

            SidebarInput::FolderRowActivated { account_id, index } => {
                // Single-clicking a folder that has sub-folders toggles them —
                // no need to aim for the caret. Leaves just select as before.
                let target = self.custom_folders.get(&account_id).and_then(|folders| {
                    folders.get(index as usize).and_then(|f| {
                        folders
                            .iter()
                            .any(|g| path_is_under(&g.path, &f.path))
                            .then(|| f.path.clone())
                    })
                });
                if let Some(path) = target {
                    self.toggle_folder_node(account_id, path, &sender);
                }
            }

            SidebarInput::ExpandForDrop(id) => {
                // Expand a collapsed account so its folders become drop targets.
                if let Some(rev) = self.revealers.get(&id) {
                    if !rev.reveals_child() {
                        rev.set_reveal_child(true);
                        if let Some(ch) = self.chevrons.get(&id) {
                            ch.set_icon_name(Some("co.hyprlab.Vireo-pan-down-symbolic"));
                        }
                        if let Some(s) = self.sections.iter_mut().find(|s| s.account.id == id) {
                            s.collapsed = false;
                        }
                        let _ = sender.output(SidebarOutput::ToggleCollapse(id));
                    }
                }
            }

            SidebarInput::DropOnFolder { account_id: dest_account, path: dest, payload } => {
                // A dragged folder, not messages: reparent it (#51). Folders
                // never cross accounts — mailboxes belong to one server.
                if let Some(rest) = payload.strip_prefix("vireo-folder\t") {
                    let mut it = rest.splitn(2, '\t');
                    let src_account = it.next().and_then(|s| s.parse::<u32>().ok());
                    let src_path = it.next().map(String::from);
                    if let (Some(src_account), Some(src_path)) = (src_account, src_path) {
                        if src_account == dest_account && src_path != dest {
                            let _ = sender.output(SidebarOutput::MoveFolder {
                                account_id: dest_account,
                                path: src_path,
                                dest,
                            });
                        }
                    }
                    return;
                }
                let items = parse_move_payload(&payload);
                // "" is the Folders header (a folder-move destination only);
                // messages need a real mailbox.
                if !items.is_empty() && !dest.is_empty() {
                    let _ = sender.output(SidebarOutput::MoveMessages { dest_account, dest, items });
                }
            }
        }
    }
}

impl Sidebar {
    /// Rebuild the list: optional unified row, then per-account headers with
    /// animated folder revealers, and refresh the per-account colour rules.
    /// Flip one tree node, restyle its caret, and re-apply visibility across
    /// the account's tree — no rebuild, so nothing flickers. Reports the new
    /// state for persistence.
    fn toggle_folder_node(
        &mut self,
        account_id: u32,
        path: String,
        sender: &ComponentSender<Self>,
    ) {
        let nodes = self.tree_collapsed.entry(account_id).or_default();
        let collapsed = if nodes.contains(&path) {
            nodes.remove(&path);
            false
        } else {
            nodes.insert(path.clone());
            true
        };
        if let Some(img) = self.tree_chevrons.get(&(account_id, path.clone())) {
            if collapsed {
                img.remove_css_class("open");
            } else {
                img.add_css_class("open");
            }
        }
        self.apply_tree_visibility(account_id);
        let _ = sender.output(SidebarOutput::FolderNodeCollapsed { account_id, path, collapsed });
    }

    /// Re-apply row visibility across one account's custom-folder tree (#51):
    /// a row shows unless some ancestor node is collapsed. Rows are never
    /// removed, so selection indices hold still.
    fn apply_tree_visibility(&self, account_id: u32) {
        let (Some(list), Some(folders)) = (
            self.custom_folder_lists.get(&account_id),
            self.custom_folders.get(&account_id),
        ) else {
            return;
        };
        let collapsed = self.tree_collapsed.get(&account_id).cloned().unwrap_or_default();
        let revealers = self.tree_row_revealers.get(&account_id);
        for (i, folder) in folders.iter().enumerate() {
            let Some(row) = list.row_at_index(i as i32) else { continue };
            let hidden = hidden_by_collapse(&folder.path, &collapsed);
            let rev = revealers.and_then(|r| r.get(i));
            match (hidden, rev) {
                (false, Some(rev)) => {
                    // Show the row first, then slide its content open.
                    row.set_visible(true);
                    rev.set_reveal_child(true);
                }
                (true, Some(rev)) => {
                    if row.get_visible() {
                        // Slide closed, then drop the row itself once the
                        // animation is done — an empty visible row still
                        // paints its chrome. Skipped if it was re-expanded
                        // inside the window.
                        rev.set_reveal_child(false);
                        let row = row.clone();
                        let rev = rev.clone();
                        gtk::glib::timeout_add_local_once(
                            std::time::Duration::from_millis(220),
                            move || {
                                if !rev.reveals_child() {
                                    row.set_visible(false);
                                }
                            },
                        );
                    }
                }
                (hidden, None) => row.set_visible(!hidden),
            }
        }
    }

    fn rebuild_normal(
        &mut self,
        pinned: &gtk::Box,
        container: &gtk::Box,
        footer: &gtk::Box,
        sender: &ComponentSender<Self>,
    ) {
        // Keep the scroll offset: rebuilding otherwise snaps the sidebar to
        // the top — felt on every folder drag-and-drop, whose optimistic move
        // rebuilds immediately under the pointer.
        let scroller = container
            .ancestor(gtk::ScrolledWindow::static_type())
            .and_downcast::<gtk::ScrolledWindow>();
        let saved_scroll = scroller.as_ref().map(|s| s.vadjustment().value());
        // Freeze the sidebar's last-rendered pixels over the swap: even with
        // the scroll pinned, tearing down and recreating every row can
        // shimmer for a frame. The snapshot covers the rebuild and lifts a
        // few frames later, once the fresh tree has painted beneath it —
        // identical pixels, so the crossover is invisible. The snapshot is of
        // the overlay's whole child (pinned block + scroller + footer),
        // matching the area the freeze-frame Picture stretches over.
        if let (Some(freeze), Some(area)) = (self.freeze_frame.clone(), pinned.parent()) {
            if container.first_child().is_some() || pinned.first_child().is_some() {
                use gtk::gdk::prelude::PaintableExt;
                let live = gtk::WidgetPaintable::new(Some(&area));
                freeze.set_paintable(Some(&live.current_image()));
                freeze.set_visible(true);
                let timer = gtk::glib::timeout_add_local_once(
                    std::time::Duration::from_millis(80),
                    {
                        let freeze = freeze.clone();
                        let slot = self.freeze_timer.clone();
                        move || {
                            slot.borrow_mut().take();
                            freeze.set_visible(false);
                        }
                    },
                );
                if let Some(prev) = self.freeze_timer.borrow_mut().replace(timer) {
                    prev.remove();
                }
            }
        }
        while let Some(child) = pinned.first_child() {
            pinned.remove(&child);
        }
        while let Some(child) = container.first_child() {
            container.remove(&child);
        }
        while let Some(child) = footer.first_child() {
            footer.remove(&child);
        }
        if self.collapsed {
            pinned.add_css_class("rail-collapsed");
            container.add_css_class("rail-collapsed");
            footer.add_css_class("rail-collapsed");
        } else {
            pinned.remove_css_class("rail-collapsed");
            container.remove_css_class("rail-collapsed");
            footer.remove_css_class("rail-collapsed");
        }
        self.revealers.clear();
        self.chevrons.clear();
        self.folder_lists.clear();
        self.custom_folder_lists.clear();
        self.custom_revealers.clear();
        self.custom_chevrons.clear();
        self.unified_list = None;
        self.footer_list = None;
        self.attachments_row = None;
        self.contacts_row = None;
        self.outbox_list = None;
        self.folder_badges.clear();
        self.tree_chevrons.clear();
        self.tree_row_revealers.clear();
        self.unified_badge = None;
        self.unified_revealer = None;
        self.unified_chevron = None;
        self.unified_inbox_list = None;
        self.unified_inboxes.clear();
        self.unified_inbox_badges.clear();
        self.unified_folders_revealer = None;
        self.unified_folders_chevron = None;
        self.unified_folder_list = None;
        self.unified_folder_badges.clear();
        self.unified_folders_badge = None;
        self.account_circle_badges.clear();

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
                add.set_icon_name("co.hyprlab.Vireo-list-add-symbolic");
                add.set_tooltip_text(Some("Add account"));
                add.set_margin_top(12);
                container.append(&add);
            } else {
                let label_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                label_box.append(&gtk::Image::from_icon_name("co.hyprlab.Vireo-list-add-symbolic"));
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

        // "New message" — the compose action. Expanded, the pill sits alone
        // and centred (Refresh lives in the app's header bar, top-left across
        // from the menu). The collapsed rail's header only has room for the
        // menu button, so Refresh stacks here instead — directly below it.
        {
            let bar = gtk::Box::new(
                if self.collapsed {
                    gtk::Orientation::Vertical
                } else {
                    gtk::Orientation::Horizontal
                },
                0,
            );

            self.sync_stack = None;
            self.sync_spinner = None;
            if self.collapsed {
                // Refresh, showing a spinner while any account syncs.
                let refresh = gtk::Button::new();
                refresh.set_tooltip_text(Some("Refresh or long-press for Status Bar"));
                refresh.add_css_class("flat");
                refresh.set_valign(gtk::Align::Center);
                refresh.set_halign(gtk::Align::Center);
                let stack = gtk::Stack::new();
                stack.set_transition_type(gtk::StackTransitionType::Crossfade);
                let icon = gtk::Image::from_icon_name("co.hyprlab.Vireo-view-refresh-symbolic");
                stack.add_named(&icon, Some("icon"));
                let spinner = gtk::Spinner::new();
                spinner.set_spinning(self.busy);
                stack.add_named(&spinner, Some("spinner"));
                stack.set_visible_child_name(if self.busy { "spinner" } else { "icon" });
                refresh.set_child(Some(&stack));
                let s = sender.clone();
                refresh.connect_clicked(move |_| {
                    let _ = s.output(SidebarOutput::RefreshRequested);
                });
                // Long-press reveals the status bar (same as the header
                // refresh); claiming the sequence suppresses the click.
                let long = gtk::GestureLongPress::new();
                let s = sender.clone();
                long.connect_pressed(move |gesture, _, _| {
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                    let _ = s.output(SidebarOutput::StatusBarRequested);
                });
                refresh.add_controller(long);
                bar.append(&refresh);
                self.sync_stack = Some(stack);
                self.sync_spinner = Some(spinner);
            }

            // The compose button, drawn like a row so it matches the
            // sidebar's look. Expanded: full width like the rows below, icon
            // and label centred. The collapsed rail shows the icon alone.
            let list = gtk::ListBox::new();
            list.set_selection_mode(gtk::SelectionMode::None);
            list.add_css_class("navigation-sidebar");
            let row = gtk::ListBoxRow::new();
            row.add_css_class("compose-row");
            let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            hbox.add_css_class("folder-row");
            if self.collapsed {
                // The rail has no room for a label; the icon carries it there.
                let img = gtk::Image::from_icon_name("co.hyprlab.Vireo-mail-message-new-symbolic");
                img.add_css_class("folder-icon");
                pin_icon_size(&img);
                hbox.set_halign(gtk::Align::Center);
                row.set_tooltip_text(Some("New Message"));
                hbox.append(&img);
            } else {
                hbox.set_halign(gtk::Align::Center);
                hbox.set_spacing(6);
                let icon =
                    gtk::Image::from_icon_name("co.hyprlab.Vireo-mail-message-new-symbolic");
                icon.add_css_class("folder-icon");
                hbox.append(&icon);
                let label = gtk::Label::new(Some("New Message"));
                label.add_css_class("account-name");
                // The pill must be able to shrink with the sidebar (down to its
                // 180px minimum) — otherwise the whole column's minimum width
                // exceeds the pane and every row highlight overflows the edge.
                label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                hbox.append(&label);
            }
            row.set_child(Some(&hbox));
            list.append(&row);
            let s = sender.clone();
            list.connect_row_activated(move |_, _| {
                let _ = s.output(SidebarOutput::ComposeRequested);
            });

            if !self.collapsed {
                // Full width, like the rows below it; the centred label
                // carries the action on its own.
                list.set_hexpand(true);
                list.set_halign(gtk::Align::Fill);
            }
            bar.append(&list);
            pinned.append(&bar);
        }

        // Unified "All Inboxes" row, with an expandable per-account inbox list.
        if self.show_unified {
            let list = gtk::ListBox::new();
            list.set_selection_mode(gtk::SelectionMode::Single);
            list.add_css_class("navigation-sidebar");

            let row = gtk::ListBoxRow::new();
            // Tagged so the disclosure chevron can be lined up with the
            // account headers' (see styles.css).
            row.add_css_class("unified-row");
            let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            hbox.add_css_class("folder-row");
            let img = gtk::Image::from_icon_name("co.hyprlab.Vireo-mail-inbox-symbolic");
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
                badge.set_visible(
                    self.unified_unread > 0 && !self.unified_expanded && self.show_unified_chip,
                );
                hbox.append(&overlay);
                self.unified_badge = Some(badge);
            } else {
                // The disclosure chevron toggling the per-account inbox
                // sub-list — its own button either way (selecting the
                // unified view and expanding it stay separate actions).
                // Placement follows Settings → Chevron placement: leading
                // (overlaid on the row's left edge — see below), or classic
                // trailing.
                let chevron = gtk::Image::from_icon_name(if self.unified_expanded {
                    "co.hyprlab.Vireo-pan-down-symbolic"
                } else {
                    "co.hyprlab.Vireo-pan-end-symbolic"
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
                row.add_css_class(if self.chevrons_left { "chev-left" } else { "chev-right" });
                pin_icon_size(&img);
                if self.chevrons_left {
                    // Centers this 16px icon on the avatar circles below it
                    // (rather than matching left edges) — a small icon flush
                    // with a much wider circle's left edge reads as
                    // off-centre next to it (PR #95).
                    img.set_margin_start(ROW_LEFT_INSET + 9);
                }
                hbox.append(&img);
                let label = gtk::Label::new(Some("All Inboxes"));
                label.set_halign(gtk::Align::Start);
                label.set_hexpand(true);
                label.add_css_class("account-name");
                if self.chevrons_left {
                    label.set_margin_start(-2);
                }
                hbox.append(&label);
                self.unified_chevron = Some(chevron.clone());
                // The total-unread chip right-aligns like every folder row's,
                // one shared column down the sidebar. While the sub-list is
                // expanded the per-inbox rows carry the counts, so the total
                // is redundant and hidden.
                let badge = gtk::Label::new(Some(&self.unified_unread.to_string()));
                badge.add_css_class("unread-badge");
                badge.set_valign(gtk::Align::Center);
                badge.set_visible(
                    self.unified_unread > 0 && !self.unified_expanded && self.show_unified_chip,
                );
                hbox.append(&badge);
                self.unified_badge = Some(badge);
                if self.chevrons_left {
                    // Overlaid on the row's left edge rather than packed into
                    // the layout — the same trick as the rail's unread badges
                    // — so it reserves no space of its own: icon and label
                    // keep their normal position and the chip keeps the
                    // shared flush-right column (Isaac's PR #95 mechanism).
                    chevron.set_pixel_size(15);
                    chev_btn.add_css_class("row-disclosure-chevron");
                    chev_btn.set_halign(gtk::Align::Start);
                    let overlay = gtk::Overlay::new();
                    overlay.set_child(Some(&hbox));
                    overlay.add_overlay(&chev_btn);
                    row.set_child(Some(&overlay));
                } else {
                    hbox.append(&chev_btn);
                }
            }
            if row.child().is_none() {
                row.set_child(Some(&hbox));
            }
            list.append(&row);

            let s = sender.input_sender().clone();
            list.connect_row_selected(move |_, row| {
                if row.is_some() {
                    let _ = s.send(SidebarInput::UnifiedRowSelected);
                }
            });
            // Double-click toggles the per-account sub-list, same as the
            // chevron (single click only selects; activation needs the
            // double-click once single-click activation is off).
            list.set_activate_on_single_click(false);
            let s2 = sender.input_sender().clone();
            list.connect_row_activated(move |_, _| {
                let _ = s2.send(SidebarInput::ToggleUnifiedExpand);
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
            pinned.append(&list);
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
                        "co.hyprlab.Vireo-pan-down-symbolic"
                    } else {
                        "co.hyprlab.Vireo-pan-end-symbolic"
                    });
                    toggle.set_child(Some(&chevron));
                    let cs = sender.input_sender().clone();
                    toggle.connect_clicked(move |_| {
                        let _ = cs.send(SidebarInput::ToggleUnifiedExpand);
                    });
                    pinned.append(&toggle);
                    self.unified_chevron = Some(chevron);
                }

                let sub = gtk::ListBox::new();
                sub.set_selection_mode(gtk::SelectionMode::Single);
                sub.add_css_class("navigation-sidebar");
                // Breathing room between the expanded sub-list and the first
                // account section below; folds away with the revealer. With
                // a Filtered Folders section underneath, that section's list
                // carries the gap instead.
                if self.unified_folders.is_empty() {
                    sub.set_margin_bottom(14);
                }
                for section in &sections {
                    let Some(inbox) =
                        section.folders.iter().find(|f| f.kind == FolderKind::Inbox)
                    else {
                        continue;
                    };
                    let aid = section.account.id;
                    let (row, badge) =
                        build_unified_inbox_row(section, inbox, self.collapsed, self.chevrons_left);
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
                                ("Account Settings…", CtxAction::OpenAccountSettings(r.account_id)),
                            ],
                            &cs,
                        );
                    }
                });
                sub.add_controller(click);

                // Everything that folds away with All Inboxes: the inbox
                // sub-list, then (when any rule opts in) the collapsible
                // "Filtered Folders" section beneath it.
                let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
                body.append(&sub);
                if !self.unified_folders.is_empty() {
                    self.build_unified_folders(&body, &sections, sender);
                }

                let revealer = gtk::Revealer::new();
                revealer.set_transition_type(gtk::RevealerTransitionType::SlideDown);
                // 0 during the rebuild: an animated reveal grows the content's
                // height over 200ms, which drags the scroll toward the top
                // mid-rebuild. Real duration restored one frame later (below).
                revealer.set_transition_duration(0);
                revealer.set_reveal_child(self.unified_expanded);
                revealer.set_child(Some(&body));
                pinned.append(&revealer);
                self.unified_revealer = Some(revealer);
                self.unified_inbox_list = Some(sub);
            }
        }

        // Contacts and Attachments live in the pinned footer against the
        // sidebar's bottom edge — they keep out of the way of the account
        // list and never scroll off. A faint rule sets them apart from
        // whatever the scroller above ends on. ONE list box holds both rows,
        // so they read as one gapless section (beta 1.18.0b feedback) and
        // selecting one automatically clears the other.
        if self.show_contacts || self.show_attachments {
            let sep = gtk::Separator::new(gtk::Orientation::Horizontal);
            sep.add_css_class("footer-separator");
            footer.append(&sep);

            let list = gtk::ListBox::new();
            list.set_selection_mode(gtk::SelectionMode::Single);
            list.add_css_class("navigation-sidebar");

            // "Contacts" row — shows the in-app contacts view. Right-click
            // offers a jump straight to the GNOME Contacts app.
            if self.show_contacts {
                let row = gtk::ListBoxRow::new();
                let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 12);
                hbox.add_css_class("folder-row");
                let img =
                    gtk::Image::from_icon_name("co.hyprlab.Vireo-x-office-address-book-symbolic");
                img.add_css_class("folder-icon");
                pin_icon_size(&img);
                if self.collapsed {
                    hbox.set_halign(gtk::Align::Center);
                    row.set_tooltip_text(Some("Contacts"));
                    hbox.append(&img);
                } else {
                    if self.chevrons_left {
                        img.set_margin_start(ROW_LEFT_INSET);
                    }
                    hbox.append(&img);
                    let label = gtk::Label::new(Some("Contacts"));
                    label.set_hexpand(true);
                    label.set_halign(gtk::Align::Start);
                    label.add_css_class("account-name");
                    hbox.append(&label);
                }
                row.set_child(Some(&hbox));

                let right_click = gtk::GestureClick::new();
                right_click.set_button(3);
                let s = sender.clone();
                right_click.connect_pressed(move |gesture, _, x, y| {
                    let Some(widget) = gesture.widget() else { return };
                    let s2 = s.clone();
                    show_context_menu(
                        &widget,
                        x,
                        y,
                        vec![vec![MenuEntry::new("Open GNOME Contacts", move || {
                            let _ = s2.output(SidebarOutput::OpenGnomeContacts);
                        })
                        .icon("co.hyprlab.Vireo-adw-external-link-symbolic")]],
                    );
                });
                row.add_controller(right_click);
                list.append(&row);
                self.contacts_row = Some(row);
            }

            // "Attachments" row — a gallery of every inbox attachment. The
            // very last row in the sidebar, below Contacts.
            if self.show_attachments {
                let row = gtk::ListBoxRow::new();
                let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 12);
                hbox.add_css_class("folder-row");
                let img = gtk::Image::from_icon_name("co.hyprlab.Vireo-mail-attachment-symbolic");
                img.add_css_class("folder-icon");
                pin_icon_size(&img);
                if self.collapsed {
                    hbox.set_halign(gtk::Align::Center);
                    row.set_tooltip_text(Some("Attachments"));
                    hbox.append(&img);
                } else {
                    if self.chevrons_left {
                        img.set_margin_start(ROW_LEFT_INSET);
                    }
                    hbox.append(&img);
                    let label = gtk::Label::new(Some("Attachments"));
                    label.set_hexpand(true);
                    label.set_halign(gtk::Align::Start);
                    label.add_css_class("account-name");
                    hbox.append(&label);
                }
                row.set_child(Some(&hbox));
                list.append(&row);
                self.attachments_row = Some(row);
            }

            let s = sender.clone();
            let contacts_row = self.contacts_row.clone();
            let attachments_row = self.attachments_row.clone();
            list.connect_row_selected(move |_, row| {
                let Some(row) = row else { return };
                if Some(row) == contacts_row.as_ref() {
                    s.input(SidebarInput::ContactsRowClicked);
                } else if Some(row) == attachments_row.as_ref() {
                    s.input(SidebarInput::AttachmentsRowSelected);
                }
            });
            footer.append(&list);
            self.footer_list = Some(list);
        }

        // "Outbox" row — only while something is waiting to be sent. It sits
        // directly above the accounts so a stuck message is impossible to miss.
        if self.outbox_count > 0 {
            let list = gtk::ListBox::new();
            list.set_selection_mode(gtk::SelectionMode::Single);
            list.add_css_class("navigation-sidebar");

            let row = gtk::ListBoxRow::new();
            let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            hbox.add_css_class("folder-row");
            let img = gtk::Image::from_icon_name("co.hyprlab.Vireo-mail-send-symbolic");
            img.add_css_class("folder-icon");
            pin_icon_size(&img);
            let badge = gtk::Label::new(Some(&self.outbox_count.to_string()));
            badge.add_css_class("unread-badge");
            badge.set_valign(gtk::Align::Center);
            if self.collapsed {
                hbox.set_halign(gtk::Align::Center);
                row.set_tooltip_text(Some(&format!(
                    "Outbox — {} waiting to be sent",
                    self.outbox_count
                )));
                hbox.append(&img);
            } else {
                if self.chevrons_left {
                    img.set_margin_start(ROW_LEFT_INSET);
                }
                hbox.append(&img);
                let label = gtk::Label::new(Some("Outbox"));
                label.set_hexpand(true);
                label.set_halign(gtk::Align::Start);
                label.add_css_class("account-name");
                hbox.append(&label);
                hbox.append(&badge);
            }
            row.set_child(Some(&hbox));
            list.append(&row);

            let s = sender.clone();
            list.connect_row_selected(move |_, row| {
                if row.is_some() {
                    s.input(SidebarInput::OutboxRowSelected);
                }
            });
            container.append(&list);
            self.outbox_list = Some(list);
        }

        for section in &sections {
            let id = section.account.id;

            // Header: avatar circle + name/email on the left, chevron on the right.
            let header = gtk::Button::new();
            header.add_css_class("flat");
            header.add_css_class("account-header");
            let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 10);

            // A configured label wins (same as the All Inboxes sub-rows);
            // otherwise the account's name, then its address.
            let name_str = if !section.account.label.trim().is_empty()
                && section.account.label != section.account.email
            {
                section.account.label.clone()
            } else if section.account.name.trim().is_empty() {
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
            circle.set_size_request(30, 30);
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
            // While this account's section is collapsed its Inbox row (and the
            // chip on it) is hidden inside the revealer, so surface the inbox
            // unread count on the avatar instead — mirroring how the collapsed
            // "All Inboxes" rail badges its icon.
            let inbox_unread = section
                .folders
                .iter()
                .find(|f| f.kind == FolderKind::Inbox)
                .map(|f| f.unread)
                .unwrap_or(0);
            let (circle_overlay, circle_badge) = with_unread_overlay(&circle, inbox_unread);
            circle_overlay.set_halign(gtk::Align::Center);
            circle_overlay.set_hexpand(false);
            circle_badge.set_visible(section.collapsed && inbox_unread > 0);
            self.account_circle_badges.insert(id, circle_badge);
            if !self.collapsed && self.chevrons_left {
                // Leaves room for the overlaid disclosure chevron, which
                // would otherwise sit right on top of the avatar — a bit
                // more than the minimum, so the two aren't touching.
                circle_overlay.set_margin_start(ROW_LEFT_INSET + 8);
            }

            // Chevron is tracked even when collapsed so per-account toggles
            // still update an icon; it's only shown in the expanded layout —
            // leading (overlaid on the header's left edge, reserving no
            // layout space, like the All Inboxes row's) or classic trailing,
            // per Settings → Chevron placement.
            let chevron = gtk::Image::from_icon_name(if section.collapsed {
                "co.hyprlab.Vireo-pan-end-symbolic"
            } else {
                "co.hyprlab.Vireo-pan-down-symbolic"
            });
            chevron.set_valign(gtk::Align::Center);

            if self.collapsed {
                hbox.append(&circle_overlay);
                hbox.set_halign(gtk::Align::Center);
                header.set_tooltip_text(Some(&name_str));
            } else {
                hbox.append(&circle_overlay);
                let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
                vbox.set_hexpand(true);
                vbox.set_valign(gtk::Align::Center);
                // Clearance for the unread chip that overlays the circle's
                // corner while the section is collapsed.
                vbox.set_margin_start(6);
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
                if self.chevrons_left {
                    chevron.add_css_class("row-disclosure-chevron");
                    chevron.set_pixel_size(15);
                    chevron.set_halign(gtk::Align::Start);
                    let overlay = gtk::Overlay::new();
                    overlay.set_child(Some(&hbox));
                    overlay.add_overlay(&chevron);
                    header.set_child(Some(&overlay));
                } else {
                    hbox.append(&chevron);
                }
            }

            if header.child().is_none() {
                header.set_child(Some(&hbox));
            }
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
                items.push(("Account Settings…", CtxAction::OpenAccountSettings(id)));
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
            revealer.set_transition_duration(0);
            revealer.set_reveal_child(!section.collapsed);

            // Split folders: essential (Inbox/Sent/Trash/Archive/…) are always
            // shown; user-created "custom" folders are tucked under a collapsible
            // "Folders" section. `section.folders` is already essential-first, so
            // the essential list holds row indices 0..E and the custom list E..
            let essential: Vec<&Folder> = section
                .folders
                .iter()
                .filter(|f| f.kind != FolderKind::Custom)
                .collect();
            let custom: Vec<&Folder> = section
                .folders
                .iter()
                .filter(|f| f.kind == FolderKind::Custom)
                .collect();
            let e = essential.len() as i32;

            let list = gtk::ListBox::new();
            list.set_selection_mode(gtk::SelectionMode::Single);
            list.add_css_class("navigation-sidebar");
            for folder in &essential {
                let (row, badge) =
                    build_folder_row(folder, self.collapsed, 0, None, self.chevrons_left, None);
                row.add_controller(folder_drop_target(id, folder.path.clone(), sender));
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
            attach_folder_context_menu(
                &list,
                id,
                essential.iter().map(|f| (*f).clone()).collect(),
                sender,
            );

            // The collapsible custom-folders list + its "Folders" toggle header.
            let custom_list = gtk::ListBox::new();
            custom_list.set_selection_mode(gtk::SelectionMode::Single);
            custom_list.add_css_class("navigation-sidebar");
            let custom_revealer = gtk::Revealer::new();
            let custom_chevron = gtk::Image::from_icon_name(if section.custom_expanded {
                "co.hyprlab.Vireo-pan-down-symbolic"
            } else {
                "co.hyprlab.Vireo-pan-end-symbolic"
            });
            let folders_toggle = gtk::Button::new();
            if !custom.is_empty() {
                let collapsed_nodes =
                    self.tree_collapsed.get(&id).cloned().unwrap_or_default();
                for folder in &custom {
                    let depth = folder_depth(folder, &custom);
                    // The expander slot (#51): a chevron for folders with
                    // sub-folders, an equal-width spacer for leaves so names
                    // at one depth stay aligned. Rail mode has no room for
                    // either.
                    let has_children =
                        custom.iter().any(|g| path_is_under(&g.path, &folder.path));
                    let lead: Option<gtk::Widget> = if self.collapsed {
                        None
                    } else if has_children {
                        // One right-pointing caret; the "open" class rotates it
                        // 90° via a CSS transition, so toggling spins smoothly
                        // instead of swapping glyphs.
                        let img = gtk::Image::from_icon_name("co.hyprlab.Vireo-pan-end-symbolic");
                        img.add_css_class("tree-expander-icon");
                        if !collapsed_nodes.contains(&folder.path) {
                            img.add_css_class("open");
                        }
                        img.set_pixel_size(12);
                        let btn = gtk::Button::new();
                        btn.set_child(Some(&img));
                        btn.add_css_class("flat");
                        btn.add_css_class("tree-expander");
                        btn.set_valign(gtk::Align::Center);
                        btn.set_tooltip_text(Some("Show or hide sub-folders"));
                        let st = sender.input_sender().clone();
                        let path = folder.path.clone();
                        btn.connect_clicked(move |_| {
                            let _ = st.send(SidebarInput::ToggleFolderNode {
                                account_id: id,
                                path: path.clone(),
                            });
                        });
                        self.tree_chevrons.insert((id, folder.path.clone()), img);
                        Some(btn.upcast())
                    } else {
                        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                        spacer.set_width_request(TREE_EXPANDER_WIDTH);
                        Some(spacer.upcast())
                    };
                    let (row, badge) =
                        build_folder_row(
                            folder,
                            self.collapsed,
                            depth,
                            lead.as_ref(),
                            self.chevrons_left,
                            None,
                        );
                    // Hidden while any ancestor is collapsed; the row still
                    // exists, so selection indices stay stable. Its content
                    // sits in a revealer so user toggles slide open/closed —
                    // built with no transition (rebuilds must reach full
                    // height in one pass; see the scroll-jump saga), armed to
                    // 200ms alongside the section revealers below.
                    let hidden = hidden_by_collapse(&folder.path, &collapsed_nodes);
                    if let Some(content) = row.child() {
                        row.set_child(gtk::Widget::NONE);
                        let rev = gtk::Revealer::new();
                        rev.set_transition_type(gtk::RevealerTransitionType::SlideDown);
                        rev.set_transition_duration(0);
                        rev.set_child(Some(&content));
                        rev.set_reveal_child(!hidden);
                        row.set_child(Some(&rev));
                        self.tree_row_revealers
                            .entry(id)
                            .or_default()
                            .push(rev);
                    }
                    row.set_visible(!hidden);
                    row.add_controller(folder_drop_target(id, folder.path.clone(), sender));
                    // Custom folders can be picked up and dropped on a new
                    // parent (#51); essential folders stay where the server
                    // put them.
                    if !self.collapsed {
                        let drag = gtk::DragSource::new();
                        drag.set_actions(gtk::gdk::DragAction::MOVE);
                        let payload = format!("vireo-folder\t{id}\t{}", folder.path);
                        drag.connect_prepare(move |_, _, _| {
                            Some(gtk::gdk::ContentProvider::for_value(&payload.to_value()))
                        });
                        row.add_controller(drag);
                    }
                    custom_list.append(&row);
                    if let Some(badge) = badge {
                        self.folder_badges.insert((id, folder.id), badge);
                    }
                }
                self.custom_folders
                    .insert(id, custom.iter().map(|f| (*f).clone()).collect());
                let s3 = sender.input_sender().clone();
                custom_list.connect_row_selected(move |_, row| {
                    if let Some(row) = row {
                        // Offset past the essential folders into `section.folders`.
                        let _ = s3.send(SidebarInput::FolderRowSelected {
                            account_id: id,
                            index: e + row.index(),
                        });
                    }
                });
                let s4 = sender.input_sender().clone();
                custom_list.connect_row_activated(move |_, row| {
                    let _ = s4.send(SidebarInput::FolderRowActivated {
                        account_id: id,
                        index: row.index(),
                    });
                });
                attach_folder_context_menu(
                    &custom_list,
                    id,
                    custom.iter().map(|f| (*f).clone()).collect(),
                    sender,
                );

                custom_revealer.set_transition_type(gtk::RevealerTransitionType::SlideDown);
                custom_revealer.set_transition_duration(0);
                custom_revealer.set_reveal_child(section.custom_expanded);
                custom_revealer.set_child(Some(&custom_list));

                folders_toggle.add_css_class("flat");
                folders_toggle.add_css_class("folders-toggle");
                let hb = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                hb.add_css_class("folder-row");
                if self.collapsed {
                    hb.set_halign(gtk::Align::Center);
                    hb.append(&gtk::Image::from_icon_name("co.hyprlab.Vireo-folder-symbolic"));
                    folders_toggle.set_tooltip_text(Some("Folders"));
                } else {
                    if self.chevrons_left {
                        // A chevron glyph's ink sits further into its canvas
                        // than a regular icon's, so the folder rows' full
                        // inset would land it visually right of the icons
                        // above — a smaller value keeps the same column.
                        custom_chevron.set_margin_start(2);
                    }
                    hb.append(&custom_chevron);
                    let lbl = gtk::Label::new(Some(&format!("Folders ({})", custom.len())));
                    lbl.set_halign(gtk::Align::Start);
                    lbl.set_hexpand(true);
                    hb.append(&lbl);
                }
                folders_toggle.set_child(Some(&hb));
                let st = sender.input_sender().clone();
                folders_toggle.connect_clicked(move |_| {
                    let _ = st.send(SidebarInput::ToggleCustomFoldersLocal(id));
                });
                // Dropping a folder on the section header moves it to the
                // account's top level ("" — resolved to the namespace root).
                folders_toggle.add_controller(folder_drop_target(id, String::new(), sender));
            }

            // "+ Add Folder" button at the bottom of the list for quick creation.
            let add_btn = gtk::Button::new();
            add_btn.add_css_class("flat");
            add_btn.add_css_class("add-folder-btn");
            let add_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            add_box.add_css_class("folder-row");
            let add_img = gtk::Image::from_icon_name("co.hyprlab.Vireo-list-add-symbolic");
            pin_icon_size(&add_img);
            add_box.append(&add_img);
            if self.collapsed {
                add_box.set_halign(gtk::Align::Center);
                add_btn.set_tooltip_text(Some("Add Folder"));
            } else {
                if self.chevrons_left {
                    add_img.set_margin_start(4);
                }
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
            if !custom.is_empty() {
                wrap.append(&folders_toggle);
                wrap.append(&custom_revealer);
            }
            wrap.append(&add_btn);
            revealer.set_child(Some(&wrap));
            container.append(&revealer);

            self.revealers.insert(id, revealer);
            self.chevrons.insert(id, chevron);
            self.folder_lists.insert(id, list);
            self.custom_folder_lists.insert(id, custom_list);
            self.custom_revealers.insert(id, custom_revealer);
            self.custom_chevrons.insert(id, custom_chevron);
        }

        // Per-account avatar colours (background + readable text).
        let mut css = String::new();
        for s in &sections {
            let text = crate::color::readable_text(&s.color);
            css.push_str(&format!(
                ".acct-color-{0} {{ background-color: {1}; }} \
                 .acct-color-{0} label {{ color: {2}; }} \
                 .acct-tint-{0} {{ color: {1}; }}\n",
                s.account.id, s.color, text
            ));
        }
        self.color_provider.load_from_data(&css);

        // The revealers were built with no transition so the rebuilt content
        // reaches full height in the very first layout pass; hand them their
        // real animation back once that pass is done, for user toggles.
        {
            let revs: Vec<gtk::Revealer> = self
                .revealers
                .values()
                .chain(self.custom_revealers.values())
                .chain(self.tree_row_revealers.values().flatten())
                .cloned()
                .chain(self.unified_revealer.clone())
                .chain(self.unified_folders_revealer.clone())
                .collect();
            gtk::glib::idle_add_local_once(move || {
                for r in &revs {
                    r.set_transition_duration(200);
                }
            });
        }

        // Restore the scroll offset before anything paints: an idle-time
        // restore let one frame render at the top first — a visible flash on
        // every rebuild. The adjustment's `changed` signal fires while the
        // fresh rows are being measured (same layout pass), so pinning the
        // value there means no frame ever shows the wrong offset. The pin
        // holds through the freeze-frame window — layout can keep settling
        // for a few frames — then a timer finalises and disconnects.
        if let (Some(pos), Some(scroller)) = (saved_scroll, scroller) {
            if pos > 0.0 {
                let adj = scroller.vadjustment();
                adj.set_value(pos);
                let handler: std::rc::Rc<std::cell::RefCell<Option<gtk::glib::SignalHandlerId>>> =
                    std::rc::Rc::new(std::cell::RefCell::new(None));
                *handler.borrow_mut() = Some(adj.connect_changed(move |adj| {
                    adj.set_value(pos);
                }));
                let adj = scroller.vadjustment();
                let handler = handler.clone();
                gtk::glib::timeout_add_local_once(
                    std::time::Duration::from_millis(120),
                    move || {
                        adj.set_value(pos);
                        if let Some(id) = handler.borrow_mut().take() {
                            adj.disconnect(id);
                        }
                    },
                );
            }
        }
    }

    /// Re-apply the current selection after a rebuild; on first populate (no
    /// prior selection) default to the unified row if shown, else the first folder.
    /// The "Filtered Folders" section inside All Inboxes: a toggle header
    /// and, under an animated revealer, one row per folder that a filter
    /// rule opted in (account pill, folder name, unread chip). In the rail
    /// the header is a folder-icon button and the rows are account pills.
    fn build_unified_folders(
        &mut self,
        body: &gtk::Box,
        sections: &[SectionData],
        sender: &ComponentSender<Self>,
    ) {
        let chevron = gtk::Image::from_icon_name(if self.unified_folders_expanded {
            "co.hyprlab.Vireo-pan-down-symbolic"
        } else {
            "co.hyprlab.Vireo-pan-end-symbolic"
        });
        let toggle = gtk::Button::new();
        toggle.add_css_class("flat");
        toggle.add_css_class("folders-toggle");
        // Styled like the All Inboxes row above it (full-strength label),
        // not like the account sections' dimmed "Folders" heading.
        toggle.add_css_class("unified-folders-toggle");
        let hb = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        hb.add_css_class("folder-row");
        // The header's unread chip — the section's total — shows only while
        // the section is folded up, as All Inboxes' does.
        let show_chip = self.unified_folders_unread > 0 && !self.unified_folders_expanded;
        if self.collapsed {
            // The rail has no room for a label: Jason's filter-folder glyph
            // (a folder with a funnel's bars) carries the toggle alone.
            let icon = gtk::Image::from_icon_name("co.hyprlab.Vireo-filter-folder-symbolic");
            hb.set_halign(gtk::Align::Center);
            let (overlay, badge) = with_unread_overlay(&icon, self.unified_folders_unread);
            badge.set_visible(show_chip);
            hb.append(&overlay);
            self.unified_folders_badge = Some(badge);
            toggle.set_tooltip_text(Some(&if self.unified_folders_unread > 0 {
                format!("Filtered Folders ({})", self.unified_folders_unread)
            } else {
                "Filtered Folders".to_string()
            }));
        } else {
            // A leading caret and the label, like the accounts' "Folders"
            // heading; the glyph belongs to the rows beneath.
            if self.chevrons_left {
                // Same nudge as that heading: a chevron's ink sits deeper
                // in its canvas than an icon's.
                chevron.set_margin_start(2);
            }
            hb.append(&chevron);
            let lbl = gtk::Label::new(Some("Filtered Folders"));
            lbl.add_css_class("account-name");
            lbl.set_halign(gtk::Align::Start);
            lbl.set_hexpand(true);
            lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
            hb.append(&lbl);
            let badge = gtk::Label::new(Some(&self.unified_folders_unread.to_string()));
            badge.add_css_class("unread-badge");
            badge.set_valign(gtk::Align::Center);
            badge.set_visible(show_chip);
            hb.append(&badge);
            self.unified_folders_badge = Some(badge);
        }
        toggle.set_child(Some(&hb));
        let st = sender.input_sender().clone();
        toggle.connect_clicked(move |_| {
            let _ = st.send(SidebarInput::ToggleUnifiedFoldersExpand);
        });
        body.append(&toggle);
        self.unified_folders_chevron = Some(chevron);

        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.add_css_class("navigation-sidebar");
        // The gap to the first account section below (see the inbox
        // sub-list, which carries it when this section is absent).
        list.set_margin_bottom(14);
        for r in &self.unified_folders {
            let Some(section) = sections.iter().find(|s| s.account.id == r.account_id) else {
                continue;
            };
            let tip = format!("{} \u{2014} {}", r.folder.name, section.account.label);
            // Laid out exactly like a folder under an account's "Folders"
            // heading — same builder, same leaf expander slot — so folders
            // read the same wherever they sit in the sidebar. Only the icon
            // differs: the filter-folder glyph in the account's colour, which
            // is what says whose folder this is.
            let icon = gtk::Image::from_icon_name("co.hyprlab.Vireo-filter-folder-symbolic");
            icon.add_css_class(&format!("acct-tint-{}", section.account.id));
            let lead: Option<gtk::Widget> = if self.collapsed {
                None
            } else {
                let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                spacer.set_width_request(TREE_EXPANDER_WIDTH);
                Some(spacer.upcast())
            };
            let (row, badge) = build_folder_row(
                &r.folder,
                self.collapsed,
                0,
                lead.as_ref(),
                self.chevrons_left,
                Some(icon),
            );
            let Some(badge) = badge else { continue };
            // Name the account too: the tint alone is a hint.
            row.set_tooltip_text(Some(&if self.collapsed && r.folder.unread > 0 {
                format!("{tip} ({})", r.folder.unread)
            } else {
                tip.clone()
            }));
            list.append(&row);
            self.unified_folder_badges.insert((r.account_id, r.folder.id), badge);
        }
        let ss = sender.input_sender().clone();
        list.connect_row_selected(move |_, row| {
            if let Some(row) = row {
                let _ = ss.send(SidebarInput::UnifiedFolderRowSelected(row.index()));
            }
        });
        // Right-click a filtered folder: act on that folder alone.
        let click = gtk::GestureClick::new();
        click.set_button(gtk::gdk::BUTTON_SECONDARY);
        let cs = sender.clone();
        let list_w = list.clone();
        let refs = self.unified_folders.clone();
        click.connect_pressed(move |_, _, x, y| {
            if let Some(r) = list_w
                .row_at_y(y as i32)
                .and_then(|row| refs.get(row.index() as usize))
            {
                show_sidebar_menu(
                    &list_w,
                    x,
                    y,
                    vec![
                        ("Mark as Read", CtxAction::MarkFolderRead {
                            account_id: r.account_id,
                            folder_id: r.folder.id,
                        }),
                        ("Refresh", CtxAction::RefreshFolder {
                            account_id: r.account_id,
                            folder_id: r.folder.id,
                        }),
                    ],
                    &cs,
                );
            }
        });
        list.add_controller(click);

        let revealer = gtk::Revealer::new();
        revealer.set_transition_type(gtk::RevealerTransitionType::SlideDown);
        revealer.set_transition_duration(0);
        revealer.set_reveal_child(self.unified_folders_expanded);
        revealer.set_child(Some(&list));
        body.append(&revealer);
        self.unified_folders_revealer = Some(revealer);
        self.unified_folder_list = Some(list);
    }

    /// Deselect every list except the one owning `keep` (whose own list keeps its
    /// selection). Used when a selection moves between sections.
    fn clear_other_selections(&self, keep: Sel) {
        if keep != Sel::Unified {
            if let Some(l) = &self.unified_list {
                l.unselect_all();
            }
        }
        // One list holds both footer rows; single-selection makes them
        // mutually exclusive, so it only needs clearing when neither is kept.
        if keep != Sel::Attachments && keep != Sel::Contacts {
            if let Some(l) = &self.footer_list {
                l.unselect_all();
            }
        }
        if keep != Sel::Outbox {
            if let Some(l) = &self.outbox_list {
                l.unselect_all();
            }
        }
        if !matches!(keep, Sel::UnifiedInbox(_)) {
            if let Some(l) = &self.unified_inbox_list {
                l.unselect_all();
            }
        }
        if !matches!(keep, Sel::UnifiedFolder(..)) {
            if let Some(l) = &self.unified_folder_list {
                l.unselect_all();
            }
        }
        // A folder selection lives in exactly one of the two lists (essential or
        // custom) of one account; unselect every other list, including the
        // sibling list of the same account.
        let keep_is_custom = if let Sel::Folder(kaid, kpath) = &keep {
            self.folder_kind(*kaid, kpath) == Some(FolderKind::Custom)
        } else {
            false
        };
        for (aid, lb) in &self.folder_lists {
            let keep_here = matches!(&keep, Sel::Folder(kaid, _) if kaid == aid) && !keep_is_custom;
            if !keep_here {
                lb.unselect_all();
            }
        }
        for (aid, lb) in &self.custom_folder_lists {
            let keep_here = matches!(&keep, Sel::Folder(kaid, _) if kaid == aid) && keep_is_custom;
            if !keep_here {
                lb.unselect_all();
            }
        }
    }

    fn select_attachments(&self) {
        if let (Some(list), Some(row)) = (&self.footer_list, &self.attachments_row) {
            list.select_row(Some(row));
        }
    }

    fn select_contacts(&self) {
        if let (Some(list), Some(row)) = (&self.footer_list, &self.contacts_row) {
            list.select_row(Some(row));
        }
    }

    fn select_outbox(&self) {
        if let Some(list) = &self.outbox_list {
            if let Some(row) = list.row_at_index(0) {
                list.select_row(Some(&row));
            }
        }
    }

    fn restore_selection(&mut self) {
        match self.selected.clone() {
            Sel::Unified => self.select_unified(),
            Sel::Attachments => self.select_attachments(),
            Sel::Contacts => self.select_contacts(),
            Sel::Outbox => self.select_outbox(),
            Sel::Folder(acc, path) => self.select_folder(acc, &path),
            Sel::UnifiedInbox(acc) => self.select_unified_inbox(acc),
            Sel::UnifiedFolder(acc, path) => self.select_unified_folder(acc, &path),
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

    fn select_unified_folder(&self, account_id: u32, path: &str) {
        if let Some(list) = &self.unified_folder_list {
            if let Some(idx) = self
                .unified_folders
                .iter()
                .position(|r| r.account_id == account_id && r.folder.path == path)
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
        // Essential folders live in the main list (rows 0..E); custom folders in
        // the collapsible list (rows E..). Route to the right one.
        let e = self.essential_count(account_id);
        let (list, row_idx) = if idx < e {
            (self.folder_lists.get(&account_id), idx)
        } else {
            (self.custom_folder_lists.get(&account_id), idx - e)
        };
        if let Some(list) = list {
            if let Some(row) = list.row_at_index(row_idx as i32) {
                list.select_row(Some(&row));
            }
        }
    }

    /// Number of essential (non-custom) folders for an account — the boundary
    /// between the main and custom folder lists in `section.folders`.
    fn essential_count(&self, account_id: u32) -> usize {
        self.sections
            .iter()
            .find(|s| s.account.id == account_id)
            .map(|s| s.folders.iter().filter(|f| f.kind != FolderKind::Custom).count())
            .unwrap_or(0)
    }

    /// The kind of the folder at `path` in `account_id`, if known.
    fn folder_kind(&self, account_id: u32, path: &str) -> Option<FolderKind> {
        self.sections
            .iter()
            .find(|s| s.account.id == account_id)
            .and_then(|s| s.folders.iter().find(|f| f.path == path))
            .map(|f| f.kind)
    }
}

/// The dragged messages in a drop payload: the marker "vireo-move" followed by
/// one tab-separated (account, folder, uid, id) group per message. A drag from a
/// multi-selection carries every selected message (#23); anything malformed
/// yields nothing rather than a partial move.
fn parse_move_payload(payload: &str) -> Vec<(u32, u32, u32, u32)> {
    let parts: Vec<&str> = payload.split('\t').collect();
    if parts.first() != Some(&"vireo-move") || parts.len() < 5 || parts.len() % 4 != 1 {
        return Vec::new();
    }
    let items: Vec<(u32, u32, u32, u32)> = parts[1..]
        .chunks(4)
        .filter_map(|c| {
            Some((c[0].parse().ok()?, c[1].parse().ok()?, c[2].parse().ok()?, c[3].parse().ok()?))
        })
        .collect();
    // All or nothing: a group that won't parse means the payload isn't ours.
    if items.len() * 4 + 1 == parts.len() {
        items
    } else {
        Vec::new()
    }
}

/// A drop target that moves a dragged message into `dest_path` on account `id`.
fn folder_drop_target(
    id: u32,
    dest_path: String,
    sender: &ComponentSender<Sidebar>,
) -> gtk::DropTarget {
    let drop = gtk::DropTarget::new(gtk::glib::types::Type::STRING, gtk::gdk::DragAction::MOVE);
    let ds = sender.input_sender().clone();
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
    drop
}

/// Wire a right-click menu (Mark as Read / Refresh / Delete) onto a folder list;
/// `folders` maps the list's row indices to their folders.
fn attach_folder_context_menu(
    list: &gtk::ListBox,
    id: u32,
    folders: Vec<Folder>,
    sender: &ComponentSender<Sidebar>,
) {
    let click = gtk::GestureClick::new();
    click.set_button(gtk::gdk::BUTTON_SECONDARY);
    let cs = sender.clone();
    let list_w = list.clone();
    click.connect_pressed(move |_, _, x, y| {
        if let Some(f) = list_w
            .row_at_y(y as i32)
            .and_then(|row| folders.get(row.index() as usize))
        {
            let mut items = vec![
                ("Mark as Read", CtxAction::MarkFolderRead { account_id: id, folder_id: f.id }),
                ("Refresh", CtxAction::RefreshFolder { account_id: id, folder_id: f.id }),
            ];
            // Only user-created folders can be renamed or deleted.
            if f.kind == FolderKind::Custom {
                items.push(("Rename Folder…", CtxAction::RenameFolder {
                    account_id: id,
                    name: f.name.clone(),
                    path: f.path.clone(),
                }));
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

/// Width of the tree-expander slot: chevron button and leaf spacer alike, so
/// folder names at one depth line up whether or not they have children (#51).
const TREE_EXPANDER_WIDTH: i32 = 18;

/// Whether `child` sits under `parent` in the mailbox hierarchy — a strict
/// descendant, with a real delimiter at the boundary (any of the common ones;
/// see folder_depth for why the delimiter itself never reaches the UI).
fn path_is_under(child: &str, parent: &str) -> bool {
    child.len() > parent.len() + 1
        && child.starts_with(parent)
        && matches!(child.as_bytes()[parent.len()], b'/' | b'.' | b'\\')
}

/// Whether a folder row is hidden because some ancestor node is collapsed.
fn hidden_by_collapse(
    path: &str,
    collapsed: &std::collections::HashSet<String>,
) -> bool {
    collapsed.iter().any(|p| path_is_under(path, p))
}

/// Pop up a right-click context menu of `items` anchored at (x, y) in
/// `parent`, styled to GNOME HIG (sized to content, no scrollbar).
fn show_sidebar_menu(
    parent: &impl IsA<gtk::Widget>,
    x: f64,
    y: f64,
    items: Vec<(&str, CtxAction)>,
    sender: &ComponentSender<Sidebar>,
) {
    let entries = items
        .into_iter()
        .map(|(label, action)| {
            let s = sender.clone();
            MenuEntry::new(label, move || {
                let _ = s.output(SidebarOutput::Context(action.clone()));
            })
        })
        .collect();
    show_context_menu(parent, x, y, vec![entries]);
}

/// A row in the "All Inboxes" sub-list: a small account pill, the account name,
/// and that inbox's unread badge. In the compact rail only the pill is shown
/// (centred), with the unread count as a corner chip. Returns the badge for
/// in-place updates.
fn build_unified_inbox_row(
    section: &SectionData,
    inbox: &Folder,
    collapsed: bool,
    inset: bool,
) -> (gtk::ListBoxRow, gtk::Label) {
    // Show the account's configured label (defaults to its email) so accounts are
    // easy to tell apart in the All Inboxes view.
    let label = &section.account.label;

    // Small account pill (colour + initials/emoji), like the header circle.
    let id = section.account.id;
    let circle = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    circle.add_css_class("account-circle-sm");
    circle.add_css_class(&format!("acct-color-{id}"));
    circle.set_valign(gtk::Align::Center);
    circle.set_halign(gtk::Align::Center);
    circle.set_hexpand(false);
    circle.set_size_request(21, 21);
    let glyph = gtk::Label::new(None);
    glyph.set_hexpand(true);
    glyph.set_halign(gtk::Align::Center);
    glyph.set_valign(gtk::Align::Center);
    match &section.emoji {
        Some(em) if !em.is_empty() => {
            glyph.set_text(em);
            glyph.add_css_class("account-emoji");
        }
        _ => glyph.set_text(&account_initials(label, &section.account.email)),
    }
    circle.append(&glyph);

    build_unified_sub_row(&circle, label, label, inbox.unread, collapsed, inset)
}

/// A row nested under "All Inboxes": `lead` (the account's pill), `title`,
/// and an unread badge. In the compact rail only the lead is shown
/// (centred), with the count as a corner chip and `tip` (plus the count) as
/// the tooltip. Returns the badge for in-place updates.
fn build_unified_sub_row(
    lead: &impl IsA<gtk::Widget>,
    title: &str,
    tip: &str,
    unread: u32,
    collapsed: bool,
    inset: bool,
) -> (gtk::ListBoxRow, gtk::Label) {
    let row = gtk::ListBoxRow::new();
    let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    hbox.add_css_class("folder-row");
    if !collapsed {
        hbox.add_css_class("unified-subrow");
    }

    let badge = if collapsed {
        hbox.set_halign(gtk::Align::Center);
        let tip = if unread > 0 {
            format!("{tip} ({unread})")
        } else {
            tip.to_string()
        };
        row.set_tooltip_text(Some(&tip));
        let (overlay, badge) = with_unread_overlay(lead, unread);
        hbox.append(&overlay);
        badge
    } else {
        if inset {
            lead.set_margin_start(ROW_LEFT_INSET - 6);
        }
        hbox.append(lead);
        if tip != title {
            row.set_tooltip_text(Some(tip));
        }

        let name = gtk::Label::new(Some(title));
        name.set_margin_start(6);
        name.set_hexpand(true);
        name.set_halign(gtk::Align::Start);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        hbox.append(&name);

        let badge = gtk::Label::new(Some(&unread.to_string()));
        badge.add_css_class("unread-badge");
        badge.set_valign(gtk::Align::Center);
        badge.set_visible(unread > 0);
        hbox.append(&badge);
        badge
    };

    row.set_child(Some(&hbox));
    (row, badge)
}

/// Pins a symbolic icon to an exact, deterministic 16px box so it centres on
/// the same column as the rail's avatar circles. Left to GTK's default
/// icon-size resolution, a plain `gtk::Image`'s natural size can round a
/// fraction of a pixel off from the circles' hand-pinned, always-even
/// `set_size_request` — `Align::Center` then centres that slightly-off box
/// exactly as asked, reading as the icon column drifting right of the
/// avatar column (PR #95).
fn pin_icon_size(icon: &gtk::Image) {
    icon.set_pixel_size(16);
    icon.set_halign(gtk::Align::Center);
    icon.set_valign(gtk::Align::Center);
}

/// Extra left inset on every expanded row's leading icon in the leading-
/// chevron layout, so rows don't read as shoved flush against the sidebar's
/// edge (PR #95). The classic trailing layout keeps its original geometry.
const ROW_LEFT_INSET: i32 = 8;

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

/// Nesting depth of a custom folder: how many of the *other listed* folders are
/// ancestors of its IMAP path. Working from listed ancestors (rather than
/// counting delimiters) keeps namespace prefixes honest — "INBOX.Clients" is
/// top-level on a Dovecot-style server because "INBOX" isn't in the custom
/// list, while "INBOX.Clients.Acme" is one level down because "INBOX.Clients"
/// is. The delimiter itself never reaches the UI, so any of the common ones is
/// accepted at the boundary.
fn folder_depth(folder: &Folder, all: &[&Folder]) -> usize {
    all.iter()
        .filter(|g| {
            g.id != folder.id
                && folder.path.len() > g.path.len() + 1
                && folder.path.starts_with(&g.path)
                && matches!(folder.path.as_bytes()[g.path.len()], b'/' | b'.' | b'\\')
        })
        .count()
}

/// Build one folder row. `depth` indents sub-folders to mirror the server's
/// hierarchy (0 = top level; only meaningful for custom folders).
fn build_folder_row(
    folder: &Folder,
    collapsed: bool,
    depth: usize,
    lead: Option<&gtk::Widget>,
    inset: bool,
    icon: Option<gtk::Image>,
) -> (gtk::ListBoxRow, Option<gtk::Label>) {
    let row = gtk::ListBoxRow::new();
    let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    hbox.add_css_class("folder-row");
    // Custom folders show only their leaf name; the tooltip carries the whole
    // hierarchy, so nine folders all named "Archive" stay tellable apart (#51).
    if folder.kind == FolderKind::Custom && folder.path.contains(['/', '.', '\\']) {
        let pretty = folder
            .path
            .split(['/', '.', '\\'])
            .map(crate::mutf7::decode)
            .collect::<Vec<_>>()
            .join(" › ");
        row.set_tooltip_text(Some(&pretty));
    }
    if let Some(lead) = lead {
        hbox.append(lead);
    }
    if !collapsed && depth > 0 {
        // Indent nested folders; capped so a pathological hierarchy can't push
        // the name out of the sidebar.
        hbox.set_margin_start(14 * depth.min(4) as i32);
    }

    // The folder kind's icon, unless the caller brought its own (the
    // Filtered Folders rows' account-tinted glyph).
    let img = icon.unwrap_or_else(|| gtk::Image::from_icon_name(folder.kind.icon()));
    img.add_css_class("folder-icon");
    pin_icon_size(&img);

    let badge = if collapsed {
        hbox.set_halign(gtk::Align::Center);
        let tip = if folder.unread > 0 {
            format!("{} ({})", folder.name, folder.unread)
        } else {
            folder.name.clone()
        };
        row.set_tooltip_text(Some(&tip));
        // Every folder carries an unread chip; in the rail it rides the icon's
        // corner so new mail shows without expanding the sidebar.
        let (overlay, badge) = with_unread_overlay(&img, folder.unread);
        hbox.append(&overlay);
        Some(badge)
    } else {
        if inset {
            img.set_margin_start(ROW_LEFT_INSET);
        }
        hbox.append(&img);
        let name = gtk::Label::new(Some(&folder.name));
        name.set_hexpand(true);
        name.set_halign(gtk::Align::Start);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        hbox.append(&name);

        // Every folder shows an unread count chip — present but hidden when
        // zero so it can update in place.
        let badge = gtk::Label::new(Some(&folder.unread.to_string()));
        badge.add_css_class("unread-badge");
        badge.set_valign(gtk::Align::Center);
        badge.set_visible(folder.unread > 0);
        hbox.append(&badge);
        Some(badge)
    };

    row.set_child(Some(&hbox));
    (row, badge)
}

#[cfg(test)]
mod tests {
    use super::folder_depth;
    use super::hidden_by_collapse;
    use super::parse_move_payload;
    use crate::models::{Folder, FolderKind};

    fn custom(id: u32, path: &str) -> Folder {
        Folder {
            id,
            account_id: 1,
            name: path.rsplit(['/', '.']).next().unwrap_or(path).to_string(),
            path: path.to_string(),
            kind: FolderKind::Custom,
            unread: 0,
        }
    }

    #[test]
    fn folder_depth_follows_listed_ancestors() {
        // A Dovecot-style namespace: everything lives under "INBOX.", which is
        // not itself a custom folder — so "INBOX.Clients" is top-level and only
        // real sub-folders are indented.
        let folders = vec![
            custom(1, "INBOX.Clients"),
            custom(2, "INBOX.Clients.Acme"),
            custom(3, "INBOX.Clients.Acme.Invoices"),
            custom(4, "INBOX.Travel"),
        ];
        let refs: Vec<&Folder> = folders.iter().collect();
        assert_eq!(folder_depth(&folders[0], &refs), 0);
        assert_eq!(folder_depth(&folders[1], &refs), 1);
        assert_eq!(folder_depth(&folders[2], &refs), 2);
        assert_eq!(folder_depth(&folders[3], &refs), 0);
    }

    #[test]
    fn folder_depth_needs_a_delimiter_not_just_a_prefix() {
        // "ClientsB" merely shares a prefix with "Clients" — it is a sibling,
        // not a child.
        let folders = vec![custom(1, "Clients"), custom(2, "ClientsB"), custom(3, "Clients/X")];
        let refs: Vec<&Folder> = folders.iter().collect();
        assert_eq!(folder_depth(&folders[1], &refs), 0);
        assert_eq!(folder_depth(&folders[2], &refs), 1);
    }

    #[test]
    fn a_collapsed_node_hides_descendants_and_nothing_else() {
        let mut collapsed = std::collections::HashSet::new();
        collapsed.insert("Clients".to_string());
        // Direct child and grandchild hide; a sibling sharing the prefix does
        // not, and neither does the collapsed node itself.
        assert!(hidden_by_collapse("Clients/Acme", &collapsed));
        assert!(hidden_by_collapse("Clients/Acme/Invoices", &collapsed));
        assert!(!hidden_by_collapse("ClientsB", &collapsed));
        assert!(!hidden_by_collapse("Clients", &collapsed));
        // Dotted hierarchies collapse the same way.
        collapsed.clear();
        collapsed.insert("INBOX.2025".to_string());
        assert!(hidden_by_collapse("INBOX.2025.Archive", &collapsed));
        assert!(!hidden_by_collapse("INBOX.2026.Archive", &collapsed));
    }

    #[test]
    fn a_drop_payload_carries_every_dragged_message() {
        // One message (the single-selection case).
        assert_eq!(parse_move_payload("vireo-move\t1\t2\t3\t4"), vec![(1, 2, 3, 4)]);
        // Three, as a multi-selection drag sends them — including a second
        // account, which the app filters out (mail can't cross accounts).
        assert_eq!(
            parse_move_payload("vireo-move\t1\t2\t3\t4\t1\t2\t5\t6\t7\t8\t9\t10"),
            vec![(1, 2, 3, 4), (1, 2, 5, 6), (7, 8, 9, 10)]
        );
    }

    #[test]
    fn a_payload_that_isnt_ours_moves_nothing() {
        for bad in [
            "",
            "some dragged text",
            "vireo-move",
            "vireo-move\t1\t2\t3",       // short group
            "vireo-move\t1\t2\t3\t4\t5",  // trailing partial group
            "vireo-move\t1\t2\tx\t4",     // unparsable field
        ] {
            assert!(parse_move_payload(bad).is_empty(), "{bad:?} should parse to nothing");
        }
    }
}
