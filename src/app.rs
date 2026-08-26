//! Root component: the application window, three-pane adaptive layout, and the
//! routing between the sidebar, list, reader, and the per-account mail workers.

use std::collections::{HashMap, HashSet};

use adw::prelude::*;
use relm4::actions::{AccelsPlus, RelmAction, RelmActionGroup};
use relm4::prelude::*;
use tokio::sync::mpsc::UnboundedSender;

/// Contributors whose work is in the app, shown in the About window's "Thanks"
/// list: display name, GitHub handle, and what they contributed.
const CONTRIBUTORS: &[(&str, &str, &str)] = &[
    (
        "Alfonso Lizárraga",
        "alfonsolzrg",
        "Sending to named recipients, startup message list, unread dot",
    ),
    (
        "Chris Pouliot",
        "chrispouliot",
        "Proton Bridge connections (STARTTLS, local certificates)",
    ),
    (
        "Isaac",
        "thecalamityjoe87",
        "PDF thumbnails, the attachment-opening fix, the reader's To line, remote-content option",
    ),
    (
        "Alexander Lubovenko",
        "typedev",
        "Gmail conversations: one message per thread, one fetch for its bodies",
    ),
    (
        "Anton Palgunov",
        "Toxblh",
        "Contact photos as sender avatars; Online Accounts custom ports, Mail-toggle pausing",
    ),
];

/// Width of the collapsed, icon-only sidebar rail.
/// The message list's width when the window opens: the room a row needs for its
/// Actions Palette, padding and unread dot.
const LIST_PALETTE_WIDTH: i32 = 350;

/// The narrowest the reader pane may be squeezed. Its header's own row of
/// actions (~490px) is the real floor — this request sits just under it so the
/// header, not an arbitrary figure, decides. Kept modest on purpose: the
/// window's total minimum width must stay under half of a 1920px screen, or
/// GNOME refuses to tile the window to the left/right screen edge (it only
/// offers the top-edge maximize).
const READER_MIN_WIDTH: i32 = 480;

const SIDEBAR_RAIL_WIDTH: f64 = 80.0;

/// Selection size at/above which a bulk archive/delete/spam shows a spinner and
/// applies deferred (smaller batches are fast enough to run inline).
const BULK_SPINNER_MIN: usize = 25;

relm4::new_action_group!(WindowActionGroup, "win");
relm4::new_stateless_action!(AccountsAction, WindowActionGroup, "accounts");
relm4::new_stateless_action!(PreferencesAction, WindowActionGroup, "preferences");
relm4::new_stateless_action!(AboutAction, WindowActionGroup, "about");
relm4::new_stateless_action!(ShortcutsAction, WindowActionGroup, "shortcuts");
relm4::new_stateless_action!(PrintAction, WindowActionGroup, "print");
relm4::new_stateless_action!(PrintPreviewAction, WindowActionGroup, "print-preview");

use crate::config::{self, AccountConfig};
use crate::models::{Account, Attachment, Folder, FolderKind, Message};
use crate::ui::accounts::{AccountsOutput, AccountsWindow};
use crate::ui::compose::{
    Compose, ComposeAccount, ComposeInit, ComposeInput, ComposeOutput, ComposePrefill,
};
use crate::ui::message_list::{
    BulkAction, MessageList, MessageListInput, MessageListOutput, RowAction,
};
use crate::ui::attachments_gallery::{
    AttachmentsGallery, GalleryInput, GalleryOutput,
};
use crate::ui::attachment_drawer::{AttachmentDrawer, AttachmentDrawerInput};
use crate::ui::message_view::{MessageView, MessageViewInput, MessageViewOutput};
use crate::ui::message_window::{
    MessageWindow, MessageWindowInit, MessageWindowInput, MessageWindowOutput,
};
use crate::ui::notifications::{NotificationCenter, NotifyInput, NotifyOutput};
use crate::ui::preferences::{PrefInit, PrefOutput, Preferences};
use crate::ui::sidebar::{
    CtxAction, SectionData, Sidebar, SidebarInit, SidebarInput, SidebarOutput,
};
use crate::worker::{self, MailRequest, OutgoingMessage, WorkerEvent};

/// The currently selected mailbox.
#[derive(Clone)]
struct SelectedFolder {
    account_id: u32,
    folder_id: u32,
    name: String,
    path: String,
}

/// A standalone compose window (New Message, compose-to, edit-draft, or a
/// popped-out reply) and its component. Both refs must stay alive: the window
/// holds the content, the controller holds the component root.
struct ComposeHost {
    id: u32,
    /// Held only to keep the component (and its widget tree) alive for the
    /// window's lifetime; dropped when the host is removed.
    #[allow(dead_code)]
    controller: Controller<Compose>,
    window: adw::Window,
}

/// The reader's inline reply/forward composer. `window` is `Some` only while the
/// pane has been promoted to a floating window (else it lives in the reader's
/// drop-down revealer).
struct ReaderCompose {
    id: u32,
    controller: Controller<Compose>,
    window: Option<adw::Window>,
}

pub struct AppModel {
    /// One mail worker per account (account_id → request sender).
    workers: HashMap<u32, UnboundedSender<MailRequest>>,
    config: Vec<AccountConfig>,
    window: adw::ApplicationWindow,
    prefs: Option<Controller<Preferences>>,
    accounts_win: Option<Controller<AccountsWindow>>,
    /// Standalone compose windows (multiple allowed at once). Pruned as they close.
    composers: Vec<ComposeHost>,
    /// The reader's inline reply/forward composer, if open.
    reader_compose: Option<ReaderCompose>,
    /// Superseded inline composers still finishing a save-if-dirty before closing.
    draining_composers: Vec<(u32, Controller<Compose>)>,
    /// SlideDown revealer under the reader toolbar that hosts the inline pane.
    reader_compose_revealer: gtk::Revealer,
    /// Monotonic id source for composers.
    next_compose_id: u32,
    menu: gtk::gio::Menu,
    /// All known accounts, ordered by id.
    accounts: Vec<Account>,
    /// account_id → that account's folders.
    folders: HashMap<u32, Vec<Folder>>,
    /// Preferred sidebar account order (by email).
    account_order: Vec<String>,
    /// Accounts whose folder list is collapsed in the sidebar (by email).
    collapsed: Vec<String>,
    /// Accounts whose custom-folders section is expanded in the sidebar (by email).
    folders_expanded: Vec<String>,
    selected: Option<SelectedFolder>,
    /// Attachments of the currently-open message (for the reader toolbar button).
    attachments: Vec<Attachment>,
    /// True while the current message's attachments are downloading.
    attachments_loading: bool,
    /// The current message has attachments that aren't downloaded yet; offer a
    /// "Load attachments" button instead of fetching automatically.
    attachments_available: bool,
    /// Cache of fetched attachments, keyed by (account_id, message_id), so
    /// revisiting a message doesn't re-download them.
    attachment_cache: HashMap<(u32, u32), Vec<Attachment>>,
    /// Popover content box for the attachments button.
    attach_list: gtk::Box,
    /// True when the unified "All Inboxes" view is active (no single folder).
    unified: bool,
    /// account_id → that account's latest inbox messages (for the unified view).
    unified_by_account: HashMap<u32, Vec<Message>>,
    /// (account_id, folder_id) → last-seen message list, shown instantly on
    /// revisit while a fresh sync runs in the background.
    message_cache: HashMap<(u32, u32), Vec<Message>>,
    /// (account_id, folder_id) whose background backfill has fully finished, so the
    /// message list knows no more rows will stream in for them.
    indexed_folders: HashSet<(u32, u32)>,
    /// (account_id, message_id) → fetched body, so reopening a message renders
    /// instantly with no loading spinner.
    body_cache: HashMap<(u32, u32), String>,
    /// Sender-authentication verdicts, keyed like `body_cache`. Prefetch delivers
    /// these well before a message is opened, and opening one renders from the
    /// in-memory body cache without a worker round-trip — so the verdict has to
    /// be held here or it would be lost by the time the reader needs it.
    sender_cache: HashMap<(u32, u32), Box<crate::models::SenderCheck>>,
    /// (account_id, folder_id) → server-side unread count, accurate beyond the
    /// loaded window (from IMAP STATUS/SEARCH). Drives the sidebar badges.
    folder_unread: HashMap<(u32, u32), u32>,
    /// The account-list split view, narrowed to icon-only width when collapsed.
    sidebar_split: Option<adw::OverlaySplitView>,
    /// The "Vireo" title label, hidden while the sidebar is collapsed.
    app_title: Option<gtk::Label>,
    /// Sidebar header. In the icon-only rail its window-control buttons are
    /// hidden so the header stops forcing a minimum width wider than the rail.
    sidebar_header: Option<adw::HeaderBar>,
    /// The main-menu button, which moves into the header's centre (the title
    /// slot) while the sidebar is a rail.
    sidebar_menu: Option<gtk::MenuButton>,
    /// Whether the sidebar is in icon-only (collapsed) mode.
    sidebar_collapsed: bool,
    /// The narrow-window breakpoint is currently applied (window is too narrow
    /// for the expanded sidebar + a full-width Actions Palette — e.g. tiled to
    /// half of a 1920px screen).
    auto_rail: bool,
    /// The rail's current on-screen state — the user's choice OR'd with the
    /// breakpoint, tracked so apply/unapply only animate real changes.
    rail_active: bool,
    /// While narrow: the sidebar is temporarily expanded as an overlay floating
    /// above the panes (so the list and reader keep their widths).
    sidebar_peek: bool,
    /// Held so the in-flight collapse/expand width animation isn't dropped.
    sidebar_anim: Option<adw::TimedAnimation>,
    current: Option<Message>,
    /// Sender addresses allowed to auto-load remote content (lowercased).
    allowed_senders: Vec<String>,
    /// Whether remote content is auto-loaded for every new message.
    auto_remote_content: bool,
    /// Draw the blocked-remote-content banner in its quiet grey style.
    dim_remote_banner: bool,
    /// Whether that banner is shown at all. Hiding it changes nothing about what
    /// is blocked — only whether the reader says so.
    show_remote_banner: bool,
    /// Addresses/domains whose incoming inbox mail is auto-deleted (lowercased).
    blacklist: Vec<String>,
    /// Seconds the message-list Actions Palette stays open after the cursor leaves.
    palette_collapse_secs: u64,
    /// Whether to load sender avatars from Gravatar.
    gravatar: bool,
    /// Whether the coloured sender circles are drawn at all (#29).
    avatars: bool,
    /// Whether a sender's site icon may fill their circle (#30).
    sender_logos: bool,
    /// How dates are written, and on what clock (#32).
    date_style: crate::config::DateStyle,
    clock_style: crate::config::ClockStyle,
    /// Seconds between automatic mail checks (0 = manual only).
    fetch_interval_secs: u64,
    /// Whether IMAP IDLE push is enabled.
    push: bool,
    /// Whether desktop notifications (new mail, error alerts) are posted.
    notifications_enabled: bool,
    /// Whether new-mail notifications may name the sender and subject.
    notification_content: bool,
    /// Whether the sidebar shows the "Attachments" row.
    show_attachments: bool,
    /// Lines of preview text per message-list row (1–3).
    preview_lines: u32,
    /// The keyboard-shortcut reference, while it is open — so the shortcut that
    /// opens it closes it again.
    shortcuts_win: Option<adw::Window>,
    /// Whether Vireo keeps running with no window. Shared with the window's
    /// close handler, which has to read it without the model.
    run_in_background: std::rc::Rc<std::cell::Cell<bool>>,
    /// Whether Vireo starts at login (background running only).
    autostart: bool,
    /// Whether single-key (modifier-free) shortcuts are enabled. The window's key
    /// controller needs to read this without the model, so it is shared: with the
    /// feature off, keystrokes must pass straight through rather than be consumed
    /// and dropped.
    single_key: std::rc::Rc<std::cell::Cell<bool>>,
    /// Whether messages are grouped into conversation threads.
    threading: bool,
    /// A conversation re-render is already scheduled, so further bodies arriving
    /// in the same burst don't each queue one of their own.
    thread_render_queued: bool,
    /// When the conversation on screen was opened. Its bodies arrive one at a
    /// time, and painting each arrival meant a stack of half-built renders
    /// flashing past; the reader holds its spinner until they are all in, or
    /// until this has been waiting [`THREAD_BODY_WAIT`].
    thread_opened_at: Option<std::time::Instant>,
    /// A cross-folder conversation lookup is still outstanding. Its answer can
    /// add messages (and bodies) to what is on screen, so the reader waits for
    /// it rather than painting a conversation it is about to paint again.
    thread_related_pending: bool,
    /// Whether the conversation on screen has had its one paint yet. Until it
    /// has, the reader shows its spinner — opening a thread is a wait, however
    /// short, and showing the previous message meanwhile reads as a glitch.
    thread_painted: bool,
    /// Conversations already assembled, keyed by the message that opens them.
    /// Returning to a thread paints from here rather than re-running the
    /// cross-folder lookup and re-gathering bodies: the wait belongs to the
    /// first open, not to every one. Bounded — a conversation holds its
    /// messages' bodies, which are not small.
    thread_cache: HashMap<(u32, u32), Vec<Message>>,
    /// Insertion order for `thread_cache`, oldest first.
    thread_cache_order: Vec<(u32, u32)>,
    /// Which conversation `current_thread` is, for storing it back.
    thread_key: Option<(u32, u32)>,
    /// Whether conversation threads start expanded in the message list.
    threads_expanded: bool,
    /// How email content is themed (message content only, not the app UI).
    message_theme: config::MessageTheme,
    /// The repeating auto-fetch timer, if armed.
    auto_fetch_source: Option<gtk::glib::SourceId>,
    notifications: Controller<NotificationCenter>,
    notify_count: usize,
    /// Accounts currently performing network activity (drives the spinner).
    busy: HashSet<u32>,
    sidebar: Controller<Sidebar>,
    message_list: Controller<MessageList>,
    message_view: Controller<MessageView>,
    /// In-message attachment thumbnail drawer, docked below the reader body.
    attachment_drawer: Controller<AttachmentDrawer>,
    gallery: Controller<AttachmentsGallery>,
    /// True when the attachments gallery replaces the mail panes.
    showing_gallery: bool,
    /// Whether the Outbox is showing instead of the mail panes.
    showing_outbox: bool,
    /// Messages waiting to be sent, per account, as last reported by its worker.
    outbox_by_account: HashMap<u32, Vec<crate::models::OutboxItem>>,
    /// Gallery items per account inbox, merged for display.
    gallery_by_account: HashMap<u32, Vec<crate::models::GalleryItem>>,
    /// Messages popped out into their own windows, keyed by (account, message).
    popouts: HashMap<(u32, u32), PopOut>,
    /// The conversation currently shown in the reader (newest first), with bodies
    /// filled in as they arrive. More than one entry = conversation/thread mode.
    current_thread: Vec<Message>,
    /// Every message currently selected (list rows or reader cards), newest
    /// report wins. Lets the toolbar's Delete act on the whole multi-selection.
    list_selection: Vec<(u32, u32)>,
    /// A draft awaiting its body before opening in the compose editor.
    pending_draft: Option<Message>,
    /// Outstanding bulk MoveMessages requests awaiting a worker `BulkComplete`.
    /// While > 0 (and a large selection triggered it) the list shows a spinner.
    bulk_pending: usize,
    /// A large bulk archive/delete/spam deferred one tick so its spinner paints
    /// before the (blocking) apply runs.
    pending_bulk: Option<(BulkAction, Vec<Message>)>,
    /// Messages awaiting a deferred permanent delete (same spinner-first trick as
    /// `pending_bulk`).
    pending_purge: Option<Vec<Message>>,
    /// Ids handed to conversation messages pulled in from another folder,
    /// allocated downwards from the top of the range. A message's id is its UID,
    /// which is only unique within its own folder — and the reader keys bodies by
    /// (account, id), so a Sent reply sharing a UID with the Inbox message it
    /// answers would otherwise be handed the wrong body.
    related_id_seq: u32,
    /// The id issued to each such message, keyed by where it really lives
    /// (account, folder, uid). Stable across re-opens so its body stays cached.
    related_ids: HashMap<(u32, u32, u32), u32>,
}

/// A message displayed in its own top-level window (double-click to pop out).
struct PopOut {
    window: adw::Window,
    controller: Controller<MessageWindow>,
}

#[derive(Debug)]
pub enum AppMsg {
    // User actions
    UnifiedSelected,
    /// Show the attachments gallery (sidebar "Attachments" row).
    ShowAttachments,
    /// Show the Outbox (queued, unsent messages).
    ShowOutbox,
    /// A worker reported its queue: replaces that account's entries.
    OutboxItems { account_id: u32, items: Vec<crate::models::OutboxItem> },
    /// A worker reported something noteworthy that isn't a failure.
    Notice(String),
    /// Edit the queued message currently open in the reader.
    EditCurrentOutbox,
    /// Try to send the queued message currently open in the reader.
    SendCurrentOutbox,
    /// Try to send everything waiting, across accounts.
    RetryAllOutbox,
    /// Cached gallery attachments for an account inbox arrived.
    GalleryItems { account_id: u32, items: Vec<crate::models::GalleryItem> },
    /// Gallery "Go to Message" — open the attachment's source message.
    OpenAttachmentMessage { account_id: u32, folder_path: String, uid: u32 },
    FolderSelected { account_id: u32, folder_id: u32, name: String, path: String },
    ToggleCollapse(u32),
    ToggleCustomFolders(u32),
    SidebarCollapsed(bool),
    /// The narrow-window breakpoint applied/unapplied — collapse the sidebar to
    /// its icon rail (and restore it) without touching the user's preference.
    AutoRail(bool),
    /// The floating sidebar overlay was dismissed from outside (a click on the
    /// dimmed content, an Escape) rather than via the sidebar's own button.
    SidebarPeekDismissed,
    SidebarContext(CtxAction),
    /// A folder's threading references were repaired — re-read it from the cache
    /// so what is on screen groups with what was found.
    RefsRepaired { account_id: u32, folder_id: u32 },
    /// The list's selection changed; the reader outlines the matching cards.
    SelectionKeys(Vec<(u32, u32)>),
    /// The reader's selection changed — mirror what the list can represent.
    SelectCards(Vec<(u32, u32)>),
    /// A conversation message was scrolled all the way through — mark it read on
    /// the server, in the caches and in the list.
    ThreadMessageSeen { account_id: u32, id: u32 },
    /// The conversation on screen has new bodies to show (coalesced).
    RenderThread,
    /// Messages were dropped on a sidebar folder — move them there. `items` is
    /// the dragged selection as (account, folder, uid, id).
    DropMoveMessages { dest_account: u32, dest: String, items: Vec<(u32, u32, u32, u32)> },
    /// Create a custom folder under an account (from the right-click menu).
    CreateFolder { account_id: u32, name: String },
    /// Delete a custom folder (its contents are moved to Trash first).
    DeleteFolder { account_id: u32, path: String },
    AccountsReordered(Vec<String>),
    /// `solo` marks a reply the user picked out of a conversation on screen:
    /// show that message alone and don't go looking for its siblings.
    MessageSelected { message: Message, thread: Vec<Message>, solo: bool },
    /// A new-mail desktop notification was clicked — open that message.
    OpenMessageFromNotification { account_id: u32, folder_id: u32, message_id: u32 },
    /// The search field became active/inactive — supply or drop the cross-folder
    /// search pool (every folder's messages, so search can span the mailbox).
    SearchActive(bool),
    /// The message list has no selection to show (e.g. the last message was
    /// removed), so the reader should clear.
    ClearReader,
    /// Double-click: open the message in its own standalone window.
    OpenMessageWindow(Message),
    /// A popped-out message window was closed (remove it from the map).
    PopoutClosed((u32, u32)),
    /// Add a contact from a popout window's sender.
    AddContactFrom { name: String, email: String },
    /// Download a specific message's attachments (from a popout window).
    LoadAttachmentsFor(Box<Message>),
    /// A message's sender-authentication verdict arrived with its body.
    SenderChecked {
        account_id: u32,
        message_id: u32,
        check: Box<crate::models::SenderCheck>,
    },
    /// Open a single attachment delivered from a popout window.
    OpenAttachmentItem(Box<Attachment>),
    /// Save attachments delivered from a popout window.
    SaveAttachmentItems(Vec<Attachment>),
    ToggleStar,
    Archive,
    Delete,
    /// Erase messages for good (confirmed "Delete permanently" in Trash).
    PurgeMessages(Vec<Message>),
    /// The rest of an open message's conversation, found in other folders.
    Related { account_id: u32, message_id: u32, messages: Vec<Message> },
    RowAction { action: RowAction, message: Box<Message> },
    /// A bulk action applied to every selected message.
    Bulk { action: BulkAction, messages: Vec<Message> },
    /// Apply the deferred large bulk action (runs after its spinner has painted).
    BulkApply,
    /// A worker finished one bulk MoveMessages request; clears the spinner once
    /// all outstanding bulk moves are done.
    BulkComplete,
    Compose,
    OpenAbout,
    AllowSender(String),
    AddSender(String),
    RemoveSender(String),
    AddBlacklist(String),
    RemoveBlacklist(String),
    MarkSpam,
    SetAutoRemoteContent(bool),
    SetDimRemoteBanner(bool),
    SetShowRemoteBanner(bool),
    SetGravatar(bool),
    /// The GNOME Contacts photo index changed (EDS sync, or the first load
    /// finished) — refresh the sender circles that are on screen.
    ContactPhotosChanged,
    SetAvatars(bool),
    SetSenderLogos(bool),
    SetDateStyle(crate::config::DateStyle),
    SetClockStyle(crate::config::ClockStyle),
    SetThreading(bool),
    SetThreadsExpanded(bool),
    SetFetchInterval(u64),
    SetPush(bool),
    SetNotifications(bool),
    SetNotificationContent(bool),
    SetShowAttachments(bool),
    SetPreviewLines(u32),
    SetSingleKey(bool),
    SetRunInBackground(bool),
    SetAutostart(bool),
    /// A single-key shortcut fired.
    Shortcut(Shortcut),
    /// Show the keyboard-shortcut reference.
    ShowShortcuts,
    /// Print the message in the reader (issue #16).
    PrintMessage,
    /// Render it to a PDF and open that, to see what will come out.
    PrintPreview,
    SetPaletteCollapse(u64),
    SetMessageTheme(config::MessageTheme),
    ComposeTo(String),
    Reply,
    ReplyAll,
    Forward,
    AddToContacts,
    ContactAdded(Result<crate::contacts::AddOutcome, String>),
    ViewSource,
    OpenAttachment(usize),
    SaveAllAttachments,
    /// User clicked "Load attachments" for a message whose attachments weren't
    /// pre-downloaded — fetch them from the server now.
    LoadAttachmentsNow,
    SendMessage(Box<OutgoingMessage>),
    SaveDraftMessage(Box<OutgoingMessage>),
    DraftSaved,
    /// A composer (id) finished — tear down its host (window or inline revealer).
    ComposeClosed(u32),
    /// Promote/demote the reader's inline composer (id) between inline and window.
    ComposeToggleWindow(u32),
    Refresh,
    OpenAccounts,
    /// Open the accounts window straight to the "add account" form (empty state).
    AddFirstAccount,
    AccountSaved { original_email: Option<String>, account: Box<AccountConfig> },
    /// Show the keyring / Secret Service setup help. `problem: true` when a save
    /// actually failed to persist; `false` for the proactive one-time tip.
    ShowKeyringHelp { problem: bool },
    AccountRemoved { email: String },
    AccountEnabledChanged { email: String, enabled: bool },
    ImportGoaAccount(Box<AccountConfig>),
    /// GNOME Online Accounts changed on the session bus. Carries the fresh live
    /// state, already fetched (debounced) on the watcher thread so the GTK main
    /// thread never does D-Bus I/O — re-reconcile against it.
    GoaChanged(crate::goa::GoaLiveState),
    /// The system resumed from sleep — worker IMAP sockets are stale, so
    /// reconnect every account and reload the visible folder.
    SystemResumed,
    CloseAccounts,
    OpenPreferences,
    ClosePreferences,
    // Worker events (each carries the account it came from)
    SetAccount(Account),
    SetFolders { account_id: u32, folders: Vec<Folder> },
    Messages { account_id: u32, folder_id: u32, messages: Vec<Message> },
    /// Additional indexed summaries from the background backfill (search index).
    MessagesAppend { account_id: u32, folder_id: u32, messages: Vec<Message> },
    /// A folder's background backfill finished — it's fully indexed now.
    BackfillDone { account_id: u32, folder_id: u32 },
    FolderUnread { account_id: u32, folder_id: u32, unread: u32 },
    /// `path` is the folder the body was read from — a UID only identifies a
    /// message within its own folder, so applying a body to a message means
    /// checking the folder too.
    Body { account_id: u32, message_id: u32, path: String, body: String },
    Source { text: String },
    Attachments { account_id: u32, message_id: u32, items: Vec<Attachment> },
    AttachmentsPending { account_id: u32, message_id: u32 },
    /// A flagged message turned out to have no real attachments — drop its paperclip.
    NoAttachments { account_id: u32, message_id: u32 },
    /// An unflagged message turned out to carry attachments — give it one.
    HasAttachments { account_id: u32, message_id: u32 },
    Sent { account_id: u32 },
    Status { account_id: u32, text: String },
    Error { account_id: u32, text: String, connectivity: bool },
    NotifyCount(usize),
    ToggleNotifications,
    OpenContacts,
}

#[relm4::component(pub)]
impl SimpleComponent for AppModel {
    type Init = ();
    type Input = AppMsg;
    type Output = ();

    view! {
        adw::ApplicationWindow {
            set_title: Some("Vireo"),
            set_icon_name: Some(crate::APP_ID),
            add_css_class: "vireo",

            // Persist the window size + maximized state on close. (Position and
            // which monitor can't be restored on Wayland — the compositor owns
            // placement — so only the geometry is saved.)
            connect_close_request[background = model.run_in_background.clone()] => move |w| {
                let maximized = w.is_maximized();
                let (width, height) = if maximized {
                    let (sw, sh, _) = crate::config::load_window_state();
                    (sw, sh)
                } else {
                    (w.width(), w.height())
                };
                crate::config::save_window_state(width, height, maximized);
                // Running in the background: hide the window and stay alive, so
                // mail keeps arriving and GNOME lists Vireo under Background Apps
                // (issue #3). The window is kept rather than rebuilt, so reopening
                // is instant and nothing is torn down — which is also what makes
                // this safe, given the exit below.
                if background.get() {
                    w.set_visible(false);
                    return gtk::glib::Propagation::Stop;
                }
                // Exit cleanly the moment window state is saved. Letting GTK,
                // WebKit and the per-account worker threads tear down the normal
                // way can abort — a Rust panic fired from a GObject dispose
                // callback becomes SIGABRT, which the Flatpak surfaces as a crash
                // notification. Nothing else needs persisting on quit: accounts
                // and settings are written as they change.
                std::process::exit(0)
            },

            #[wrap(Some)]
            set_content = &gtk::Box {
                set_orientation: gtk::Orientation::Vertical,

                append: model.notifications.widget(),

                #[name = "sidebar_split"]
                adw::OverlaySplitView {
                    set_vexpand: true,
                    set_max_sidebar_width: 280.0,
                    // Low enough (with the panes' own minimums) that the whole
                    // window fits half of a 1920px screen — see READER_MIN_WIDTH.
                    set_min_sidebar_width: 180.0,
                    set_sidebar_width_fraction: 0.2,

                    #[wrap(Some)]
                    set_sidebar = &adw::ToolbarView {
                        #[name = "sidebar_header"]
                        add_top_bar = &adw::HeaderBar {
                            add_css_class: "flat",
                            #[wrap(Some)]
                            #[name = "app_title"]
                            set_title_widget = &gtk::Label {
                                set_label: "Vireo",
                                add_css_class: "app-title",
                            },
                            #[name = "sidebar_menu"]
                            pack_end = &gtk::MenuButton {
                                set_icon_name: "co.hyprlab.Vireo-open-menu-symbolic",
                                set_tooltip_text: Some("Main Menu"),
                                add_css_class: "flat",
                                set_menu_model: Some(&model.menu),
                            },
                        },
                        #[wrap(Some)]
                        set_content = model.sidebar.widget(),
                    },

                    #[wrap(Some)]
                    #[name = "content_stack"]
                    set_content = &gtk::Stack {
                        set_transition_type: gtk::StackTransitionType::Crossfade,
                        // Swap the mail panes for the attachments gallery.
                        #[watch]
                        set_visible_child_name: if model.showing_gallery { "gallery" } else { "mail" },

                    add_named[Some("mail")] = &gtk::Paned {
                        set_orientation: gtk::Orientation::Horizontal,
                        // Thin handle so the panes sit flush (just a 1px divider),
                        // no wide-handle gap between them.
                        set_wide_handle: false,
                        // Launch wide enough for a row's Actions Palette. That is
                        // also the list's minimum while the sender circles are on,
                        // so `shrink_start_child: false` clamps to the same figure
                        // either way. With the circles off the minimum drops to what
                        // the sender and subject need (#29) — a fine width to be
                        // able to drag down to, but a poor one to open at.
                        set_position: LIST_PALETTE_WIDTH,
                        // The list keeps its width as the window resizes (the
                        // reader absorbs the change), and can't be dragged narrower
                        // than its own minimum. The reader has a floor of its own:
                        // its header carries a full row of actions, and squeezing it
                        // pushed them off the right-hand edge of the window.
                        set_resize_start_child: false,
                        set_shrink_start_child: false,
                        set_resize_end_child: true,
                        set_shrink_end_child: false,

                        #[wrap(Some)]
                        set_start_child = &adw::ToolbarView {
                            add_top_bar = &adw::HeaderBar {
                                add_css_class: "flat",
                                // Middle pane: no window controls (the reader pane's
                                // header carries the window's close button).
                                set_show_start_title_buttons: false,
                                set_show_end_title_buttons: false,
                                #[wrap(Some)]
                                set_title_widget = &gtk::Label {
                                    #[watch]
                                    set_label: model.pane_title(),
                                    // Gives way as the pane narrows instead of
                                    // holding the whole window wider — a long
                                    // folder name was part of what kept the
                                    // window too wide to tile to a screen edge.
                                    set_ellipsize: gtk::pango::EllipsizeMode::End,
                                    add_css_class: "pane-title",
                                },
                                // Compose leads this header: it is the window's
                                // primary action and sits directly above the list
                                // of messages it adds to.
                                pack_start = &gtk::Button {
                                    set_icon_name: "co.hyprlab.Vireo-mail-message-new-symbolic",
                                    set_tooltip_text: Some("Compose"),
                                    add_css_class: "suggested-action",
                                    connect_clicked[sender] => move |_| sender.input(AppMsg::Compose),
                                },
                                pack_start = &gtk::Button {
                                    set_tooltip_text: Some("Status Bar"),
                                    add_css_class: "flat",
                                    connect_clicked[sender] => move |_| sender.input(AppMsg::ToggleNotifications),
                                    gtk::Box {
                                        set_spacing: 5,
                                        gtk::Image {
                                            #[watch]
                                            set_icon_name: Some(if model.notify_count > 0 {
                                                "co.hyprlab.Vireo-dialog-warning-symbolic"
                                            } else {
                                                "co.hyprlab.Vireo-preferences-system-notifications-symbolic"
                                            }),
                                            #[watch]
                                            set_css_classes: if model.notify_count > 0 {
                                                &["attention-icon"] as &[&str]
                                            } else {
                                                &[] as &[&str]
                                            },
                                        },
                                        gtk::Label {
                                            #[watch]
                                            set_visible: model.notify_count > 0,
                                            #[watch]
                                            set_label: &model.notify_count.to_string(),
                                            add_css_class: "needs-attention",
                                        },
                                    },
                                },
                                pack_start = &gtk::Button {
                                    set_icon_name: "co.hyprlab.Vireo-x-office-address-book-symbolic",
                                    set_tooltip_text: Some("Open Contacts"),
                                    add_css_class: "flat",
                                    connect_clicked[sender] => move |_| sender.input(AppMsg::OpenContacts),
                                },
                                pack_end = &gtk::Button {
                                    set_tooltip_text: Some("Refresh"),
                                    add_css_class: "flat",
                                    connect_clicked[sender] => move |_| sender.input(AppMsg::Refresh),
                                    gtk::Stack {
                                        set_transition_type: gtk::StackTransitionType::Crossfade,
                                        add_named[Some("icon")] = &gtk::Image {
                                            set_icon_name: Some("co.hyprlab.Vireo-view-refresh-symbolic"),
                                        },
                                        add_named[Some("spinner")] = &gtk::Spinner {
                                            #[watch]
                                            set_spinning: !model.busy.is_empty(),
                                        },
                                        #[watch]
                                        set_visible_child_name: if model.busy.is_empty() { "icon" } else { "spinner" },
                                    },
                                },
                            },
                            #[wrap(Some)]
                            set_content = model.message_list.widget(),
                        },

                        #[wrap(Some)]
                        set_end_child = &adw::ToolbarView {
                            // The narrowest the reader may become: enough for every
                            // action in its header at once.
                            set_size_request: (READER_MIN_WIDTH, -1),
                            add_top_bar = &adw::HeaderBar {
                                add_css_class: "flat",
                                // Empty title so the window's "Vireo" title isn't
                                // shown here; the app title lives above the sidebar.
                                #[wrap(Some)]
                                set_title_widget = &gtk::Label {
                                    set_label: "",
                                },
                                // Outbox actions, in place of Reply/Forward/Flag —
                                // a message that hasn't been sent can't be replied
                                // to, and the questions worth asking about it are
                                // whether to edit, send or bin it.
                                pack_start = &gtk::Button {
                                    set_icon_name: "co.hyprlab.Vireo-document-edit-symbolic",
                                    set_tooltip_text: Some("Edit this message"),
                                    add_css_class: "flat",
                                    #[watch]
                                    set_visible: model.showing_outbox,
                                    #[watch]
                                    set_sensitive: model.current.is_some(),
                                    connect_clicked[sender] => move |_| sender.input(AppMsg::EditCurrentOutbox),
                                },
                                pack_start = &gtk::Button {
                                    set_icon_name: "co.hyprlab.Vireo-mail-send-symbolic",
                                    set_tooltip_text: Some("Try to send this message now"),
                                    add_css_class: "flat",
                                    #[watch]
                                    set_visible: model.showing_outbox,
                                    #[watch]
                                    set_sensitive: model.current.is_some(),
                                    connect_clicked[sender] => move |_| sender.input(AppMsg::SendCurrentOutbox),
                                },
                                pack_start = &gtk::Button {
                                    set_label: "Send all",
                                    add_css_class: "flat",
                                    #[watch]
                                    set_visible: model.showing_outbox,
                                    connect_clicked[sender] => move |_| sender.input(AppMsg::RetryAllOutbox),
                                },
                                pack_start = &gtk::Button {
                                    set_icon_name: "co.hyprlab.Vireo-mail-reply-sender-symbolic",
                                    set_tooltip_text: Some("Reply"),
                                    add_css_class: "flat",
                                    #[watch]
                                    set_visible: !model.showing_outbox,
                                    // A conversation is on screen: which message
                                    // would this reply to? Each card carries its
                                    // own Reply/Reply all/Forward instead.
                                    #[watch]
                                    set_sensitive: model.current.is_some() && model.current_thread.len() <= 1,
                                    connect_clicked[sender] => move |_| sender.input(AppMsg::Reply),
                                },
                                pack_start = &gtk::Button {
                                    set_icon_name: "co.hyprlab.Vireo-mail-reply-all-symbolic",
                                    set_tooltip_text: Some("Reply All"),
                                    add_css_class: "flat",
                                    #[watch]
                                    set_visible: !model.showing_outbox,
                                    // A conversation is on screen: which message
                                    // would this reply to? Each card carries its
                                    // own Reply/Reply all/Forward instead.
                                    #[watch]
                                    set_sensitive: model.current.is_some() && model.current_thread.len() <= 1,
                                    connect_clicked[sender] => move |_| sender.input(AppMsg::ReplyAll),
                                },
                                pack_start = &gtk::Button {
                                    set_icon_name: "co.hyprlab.Vireo-mail-forward-symbolic",
                                    set_tooltip_text: Some("Forward"),
                                    add_css_class: "flat",
                                    #[watch]
                                    set_visible: !model.showing_outbox,
                                    // A conversation is on screen: which message
                                    // would this reply to? Each card carries its
                                    // own Reply/Reply all/Forward instead.
                                    #[watch]
                                    set_sensitive: model.current.is_some() && model.current_thread.len() <= 1,
                                    connect_clicked[sender] => move |_| sender.input(AppMsg::Forward),
                                },
                                pack_start = &gtk::Button {
                                    set_icon_name: "co.hyprlab.Vireo-contact-new-symbolic",
                                    set_tooltip_text: Some("Add sender to Contacts"),
                                    add_css_class: "flat",
                                    #[watch]
                                    set_visible: !model.showing_outbox,
                                    #[watch]
                                    set_sensitive: model.current.is_some(),
                                    connect_clicked[sender] => move |_| sender.input(AppMsg::AddToContacts),
                                },
                                pack_start = &gtk::Button {
                                    set_tooltip_text: Some("Flag"),
                                    add_css_class: "flat",
                                    #[watch]
                                    set_visible: !model.showing_outbox,
                                    #[watch]
                                    set_icon_name: if model.current.as_ref().is_some_and(|m| m.starred) {
                                        "co.hyprlab.Vireo-starred-symbolic"
                                    } else {
                                        "co.hyprlab.Vireo-non-starred-symbolic"
                                    },
                                    #[watch]
                                    set_sensitive: model.current.is_some(),
                                    connect_clicked[sender] => move |_| sender.input(AppMsg::ToggleStar),
                                },
                                // pack_end fills right-to-left, so these are declared
                                // in reverse of their visual order. Left to right:
                                // Archive, Delete, Spam, View Source, Print, sender check.
                                pack_end = &gtk::MenuButton {
                                    set_icon_name: "co.hyprlab.Vireo-lightbulb-symbolic",
                                    add_css_class: "flat",
                                    add_css_class: "image-button",
                                    // Always on screen so the toolbar's icons never
                                    // shift; greyed out like its neighbours until a
                                    // verdict for the open message has arrived.
                                    #[watch]
                                    set_sensitive: model.sender_verdict().is_some(),
                                    #[watch]
                                    set_css_classes: &model.sender_badge_classes(),
                                    #[watch]
                                    set_tooltip_text: Some(if model.sender_verdict().is_some() {
                                        model.sender_trust().label()
                                    } else {
                                        "Sender authentication"
                                    }),
                                    #[wrap(Some)]
                                    set_popover = &gtk::Popover {
                                        set_width_request: 380,
                                        gtk::Box {
                                            set_orientation: gtk::Orientation::Vertical,
                                            set_spacing: 10,
                                            add_css_class: "sender-detail",

                                            gtk::Label {
                                                #[watch]
                                                set_label: model.sender_trust().label(),
                                                set_halign: gtk::Align::Start,
                                                add_css_class: "heading",
                                            },
                                            gtk::Label {
                                                #[watch]
                                                set_label: &model
                                                    .sender_verdict()
                                                    .map(|c| c.summary.clone())
                                                    .unwrap_or_default(),
                                                set_halign: gtk::Align::Start,
                                                set_wrap: true,
                                                set_xalign: 0.0,
                                                set_max_width_chars: 44,
                                            },
                                            gtk::Separator {},
                                            gtk::Label {
                                                #[watch]
                                                set_label: &model
                                                    .sender_verdict()
                                                    .map(|c| c.findings.join("\n"))
                                                    .unwrap_or_default(),
                                                set_halign: gtk::Align::Start,
                                                set_wrap: true,
                                                set_xalign: 0.0,
                                                set_max_width_chars: 44,
                                                add_css_class: "dim-label",
                                            },
                                            gtk::Label {
                                                set_label: "A pass proves the address wasn't forged — not that the message is safe.",
                                                set_halign: gtk::Align::Start,
                                                set_wrap: true,
                                                set_xalign: 0.0,
                                                set_max_width_chars: 44,
                                                add_css_class: "dim-label",
                                                add_css_class: "caption",
                                            },
                                        },
                                    },
                                },
                                pack_end = &gtk::Button {
                                    set_icon_name: "co.hyprlab.Vireo-printer-symbolic",
                                    set_tooltip_text: Some("Print Preview (Ctrl+Shift+P)"),
                                    add_css_class: "flat",
                                    #[watch]
                                    set_visible: !model.showing_outbox,
                                    #[watch]
                                    set_sensitive: model.current.is_some(),
                                    // The preview, not the print dialog: the button
                                    // shows what will come out and prints from
                                    // there, so nobody spends paper to find out.
                                    connect_clicked[sender] => move |_| sender.input(AppMsg::PrintPreview),
                                },
                                pack_end = &gtk::Button {
                                    set_icon_name: "co.hyprlab.Vireo-background-app-ghost-symbolic",
                                    set_tooltip_text: Some("View Source"),
                                    add_css_class: "flat",
                                    #[watch]
                                    set_sensitive: model.current.is_some(),
                                    connect_clicked[sender] => move |_| sender.input(AppMsg::ViewSource),
                                },
                                pack_end = &gtk::Button {
                                    set_icon_name: "co.hyprlab.Vireo-mail-mark-junk-symbolic",
                                    set_tooltip_text: Some("Mark as Spam"),
                                    add_css_class: "flat",
                                    #[watch]
                                    set_visible: !model.showing_outbox,
                                    #[watch]
                                    set_sensitive: model.current.is_some(),
                                    connect_clicked[sender] => move |_| sender.input(AppMsg::MarkSpam),
                                },
                                pack_end = &gtk::Button {
                                    set_icon_name: "co.hyprlab.Vireo-user-trash-symbolic",
                                    #[watch]
                                    set_tooltip_text: Some(&model.delete_tooltip()),
                                    add_css_class: "flat",
                                    #[watch]
                                    set_sensitive: model.current.is_some() || model.list_selection.len() > 1,
                                    connect_clicked[sender] => move |_| sender.input(AppMsg::Delete),
                                },
                                pack_end = &gtk::Button {
                                    set_icon_name: "co.hyprlab.Vireo-mail-archive-symbolic",
                                    set_tooltip_text: Some("Archive"),
                                    add_css_class: "flat",
                                    #[watch]
                                    set_visible: !model.showing_outbox,
                                    #[watch]
                                    set_sensitive: model.current.is_some(),
                                    connect_clicked[sender] => move |_| sender.input(AppMsg::Archive),
                                },
                                pack_end = &gtk::Spinner {
                                    set_valign: gtk::Align::Center,
                                    set_tooltip_text: Some("Downloading attachments…"),
                                    #[watch]
                                    set_spinning: model.attachments_loading,
                                    #[watch]
                                    set_visible: model.attachments_loading,
                                },
                                // Shown for messages whose attachments weren't
                                // pre-downloaded — load them only when asked.
                                pack_end = &gtk::Button {
                                    set_icon_name: "co.hyprlab.Vireo-folder-download-symbolic",
                                    set_tooltip_text: Some("Load attachments from server"),
                                    add_css_class: "flat",
                                    add_css_class: "attach-present",
                                    #[watch]
                                    set_visible: model.attachments_available && !model.attachments_loading,
                                    connect_clicked[sender] => move |_| sender.input(AppMsg::LoadAttachmentsNow),
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
                                            set_width_request: 340,
                                        },
                                    },
                                },
                            },
                            // Reader content: the inline reply/forward pane drops
                            // down (SlideDown revealer) above the message body,
                            // pushing it down to make room. The revealer is
                            // prepended in `init`. The drawer's widget is a Paned
                            // that holds the reader body + attachment footer.
                            #[wrap(Some)]
                            #[name = "reader_content_box"]
                            set_content = &gtk::Box {
                                set_orientation: gtk::Orientation::Vertical,
                                append: model.attachment_drawer.widget(),
                            },
                        },
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
        relm4::set_global_css(include_str!("styles.css"));
        register_icons();

        let mut sidebar_state = config::load_sidebar_state();
        let icon_only = sidebar_state.icon_only;

        // Load accounts, then reconcile against GNOME Online Accounts: drop any
        // imported account GOA no longer has, pause any whose Mail service is
        // switched off there. Reconciliation is skipped when GOA is unreachable,
        // so a momentary outage never wipes imported accounts. Live changes are
        // handled by the watcher below.
        let mut config = config::load().unwrap_or_default();
        let goa_outcome = match crate::goa::live_state() {
            Some(live) => reconcile_goa(&mut config, &live),
            None => GoaReconcile::default(),
        };
        let goa_removed = goa_outcome.removed;
        if !goa_removed.is_empty() {
            for email in &goa_removed {
                config::delete_password(email);
            }
            sidebar_state.order.retain(|e| !goa_removed.contains(e));
            sidebar_state.collapsed.retain(|e| !goa_removed.contains(e));
            sidebar_state.folders_expanded.retain(|e| !goa_removed.contains(e));
            config::save_sidebar_state(&sidebar_state);
        }
        if !goa_removed.is_empty() || goa_outcome.paused_changed {
            let _ = config::save(&config);
        }
        let order = sidebar_state.order;
        let collapsed = sidebar_state.collapsed;
        let folders_expanded = sidebar_state.folders_expanded;

        let show_attachments = config::load_show_attachments();
        let sidebar = Sidebar::builder()
            .launch(SidebarInit { collapsed: icon_only, show_attachments })
            .forward(sender.input_sender(), |out| match out {
                SidebarOutput::UnifiedSelected => AppMsg::UnifiedSelected,
                SidebarOutput::AttachmentsSelected => AppMsg::ShowAttachments,
                SidebarOutput::OutboxSelected => AppMsg::ShowOutbox,
                SidebarOutput::FolderSelected { account_id, folder_id, name, path } => {
                    AppMsg::FolderSelected { account_id, folder_id, name, path }
                }
                SidebarOutput::ToggleCollapse(id) => AppMsg::ToggleCollapse(id),
                SidebarOutput::ToggleCustomFolders(id) => AppMsg::ToggleCustomFolders(id),
                SidebarOutput::CollapsedChanged(collapsed) => AppMsg::SidebarCollapsed(collapsed),
                SidebarOutput::AddAccount => AppMsg::AddFirstAccount,
                SidebarOutput::Context(action) => AppMsg::SidebarContext(action),
                SidebarOutput::MoveMessages { dest_account, dest, items } => {
                    AppMsg::DropMoveMessages { dest_account, dest, items }
                }
            });

        let message_list =
            MessageList::builder()
                .launch(())
                .forward(sender.input_sender(), |out| match out {
                    MessageListOutput::Selected { message, thread, solo } => {
                        AppMsg::MessageSelected { message, thread, solo }
                    }
                    MessageListOutput::SelectionKeys(keys) => AppMsg::SelectionKeys(keys),
                    MessageListOutput::Activated(m) => AppMsg::OpenMessageWindow(m),
                    MessageListOutput::Action { action, message } => {
                        AppMsg::RowAction { action, message }
                    }
                    MessageListOutput::Bulk { action, messages } => {
                        AppMsg::Bulk { action, messages }
                    }
                    MessageListOutput::SelectionCleared => AppMsg::ClearReader,
                    MessageListOutput::SearchActive(active) => AppMsg::SearchActive(active),
                });

        let message_view =
            MessageView::builder()
                .launch(())
                .forward(sender.input_sender(), |out| match out {
                    MessageViewOutput::AllowSender(addr) => AppMsg::AllowSender(addr),
                    MessageViewOutput::ComposeTo(addr) => AppMsg::ComposeTo(addr),
                    MessageViewOutput::OpenWindow(m) => AppMsg::OpenMessageWindow(*m),
                    // A card's own Reply/Reply all/Forward: the same action the
                    // list's row menu performs, aimed at that message.
                    MessageViewOutput::CardAction { action, message } => {
                        AppMsg::RowAction { action, message }
                    }
                    MessageViewOutput::MarkSeen { account_id, id } => {
                        AppMsg::ThreadMessageSeen { account_id, id }
                    }
                    MessageViewOutput::SelectCards(keys) => AppMsg::SelectCards(keys),
                });

        // The drawer owns a Paned whose top pane is the reader body, so hand it
        // the message-view widget to dock beneath.
        let attachment_drawer = AttachmentDrawer::builder()
            .launch(crate::ui::attachment_drawer::DrawerInit {
                state: config::load_drawer_state(),
                reader: message_view.widget().clone().upcast(),
            })
            .detach();

        let gallery =
            AttachmentsGallery::builder()
                .launch(())
                .forward(sender.input_sender(), |out| match out {
                    GalleryOutput::OpenMessage { account_id, folder_path, uid } => {
                        AppMsg::OpenAttachmentMessage { account_id, folder_path, uid }
                    }
                });

        let notifications = NotificationCenter::builder().launch(()).forward(
            sender.input_sender(),
            |out| match out {
                NotifyOutput::CountChanged(n) => AppMsg::NotifyCount(n),
            },
        );

        let menu = gtk::gio::Menu::new();
        menu.append(Some("Accounts"), Some("win.accounts"));
        menu.append(Some("Preferences"), Some("win.preferences"));
        menu.append(Some("Print Preview…"), Some("win.print-preview"));
        menu.append(Some("Print Message…"), Some("win.print"));
        menu.append(Some("Keyboard Shortcuts"), Some("win.shortcuts"));
        menu.append(Some("About Vireo"), Some("win.about"));
        // Last, where a Quit item belongs.
        menu.append(Some("Quit"), Some("app.quit"));

        let mut model = AppModel {
            workers: HashMap::new(),
            config,
            window: root.clone(),
            prefs: None,
            accounts_win: None,
            composers: Vec::new(),
            reader_compose: None,
            draining_composers: Vec::new(),
            reader_compose_revealer: {
                let r = gtk::Revealer::new();
                r.set_transition_type(gtk::RevealerTransitionType::SlideDown);
                r.set_transition_duration(200);
                r.set_reveal_child(false);
                r
            },
            next_compose_id: 1,
            menu,
            accounts: Vec::new(),
            folders: HashMap::new(),
            account_order: order,
            collapsed,
            folders_expanded,
            selected: None,
            attachments: Vec::new(),
            attachments_loading: false,
            attachments_available: false,
            attachment_cache: HashMap::new(),
            attach_list: gtk::Box::new(gtk::Orientation::Vertical, 0),
            unified: false,
            unified_by_account: HashMap::new(),
            message_cache: HashMap::new(),
            indexed_folders: HashSet::new(),
            body_cache: HashMap::new(),
            sender_cache: HashMap::new(),
            pending_draft: None,
            popouts: HashMap::new(),
            current_thread: Vec::new(),
            list_selection: Vec::new(),
            bulk_pending: 0,
            pending_bulk: None,
            pending_purge: None,
            related_id_seq: u32::MAX,
            related_ids: HashMap::new(),
            folder_unread: HashMap::new(),
            sidebar_split: None,
            app_title: None,
            sidebar_menu: None,
            sidebar_header: None,
            sidebar_collapsed: icon_only,
            sidebar_anim: None,
            auto_rail: false,
            rail_active: icon_only,
            sidebar_peek: false,
            current: None,
            allowed_senders: config::load_allowed_senders(),
            auto_remote_content: config::load_auto_remote_content(),
            dim_remote_banner: config::load_dim_remote_banner(),
            show_remote_banner: config::load_show_remote_banner(),
            blacklist: config::load_blacklist(),
            palette_collapse_secs: config::load_palette_collapse(),
            gravatar: config::load_gravatar(),
            avatars: config::load_avatars(),
            sender_logos: config::load_sender_logos(),
            date_style: config::load_date_format().0,
            clock_style: config::load_date_format().1,
            fetch_interval_secs: config::load_fetch_interval(),
            push: config::load_push(),
            notifications_enabled: config::load_notifications(),
            notification_content: config::load_notification_content(),
            show_attachments,
            preview_lines: config::load_preview_lines(),
            shortcuts_win: None,
            run_in_background: std::rc::Rc::new(std::cell::Cell::new(
                config::load_run_in_background(),
            )),
            autostart: config::load_autostart(),
            single_key: std::rc::Rc::new(std::cell::Cell::new(
                config::load_single_key_shortcuts(),
            )),
            threading: config::load_threading(),
            thread_render_queued: false,
            thread_opened_at: None,
            thread_related_pending: false,
            thread_painted: false,
            thread_cache: HashMap::new(),
            thread_cache_order: Vec::new(),
            thread_key: None,
            threads_expanded: config::load_threads_expanded(),
            message_theme: config::load_message_theme(),
            auto_fetch_source: None,
            notifications,
            notify_count: 0,
            busy: HashSet::new(),
            sidebar,
            message_list,
            message_view,
            attachment_drawer,
            gallery,
            showing_gallery: false,
            showing_outbox: false,
            outbox_by_account: HashMap::new(),
            gallery_by_account: HashMap::new(),
        };
        model.spawn_workers(&sender);
        // Refresh visible sender circles when the GNOME Contacts photo index
        // changes (first load finishing, or an EDS/CardDAV sync).
        crate::contacts::watch_photo_changes({
            let input = sender.input_sender().clone();
            move || {
                let _ = input.send(AppMsg::ContactPhotosChanged);
            }
        });
        // Watch GNOME Online Accounts so a change there (account removed, Mail
        // toggled) is reflected in Vireo live, no restart needed. The watcher
        // debounces signal bursts and snapshots GOA on its own thread;
        // reconciliation happens on GoaChanged.
        crate::goa::watch_changes({
            let s = sender.input_sender().clone();
            move |state| {
                let _ = s.send(AppMsg::GoaChanged(state));
            }
        });
        // Watch for resume-from-sleep: suspended IMAP sockets die silently, so
        // on wake we reconnect every worker and refresh, otherwise no new mail
        // arrives until the app is restarted.
        crate::power::watch_resume({
            let s = sender.input_sender().clone();
            move || {
                let _ = s.send(AppMsg::SystemResumed);
            }
        });
        // With no accounts, no worker events will populate the sidebar, so render
        // its empty state (the "Add first account" prompt) up front.
        if model.config.is_empty() {
            model.rebuild_sidebar();
        }
        model
            .message_list
            .emit(MessageListInput::SetGravatar(model.gravatar));
        model
            .message_list
            .emit(MessageListInput::SetAvatars(model.avatars));
        // The formatter is a free function, so the preference has to be handed to
        // it before anything draws a date.
        model
            .message_list
            .emit(MessageListInput::SetSenderLogos(model.sender_logos));
        model
            .message_view
            .emit(MessageViewInput::SetSenderLogos(model.sender_logos));
        crate::datefmt::set_style(model.date_style, model.clock_style);
        model
            .message_list
            .emit(MessageListInput::SetPreviewLines(model.preview_lines));
        model
            .message_list
            .emit(MessageListInput::SetThreading(model.threading));
        model
            .message_list
            .emit(MessageListInput::SetThreadsExpanded(model.threads_expanded));
        model
            .message_list
            .emit(MessageListInput::SetPaletteCollapse(model.palette_collapse_secs));
        model
            .message_view
            .emit(MessageViewInput::SetContentTheme(model.message_theme.dark_override()));
        model.arm_auto_fetch(&sender);

        let attach_list = &model.attach_list;
        let widgets = view_output!();
        // The inline reply/forward pane sits above the reader body (top of the
        // content box), sliding down over it when revealed.
        widgets
            .reader_content_box
            .prepend(&model.reader_compose_revealer);
        // The attachments gallery is the content stack's second page. Wrap it in a
        // ToolbarView + HeaderBar so it keeps the window controls (close/minimize)
        // that otherwise live only on the reader pane's header.
        {
            use gtk::prelude::*;
            let gallery_tv = adw::ToolbarView::new();
            let gallery_hb = adw::HeaderBar::new();
            gallery_hb.add_css_class("flat");
            let title = gtk::Label::new(Some("Attachments"));
            title.add_css_class("pane-title");
            gallery_hb.set_title_widget(Some(&title));
            gallery_tv.add_top_bar(&gallery_hb);
            gallery_tv.set_content(Some(model.gallery.widget()));
            widgets.content_stack.add_named(&gallery_tv, Some("gallery"));

        }
        // Desktop-notification click actions: raise the window (error alerts) and
        // raise + open a specific message (new-mail alerts). Registered here rather
        // than in `notify` because opening a message needs the app's channel.
        {
            use gtk::prelude::*;
            let app = relm4::main_application();
            let present = gtk::gio::SimpleAction::new(crate::notify::PRESENT_ACTION, None);
            let win = model.window.clone();
            present.connect_activate(move |_, _| {
                win.set_visible(true);
                win.present();
            });
            app.add_action(&present);

            let ty = gtk::glib::VariantTy::new("(uuu)").unwrap();
            let open = gtk::gio::SimpleAction::new(crate::notify::OPEN_MESSAGE_ACTION, Some(ty));
            let win = model.window.clone();
            let osender = sender.clone();
            open.connect_activate(move |_, param| {
                win.set_visible(true);
                win.present();
                if let Some((account_id, folder_id, message_id)) =
                    param.and_then(|v| v.get::<(u32, u32, u32)>())
                {
                    osender.input(AppMsg::OpenMessageFromNotification {
                        account_id,
                        folder_id,
                        message_id,
                    });
                }
            });
            app.add_action(&open);
        }
        // Restore the last window size + maximized state (Wayland can't restore
        // position/monitor).
        let (win_w, win_h, win_max) = config::load_window_state();
        root.set_default_size(win_w, win_h);
        if win_max {
            root.maximize();
        }
        // Below this width the expanded sidebar and a full-width Actions
        // Palette can't both fit (280 sidebar + 350 list + 492 reader), so the
        // sidebar drops to its icon rail automatically — this is what keeps the
        // palette whole when the window is tiled to half of a 1920px screen.
        // With a breakpoint present the window no longer derives its minimum
        // size from its content, so pin an explicit floor: the sidebar rail
        // (80) + the list's palette floor (350) + the reader header (~492).
        root.set_size_request(930, 360);
        let narrow = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
            adw::BreakpointConditionLengthType::MaxWidth,
            1120.0,
            adw::LengthUnit::Px,
        ));
        {
            let s = sender.clone();
            narrow.connect_apply(move |_| s.input(AppMsg::AutoRail(true)));
            let s = sender.clone();
            narrow.connect_unapply(move |_| s.input(AppMsg::AutoRail(false)));
        }
        root.add_breakpoint(narrow);
        {
            let s = sender.clone();
            widgets.sidebar_split.connect_show_sidebar_notify(move |split| {
                if split.is_collapsed() && !split.shows_sidebar() {
                    s.input(AppMsg::SidebarPeekDismissed);
                }
            });
        }
        model.sidebar_split = Some(widgets.sidebar_split.clone());
        model.app_title = Some(widgets.app_title.clone());
        model.sidebar_header = Some(widgets.sidebar_header.clone());
        model.sidebar_menu = Some(widgets.sidebar_menu.clone());
        if model.sidebar_collapsed {
            widgets.sidebar_split.set_min_sidebar_width(SIDEBAR_RAIL_WIDTH);
            widgets.sidebar_split.set_max_sidebar_width(SIDEBAR_RAIL_WIDTH);
            set_sidebar_header_compact(
                &widgets.sidebar_header,
                &widgets.app_title,
                &widgets.sidebar_menu,
                true,
            );
        }

        // GNOME's Background Apps menu quits an app by activating its `quit`
        // action over D-Bus, falling back to `flatpak kill` if nothing answers
        // within five seconds. Without this the ✕ there would be a hard kill.
        {
            let app = relm4::main_application();
            let window = root.clone();
            let quit = gtk::gio::SimpleAction::new("quit", None);
            quit.connect_activate(move |_, _| {
                // Same teardown as closing the last window: save the geometry,
                // then exit rather than unwinding through GTK and WebKit.
                let maximized = window.is_maximized();
                let (width, height) = if maximized {
                    let (sw, sh, _) = crate::config::load_window_state();
                    (sw, sh)
                } else {
                    (window.width(), window.height())
                };
                crate::config::save_window_state(width, height, maximized);
                crate::background::set_status("");
                std::process::exit(0)
            });
            app.add_action(&quit);
            gtk::prelude::GtkApplicationExt::set_accels_for_action(&app, "app.quit", &["<Ctrl>q"]);

            // Activating the app again — its icon, a notification, or the
            // autostart entry — brings the hidden window back rather than doing
            // nothing.
            let window = root.clone();
            // Started hidden? Then the first activation is this very launch, and
            // presenting here would undo it. Every activation after that is a
            // person asking for the window.
            let pending_hidden_start =
                std::cell::Cell::new(crate::HIDDEN_START.load(std::sync::atomic::Ordering::Relaxed));
            app.connect_activate(move |_| {
                if pending_hidden_start.replace(false) {
                    return;
                }
                window.set_visible(true);
                window.present();
            });
        }

        let mut group = RelmActionGroup::<WindowActionGroup>::new();
        let accounts_sender = sender.clone();
        group.add_action(RelmAction::<AccountsAction>::new_stateless(move |_| {
            accounts_sender.input(AppMsg::OpenAccounts);
        }));
        let prefs_sender = sender.clone();
        group.add_action(RelmAction::<PreferencesAction>::new_stateless(move |_| {
            prefs_sender.input(AppMsg::OpenPreferences);
        }));
        let about_sender = sender.clone();
        group.add_action(RelmAction::<AboutAction>::new_stateless(move |_| {
            about_sender.input(AppMsg::OpenAbout);
        }));
        let shortcuts_sender = sender.clone();
        group.add_action(RelmAction::<ShortcutsAction>::new_stateless(move |_| {
            shortcuts_sender.input(AppMsg::ShowShortcuts);
        }));
        let print_sender = sender.clone();
        group.add_action(RelmAction::<PrintAction>::new_stateless(move |_| {
            print_sender.input(AppMsg::PrintMessage);
        }));
        let preview_sender = sender.clone();
        group.add_action(RelmAction::<PrintPreviewAction>::new_stateless(move |_| {
            preview_sender.input(AppMsg::PrintPreview);
        }));
        group.register_for_widget(&root);

        // A real accelerator rather than a key handler: GTK matches these before
        // the keystroke reaches whatever has focus, so Ctrl+? works while reading
        // a message (the web view would otherwise swallow it). Both spellings are
        // bound because layouts disagree about whether Ctrl+Shift+/ arrives as
        // `question` or as `slash`, and F1 is the GNOME convention.
        relm4::main_application().set_accelerators_for_action::<PrintAction>(&["<Ctrl>p"]);
        relm4::main_application()
            .set_accelerators_for_action::<PrintPreviewAction>(&["<Ctrl><Shift>p"]);
        relm4::main_application().set_accelerators_for_action::<ShortcutsAction>(&[
            "<Ctrl>question",
            "<Ctrl><Shift>question",
            "<Ctrl>slash",
            "<Ctrl><Shift>slash",
            "F1",
        ]);

        // Single-key shortcuts. The controller is on the window in the bubble
        // phase, so the focused widget always gets first refusal: a search entry
        // or the composer consumes the letter itself and nothing here fires.
        // `focus_takes_keys` covers the rest — chiefly the reader's web view,
        // which handles keys without consuming them.
        {
            let keys = gtk::EventControllerKey::new();
            let s = sender.clone();
            let window = root.clone();
            let enabled = model.single_key.clone();
            keys.connect_key_pressed(move |_, keyval, _, state| {
                let ctrl = state.contains(gtk::gdk::ModifierType::CONTROL_MASK);
                let shift = state.contains(gtk::gdk::ModifierType::SHIFT_MASK);
                // Ctrl+? opens the reference whether or not the shortcuts
                // themselves are switched on.
                if ctrl && matches!(keyval, gtk::gdk::Key::question | gtk::gdk::Key::slash) {
                    s.input(AppMsg::ShowShortcuts);
                    return gtk::glib::Propagation::Stop;
                }
                if ctrl || state.contains(gtk::gdk::ModifierType::ALT_MASK) {
                    return gtk::glib::Propagation::Proceed;
                }
                // Escape backs out to the message list whatever else is going on,
                // and without needing single-key shortcuts switched on. A text
                // field keeps it, though — there it clears the search.
                if keyval == gtk::gdk::Key::Escape {
                    if focus_is_text(&window) {
                        return gtk::glib::Propagation::Proceed;
                    }
                    s.input(AppMsg::Shortcut(Shortcut::BackToList));
                    return gtk::glib::Propagation::Stop;
                }
                if !enabled.get() || focus_takes_keys(&window) {
                    return gtk::glib::Propagation::Proceed;
                }
                match shortcut_for(keyval, shift) {
                    Some(action) => {
                        s.input(AppMsg::Shortcut(action));
                        gtk::glib::Propagation::Stop
                    }
                    None => gtk::glib::Propagation::Proceed,
                }
            });
            root.add_controller(keys);
        }

        // One-time, dismissible keyring setup tip for Linux Mint / Cinnamon, where
        // the Secret Service often needs configuring so passwords persist and the
        // keyring auto-unlocks at login. Only shown once the user actually has an
        // account (so it isn't the very first thing a new user sees), and never
        // again after "Don't show again".
        if !model.config.is_empty()
            && crate::platform::is_mint_cinnamon()
            && !config::mint_keyring_help_dismissed()
        {
            sender.input(AppMsg::ShowKeyringHelp { problem: false });
        }

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            AppMsg::ShowOutbox => {
                self.close_sidebar_peek();
                // Treated as a folder: same list, same reader, nothing swapped
                // out from under the user. `selected` stays None so no sync or
                // server request is ever aimed at it.
                self.showing_outbox = true;
                self.showing_gallery = false;
                self.unified = false;
                self.selected = None;
                self.current = None;
                self.current_thread.clear();
                self.attachments.clear();
                self.attachments_loading = false;
                self.attachments_available = false;
                self.sync_attachment_drawer();
                self.message_list.emit(MessageListInput::SetSelected(None));
                self.message_list.emit(MessageListInput::SetColorize(self.accounts.len() > 1));
                self.message_list.emit(MessageListInput::ResetPaging);
                self.show_message(None, false);
                self.push_outbox();
            }

            AppMsg::OutboxItems { account_id, items } => {
                self.outbox_by_account.insert(account_id, items);
                self.push_outbox();
            }




            AppMsg::EditCurrentOutbox => {
                if let Some(m) = self.current.clone() {
                    self.compose_from_outbox(m.account_id, m.id, &sender);
                }
            }

            AppMsg::SendCurrentOutbox => {
                if let Some(m) = self.current.clone() {
                    self.send_to(m.account_id, MailRequest::FlushOutbox { id: Some(m.id) });
                }
            }

            AppMsg::RetryAllOutbox => {
                let accounts: Vec<u32> = self.outbox_by_account
                    .iter()
                    .filter(|(_, items)| !items.is_empty())
                    .map(|(id, _)| *id)
                    .collect();
                for account_id in accounts {
                    self.send_to(account_id, MailRequest::FlushOutbox { id: None });
                }
            }

            AppMsg::Notice(text) => {
                self.notifications.emit(NotifyInput::Push {
                    text,
                    error: false,
                    connectivity: false,
                });
            }

            AppMsg::ShowAttachments => {
                self.close_sidebar_peek();
                self.showing_outbox = false;
                self.showing_gallery = true;
                self.gallery_by_account.clear();
                self.gallery.emit(GalleryInput::SetLoading(true));
                self.gallery.emit(GalleryInput::SetItems(Vec::new()));
                // Load each account's attachments (across all gallery folders)
                // from the cache.
                let ids: Vec<u32> = self.accounts.iter().map(|a| a.id).collect();
                for account_id in ids {
                    self.send_to(account_id, MailRequest::LoadGallery);
                }
            }

            AppMsg::GalleryItems { account_id, items } => {
                self.gallery_by_account.insert(account_id, items);
                let mut merged: Vec<crate::models::GalleryItem> = self
                    .gallery_by_account
                    .values()
                    .flatten()
                    .cloned()
                    .collect();
                merged.sort_by_key(|i| std::cmp::Reverse(i.timestamp));
                self.gallery.emit(GalleryInput::SetItems(merged));
            }

            AppMsg::OpenAttachmentMessage { account_id, folder_path, uid } => {
                self.showing_gallery = false;
                self.showing_outbox = false;
                if let Some(folder) = self
                    .folders
                    .get(&account_id)
                    .and_then(|fs| fs.iter().find(|f| f.path == folder_path))
                    .cloned()
                {
                    self.select_folder(account_id, folder.id, folder.name.clone(), folder.path.clone());
                    // Messages use their UID as id, so select by (account, uid).
                    self.message_list
                        .emit(MessageListInput::SelectAndLoad((account_id, uid)));
                }
            }

            AppMsg::UnifiedSelected => {
                self.close_sidebar_peek();
                self.showing_gallery = false;
                self.showing_outbox = false;
                self.unified = true;
                self.selected = None;
                self.current = None;
                self.current_thread.clear();
                self.attachments.clear();
                self.attachments_loading = false;
                self.attachments_available = false;
                self.sync_attachment_drawer();
                self.show_message(None, false);
                self.message_list.emit(MessageListInput::SetSelected(None));
                self.message_list.emit(MessageListInput::SetColorize(true));
                self.message_list.emit(MessageListInput::ResetPaging);
                let reqs: Vec<(u32, u32, String)> = self
                    .accounts
                    .iter()
                    .filter_map(|a| self.inbox_of(a.id).map(|f| (a.id, f.id, f.path.clone())))
                    .collect();
                // Keep every account's last known inbox and top it up from the
                // folder caches, the way opening a single folder does. This used
                // to clear the lot and wait: an account whose worker was slow to
                // answer — busy backfilling a large mailbox, reconnecting, or
                // offline — was simply absent from "All Inboxes", while its own
                // Inbox, served from cache, still listed its mail. Each account's
                // slice is replaced when its load lands.
                for (account_id, folder_id, _) in &reqs {
                    if let Some(cached) = self.message_cache.get(&(*account_id, *folder_id)) {
                        if !cached.is_empty() {
                            self.unified_by_account.insert(*account_id, cached.clone());
                        }
                    }
                }
                // Forget accounts that no longer have an inbox to contribute
                // (removed or disabled since the last visit).
                let live: std::collections::HashSet<u32> =
                    reqs.iter().map(|(a, _, _)| *a).collect();
                self.unified_by_account.retain(|a, _| live.contains(a));
                if self.unified_by_account.is_empty() {
                    self.message_list
                        .emit(MessageListInput::SetLoading { title: "All Inboxes".into() });
                } else {
                    self.emit_unified();
                }
                // Request every account's inbox; each result replaces that
                // account's slice as it arrives.
                for (account_id, folder_id, path) in reqs {
                    self.send_to(account_id, MailRequest::LoadMessages { folder_id, path });
                }
                self.push_index_complete();
            }

            AppMsg::FolderSelected { account_id, folder_id, name, path } => {
                self.close_sidebar_peek();
                self.select_folder(account_id, folder_id, name, path);
            }

            AppMsg::OpenMessageFromNotification { account_id, folder_id, message_id } => {
                // The user clicked a new-mail notification: they've engaged with
                // that account's mail, so clear its toast, then navigate to the
                // message's folder and open it in the reader.
                crate::notify::withdraw_mail(account_id);
                if let Some((name, path)) = self
                    .folders
                    .get(&account_id)
                    .and_then(|fs| fs.iter().find(|f| f.id == folder_id))
                    .map(|f| (f.name.clone(), f.path.clone()))
                {
                    // select_folder emits the (cached) list synchronously, so the
                    // subsequent SelectAndLoad finds the row and opens it.
                    self.select_folder(account_id, folder_id, name, path);
                    self.message_list
                        .emit(MessageListInput::SelectAndLoad((account_id, message_id)));
                }
            }

            AppMsg::ToggleCollapse(account_id) => {
                // The sidebar already animated the toggle locally; just record
                // the new state (a rebuild here would interrupt the animation).
                if let Some(email) = self.email_of(account_id) {
                    if let Some(pos) = self.collapsed.iter().position(|e| *e == email) {
                        self.collapsed.remove(pos);
                    } else {
                        self.collapsed.push(email);
                    }
                    self.save_sidebar_state();
                }
            }

            AppMsg::ToggleCustomFolders(account_id) => {
                // The sidebar animated the toggle locally; record the new state
                // (the "folders_expanded" list holds accounts whose custom
                // folders are revealed; absence means hidden, the default).
                if let Some(email) = self.email_of(account_id) {
                    if let Some(pos) = self.folders_expanded.iter().position(|e| *e == email) {
                        self.folders_expanded.remove(pos);
                    } else {
                        self.folders_expanded.push(email);
                    }
                    self.save_sidebar_state();
                }
            }

            AppMsg::SidebarCollapsed(collapsed) => {
                // The sidebar component has already switched its own rows; this
                // is the app-side reaction (split widths, header, persistence).
                if self.auto_rail {
                    // Narrow window: expanding is a transient overlay *peek*
                    // floating above the panes — the list and reader keep their
                    // widths — and collapsing just closes it back to the rail.
                    // Neither touches the persisted preference: this is the
                    // window's shape talking, not the user's setting.
                    self.rail_active = collapsed;
                    self.set_sidebar_peek(!collapsed, false);
                } else {
                    self.sidebar_collapsed = collapsed;
                    self.rail_active = collapsed;
                    self.animate_sidebar(collapsed);
                    self.compact_sidebar_header(collapsed);
                    self.save_sidebar_state();
                }
            }

            AppMsg::AutoRail(on) => {
                self.auto_rail = on;
                if !on && self.sidebar_peek {
                    // Widened with the overlay open: fold it back before the
                    // split view returns to side-by-side. Closing puts the rows
                    // in rail mode, so mark the rail active for the restore
                    // comparison below.
                    self.set_sidebar_peek(false, true);
                    self.rail_active = true;
                }
                // The rail wins while the window is narrow; the user's own
                // choice comes back the moment there is room again. Nothing is
                // persisted here — this is the window's shape, not a preference.
                let want = on || self.sidebar_collapsed;
                if want != self.rail_active {
                    self.rail_active = want;
                    self.sidebar.emit(SidebarInput::SetCollapsed(want));
                    self.animate_sidebar(want);
                    self.compact_sidebar_header(want);
                }
            }

            AppMsg::SidebarPeekDismissed => {
                if self.auto_rail && self.sidebar_peek {
                    self.rail_active = true;
                    self.set_sidebar_peek(false, true);
                }
            }

            AppMsg::SidebarContext(action) => match action {
                CtxAction::MarkFolderRead { account_id, folder_id } => {
                    self.mark_folder_read(account_id, folder_id);
                }
                CtxAction::RefreshFolder { account_id, folder_id } => {
                    if let Some(path) = self
                        .folders
                        .get(&account_id)
                        .and_then(|fs| fs.iter().find(|f| f.id == folder_id))
                        .map(|f| f.path.clone())
                    {
                        self.send_to(account_id, MailRequest::LoadMessages { folder_id, path });
                    }
                }
                CtxAction::MarkAllInboxesRead => {
                    let inboxes: Vec<(u32, u32)> = self
                        .accounts
                        .iter()
                        .filter_map(|a| self.inbox_of(a.id).map(|f| (a.id, f.id)))
                        .collect();
                    for (account_id, folder_id) in inboxes {
                        self.mark_folder_read(account_id, folder_id);
                    }
                }
                CtxAction::RefreshAllInboxes => {
                    let reqs: Vec<(u32, u32, String)> = self
                        .accounts
                        .iter()
                        .filter_map(|a| self.inbox_of(a.id).map(|f| (a.id, f.id, f.path.clone())))
                        .collect();
                    for (account_id, folder_id, path) in reqs {
                        self.send_to(account_id, MailRequest::LoadMessages { folder_id, path });
                    }
                }
                CtxAction::OpenAccountSettings => sender.input(AppMsg::OpenAccounts),
                CtxAction::RemoveAccount(account_id) => {
                    self.confirm_remove_account(account_id, &sender);
                }
                CtxAction::NewFolder(account_id) => {
                    self.prompt_new_folder(account_id, &sender);
                }
                CtxAction::DeleteFolder { account_id, name, path } => {
                    self.confirm_delete_folder(account_id, name, path, &sender);
                }
            },

            AppMsg::DropMoveMessages { dest_account, dest, items } => {
                self.drop_move(dest_account, dest, items);
            }

            AppMsg::CreateFolder { account_id, name } => {
                let name = name.trim();
                if !name.is_empty() {
                    // The server names mailboxes in modified UTF-7, so a name
                    // with any non-ASCII character has to be encoded before it
                    // becomes a path (issue #1, in the other direction).
                    let path = format!(
                        "{}{}",
                        self.folder_namespace(account_id),
                        crate::mutf7::encode(name)
                    );
                    self.send_to(account_id, MailRequest::CreateFolder { path });
                }
            }

            AppMsg::DeleteFolder { account_id, path } => {
                let trash = self
                    .folders
                    .get(&account_id)
                    .and_then(|fs| fs.iter().find(|f| f.kind == FolderKind::Trash))
                    .map(|f| f.path.clone())
                    .or_else(|| self.default_folder_path(account_id, FolderKind::Trash));
                // If the deleted folder is currently open, clear the view.
                if self.selected.as_ref().is_some_and(|s| s.account_id == account_id && s.path == path) {
                    self.current = None;
                    self.current_thread.clear();
                    self.show_message(None, false);
                    self.message_list.emit(MessageListInput::SetLoading { title: String::new() });
                }
                self.send_to(account_id, MailRequest::DeleteFolder { path, trash });
            }

            AppMsg::AccountsReordered(emails) => {
                // Display order only (by email) — no reconnect needed.
                if !emails.is_empty() {
                    self.account_order = emails;
                    self.save_sidebar_state();
                    self.rebuild_sidebar();
                }
            }

            AppMsg::ClearReader => {
                self.current = None;
                self.current_thread.clear();
                self.attachments.clear();
                self.attachments_loading = false;
                self.attachments_available = false;
                self.sync_attachment_drawer();
                self.show_message(None, false);
            }
            AppMsg::SearchActive(active) => {
                if active {
                    // Snapshot every folder's indexed messages so the search can
                    // span the whole mailbox.
                    self.message_list.emit(MessageListInput::SetSearchPool(
                        build_search_pool(&self.message_cache),
                    ));
                    // Results span accounts; tint rows by account (as in the unified
                    // inbox) so their origin is legible.
                    if self.accounts.len() > 1 {
                        self.message_list.emit(MessageListInput::SetColorize(true));
                    }
                } else {
                    self.message_list
                        .emit(MessageListInput::SetSearchPool(Vec::new()));
                    // Restore the tint state the underlying view wants.
                    self.message_list
                        .emit(MessageListInput::SetColorize(self.unified));
                }
            }
            AppMsg::MessageSelected { message: m, thread, solo } => {
                // Navigating away releases any inline reply (save-if-dirty, or keep
                // it as an independent window if it was popped out).
                self.release_reader_compose();
                // A queued message reads like any other, straight from the bytes
                // already on disk — no folder, no UID, nothing to ask the server.
                if let Some(item) = self.outbox_item(m.account_id, m.id) {
                    self.show_outbox_message(&item);
                    return;
                }
                // Clicking a draft opens it in the compose editor, not the reader.
                if self.is_drafts_folder(m.account_id, m.folder_id) {
                    self.open_draft(m, &sender);
                    return;
                }
                self.attachments.clear();
                self.attachments_loading = false;
                self.attachments_available = false;
                self.sync_attachment_drawer();
                let account_id = m.account_id;
                let folder_path = self.resolve_folder_path(&m);
                // Use an already-fetched body if we have one (on the message or in
                // our cache) so reopening renders instantly without a spinner.
                let cached_body = if !m.body.is_empty() {
                    Some(m.body.clone())
                } else {
                    self.body_cache.get(&(account_id, m.id)).cloned()
                };
                let needs_body = cached_body.is_none();

                if m.unread {
                    if let Some(path) = folder_path.clone() {
                        self.send_to(account_id, MailRequest::SetSeen { path, uid: m.uid, seen: true });
                    }
                    // Reading new mail clears that account's new-mail notification.
                    crate::notify::withdraw_mail(account_id);
                    self.message_list.emit(MessageListInput::MarkRead(m.id));
                    self.mark_cached_read(account_id, m.id);
                    // Optimistically drop the badge by one; the next server count
                    // (after the sync below) reconciles any drift.
                    if let Some(n) = self.folder_unread.get_mut(&(account_id, m.folder_id)) {
                        *n = n.saturating_sub(1);
                    }
                    self.push_unread_counts();
                }

                let mut current = m.clone();
                current.unread = false;
                if let Some(body) = cached_body {
                    current.body = body;
                }
                self.current = Some(current.clone());
                // A new selection: whatever the last one was waiting for no
                // longer holds this one back.
                self.thread_related_pending = false;

                // The folder being read holds only its half of the conversation:
                // your own replies are filed in Sent, and older parts may have
                // been archived. Ask for the rest from the cache (#21).
                // …unless the user picked this reply out of a conversation already
                // on screen. They asked for one message; assembling its thread
                // again would swap the conversation back in under them.
                if self.threading && !solo {
                    let only = [m.clone()];
                    let ids = thread_ids(if thread.is_empty() { &only[..] } else { &thread });
                    if !ids.is_empty() {
                        self.send_to(account_id, MailRequest::LoadRelated {
                            message_id: m.id,
                            ids,
                        });
                        self.thread_related_pending = true;
                    }
                }

                if thread.len() > 1 {
                    self.thread_key = Some((account_id, m.id));
                    // Already assembled: paint it now. No lookup, no body
                    // gathering, no spinner — returning to a thread shouldn't
                    // cost what opening it did.
                    if let Some(cached) = self.thread_cache.get(&(account_id, m.id)).cloned() {
                        self.current_thread = cached;
                        // The message just opened is read, whatever the stored
                        // copy said when it was put away.
                        for tm in self.current_thread.iter_mut() {
                            if tm.id == m.id && tm.account_id == account_id {
                                tm.unread = false;
                                tm.body = current.body.clone();
                            }
                        }
                        self.thread_painted = true;
                        self.thread_related_pending = false;
                        self.show_thread();
                    } else {
                        // Conversation: assemble the thread with any cached bodies,
                        // request the rest, and render it as a scrollable conversation.
                        let mut conv: Vec<Message> = Vec::with_capacity(thread.len());
                        for tm in &thread {
                            let mut tm = tm.clone();
                            // The others keep their real unread state: each is marked
                            // in the reader until it is scrolled through. Only the one
                            // just opened is read outright.
                            if tm.id == m.id && tm.account_id == account_id {
                                tm.unread = false;
                                tm.body = current.body.clone();
                            } else if tm.body.is_empty() {
                                if let Some(b) = self.body_cache.get(&(tm.account_id, tm.id)) {
                                    tm.body = b.clone();
                                }
                            }
                            conv.push(tm);
                        }
                        self.current_thread = conv;
                        self.thread_opened_at = Some(std::time::Instant::now());
                        self.thread_painted = false;
                        // The paint happens once the conversation has settled: the
                        // lookup answered and the bodies in. Until then the reader
                        // holds its spinner.
                        self.queue_thread_render(&sender);
                        // Fetch the bodies we don't have yet (primary first via order).
                        let to_load: Vec<MissingBody> = self
                            .current_thread
                            .iter()
                            .filter(|tm| tm.body.is_empty())
                            .filter_map(|tm| {
                                self.resolve_folder_path(tm)
                                    .map(|p| (tm.account_id, tm.id, tm.uid, p))
                            })
                            .collect();
                        for ((aid, path), items) in batch_bodies_by_folder(to_load) {
                            self.send_to(aid, MailRequest::LoadBodies { items, path });
                        }
                        self.show_thread();
                    }
                } else {
                    self.current_thread.clear();
                    self.thread_key = None;
                    let display = current;
                    // Request the body FIRST so it renders before attachments — the
                    // worker processes requests in order, so the body must come first.
                    if needs_body {
                        if let Some(path) = folder_path.clone() {
                            self.send_to(account_id, MailRequest::LoadBody {
                                message_id: m.id,
                                path,
                                uid: m.uid,
                            });
                        }
                    }
                    self.show_message(Some(display), needs_body);
                }

                // Attachments: use the in-memory cache if present; otherwise ask
                // the worker to serve only from its disk cache (download = false).
                // Pre-downloaded (recent) attachments come back immediately; for
                // others the worker replies AttachmentsPending and we offer a
                // "Load attachments" button rather than fetching automatically.
                if m.has_attachment {
                    if let Some(cached) = self.attachment_cache.get(&(account_id, m.id)).cloned() {
                        self.attachments = cached;
                        self.rebuild_attach_popover(&sender);
                    } else if let Some(path) = folder_path {
                        self.send_to(account_id, MailRequest::LoadAttachments {
                            message_id: m.id,
                            path,
                            uid: m.uid,
                            download: false,
                        });
                    }
                }
            }

            AppMsg::OpenMessageWindow(m) => {
                // Drafts open in the editor rather than a read-only window.
                if self.is_drafts_folder(m.account_id, m.folder_id) {
                    self.open_draft(m, &sender);
                } else {
                    self.open_message_window(m, &sender);
                }
            }

            AppMsg::PopoutClosed(key) => {
                self.popouts.remove(&key);
            }

            AppMsg::AddContactFrom { name, email } => {
                self.show_add_contact_dialog(&name, &email, &sender);
            }

            AppMsg::LoadAttachmentsFor(m) => {
                let m = *m;
                if let Some(path) = self.resolve_folder_path(&m) {
                    self.send_to(m.account_id, MailRequest::LoadAttachments {
                        message_id: m.id,
                        path,
                        uid: m.uid,
                        download: true,
                    });
                }
            }

            AppMsg::OpenAttachmentItem(att) => {
                open_attachment(&att);
            }

            AppMsg::SaveAttachmentItems(items) => {
                save_all_attachments(items, Some(self.window.clone()));
            }

            AppMsg::ToggleStar => {
                if let Some(m) = self.current.clone() {
                    self.set_star(&m, !m.starred);
                }
            }

            AppMsg::Archive => {
                if let Some(m) = self.current.clone() {
                    self.move_to(m, FolderKind::Archive);
                }
            }
            AppMsg::Delete => {
                // More than one message selected: the trash button deletes the
                // whole selection, exactly as the bulk bar's Delete would.
                if self.list_selection.len() > 1 {
                    self.message_list.emit(MessageListInput::Bulk(BulkAction::Delete));
                    return;
                }
                if let Some(m) = self.current.clone() {
                    // In the Outbox, deleting means giving up on sending it —
                    // there is no server-side copy to move to Trash.
                    if self.outbox_item(m.account_id, m.id).is_some() {
                        self.send_to(m.account_id, MailRequest::DeleteOutbox { id: m.id });
                        self.current = None;
                        self.show_message(None, false);
                    } else {
                        self.delete_messages(vec![m], &sender);
                    }
                }
            }

            AppMsg::Related { account_id, message_id, messages } => {
                if self
                    .current
                    .as_ref()
                    .is_some_and(|c| c.account_id == account_id && c.id == message_id)
                {
                    self.thread_related_pending = false;
                }
                self.merge_related(account_id, message_id, messages);
                // One render for the settled conversation, rather than one here
                // and another for whatever this brought with it.
                if self.current_thread.len() > 1 {
                    self.queue_thread_render(&sender);
                }
            }

            AppMsg::PurgeMessages(messages) => {
                if messages.len() >= BULK_SPINNER_MIN {
                    self.message_list.emit(MessageListInput::SetBusy(Some(format!(
                        "Deleting {} messages…",
                        messages.len()
                    ))));
                    self.pending_purge = Some(messages);
                    let s = sender.clone();
                    gtk::glib::timeout_add_local_once(
                        std::time::Duration::from_millis(16),
                        move || s.input(AppMsg::BulkApply),
                    );
                } else {
                    self.purge_messages(messages);
                }
            }

            AppMsg::RowAction { action, message } => {
                let m = *message;
                if self.outbox_item(m.account_id, m.id).is_some() {
                    // Nothing else in the palette applies to an unsent message,
                    // and every other action would aim an IMAP command at a UID
                    // that doesn't exist on any server.
                    if matches!(action, RowAction::Delete) {
                        self.send_to(m.account_id, MailRequest::DeleteOutbox { id: m.id });
                    }
                    return;
                }
                match action {
                    RowAction::Reply => {
                        let m = self.with_cached_body(m);
                        self.open_compose(m.account_id, reply_prefill(&m), &sender);
                    }
                    RowAction::ReplyAll => {
                        let m = self.with_cached_body(m);
                        let self_email = self.email_of(m.account_id).unwrap_or_default();
                        self.open_compose(
                            m.account_id,
                            reply_all_prefill(&m, &self_email),
                            &sender,
                        );
                    }
                    RowAction::Forward => {
                        let m = self.with_cached_body(m);
                        self.open_compose(m.account_id, forward_prefill(&m), &sender);
                    }
                    RowAction::ToggleStar => self.set_star(&m, !m.starred),
                    RowAction::ToggleRead => self.set_read(&m, m.unread),
                    RowAction::Spam => self.mark_spam_msg(m),
                    RowAction::Archive => self.move_to(m, FolderKind::Archive),
                    RowAction::Delete => self.delete_messages(vec![m], &sender),
                    RowAction::ViewSource => {
                        if let Some(path) = self.resolve_folder_path(&m) {
                            self.send_to(m.account_id, MailRequest::LoadSource {
                                message_id: m.id,
                                path,
                                uid: m.uid,
                            });
                        }
                    }
                }
            }

            AppMsg::Bulk { action, messages } => {
                if self.showing_outbox {
                    if matches!(action, BulkAction::Delete) {
                        for m in &messages {
                            self.send_to(m.account_id, MailRequest::DeleteOutbox { id: m.id });
                        }
                    }
                    self.message_list.emit(MessageListInput::ClearSelection);
                    return;
                }
                match action {
                    // Flag/read changes update rows in place (no removal).
                    BulkAction::MarkRead => for m in &messages { self.set_read(m, true); },
                    BulkAction::MarkUnread => for m in &messages { self.set_read(m, false); },
                    BulkAction::Flag => for m in &messages { self.set_star(m, true); },
                    // Archive/Delete/Spam remove every selected row. Doing that one
                    // at a time blocks the UI thread (a render cycle per message) and
                    // trips GTK's "app is not responding" dialog for large selections.
                    // Batch it; for big selections show a spinner and defer the apply
                    // one tick so the spinner paints before the blocking work runs.
                    BulkAction::Archive | BulkAction::Spam | BulkAction::Delete => {
                        // Deleting in Trash means erasing, not moving — split the
                        // selection so each half takes the right path (and the
                        // erasures get their confirmation).
                        if matches!(action, BulkAction::Delete)
                            && messages.iter().any(|m| self.in_trash(m))
                        {
                            self.delete_messages(messages, &sender);
                            return;
                        }
                        if messages.len() >= BULK_SPINNER_MIN {
                            self.message_list.emit(MessageListInput::SetBusy(Some(
                                bulk_busy_label(action, messages.len()),
                            )));
                            self.pending_bulk = Some((action, messages));
                            let s = sender.clone();
                            gtk::glib::timeout_add_local_once(
                                std::time::Duration::from_millis(16),
                                move || s.input(AppMsg::BulkApply),
                            );
                        } else {
                            self.apply_bulk_move(action, messages);
                        }
                    }
                }
            }

            AppMsg::BulkApply => {
                if let Some(messages) = self.pending_purge.take() {
                    self.purge_messages(messages);
                }
                if let Some((action, messages)) = self.pending_bulk.take() {
                    self.apply_bulk_move(action, messages);
                }
                // The optimistic removal is done; keep the spinner up until the
                // server-side work finishes (BulkComplete). If nothing was sent,
                // clear it now.
                if self.bulk_pending == 0 {
                    self.message_list.emit(MessageListInput::SetBusy(None));
                }
            }

            AppMsg::BulkComplete => {
                self.bulk_pending = self.bulk_pending.saturating_sub(1);
                if self.bulk_pending == 0 {
                    self.message_list.emit(MessageListInput::SetBusy(None));
                }
            }

            AppMsg::Refresh => {
                if self.unified {
                    let reqs: Vec<(u32, u32, String)> = self
                        .accounts
                        .iter()
                        .filter_map(|a| self.inbox_of(a.id).map(|f| (a.id, f.id, f.path.clone())))
                        .collect();
                    for (account_id, folder_id, path) in reqs {
                        self.send_to(account_id, MailRequest::LoadMessages { folder_id, path });
                    }
                } else {
                    match self.selected.clone() {
                        Some(sel) => self.send_to(sel.account_id, MailRequest::LoadMessages {
                            folder_id: sel.folder_id,
                            path: sel.path,
                        }),
                        None => {
                            for w in self.workers.values() {
                                let _ = w.send(MailRequest::Reconnect);
                            }
                        }
                    }
                }
            }

            AppMsg::Compose => {
                let account = self.active_account();
                self.open_compose(account, ComposePrefill::default(), &sender);
            }

            AppMsg::Reply => {
                if let Some(m) = self.current.clone() {
                    self.open_inline_reply(m.account_id, reply_prefill(&m), &sender);
                }
            }

            AppMsg::ReplyAll => {
                if let Some(m) = self.current.clone() {
                    let self_email = self.email_of(m.account_id).unwrap_or_default();
                    self.open_inline_reply(
                        m.account_id,
                        reply_all_prefill(&m, &self_email),
                        &sender,
                    );
                }
            }

            AppMsg::Forward => {
                if let Some(m) = self.current.clone() {
                    self.open_inline_reply(m.account_id, forward_prefill(&m), &sender);
                }
            }

            AppMsg::AddToContacts => {
                if let Some(m) = self.current.clone() {
                    self.show_add_contact_dialog(&m.from_name, &m.from_addr, &sender);
                }
            }

            AppMsg::ContactAdded(result) => {
                use crate::contacts::AddOutcome;
                let (text, error) = match result {
                    Ok(AddOutcome::Created) => ("Added to Contacts".to_string(), false),
                    Ok(AddOutcome::Merged(name)) => (format!("Added email to {name}"), false),
                    Ok(AddOutcome::AlreadyPresent(name)) => {
                        (format!("Already in Contacts ({name})"), false)
                    }
                    Err(e) => (format!("Could not add contact: {e}"), true),
                };
                self.notifications.emit(NotifyInput::Push { text, error, connectivity: false });
            }

            AppMsg::ViewSource => {
                if let Some(m) = self.current.clone() {
                    if let Some(path) = self.resolve_folder_path(&m) {
                        self.send_to(m.account_id, MailRequest::LoadSource {
                            message_id: m.id,
                            path,
                            uid: m.uid,
                        });
                    }
                }
            }

            AppMsg::OpenAbout => {
                self.open_about(&sender);
            }

            AppMsg::AllowSender(addr) | AppMsg::AddSender(addr) => {
                let addr = addr.trim().to_lowercase();
                if !addr.is_empty() && !self.allowed_senders.contains(&addr) {
                    self.allowed_senders.push(addr);
                    self.save_settings();
                }
            }

            AppMsg::RemoveSender(addr) => {
                let addr = addr.to_lowercase();
                self.allowed_senders.retain(|s| *s != addr);
                self.save_settings();
            }

            AppMsg::AddBlacklist(addr) => {
                let addr = addr.trim().trim_start_matches('@').to_lowercase();
                if !addr.is_empty() && !self.blacklist.contains(&addr) {
                    self.blacklist.push(addr);
                    self.save_settings();
                    // Sweep mail already in view from the newly-blocked sender.
                    self.sweep_blacklisted();
                }
            }

            AppMsg::RemoveBlacklist(addr) => {
                let addr = addr.to_lowercase();
                self.blacklist.retain(|s| *s != addr);
                self.save_settings();
            }

            AppMsg::MarkSpam => self.mark_spam(),

            AppMsg::SetAvatars(on) => {
                if self.avatars != on {
                    self.avatars = on;
                    self.save_settings();
                    self.message_list.emit(MessageListInput::SetAvatars(on));
                    self.message_view.emit(MessageViewInput::SetAvatars(on));
                    for p in self.popouts.values() {
                        p.controller.emit(MessageWindowInput::SetAvatars(on));
                    }
                }
            }

            AppMsg::SetSenderLogos(on) => {
                if self.sender_logos != on {
                    self.sender_logos = on;
                    self.save_settings();
                    self.message_list.emit(MessageListInput::SetSenderLogos(on));
                    self.message_view.emit(MessageViewInput::SetSenderLogos(on));
                    for p in self.popouts.values() {
                        p.controller.emit(MessageWindowInput::SetSenderLogos(on));
                    }
                }
            }

            AppMsg::SetDimRemoteBanner(on) => {
                if self.dim_remote_banner != on {
                    self.dim_remote_banner = on;
                    self.save_settings();
                    self.message_view.emit(MessageViewInput::SetBannerStyle {
                        dim: on,
                        show: self.show_remote_banner,
                    });
                }
            }

            AppMsg::SetShowRemoteBanner(on) => {
                if self.show_remote_banner != on {
                    self.show_remote_banner = on;
                    self.save_settings();
                    self.message_view.emit(MessageViewInput::SetBannerStyle {
                        dim: self.dim_remote_banner,
                        show: on,
                    });
                }
            }

            AppMsg::SetAutoRemoteContent(on) => {
                if self.auto_remote_content != on {
                    self.auto_remote_content = on;
                    self.save_settings();
                    // Re-render what is open so the change takes effect there too:
                    // on, the blocked content loads; off, it is stripped again.
                    if self.current_thread.len() > 1 {
                        self.show_thread();
                    } else {
                        let current = self.current.clone();
                        self.show_message(current, false);
                    }
                }
            }

            AppMsg::SetDateStyle(style) => {
                if self.date_style != style {
                    self.date_style = style;
                    self.save_settings();
                    self.apply_date_style();
                }
            }

            AppMsg::SetClockStyle(style) => {
                if self.clock_style != style {
                    self.clock_style = style;
                    self.save_settings();
                    self.apply_date_style();
                }
            }

            AppMsg::SetGravatar(on) => {
                if self.gravatar != on {
                    self.gravatar = on;
                    self.save_settings();
                    self.message_list.emit(MessageListInput::SetGravatar(on));
                    // Refresh the reader's avatar for the open message.
                    let current = self.current.clone();
                    self.show_message(current, false);
                }
            }

            AppMsg::ContactPhotosChanged => {
                // Both components skip the work when sender circles are off.
                self.message_list.emit(MessageListInput::ContactPhotosChanged);
                self.message_view.emit(MessageViewInput::ContactPhotosChanged);
            }

            AppMsg::SetFetchInterval(secs) => {
                if self.fetch_interval_secs != secs {
                    self.fetch_interval_secs = secs;
                    self.save_settings();
                    self.arm_auto_fetch(&sender);
                }
            }

            AppMsg::SetPush(on) => {
                if self.push != on {
                    self.push = on;
                    self.save_settings();
                    // Workers read the push setting at startup; restart to apply.
                    self.reconnect_all(&sender);
                }
            }

            AppMsg::SetNotifications(on) => {
                if self.notifications_enabled != on {
                    self.notifications_enabled = on;
                    self.save_settings();
                }
            }

            AppMsg::SetNotificationContent(on) => {
                if self.notification_content != on {
                    self.notification_content = on;
                    self.save_settings();
                }
            }

            AppMsg::SetRunInBackground(on) => {
                if self.run_in_background.get() != on {
                    self.run_in_background.set(on);
                    self.save_settings();
                    // Ask the portal as the setting changes, so the permission
                    // dialog appears while the user is looking at the switch they
                    // just moved rather than at some later, unexplained moment.
                    crate::background::request(on && self.autostart);
                    if !on {
                        crate::background::set_status("");
                    }
                }
            }

            AppMsg::SetAutostart(on) => {
                if self.autostart != on {
                    self.autostart = on;
                    self.save_settings();
                    crate::background::request(self.run_in_background.get() && on);
                }
            }

            AppMsg::SetSingleKey(on) => {
                if self.single_key.get() != on {
                    self.single_key.set(on);
                    self.save_settings();
                }
            }

            AppMsg::ShowShortcuts => self.show_shortcuts(),

            AppMsg::PrintPreview | AppMsg::PrintMessage => {
                // Only the reader can print: it holds the rendered message, and
                // printing from an empty reader would offer a blank page. Say so
                // rather than appearing to do nothing, which is indistinguishable
                // from a broken menu item.
                let preview = matches!(msg, AppMsg::PrintPreview);
                tracing::info!(preview, open = self.current.is_some(), "print requested");
                if self.current.is_none() {
                    self.notifications.emit(NotifyInput::Push {
                        text: "Open a message first, then print it.".to_string(),
                        error: false,
                        connectivity: false,
                    });
                    return;
                }
                self.message_view.emit(if preview {
                    MessageViewInput::PrintPreview
                } else {
                    MessageViewInput::Print
                });
            }

            AppMsg::Shortcut(action) => self.run_shortcut(action, &sender),

            AppMsg::SetPreviewLines(lines) => {
                if self.preview_lines != lines {
                    self.preview_lines = lines;
                    self.save_settings();
                    self.message_list.emit(MessageListInput::SetPreviewLines(lines));
                }
            }

            AppMsg::SetShowAttachments(on) => {
                if self.show_attachments != on {
                    self.show_attachments = on;
                    self.save_settings();
                    self.sidebar.emit(SidebarInput::SetShowAttachments(on));
                }
            }

            AppMsg::SetThreading(on) => {
                if self.threading != on {
                    self.threading = on;
                    self.save_settings();
                    self.message_list.emit(MessageListInput::SetThreading(on));
                }
            }

            AppMsg::SetThreadsExpanded(on) => {
                if self.threads_expanded != on {
                    self.threads_expanded = on;
                    self.save_settings();
                    self.message_list.emit(MessageListInput::SetThreadsExpanded(on));
                }
            }

            AppMsg::SetPaletteCollapse(secs) => {
                if self.palette_collapse_secs != secs {
                    self.palette_collapse_secs = secs;
                    self.save_settings();
                    self.message_list.emit(MessageListInput::SetPaletteCollapse(secs));
                }
            }

            AppMsg::SetMessageTheme(theme) => {
                if self.message_theme != theme {
                    self.message_theme = theme;
                    self.save_settings();
                    let dark = theme.dark_override();
                    // Message content only — the reader and any popped-out windows.
                    self.message_view.emit(MessageViewInput::SetContentTheme(dark));
                    for p in self.popouts.values() {
                        p.controller.emit(MessageWindowInput::SetContentTheme(dark));
                    }
                }
            }

            AppMsg::ComposeTo(addr) => {
                let account = self
                    .current
                    .as_ref()
                    .map(|m| m.account_id)
                    .unwrap_or_else(|| self.active_account());
                let prefill = ComposePrefill {
                    to: addr,
                    ..Default::default()
                };
                self.open_compose(account, prefill, &sender);
            }

            AppMsg::SendMessage(out) => {
                let account_id = out.from_account_id;
                let sent_path = self
                    .folders
                    .get(&account_id)
                    .and_then(|fs| fs.iter().find(|f| f.kind == FolderKind::Sent))
                    .map(|f| f.path.clone());
                self.send_to(account_id, MailRequest::Send { message: out, sent_path });
            }

            AppMsg::SaveDraftMessage(out) => {
                let account_id = out.from_account_id;
                // Existing Drafts folder, else a default path (worker creates it).
                let drafts = self
                    .folders
                    .get(&account_id)
                    .and_then(|fs| fs.iter().find(|f| f.kind == FolderKind::Drafts))
                    .map(|f| (f.id, f.path.clone()))
                    .or_else(|| {
                        self.default_folder_path(account_id, FolderKind::Drafts)
                            .map(|p| (0, p))
                    });
                let Some((folder_id, path)) = drafts else {
                    self.notifications.emit(NotifyInput::Push {
                        text: "No Drafts folder available for this account".to_string(),
                        error: true,
                        connectivity: false,
                    });
                    return;
                };
                self.send_to(account_id, MailRequest::SaveDraft { message: out, folder_id, path });
            }

            AppMsg::DraftSaved => {
                // The Drafts folder reload already reflects the saved draft; the
                // compose window has closed. No notification (mirrors silent send).
            }

            AppMsg::ComposeClosed(id) => {
                self.close_compose(id);
                self.message_list.emit(MessageListInput::FocusList);
            }

            AppMsg::ComposeToggleWindow(id) => self.toggle_compose_window(id, &sender),

            AppMsg::Sent { account_id } => {
                // No success notification — only send failures are surfaced (via
                // WorkerEvent::Error). Just refresh the Sent folder if it's open.
                // Reload Sent if it's the open folder for that account.
                if let Some(sel) = self.selected.clone() {
                    let viewing_sent = sel.account_id == account_id
                        && self
                            .folders
                            .get(&account_id)
                            .is_some_and(|fs| fs.iter().any(|f| f.id == sel.folder_id && f.kind == FolderKind::Sent));
                    if viewing_sent {
                        self.send_to(account_id, MailRequest::LoadMessages {
                            folder_id: sel.folder_id,
                            path: sel.path,
                        });
                    }
                }
            }

            AppMsg::OpenAccounts => self.open_accounts_window(&sender, false),

            AppMsg::AddFirstAccount => self.open_accounts_window(&sender, true),

            AppMsg::AccountSaved { original_email, account } => {
                let new_email = account.email.clone();
                // Remember the secret we expect to persist, so we can verify the
                // keyring actually stored it (a silent keyring failure would
                // otherwise leave the account unable to log in after a restart).
                let expected_secret = (!account.password.is_empty())
                    .then(|| account.password.clone());
                match original_email {
                    // Editing an existing account (matched by its previous email).
                    Some(orig) => {
                        if let Some(slot) = self.config.iter_mut().find(|c| c.email == orig) {
                            *slot = *account;
                        } else {
                            self.config.push(*account);
                        }
                        // Track an email change in the display-order/collapsed lists.
                        if orig != new_email {
                            for e in self.account_order.iter_mut().chain(self.collapsed.iter_mut()) {
                                if *e == orig {
                                    *e = new_email.clone();
                                }
                            }
                        }
                    }
                    // Adding a new account.
                    None => self.config.push(*account),
                }
                match config::save(&self.config) {
                    Ok(()) => {
                        // config::save() only logs keyring errors, so confirm the
                        // password can actually be read back. If not, the Secret
                        // Service isn't persisting it — tell the user how to fix it
                        // instead of silently "saving" an account that won't stay
                        // logged in.
                        if let Some(secret) = expected_secret {
                            if config::load_password(&new_email).as_deref() != Some(secret.as_str()) {
                                sender.input(AppMsg::ShowKeyringHelp { problem: true });
                            }
                        }
                        self.save_sidebar_state();
                        self.reconnect_all(&sender);
                    }
                    Err(e) => self.notifications.emit(NotifyInput::Push {
                        text: format!("Could not save account: {e}"),
                        error: true,
                        connectivity: false,
                    }),
                }
            }

            AppMsg::ShowKeyringHelp { problem } => self.show_keyring_help(problem),

            AppMsg::AccountEnabledChanged { email, enabled } => {
                if let Some(slot) = self.config.iter_mut().find(|c| c.email == email) {
                    if slot.enabled != enabled {
                        slot.enabled = enabled;
                        if let Err(e) = config::save(&self.config) {
                            self.notifications.emit(NotifyInput::Push {
                                text: format!("Could not save account: {e}"),
                                error: true,
                                connectivity: false,
                            });
                        }
                        // Respawn workers so the account starts/stops syncing.
                        self.reconnect_all(&sender);
                    }
                }
            }

            AppMsg::ImportGoaAccount(account) => {
                // Enable a GNOME Online Account in Vireo (or re-enable if already
                // imported). Its password came from GOA and is stored in the keyring.
                let email = account.email.clone();
                if let Some(slot) = self.config.iter_mut().find(|c| c.email == email) {
                    slot.enabled = true;
                } else {
                    self.config.push(*account);
                }
                match config::save(&self.config) {
                    Ok(()) => self.reconnect_all(&sender),
                    Err(e) => self.notifications.emit(NotifyInput::Push {
                        text: format!("Could not import account: {e}"),
                        error: true,
                        connectivity: false,
                    }),
                }
            }

            AppMsg::AccountRemoved { email } => {
                self.config.retain(|c| c.email != email);
                config::delete_password(&email);
                self.account_order.retain(|e| *e != email);
                self.collapsed.retain(|e| *e != email);
                if let Err(e) = config::save(&self.config) {
                    tracing::error!("could not save config: {e}");
                }
                self.save_sidebar_state();
                self.reconnect_all(&sender);
            }

            AppMsg::GoaChanged(live) => {
                // GOA changed in GNOME Settings. Drop any imported account that
                // no longer exists there; pause/resume any whose Mail service
                // was toggled. (Adding an account to GOA never auto-imports —
                // that stays a manual choice.)
                let outcome = reconcile_goa(&mut self.config, &live);
                if !outcome.removed.is_empty() {
                    for email in &outcome.removed {
                        config::delete_password(email);
                        self.account_order.retain(|e| e != email);
                        self.collapsed.retain(|e| e != email);
                        self.folders_expanded.retain(|e| e != email);
                    }
                    self.save_sidebar_state();
                }
                if !outcome.removed.is_empty() || outcome.paused_changed {
                    if let Err(e) = config::save(&self.config) {
                        tracing::error!("could not save config after GOA change: {e}");
                    }
                    self.reconnect_all(&sender);
                }
            }

            AppMsg::SystemResumed => {
                // Sockets left open across suspend are dead. Reconnect drops the
                // stale session, logs in fresh and re-arms IMAP IDLE — and it
                // unsticks any worker parked inside an IDLE wait, since the
                // request breaks its select loop. Then reload the visible folder
                // so new mail appears without waiting for the next auto-fetch.
                for w in self.workers.values() {
                    let _ = w.send(MailRequest::Reconnect);
                }
                sender.input(AppMsg::Refresh);
                // Realign the auto-fetch timer to now; its monotonic countdown
                // did not advance during sleep.
                self.arm_auto_fetch(&sender);
            }

            AppMsg::CloseAccounts => self.accounts_win = None,

            AppMsg::OpenPreferences => {
                // Already open? Bring it forward instead of opening another.
                if let Some(p) = self.prefs.as_ref().filter(|p| p.widget().is_visible()) {
                    p.widget().present();
                    return;
                }
                let init = PrefInit {
                    allowed_senders: self.allowed_senders.clone(),
                    auto_remote_content: self.auto_remote_content,
                    dim_remote_banner: self.dim_remote_banner,
                    show_remote_banner: self.show_remote_banner,
                    gravatar: self.gravatar,
                    avatars: self.avatars,
                    sender_logos: self.sender_logos,
                    date_style: self.date_style,
                    clock_style: self.clock_style,
                    fetch_interval_secs: self.fetch_interval_secs,
                    push: self.push,
                    blacklist: self.blacklist.clone(),
                    palette_collapse_secs: self.palette_collapse_secs,
                    threading: self.threading,
                    threads_expanded: self.threads_expanded,
                    message_theme: self.message_theme,
                    notifications: self.notifications_enabled,
                    notification_content: self.notification_content,
                    show_attachments: self.show_attachments,
                    preview_lines: self.preview_lines,
                    single_key_shortcuts: self.single_key.get(),
                    run_in_background: self.run_in_background.get(),
                    autostart: self.autostart,
                };
                let prefs = Preferences::builder()
                    .transient_for(&self.window)
                    .launch(init)
                    .forward(sender.input_sender(), |out| match out {
                        PrefOutput::AddSender(addr) => AppMsg::AddSender(addr),
                        PrefOutput::RemoveSender(addr) => AppMsg::RemoveSender(addr),
                        PrefOutput::AddBlacklist(addr) => AppMsg::AddBlacklist(addr),
                        PrefOutput::RemoveBlacklist(addr) => AppMsg::RemoveBlacklist(addr),
                        PrefOutput::SetAutoRemoteContent(on) => AppMsg::SetAutoRemoteContent(on),
                        PrefOutput::SetDimRemoteBanner(on) => AppMsg::SetDimRemoteBanner(on),
                        PrefOutput::SetShowRemoteBanner(on) => AppMsg::SetShowRemoteBanner(on),
                        PrefOutput::SetGravatar(on) => AppMsg::SetGravatar(on),
                        PrefOutput::SetAvatars(on) => AppMsg::SetAvatars(on),
                        PrefOutput::SetSenderLogos(on) => AppMsg::SetSenderLogos(on),
                        PrefOutput::SetDateStyle(style) => AppMsg::SetDateStyle(style),
                        PrefOutput::SetClockStyle(style) => AppMsg::SetClockStyle(style),
                        PrefOutput::SetThreading(on) => AppMsg::SetThreading(on),
                        PrefOutput::SetThreadsExpanded(on) => AppMsg::SetThreadsExpanded(on),
                        PrefOutput::SetFetchInterval(secs) => AppMsg::SetFetchInterval(secs),
                        PrefOutput::SetPush(on) => AppMsg::SetPush(on),
                        PrefOutput::SetNotifications(on) => AppMsg::SetNotifications(on),
                        PrefOutput::SetNotificationContent(on) => {
                            AppMsg::SetNotificationContent(on)
                        }
                        PrefOutput::SetShowAttachments(on) => AppMsg::SetShowAttachments(on),
                        PrefOutput::SetPreviewLines(n) => AppMsg::SetPreviewLines(n),
                        PrefOutput::SetSingleKey(on) => AppMsg::SetSingleKey(on),
                        PrefOutput::SetRunInBackground(on) => AppMsg::SetRunInBackground(on),
                        PrefOutput::SetAutostart(on) => AppMsg::SetAutostart(on),
                        PrefOutput::SetPaletteCollapse(secs) => AppMsg::SetPaletteCollapse(secs),
                        PrefOutput::SetMessageTheme(t) => AppMsg::SetMessageTheme(t),
                        PrefOutput::Closed => AppMsg::ClosePreferences,
                    });
                prefs.widget().present();
                self.prefs = Some(prefs);
            }

            AppMsg::ClosePreferences => self.prefs = None,

            AppMsg::SetAccount(account) => {
                if let Some(existing) = self.accounts.iter_mut().find(|a| a.id == account.id) {
                    *existing = account;
                } else {
                    self.accounts.push(account);
                    self.accounts.sort_by_key(|a| a.id);
                }
                self.rebuild_sidebar();
            }

            AppMsg::SetFolders { account_id, folders } => {
                self.notifications.emit(NotifyInput::ClearConnectivity);
                for f in &folders {
                    self.folder_unread.insert((account_id, f.id), f.unread);
                }
                self.folders.insert(account_id, folders);
                self.rebuild_sidebar();
            }

            AppMsg::FolderUnread { account_id, folder_id, unread } => {
                self.folder_unread.insert((account_id, folder_id), unread);
                self.push_unread_counts();
            }

            AppMsg::Messages { account_id, folder_id, messages } => {
                self.notifications.emit(NotifyInput::ClearConnectivity);
                // Auto-delete blacklisted senders from the inbox before anything
                // else sees them.
                let messages = self.apply_blacklist(account_id, folder_id, messages);
                // Did this sync remove the message currently open in the reader
                // (deleted/moved on another device)? Scope the check to the reader's
                // own folder so a folder switch or another folder's sync doesn't
                // count. Capture where it sat so we can advance to that slot.
                let vanished = self.current.as_ref().is_some_and(|c| {
                    c.account_id == account_id
                        && c.folder_id == folder_id
                        && !messages.iter().any(|m| m.uid == c.uid)
                });
                let next_after_vanish = if vanished {
                    let cur_uid = self.current.as_ref().unwrap().uid;
                    next_after_vanish(
                        self.message_cache.get(&(account_id, folder_id)),
                        &messages,
                        cur_uid,
                    )
                } else {
                    None
                };
                // Desktop-notify for genuinely new inbox mail. Only when Vireo
                // isn't the active window (no point notifying about mail you're
                // watching arrive), only for the Inbox, and never on the first load
                // of a folder (no prior cache) — that would fire for every existing
                // message on startup. "New" = unread and not in the previous sync.
                if self.notifications_enabled && !self.window.is_active() {
                    let is_inbox = self.folder_kind(account_id, folder_id) == Some(FolderKind::Inbox);
                    if let (true, Some(old)) =
                        (is_inbox, self.message_cache.get(&(account_id, folder_id)))
                    {
                        let old_uids: std::collections::HashSet<u32> =
                            old.iter().map(|m| m.uid).collect();
                        let fresh: Vec<&Message> = messages
                            .iter()
                            .filter(|m| m.unread && !old_uids.contains(&m.uid))
                            .collect();
                        if let Some(newest) = fresh.iter().max_by_key(|m| m.timestamp) {
                            crate::notify::new_mail(
                                account_id,
                                folder_id,
                                newest.id,
                                &newest.from_name,
                                &newest.subject,
                                fresh.len() - 1,
                            );
                        }
                    }
                }
                // Cache for instant display when revisiting this folder.
                self.message_cache
                    .insert((account_id, folder_id), messages.clone());
                // A sync can add a reply to a conversation already assembled, so
                // what was stored is no longer necessarily the conversation.
                self.forget_threads(account_id);
                self.push_thread_links();
                if self.unified {
                    // Accept only each account's inbox; merge all by recency.
                    if self.inbox_of(account_id).map(|f| f.id) == Some(folder_id) {
                        self.unified_by_account.insert(account_id, messages);
                        self.emit_unified();
                    }
                } else if let Some(sel) = self.selected.as_ref() {
                    if sel.account_id == account_id && sel.folder_id == folder_id {
                        let title = sel.name.clone();
                        self.message_list
                            .emit(MessageListInput::SetMessages { title, messages });
                    }
                }
                // The reader's message was removed by this sync (deleted/moved on
                // another device). Clear it right away so nothing stale lingers, then
                // advance to whatever now sits in its place if there is one — the
                // SelectAndLoad runs after SetMessages, so the target row exists.
                if vanished {
                    sender.input(AppMsg::ClearReader);
                    if let Some(key) = next_after_vanish {
                        self.message_list.emit(MessageListInput::SelectAndLoad(key));
                    }
                }
                // Refresh unread badges with the freshly-synced counts.
                self.push_unread_counts();
            }

            AppMsg::MessagesAppend { account_id, folder_id, messages } => {
                // Background backfill: grow the folder's search index without
                // disturbing the current view (no title/query reset).
                let messages = self.apply_blacklist(account_id, folder_id, messages);
                let entry = self.message_cache.entry((account_id, folder_id)).or_default();
                let existing: std::collections::HashSet<u32> =
                    entry.iter().map(|m| m.uid).collect();
                let fresh: Vec<Message> = messages
                    .into_iter()
                    .filter(|m| !existing.contains(&m.uid))
                    .collect();
                if fresh.is_empty() {
                    return;
                }
                entry.extend(fresh.iter().cloned());
                // Feed the visible list so search covers the new messages live.
                if self.unified {
                    if self.inbox_of(account_id).map(|f| f.id) == Some(folder_id) {
                        self.unified_by_account
                            .entry(account_id)
                            .or_default()
                            .extend(fresh.iter().cloned());
                        self.message_list
                            .emit(MessageListInput::AppendMessages { messages: fresh });
                    }
                } else if let Some(sel) = self.selected.as_ref() {
                    if sel.account_id == account_id && sel.folder_id == folder_id {
                        self.message_list
                            .emit(MessageListInput::AppendMessages { messages: fresh });
                    }
                }
            }

            AppMsg::BackfillDone { account_id, folder_id } => {
                self.indexed_folders.insert((account_id, folder_id));
                self.push_index_complete();
            }

            AppMsg::SenderChecked { account_id, message_id, check } => {
                // Remember it: prefetch delivers the verdict long before the
                // message is opened, and opening it renders from the in-memory
                // body cache without asking the worker for anything.
                self.sender_cache
                    .insert((account_id, message_id), check.clone());
                // Only the message actually on screen; a verdict that arrives
                // from a background prefetch must not relabel a different one.
                if self
                    .current
                    .as_ref()
                    .is_some_and(|c| c.id == message_id && c.account_id == account_id)
                {
                    self.message_view
                        .emit(MessageViewInput::SetSenderCheck(check.clone()));
                }
                if let Some(p) = self.popouts.get(&(account_id, message_id)) {
                    p.controller.emit(MessageWindowInput::SetSenderCheck(check));
                }
            }

            AppMsg::Body { account_id, message_id, path, body } => {
                self.body_cache
                    .insert((account_id, message_id), body.clone());
                // If this body was fetched to open a draft, open the editor now.
                if let Some(pd) = self.pending_draft.take() {
                    if pd.account_id == account_id && pd.id == message_id {
                        self.compose_from_draft(pd, body, &sender);
                        return;
                    }
                    self.pending_draft = Some(pd);
                }
                // A UID is unique only within its folder, and the background
                // prefetch pushes bodies from every folder it syncs. Matching on
                // the number alone would let one folder's body overwrite a
                // different message that happens to share it.
                let folder = self
                    .folders
                    .get(&account_id)
                    .and_then(|fs| fs.iter().find(|f| f.path == path))
                    .map(|f| f.id);
                let is_target = |m: &Message| {
                    m.account_id == account_id
                        && m.id == message_id
                        && folder.is_none_or(|fid| m.folder_id == fid)
                };
                // Keep the primary's body up to date in either mode.
                if let Some(current) = self.current.as_mut() {
                    if current.account_id == account_id
                        && current.id == message_id
                        && folder.is_none_or(|fid| current.folder_id == fid)
                    {
                        current.body = body.clone();
                    }
                }
                if self.current_thread.len() > 1 {
                    // Conversation mode: fill the matching message's body. The
                    // whole conversation renders as one document holding every
                    // body, so a burst of arrivals is coalesced into a single
                    // re-render rather than one per body.
                    let mut changed = false;
                    for tm in self.current_thread.iter_mut() {
                        if is_target(tm) {
                            tm.body = body.clone();
                            changed = true;
                        }
                    }
                    if changed {
                        self.queue_thread_render(&sender);
                    }
                } else if self.current.as_ref().is_some_and(is_target) {
                    let current = self.current.clone();
                    self.show_message(current, false);
                }
                // Re-render any popped-out window showing this message.
                if let Some(p) = self.popouts.get(&(account_id, message_id)) {
                    p.controller.emit(MessageWindowInput::SetBody(body));
                }
            }

            AppMsg::RefsRepaired { account_id, folder_id } => {
                // Only the folder on screen, and only its cached copy: the repair
                // rewrote rows on disk, and re-reading is what lets the list see
                // the conversations they now form.
                let showing = self
                    .selected
                    .as_ref()
                    .is_some_and(|sel| sel.account_id == account_id && sel.folder_id == folder_id);
                if showing || self.unified {
                    if let Some(path) = self
                        .folders
                        .get(&account_id)
                        .and_then(|fs| fs.iter().find(|f| f.id == folder_id))
                        .map(|f| f.path.clone())
                    {
                        self.send_to(account_id, MailRequest::LoadMessages { folder_id, path });
                    }
                }
            }

            AppMsg::SelectionKeys(keys) => {
                self.list_selection = keys.clone();
                self.message_view.emit(MessageViewInput::SetSelectedCards(keys));
            }

            AppMsg::SelectCards(keys) => {
                self.list_selection = keys.clone();
                self.message_list.emit(MessageListInput::SelectFromReader { keys });
            }

            AppMsg::ThreadMessageSeen { account_id, id } => {
                // Reading one message of a conversation marks only that message,
                // exactly as opening it on its own would.
                let target = self
                    .current_thread
                    .iter_mut()
                    .find(|m| m.account_id == account_id && m.id == id)
                    .filter(|m| m.unread)
                    .map(|m| {
                        m.unread = false;
                        (m.uid, m.folder_id)
                    });
                let Some((uid, folder_id)) = target else { return };
                if let Some(path) = self
                    .folders
                    .get(&account_id)
                    .and_then(|fs| fs.iter().find(|f| f.id == folder_id))
                    .map(|f| f.path.clone())
                {
                    self.send_to(account_id, MailRequest::SetSeen { path, uid, seen: true });
                }
                self.message_list.emit(MessageListInput::MarkRead(id));
                self.mark_cached_read(account_id, id);
                if let Some(n) = self.folder_unread.get_mut(&(account_id, folder_id)) {
                    *n = n.saturating_sub(1);
                }
                self.remember_thread();
                self.push_unread_counts();
            }

            AppMsg::RenderThread => {
                self.thread_render_queued = false;
                if self.current_thread.len() > 1 {
                    // Still assembling and still within the grace period: come
                    // back rather than paint a conversation that is about to be
                    // replaced. The re-queue is what makes the deadline fire.
                    let unsettled = self.current_thread.iter().any(|m| m.body.is_empty())
                        || self.thread_related_pending;
                    let waiting = self
                        .thread_opened_at
                        .is_some_and(|opened| opened.elapsed() < THREAD_BODY_WAIT);
                    if unsettled && waiting {
                        self.queue_thread_render(&sender);
                        return;
                    }
                    self.thread_painted = true;
                    self.show_thread();
                    self.remember_thread();
                }
            }

            AppMsg::Source { text } => {
                // Source is only fetched on explicit request (toolbar or context
                // menu), so always show it — even for a message that isn't open.
                self.show_source_window(&text);
            }

            AppMsg::Attachments { account_id, message_id, items } => {
                self.attachment_cache
                    .insert((account_id, message_id), items.clone());
                if self
                    .current
                    .as_ref()
                    .is_some_and(|c| c.id == message_id && c.account_id == account_id)
                {
                    self.attachments_loading = false;
                    self.attachments_available = false;
                    self.attachments = items.clone();
                    self.rebuild_attach_popover(&sender);
                }
                if let Some(p) = self.popouts.get(&(account_id, message_id)) {
                    p.controller.emit(MessageWindowInput::SetAttachments(items));
                }
            }

            AppMsg::AttachmentsPending { account_id, message_id } => {
                // Attachments exist but aren't downloaded; offer the load button.
                if self
                    .current
                    .as_ref()
                    .is_some_and(|c| c.id == message_id && c.account_id == account_id)
                {
                    self.attachments_loading = false;
                    self.attachments_available = true;
                }
                if let Some(p) = self.popouts.get(&(account_id, message_id)) {
                    p.controller.emit(MessageWindowInput::AttachmentsPending);
                }
            }

            AppMsg::NoAttachments { account_id, message_id } => {
                // Clear a false paperclip live. Update every cached folder for the
                // account (a UID is per-folder, but the same message copied across
                // folders shares its attachment status) and the visible row.
                for ((aid, _), msgs) in self.message_cache.iter_mut() {
                    if *aid == account_id {
                        for m in msgs.iter_mut().filter(|m| m.id == message_id) {
                            m.has_attachment = false;
                        }
                    }
                }
                if let Some(c) = self.current.as_mut() {
                    if c.id == message_id && c.account_id == account_id {
                        c.has_attachment = false;
                    }
                }
                self.message_list
                    .emit(MessageListInput::SetHasAttachment { id: message_id, has: false });
            }

            AppMsg::HasAttachments { account_id, message_id } => {
                // The mirror of NoAttachments: a message whose structure didn't
                // advertise its attachments (an inline PDF, say — issue #9) is
                // given its paperclip once the body has proved they are there.
                for ((aid, _), msgs) in self.message_cache.iter_mut() {
                    if *aid == account_id {
                        for m in msgs.iter_mut().filter(|m| m.id == message_id) {
                            m.has_attachment = true;
                        }
                    }
                }
                self.message_list
                    .emit(MessageListInput::SetHasAttachment { id: message_id, has: true });
                // If it is the message on screen, fetch the files too: the reader
                // only asks for them when the flag was already set, which by
                // definition it wasn't.
                let already = self
                    .current
                    .as_ref()
                    .is_some_and(|c| c.id == message_id && c.account_id == account_id && c.has_attachment);
                let open = self
                    .current
                    .as_mut()
                    .filter(|c| c.id == message_id && c.account_id == account_id);
                if let (false, Some(current)) = (already, open) {
                    current.has_attachment = true;
                    let message = current.clone();
                    if let Some(cached) =
                        self.attachment_cache.get(&(account_id, message_id)).cloned()
                    {
                        self.attachments = cached;
                        self.rebuild_attach_popover(&sender);
                    } else if let Some(path) = self.resolve_folder_path(&message) {
                        self.send_to(account_id, MailRequest::LoadAttachments {
                            message_id,
                            path,
                            uid: message.uid,
                            download: false,
                        });
                    }
                }
            }

            AppMsg::LoadAttachmentsNow => {
                if let Some(m) = self.current.clone() {
                    if let Some(path) = self.resolve_folder_path(&m) {
                        self.attachments_available = false;
                        self.attachments_loading = true;
                        self.send_to(m.account_id, MailRequest::LoadAttachments {
                            message_id: m.id,
                            path,
                            uid: m.uid,
                            download: true,
                        });
                    }
                }
            }

            AppMsg::OpenAttachment(i) => {
                if let Some(att) = self.attachments.get(i) {
                    open_attachment(att);
                }
            }

            AppMsg::SaveAllAttachments => {
                save_all_attachments(self.attachments.clone(), Some(self.window.clone()));
            }

            AppMsg::Status { account_id, text } => {
                if text.is_empty() {
                    self.busy.remove(&account_id);
                } else {
                    self.busy.insert(account_id);
                }
                self.notifications.emit(NotifyInput::SetStatus(text));
            }

            AppMsg::Error { account_id, text, connectivity } => {
                tracing::error!("[account {account_id}] {text}");
                let label = self.account_label(account_id);
                // Desktop-notify only genuine failures (not transient connectivity
                // blips that auto-recover), and only when unfocused — the in-app bar
                // already surfaces it while you're looking.
                if self.notifications_enabled && !connectivity && !self.window.is_active() {
                    crate::notify::error(account_id, &format!("{label}: mail error"), &text);
                }
                self.notifications.emit(NotifyInput::Push {
                    text: format!("{label}: {text}"),
                    error: true,
                    connectivity,
                });
            }

            AppMsg::NotifyCount(n) => self.notify_count = n,
            AppMsg::ToggleNotifications => self.notifications.emit(NotifyInput::TogglePanel),

            AppMsg::OpenContacts => self.show_contacts_window(&sender),
        }
    }
}

/// How long a conversation waits for its outstanding bodies before painting what
/// it has. Long enough that a cached conversation — the common case — always
/// arrives whole, short enough that one unreachable message can't hold the
/// reader hostage.
const THREAD_BODY_WAIT: std::time::Duration = std::time::Duration::from_millis(1200);

/// How many messages' reply headers are held for joining conversations across
/// folders. Only a bound on memory and rebuild cost — never on correctness: a
/// conversation whose members are all on screen groups without consulting these
/// at all. They matter for the join that runs through a message that isn't
/// shown, most often your own reply in Sent. The newest are kept, since that is
/// where the mail being read lives.
const THREAD_LINK_LIMIT: usize = 20_000;



impl AppModel {
    /// Push the date and clock preference into the formatter and redraw whatever
    /// shows a date: every row carries one, as does the open message.
    fn apply_date_style(&self) {
        crate::datefmt::set_style(self.date_style, self.clock_style);
        self.message_list.emit(MessageListInput::RefreshDates);
        let current = self.current.clone();
        self.show_message(current, false);
    }

    /// Persist all app settings together.
    fn save_settings(&self) {
        config::save_privacy(
            &self.allowed_senders,
            self.auto_remote_content,
            self.gravatar,
            self.avatars,
            self.sender_logos,
            self.date_style,
            self.clock_style,
            self.fetch_interval_secs,
            self.push,
            &self.blacklist,
            self.palette_collapse_secs,
            self.threading,
            self.threads_expanded,
            self.message_theme,
            self.notifications_enabled,
            self.notification_content,
            self.show_attachments,
            self.preview_lines,
            self.single_key.get(),
            self.run_in_background.get(),
            self.autostart,
            self.dim_remote_banner,
            self.show_remote_banner,
        );
    }

    /// Carry out a single-key shortcut.
    fn run_shortcut(&mut self, action: Shortcut, sender: &ComponentSender<Self>) {
        match action {
            Shortcut::NextMessage => self.message_list.emit(MessageListInput::MoveSelection(1)),
            Shortcut::PrevMessage => self.message_list.emit(MessageListInput::MoveSelection(-1)),
            Shortcut::ToggleSelect => self.message_list.emit(MessageListInput::ToggleSelection),
            Shortcut::BackToList => self.message_list.emit(MessageListInput::FocusList),
            Shortcut::Search => self.message_list.emit(MessageListInput::FocusSearch),
            Shortcut::OpenMessage => {
                if let Some(view) = self.message_view.widget().first_child() {
                    view.grab_focus();
                }
            }
            Shortcut::NextInThread => self.step_thread(1),
            Shortcut::PrevInThread => self.step_thread(-1),
            Shortcut::Reply => sender.input(AppMsg::Reply),
            Shortcut::ReplyAll => sender.input(AppMsg::ReplyAll),
            Shortcut::Forward => sender.input(AppMsg::Forward),
            Shortcut::Archive => sender.input(AppMsg::Archive),
            Shortcut::Delete => sender.input(AppMsg::Delete),
            Shortcut::Spam => sender.input(AppMsg::MarkSpam),
            Shortcut::Star => sender.input(AppMsg::ToggleStar),
            Shortcut::ToggleRead => {
                if let Some(m) = self.current.clone() {
                    let read = !m.unread;
                    self.set_read(&m, !read);
                }
            }
            Shortcut::Compose => sender.input(AppMsg::Compose),
            Shortcut::Shortcuts => self.show_shortcuts(),
        }
    }

    /// Move to the next (or previous) message of the open conversation.
    fn step_thread(&mut self, delta: i32) {
        let Some(current) = self.current.clone() else {
            return;
        };
        let thread = self.current_thread.clone();
        let Some(index) = thread
            .iter()
            .position(|m| m.id == current.id && m.account_id == current.account_id)
        else {
            // Not in a conversation: fall back to moving through the list, which
            // is what someone pressing "next" almost certainly meant.
            self.message_list.emit(MessageListInput::MoveSelection(delta));
            return;
        };
        let next = index as i32 + delta;
        if next < 0 || next as usize >= thread.len() {
            return;
        }
        let target = &thread[next as usize];
        self.message_list
            .emit(MessageListInput::SelectAndLoad((target.account_id, target.id)));
    }

    /// Open the keyboard-shortcut reference, or close it if it is already up:
    /// the key that summons a cheatsheet is the obvious one to dismiss it with.
    fn show_shortcuts(&mut self) {
        if let Some(win) = self.shortcuts_win.take() {
            if win.is_visible() {
                win.close();
                return;
            }
            // Closed from its own titlebar; fall through and open a fresh one.
        }
        self.shortcuts_win = Some(self.build_shortcuts_window());
    }

    /// A plain window listing every single-key shortcut.
    fn build_shortcuts_window(&self) -> adw::Window {
        let win = adw::Window::builder()
            .transient_for(&self.window)
            .modal(true)
            .title("Keyboard Shortcuts")
            .default_width(420)
            .default_height(560)
            .build();

        let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        page.set_margin_top(12);
        page.set_margin_bottom(18);
        page.set_margin_start(18);
        page.set_margin_end(18);

        if !self.single_key.get() {
            let off = gtk::Label::new(Some(
                "Single-key shortcuts are switched off. Turn them on in Preferences → Message List.",
            ));
            off.add_css_class("dim-label");
            off.set_wrap(true);
            off.set_xalign(0.0);
            off.set_margin_bottom(12);
            page.append(&off);
        }

        for (section, keys) in SHORTCUT_HELP {
            let title = gtk::Label::new(Some(section));
            title.add_css_class("heading");
            title.set_halign(gtk::Align::Start);
            title.set_margin_top(14);
            title.set_margin_bottom(6);
            page.append(&title);

            let list = gtk::ListBox::new();
            list.add_css_class("boxed-list");
            list.set_selection_mode(gtk::SelectionMode::None);
            for (key, what) in *keys {
                let row = adw::ActionRow::builder().title(*what).build();
                let label = gtk::Label::new(Some(key));
                label.add_css_class("shortcut-key");
                label.set_valign(gtk::Align::Center);
                row.add_suffix(&label);
                list.append(&row);
            }
            page.append(&list);
        }

        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&page)
            .build();
        let tv = adw::ToolbarView::new();
        tv.add_top_bar(&adw::HeaderBar::new());
        tv.set_content(Some(&scroller));
        win.set_content(Some(&tv));

        // Escape closes it, as does the accelerator that opened it. A reference
        // you can't dismiss with the key you opened it with is a nuisance.
        let keys = gtk::EventControllerKey::new();
        let closer = win.clone();
        keys.connect_key_pressed(move |_, keyval, _, state| {
            let ctrl = state.contains(gtk::gdk::ModifierType::CONTROL_MASK);
            let toggles = ctrl
                && matches!(keyval, gtk::gdk::Key::question | gtk::gdk::Key::slash);
            if keyval == gtk::gdk::Key::Escape || toggles || keyval == gtk::gdk::Key::F1 {
                closer.close();
                return gtk::glib::Propagation::Stop;
            }
            gtk::glib::Propagation::Proceed
        });
        win.add_controller(keys);

        win.present();
        win
    }

    /// Push the merged queue to the Outbox view and the sidebar's badge. Oldest
    /// first, which is the order the workers send in.
    fn push_outbox(&self) {
        let items = self.outbox_items();
        self.sidebar.emit(SidebarInput::SetOutboxCount(items.len() as u32));
        if self.showing_outbox {
            self.message_list.emit(MessageListInput::SetMessages {
                title: "Outbox".into(),
                messages: items.iter().map(|i| i.as_message()).collect(),
            });
        }
    }

    /// Every account's queue merged, oldest first — the order they will send in.
    fn outbox_items(&self) -> Vec<crate::models::OutboxItem> {
        let mut items: Vec<crate::models::OutboxItem> = self
            .outbox_by_account
            .values()
            .flatten()
            .cloned()
            .collect();
        items.sort_by_key(|i| (i.queued_at, i.id));
        items
    }

    /// The queued message behind a list row, if the Outbox is what's shown.
    fn outbox_item(&self, account_id: u32, id: u32) -> Option<crate::models::OutboxItem> {
        if !self.showing_outbox {
            return None;
        }
        self.outbox_by_account
            .get(&account_id)?
            .iter()
            .find(|i| i.id == id)
            .cloned()
    }

    /// Tell the message list whether the folder(s) currently shown are fully
    /// indexed, so it knows whether to expect more rows while scrolling.
    fn push_index_complete(&self) {
        self.message_list
            .emit(MessageListInput::SetIndexComplete(self.current_index_complete()));
    }

    fn current_index_complete(&self) -> bool {
        if self.unified {
            self.accounts.iter().all(|a| {
                self.inbox_of(a.id)
                    .map_or(true, |f| self.indexed_folders.contains(&(a.id, f.id)))
            })
        } else if let Some(sel) = &self.selected {
            self.indexed_folders.contains(&(sel.account_id, sel.folder_id))
        } else {
            true
        }
    }

    /// (Re)arm the repeating auto-fetch timer to the current interval.
    fn arm_auto_fetch(&mut self, sender: &ComponentSender<Self>) {
        if let Some(id) = self.auto_fetch_source.take() {
            id.remove();
        }
        if self.fetch_interval_secs > 0 {
            let input = sender.input_sender().clone();
            let secs = self.fetch_interval_secs.min(u32::MAX as u64) as u32;
            let id = gtk::glib::timeout_add_seconds_local(secs, move || {
                let _ = input.send(AppMsg::Refresh);
                gtk::glib::ControlFlow::Continue
            });
            self.auto_fetch_source = Some(id);
        }
    }

    /// Send a request to a specific account's worker.
    fn send_to(&self, account_id: u32, req: MailRequest) {
        if let Some(worker) = self.workers.get(&account_id) {
            let _ = worker.send(req);
        }
    }

    /// The account to act on by default (selected folder's account, else first).
    fn active_account(&self) -> u32 {
        self.selected
            .as_ref()
            .map(|s| s.account_id)
            .or_else(|| self.accounts.first().map(|a| a.id))
            .unwrap_or(1)
    }

    /// Spawn one worker per configured account (or a single mock worker when no
    /// account is configured).
    fn spawn_workers(&mut self, sender: &ComponentSender<Self>) {
        self.workers.clear();
        // account_id is the config index + 1 (a load-bearing invariant), so we keep
        // every account's slot but only spawn a worker for enabled ones — disabled
        // accounts simply have no worker (no sync, no sidebar presence). With no
        // accounts configured, the app is blank — the sample/demo data only appears
        // when explicitly requested via VIREO_DEMO (so removing all real accounts
        // doesn't fall back to fake content).
        if self.config.is_empty() {
            if demo_mode() {
                for account_id in [1, 2] {
                    self.workers.insert(account_id, Self::spawn_worker(account_id, None, sender));
                }
            }
        } else {
            for (i, account) in self.config.iter().enumerate() {
                if !account.enabled {
                    continue;
                }
                let account_id = i as u32 + 1;
                let worker = Self::spawn_worker(account_id, Some(account.clone()), sender);
                self.workers.insert(account_id, worker);
                // Anything left queued by a previous run has to show up now, not
                // only after the next failed send.
                self.send_to(account_id, MailRequest::LoadOutbox);
            }
        }
    }

    fn spawn_worker(
        account_id: u32,
        account: Option<AccountConfig>,
        sender: &ComponentSender<Self>,
    ) -> UnboundedSender<MailRequest> {
        let input = sender.input_sender().clone();
        worker::spawn(account_id, account, move |event| {
            let _ = input.send(map_event(account_id, event));
        })
    }

    /// Tear down all workers and reconnect from the current config.
    fn reconnect_all(&mut self, sender: &ComponentSender<Self>) {
        self.accounts.clear();
        self.folders.clear();
        self.selected = None;
        self.unified = false;
        self.unified_by_account.clear();
        self.message_cache.clear();
        self.body_cache.clear();
        self.attachments.clear();
        self.sync_attachment_drawer();
        self.attachments_loading = false;
        self.attachments_available = false;
        self.attachment_cache.clear();
        self.current = None;
        self.busy.clear();
        self.show_message(None, false);
        self.message_list.emit(MessageListInput::SetLoading { title: String::new() });
        self.rebuild_sidebar();
        self.spawn_workers(sender);
    }

    /// Resolved avatar/accent colour for an account (custom, else auto accent).
    fn account_color(&self, account_id: u32) -> String {
        self.config
            .get(account_id.saturating_sub(1) as usize)
            .and_then(|c| c.color.clone())
            .or_else(|| {
                self.accounts
                    .iter()
                    .find(|a| a.id == account_id)
                    .map(|a| a.accent.clone())
            })
            .unwrap_or_else(|| "#3584e4".to_string())
    }

    /// Custom avatar emoji for an account, if set.
    fn account_emoji(&self, account_id: u32) -> Option<String> {
        // Demo mode only: showcase the emoji-avatar feature on the sample accounts.
        if self.config.is_empty() && demo_mode() {
            return match account_id {
                1 => Some("🚀".into()),
                2 => Some("🦀".into()),
                _ => None,
            };
        }
        self.config
            .get(account_id.saturating_sub(1) as usize)
            .and_then(|c| c.emoji.clone())
    }

    /// A label for an account in messages (name, else email, else "Account N").
    /// Uses config (available even before the account connects), then live data.
    fn account_label(&self, account_id: u32) -> String {
        let pick = |name: &str, email: &str| -> Option<String> {
            if !name.trim().is_empty() {
                Some(name.to_string())
            } else if !email.trim().is_empty() {
                Some(email.to_string())
            } else {
                None
            }
        };
        self.config
            .get(account_id.saturating_sub(1) as usize)
            .and_then(|c| pick(&c.name, &c.email))
            .or_else(|| {
                self.accounts
                    .iter()
                    .find(|a| a.id == account_id)
                    .and_then(|a| pick(&a.name, &a.email))
            })
            .unwrap_or_else(|| format!("Account {account_id}"))
    }

    /// Display name for an account (name, else email).
    fn account_name(&self, account_id: u32) -> String {
        // The account's UI label (how it's shown in All Inboxes / the reader chip).
        self.accounts
            .iter()
            .find(|a| a.id == account_id)
            .map(|a| a.label.clone())
            .unwrap_or_default()
    }

    /// The account email for an id, if known.
    fn email_of(&self, account_id: u32) -> Option<String> {
        self.accounts
            .iter()
            .find(|a| a.id == account_id)
            .map(|a| a.email.clone())
    }

    /// Persist the sidebar's per-account state (order, collapse, custom-folders
    /// expansion, and icon-only mode).
    fn save_sidebar_state(&self) {
        config::save_sidebar_state(&config::SidebarState {
            order: self.account_order.clone(),
            collapsed: self.collapsed.clone(),
            folders_expanded: self.folders_expanded.clone(),
            icon_only: self.sidebar_collapsed,
        });
    }

    /// Account emails in display order: those listed in `account_order` first
    /// (in that order), then any remaining accounts by id.
    fn ordered_emails(&self) -> Vec<String> {
        let mut result: Vec<String> = Vec::new();
        for email in &self.account_order {
            if self.accounts.iter().any(|a| &a.email == email) && !result.contains(email) {
                result.push(email.clone());
            }
        }
        let mut rest: Vec<&Account> = self
            .accounts
            .iter()
            .filter(|a| !result.contains(&a.email))
            .collect();
        rest.sort_by_key(|a| a.id);
        for a in rest {
            result.push(a.email.clone());
        }
        result
    }

    /// Close the floating sidebar overlay if it is open — navigation picked in
    /// it is done with it (mirrors how GNOME's own adaptive sidebars behave).
    fn close_sidebar_peek(&mut self) {
        if self.auto_rail && self.sidebar_peek {
            self.rail_active = true;
            self.set_sidebar_peek(false, true);
        }
    }

    /// Hide or show the sidebar header's title and window controls (icon rail).
    fn compact_sidebar_header(&self, compact: bool) {
        if let (Some(header), Some(title), Some(menu)) = (
            self.sidebar_header.as_ref(),
            self.app_title.as_ref(),
            self.sidebar_menu.as_ref(),
        ) {
            set_sidebar_header_compact(header, title, menu, compact);
        }
    }

    /// Open or close the narrow-window sidebar *peek*: the expanded sidebar
    /// floating above the panes as an overlay (the split view's collapsed
    /// mode), so neither the message list nor the reader is resized. `sync_rows`
    /// also switches the sidebar component's rows — the sidebar's own toggle
    /// button has already done that itself, an outside dismissal has not.
    fn set_sidebar_peek(&mut self, open: bool, sync_rows: bool) {
        let Some(split) = self.sidebar_split.clone() else { return };
        self.sidebar_peek = open;
        if open {
            if sync_rows {
                self.sidebar.emit(SidebarInput::SetCollapsed(false));
            }
            self.compact_sidebar_header(false);
            split.set_min_sidebar_width(280.0);
            split.set_max_sidebar_width(280.0);
            split.set_collapsed(true);
            split.set_show_sidebar(true);
        } else {
            if sync_rows {
                self.sidebar.emit(SidebarInput::SetCollapsed(true));
            }
            self.compact_sidebar_header(true);
            split.set_min_sidebar_width(SIDEBAR_RAIL_WIDTH);
            split.set_max_sidebar_width(SIDEBAR_RAIL_WIDTH);
            split.set_collapsed(false);
            split.set_show_sidebar(true);
        }
    }

    /// Smoothly animate the sidebar rail between its expanded width and the
    /// narrow icon-only width by interpolating the split view's pinned width.
    fn animate_sidebar(&mut self, collapsing: bool) {
        let Some(split) = self.sidebar_split.clone() else {
            return;
        };
        // Start from the current on-screen width; fall back to sensible defaults.
        let from = split
            .sidebar()
            .map(|w| w.width() as f64)
            .filter(|w| *w > 1.0)
            .unwrap_or(if collapsing { 256.0 } else { SIDEBAR_RAIL_WIDTH });
        let expanded = (0.2 * self.window.width() as f64).clamp(220.0, 280.0);
        let to = if collapsing { SIDEBAR_RAIL_WIDTH } else { expanded };

        let s = split.clone();
        let target = adw::CallbackAnimationTarget::new(move |v| {
            s.set_min_sidebar_width(v);
            s.set_max_sidebar_width(v);
        });
        let anim = adw::TimedAnimation::new(&split, from, to, 200, target);
        anim.set_easing(adw::Easing::EaseOutCubic);
        if !collapsing {
            // Restore responsive sizing once expanded, so window resizes track again.
            let s2 = split.clone();
            anim.connect_done(move |_| {
                s2.set_min_sidebar_width(180.0);
                s2.set_max_sidebar_width(280.0);
                s2.set_sidebar_width_fraction(0.2);
            });
        }
        anim.play();
        self.sidebar_anim = Some(anim);
    }

    /// Update the sidebar's unread badges in place (no rebuild), derived from the
    /// loaded message lists. Cheap enough to call on every read/sync.
    fn push_unread_counts(&self) {
        let folders = self.folder_unread.clone();
        let unified = self.accounts.iter().map(|a| self.inbox_unread(a.id)).sum();
        self.sidebar
            .emit(SidebarInput::SetUnread { folders, unified });
        // The same number is what GNOME shows beside Vireo in Background Apps,
        // so a process with no window still says what it is there for.
        if self.run_in_background.get() {
            crate::background::set_status(&crate::background::status_text(unified));
        }
    }

    /// Push the current accounts + folders to the sidebar, in the user's chosen
    /// order and with each account's collapsed state.
    fn rebuild_sidebar(&self) {
        let order = self.ordered_emails();
        let sections: Vec<SectionData> = order
            .iter()
            .filter_map(|email| {
                let account = self.accounts.iter().find(|a| &a.email == email)?.clone();
                // Use the server-side unread count (accurate beyond the loaded
                // window) for each folder's badge.
                let folders = self
                    .folders
                    .get(&account.id)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|mut f| {
                        if let Some(n) = self.folder_unread.get(&(account.id, f.id)) {
                            f.unread = *n;
                        }
                        f
                    })
                    .collect();
                let color = self.account_color(account.id);
                let emoji = self.account_emoji(account.id);
                Some(SectionData {
                    collapsed: self.collapsed.contains(email),
                    custom_expanded: self.folders_expanded.contains(email),
                    color,
                    emoji,
                    account,
                    folders,
                })
            })
            .collect();
        let show_unified = self.accounts.len() > 1;
        let unified_unread = self.accounts.iter().map(|a| self.inbox_unread(a.id)).sum();
        self.sidebar.emit(SidebarInput::SetContents {
            sections,
            show_unified,
            unified_unread,
        });

        // Keep the list's per-account tint colours in sync.
        let colors: std::collections::HashMap<u32, String> = self
            .accounts
            .iter()
            .map(|a| (a.id, self.account_color(a.id)))
            .collect();
        self.message_list
            .emit(MessageListInput::SetAccountColors(colors));
    }

    fn remote_allowed(&self, m: &Message) -> bool {
        if self.auto_remote_content {
            return true;
        }
        let addr = m.from_addr.to_lowercase();
        self.allowed_senders.iter().any(|s| *s == addr)
    }

    /// Rebuild the attachments popover (a row per attachment + "Save All").
    fn rebuild_attach_popover(&self, sender: &ComponentSender<Self>) {
        use crate::models::is_image_name;
        use crate::ui::attachments_gallery::{icon_color_class, icon_for, texture_from};

        while let Some(child) = self.attach_list.first_child() {
            self.attach_list.remove(&child);
        }

        // So the action buttons can dismiss the popover before opening a dialog
        // or the lightbox.
        let popover = self
            .attach_list
            .ancestor(gtk::Popover::static_type())
            .and_downcast::<gtk::Popover>();

        for (i, att) in self.attachments.iter().enumerate() {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            row.add_css_class("attach-row");

            // Image attachments show a thumbnail; everything else a type icon.
            let thumb = is_image_name(&att.name)
                .then(|| texture_from(&att.data))
                .flatten();
            match &thumb {
                Some(tex) => {
                    let img = gtk::Image::from_paintable(Some(tex));
                    img.set_pixel_size(36);
                    img.add_css_class("attach-thumb");
                    row.append(&img);
                }
                None => {
                    let img = gtk::Image::from_icon_name(icon_for(&att.name));
                    img.set_pixel_size(28);
                    img.add_css_class("gallery-file-icon");
                    img.add_css_class(icon_color_class(&att.name));
                    row.append(&img);
                }
            }

            let info = gtk::Box::new(gtk::Orientation::Vertical, 0);
            info.set_hexpand(true);
            info.set_valign(gtk::Align::Center);
            let name = gtk::Label::new(Some(&att.name));
            name.set_halign(gtk::Align::Start);
            name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
            name.set_max_width_chars(22);
            let size = gtk::Label::new(Some(&att.human_size()));
            size.set_halign(gtk::Align::Start);
            size.add_css_class("dim-label");
            size.add_css_class("caption");
            info.append(&name);
            info.append(&size);
            row.append(&info);

            let action = |icon: &str, tip: &str| {
                let b = gtk::Button::from_icon_name(icon);
                b.add_css_class("flat");
                b.set_valign(gtk::Align::Center);
                b.set_tooltip_text(Some(tip));
                b
            };

            // Preview (images only) reuses the drawer's lightbox; Download reuses
            // its file chooser; Open launches the default app.
            if thumb.is_some() {
                let preview = action("co.hyprlab.Vireo-system-search-symbolic", "Preview");
                let d = self.attachment_drawer.sender().clone();
                let pop = popover.clone();
                preview.connect_clicked(move |_| {
                    if let Some(p) = &pop {
                        p.popdown();
                    }
                    let _ = d.send(AttachmentDrawerInput::Activate(i));
                });
                row.append(&preview);
            }

            let open = action("co.hyprlab.Vireo-document-open-symbolic", "Open");
            let s = sender.input_sender().clone();
            let pop = popover.clone();
            open.connect_clicked(move |_| {
                if let Some(p) = &pop {
                    p.popdown();
                }
                let _ = s.send(AppMsg::OpenAttachment(i));
            });
            row.append(&open);

            let download = action("co.hyprlab.Vireo-folder-download-symbolic", "Download");
            let d = self.attachment_drawer.sender().clone();
            let pop = popover.clone();
            download.connect_clicked(move |_| {
                if let Some(p) = &pop {
                    p.popdown();
                }
                let _ = d.send(AttachmentDrawerInput::Download(i));
            });
            row.append(&download);

            self.attach_list.append(&row);
        }
        if !self.attachments.is_empty() {
            self.attach_list
                .append(&gtk::Separator::new(gtk::Orientation::Horizontal));
            let save = gtk::Button::with_label("Save All…");
            save.add_css_class("flat");
            let s = sender.input_sender().clone();
            save.connect_clicked(move |_| {
                let _ = s.send(AppMsg::SaveAllAttachments);
            });
            self.attach_list.append(&save);
        }
        self.sync_attachment_drawer();
    }

    /// Push the current attachments into the in-message thumbnail drawer (which
    /// hides itself when the list is empty). Called wherever `self.attachments`
    /// changes so the drawer always mirrors the open message.
    fn sync_attachment_drawer(&self) {
        self.attachment_drawer
            .emit(AttachmentDrawerInput::SetItems(self.attachments.clone()));
    }

    /// Present a read-only window showing raw message source (monospace).
    fn show_source_window(&self, text: &str) {
        let buffer = gtk::TextBuffer::new(None);
        buffer.set_text(text);
        let view = gtk::TextView::with_buffer(&buffer);
        view.set_editable(false);
        view.set_monospace(true);
        view.set_wrap_mode(gtk::WrapMode::WordChar);
        view.set_left_margin(12);
        view.set_right_margin(12);
        view.set_top_margin(8);
        view.set_bottom_margin(8);

        let scroller = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .child(&view)
            .build();

        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&adw::HeaderBar::new());
        toolbar.set_content(Some(&scroller));

        let window = adw::Window::builder()
            .transient_for(&self.window)
            .title("Message Source")
            .default_width(720)
            .default_height(620)
            .content(&toolbar)
            .build();
        window.present();
    }

    /// Switch the message list to a folder: reset the view, show its cached
    /// messages instantly (if any), and kick off a background sync. Shared by the
    /// sidebar selection and the "open message from notification" flow.
    fn select_folder(&mut self, account_id: u32, folder_id: u32, name: String, path: String) {
        self.showing_gallery = false;
        self.showing_outbox = false;
        self.unified = false;
        self.attachments.clear();
        self.sync_attachment_drawer();
        self.attachments_loading = false;
        self.attachments_available = false;
        self.message_list.emit(MessageListInput::SetSelected(None));
        self.message_list.emit(MessageListInput::SetColorize(false));
        self.message_list.emit(MessageListInput::ResetPaging);
        self.selected = Some(SelectedFolder {
            account_id,
            folder_id,
            name: name.clone(),
            path: path.clone(),
        });
        self.current = None;
        self.current_thread.clear();
        self.show_message(None, false);
        match self.message_cache.get(&(account_id, folder_id)) {
            Some(cached) => self.message_list.emit(MessageListInput::SetMessages {
                title: name,
                messages: cached.clone(),
            }),
            None => self.message_list.emit(MessageListInput::SetLoading { title: name }),
        }
        self.push_index_complete();
        self.send_to(account_id, MailRequest::LoadMessages { folder_id, path });
    }

    fn show_message(&self, message: Option<Message>, loading: bool) {
        let allow_remote = message.as_ref().is_some_and(|m| self.remote_allowed(m));
        let (account_name, account_color) = match message.as_ref() {
            Some(m) => (
                Some(self.account_name(m.account_id)),
                Some(self.account_color(m.account_id)),
            ),
            None => (None, None),
        };
        let stored_check = message
            .as_ref()
            .and_then(|m| self.sender_cache.get(&(m.account_id, m.id)))
            .cloned();
        self.message_view.emit(MessageViewInput::Show {
            thread: message.into_iter().collect(),
            allow_remote,
            gravatar: self.gravatar,
            account_name,
            account_color,
            loading,
            primary: None, // a single message is its own primary
            folder_labels: HashMap::new(),
            // A single message is one small frame; it is never covered anyway.
            instant: true,
        });
        // After `Show`, which clears the outgoing message's verdict.
        if let Some(check) = stored_check {
            self.message_view
                .emit(MessageViewInput::SetSenderCheck(check));
        }
    }

    /// Render the current conversation (thread) in the reader, newest first.
    ///
    /// The header follows the message the user selected, not whatever sorted to
    /// the top: a conversation can now include replies of your own pulled in from
    /// Sent, and one of those being the newest shouldn't retitle the reader.
    /// Ask for one conversation re-render shortly from now, collapsing a burst of
    /// arriving bodies into a single one.
    ///
    /// A conversation is rendered as one document with every member's body
    /// inlined and handed to WebKit. Rendering per arriving body meant N loads of
    /// an N-body document, issued far faster than WebKit could retire them.
    fn queue_thread_render(&mut self, sender: &ComponentSender<Self>) {
        if self.thread_render_queued {
            return;
        }
        self.thread_render_queued = true;
        let s = sender.clone();
        gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(120), move || {
            s.input(AppMsg::RenderThread);
        });
    }

    fn show_thread(&self) {
        let Some(head) = self.current_thread.first() else {
            return;
        };
        let primary = self.current.clone().unwrap_or_else(|| head.clone());
        let account_id = primary.account_id;
        let allow_remote = self.remote_allowed(&primary);
        // Spin while any of the conversation is still on its way: a partial
        // render would be replaced moments later, and each replacement is a full
        // page load in the WebView — which is what the flashing was.
        let missing = self.current_thread.iter().any(|m| m.body.is_empty());
        let waiting = self
            .thread_opened_at
            .is_some_and(|opened| opened.elapsed() < THREAD_BODY_WAIT);
        let unsettled = missing || self.thread_related_pending;
        let loading = if self.current_thread.len() > 1 {
            // The spinner holds the reader from the moment a thread is opened
            // until its single paint, so the wait is shown rather than papered
            // over with the message that was there before.
            !self.thread_painted || (unsettled && waiting)
        } else {
            primary.body.is_empty()
        };
        self.message_view.emit(MessageViewInput::Show {
            thread: self.current_thread.clone(),
            allow_remote,
            gravatar: self.gravatar,
            account_name: Some(self.account_name(account_id)),
            account_color: Some(self.account_color(account_id)),
            loading,
            folder_labels: self.thread_folder_labels(),
            primary: Some(Box::new(primary)),
            // Nothing to wait for when the conversation was already assembled
            // and its bodies are in hand.
            instant: self.thread_painted && !loading,
        });
    }

    /// Name the folder each conversation message came from, for the ones that
    /// aren't from the folder on screen. The message list only ever shows one
    /// folder, so anything else was pulled in from the cache (#21).
    fn thread_folder_labels(&self) -> HashMap<(u32, u32), String> {
        let shown_folder = self.current.as_ref().map(|m| m.folder_id);
        self.current_thread
            .iter()
            .filter(|m| Some(m.folder_id) != shown_folder)
            .filter_map(|m| {
                let name = self
                    .folders
                    .get(&m.account_id)?
                    .iter()
                    .find(|f| f.id == m.folder_id)
                    .map(|f| f.name.clone())?;
                Some(((m.account_id, m.id), name))
            })
            .collect()
    }

    /// Pop a message out into its own standalone window with a dedicated reader.
    fn open_message_window(&mut self, m: Message, sender: &ComponentSender<Self>) {
        let key = (m.account_id, m.id);
        // Already open? Just bring it forward.
        if let Some(p) = self.popouts.get(&key) {
            p.window.present();
            return;
        }
        let account_id = m.account_id;

        // Reuse an already-fetched body so the window renders instantly.
        let cached_body = if !m.body.is_empty() {
            Some(m.body.clone())
        } else if self
            .current
            .as_ref()
            .is_some_and(|c| c.id == m.id && c.account_id == account_id && !c.body.is_empty())
        {
            self.current.as_ref().map(|c| c.body.clone())
        } else {
            self.body_cache.get(&key).cloned()
        };
        let needs_body = cached_body.is_none();

        let mut display = m.clone();
        display.unread = false;
        if let Some(body) = cached_body {
            display.body = body;
        }

        // Fetch the body unless the in-flight selection request already will
        // (single click precedes the double click), to avoid a duplicate fetch.
        let already_loading = self
            .current
            .as_ref()
            .is_some_and(|c| c.id == m.id && c.account_id == account_id);
        if needs_body && !already_loading {
            if let Some(path) = self.resolve_folder_path(&m) {
                self.send_to(account_id, MailRequest::LoadBody {
                    message_id: m.id,
                    path,
                    uid: m.uid,
                });
            }
        }

        // Attachments: use the in-memory cache if present; otherwise ask the
        // worker for them (cache-only) and route the reply to this window, just
        // like the main reader does.
        let mut atts: Vec<Attachment> = Vec::new();
        if display.has_attachment {
            if let Some(cached) = self.attachment_cache.get(&key).cloned() {
                atts = cached;
            } else if let Some(path) = self.resolve_folder_path(&m) {
                self.send_to(account_id, MailRequest::LoadAttachments {
                    message_id: m.id,
                    path,
                    uid: m.uid,
                    download: false,
                });
            }
        }

        let allow_remote = self.remote_allowed(&display);
        let init = MessageWindowInit {
            message: display,
            gravatar: self.gravatar,
            avatars: self.avatars,
            sender_logos: self.sender_logos,
            account_name: Some(self.account_name(account_id)),
            account_color: Some(self.account_color(account_id)),
            allow_remote,
            loading: needs_body,
            attachments: atts,
            attachments_available: false,
            attachments_loading: false,
            content_dark: self.message_theme.dark_override(),
        };

        let controller = MessageWindow::builder()
            .launch(init)
            .forward(sender.input_sender(), move |out| match out {
                MessageWindowOutput::Action { action, message } => {
                    AppMsg::RowAction { action, message }
                }
                MessageWindowOutput::AddToContacts { name, email } => {
                    AppMsg::AddContactFrom { name, email }
                }
                MessageWindowOutput::LoadAttachments(message) => AppMsg::LoadAttachmentsFor(message),
                MessageWindowOutput::OpenAttachment(att) => AppMsg::OpenAttachmentItem(att),
                MessageWindowOutput::SaveAllAttachments(items) => AppMsg::SaveAttachmentItems(items),
                MessageWindowOutput::AllowSender(addr) => AppMsg::AllowSender(addr),
                MessageWindowOutput::ComposeTo(addr) => AppMsg::ComposeTo(addr),
                MessageWindowOutput::Closed => AppMsg::PopoutClosed(key),
            });

        let window = controller.widget().clone();
        window.set_transient_for(Some(&self.window));
        window.present();

        self.popouts.insert(key, PopOut { window, controller });
    }

    /// The sender verdict for the message on screen, if one has arrived.
    fn sender_verdict(&self) -> Option<&crate::models::SenderCheck> {
        let m = self.current.as_ref()?;
        self.sender_cache.get(&(m.account_id, m.id)).map(|c| &**c)
    }

    /// CSS classes for the sender-check badge. The verdict tint is only added
    /// once a verdict exists — without one the button is insensitive and should
    /// grey out exactly like the other toolbar icons.
    /// Tooltip for the toolbar's trash button: says when it will delete the
    /// whole multi-selection rather than just the open message.
    fn delete_tooltip(&self) -> String {
        match self.list_selection.len() {
            n if n > 1 => format!("Delete {n} messages"),
            _ => "Delete".to_string(),
        }
    }

    fn sender_badge_classes(&self) -> Vec<&'static str> {
        let mut classes = vec!["flat", "image-button"];
        if let Some(check) = self.sender_verdict() {
            classes.push(check.trust.css_class());
        }
        classes
    }

    /// That verdict's trust level, defaulting to "unverified".
    fn sender_trust(&self) -> crate::models::SenderTrust {
        self.sender_verdict()
            .map(|c| c.trust)
            .unwrap_or(crate::models::SenderTrust::Unverified)
    }

    /// Whether the given folder is the account's Drafts folder.
    fn is_drafts_folder(&self, account_id: u32, folder_id: u32) -> bool {
        self.folder_kind(account_id, folder_id) == Some(FolderKind::Drafts)
    }

    /// The kind of a folder by id, if known.
    fn folder_kind(&self, account_id: u32, folder_id: u32) -> Option<FolderKind> {
        self.folders
            .get(&account_id)?
            .iter()
            .find(|f| f.id == folder_id)
            .map(|f| f.kind)
    }

    /// Open a draft for editing: reuse a cached body if we have one, otherwise
    /// fetch it and open the editor once it arrives (see the `Body` handler).
    fn open_draft(&mut self, m: Message, sender: &ComponentSender<Self>) {
        let body = if !m.body.is_empty() {
            Some(m.body.clone())
        } else {
            self.body_cache.get(&(m.account_id, m.id)).cloned()
        };
        match body {
            Some(html) => self.compose_from_draft(m, html, sender),
            None => {
                if let Some(path) = self.resolve_folder_path(&m) {
                    self.send_to(
                        m.account_id,
                        MailRequest::LoadBody { message_id: m.id, path, uid: m.uid },
                    );
                }
                self.pending_draft = Some(m);
            }
        }
    }

    /// Render a queued message in the reader. Its bytes are stored with it, so
    /// this is a local render — the same one the worker would produce.
    fn show_outbox_message(&mut self, item: &crate::models::OutboxItem) {
        let mut message = item.as_message();
        message.body = crate::worker::extract_body(&item.raw);
        message.date = crate::models::OutboxItem::waiting_label(item.queued_at);
        self.attachments = crate::worker::extract_attachments_of(&item.raw);
        self.attachments_available = !self.attachments.is_empty();
        self.attachments_loading = false;
        self.sync_attachment_drawer();
        self.current = Some(message.clone());
        self.current_thread.clear();
        self.show_message(Some(message), false);
    }

    /// Open a queued Outbox message in the composer.
    ///
    /// The queued copy stays put until the edited version is handed back to the
    /// worker: an editor that is closed again must not have destroyed the only
    /// copy of the message. Attachments are written to a private temp directory,
    /// because the composer attaches files by path and the originals are long
    /// gone (under Flatpak the portal's paths expire).
    fn compose_from_outbox(&mut self, account_id: u32, id: u32, sender: &ComponentSender<Self>) {
        let Some(item) = self
            .outbox_by_account
            .get(&account_id)
            .and_then(|items| items.iter().find(|i| i.id == id))
            .cloned()
        else {
            return;
        };
        let editable = crate::worker::editable_from_raw(&item.raw, &item.rcpts);

        let mut attachments = Vec::new();
        if !editable.attachments.is_empty() {
            let dir = std::env::temp_dir().join(format!("vireo-outbox-{account_id}-{id}"));
            if std::fs::create_dir_all(&dir).is_ok() {
                for (i, att) in editable.attachments.iter().enumerate() {
                    // The name came out of a message header; keep it to a single
                    // path component.
                    let safe = att.name.replace(['/', '\\'], "_");
                    let name = if safe.trim().is_empty() {
                        format!("attachment-{}", i + 1)
                    } else {
                        safe
                    };
                    let path = dir.join(&name);
                    match std::fs::write(&path, &att.data) {
                        Ok(()) => attachments.push(path),
                        Err(e) => tracing::warn!("could not stage {name} for editing: {e}"),
                    }
                }
            }
        }

        let prefill = ComposePrefill {
            to: editable.to,
            cc: editable.cc,
            bcc: editable.bcc,
            subject: editable.subject,
            body_html: editable.body_html,
            attachments,
            // Re-editing a queued message: it keeps whatever thread it was
            // already part of (carried in the raw MIME it was built from).
            in_reply_to: String::new(),
            references: String::new(),
            draft_origin: None,
            outbox_origin: Some(id),
        };
        // The Outbox stays the folder on screen: its list is still what's listed,
        // so its toolbar has to stay too. Leaving it would strand the user in a
        // reader offering Reply and Forward for a message that hasn't been sent.
        self.open_compose(account_id, prefill, sender);
    }

    /// Open the compose editor pre-filled from a draft, remembering its origin so
    /// saving/sending replaces it.
    fn compose_from_draft(&mut self, m: Message, body_html: String, sender: &ComponentSender<Self>) {
        let path = self.resolve_folder_path(&m).unwrap_or_default();
        let prefill = ComposePrefill {
            to: m.to.clone(),
            cc: m.cc.clone(),
            subject: m.subject.clone(),
            body_html,
            draft_origin: Some(crate::models::DraftOrigin {
                account_id: m.account_id,
                folder_id: m.folder_id,
                path,
                uid: m.uid,
            }),
            ..Default::default()
        };
        self.open_compose(m.account_id, prefill, sender);
    }

    /// Assemble the `ComposeInit` for a composer (from-accounts + signatures,
    /// autocomplete suggestions, a fresh id, host mode).
    fn build_compose_init(
        &mut self,
        account_id: u32,
        prefill: ComposePrefill,
        windowed: bool,
        can_toggle: bool,
    ) -> (u32, ComposeInit) {
        // Selectable "from" accounts, in display order, with their signatures.
        let accounts: Vec<ComposeAccount> = self
            .ordered_emails()
            .iter()
            .filter_map(|email| {
                let a = self.accounts.iter().find(|a| &a.email == email)?;
                let label = if a.name.trim().is_empty() {
                    a.email.clone()
                } else {
                    format!("{} <{}>", a.name, a.email)
                };
                let signature = self
                    .config
                    .get(a.id.saturating_sub(1) as usize)
                    .and_then(|c| c.signature.clone())
                    .unwrap_or_default();
                Some(ComposeAccount { id: a.id, label, signature })
            })
            .collect();
        let selected = accounts.iter().position(|c| c.id == account_id).unwrap_or(0);

        // Exclude the user's own addresses from recipient suggestions.
        let own: Vec<String> = self.accounts.iter().map(|a| a.email.clone()).collect();
        let id = self.next_compose_id;
        self.next_compose_id += 1;
        let init = ComposeInit {
            compose_id: id,
            prefill,
            accounts,
            selected,
            suggestions: crate::contacts::suggestions(&own),
            windowed,
            can_toggle,
        };
        (id, init)
    }

    /// Launch a `Compose` component, forwarding its outputs into `AppMsg`.
    fn spawn_compose(
        &self,
        init: ComposeInit,
        sender: &ComponentSender<Self>,
    ) -> Controller<Compose> {
        Compose::builder()
            .launch(init)
            .forward(sender.input_sender(), |out| match out {
                ComposeOutput::Send(msg) => AppMsg::SendMessage(msg),
                ComposeOutput::SaveDraft(msg) => AppMsg::SaveDraftMessage(msg),
                ComposeOutput::ToggleWindow(id) => AppMsg::ComposeToggleWindow(id),
                ComposeOutput::Close(id) => AppMsg::ComposeClosed(id),
            })
    }

    /// Host a compose pane in a fresh standalone window, transient for the app.
    fn compose_window_host(
        &self,
        content: &impl IsA<gtk::Widget>,
        id: u32,
        sender: &ComponentSender<Self>,
    ) -> adw::Window {
        let win = adw::Window::builder()
            .modal(false)
            .default_width(660)
            .default_height(760)
            .title("New Message")
            .transient_for(&self.window)
            .build();
        win.set_content(Some(content));
        let s = sender.input_sender().clone();
        win.connect_close_request(move |_| {
            let _ = s.send(AppMsg::ComposeClosed(id));
            gtk::glib::Propagation::Proceed
        });
        win.present();
        win
    }

    /// Open a standalone compose window (New Message, compose-to, edit-draft).
    fn open_compose(
        &mut self,
        account_id: u32,
        prefill: ComposePrefill,
        sender: &ComponentSender<Self>,
    ) {
        let (id, init) = self.build_compose_init(account_id, prefill, true, false);
        let controller = self.spawn_compose(init, sender);
        let window = self.compose_window_host(controller.widget(), id, sender);
        self.composers.push(ComposeHost { id, controller, window });
    }

    /// Open (or replace) the reader's inline reply/forward drop-down pane.
    fn open_inline_reply(
        &mut self,
        account_id: u32,
        prefill: ComposePrefill,
        sender: &ComponentSender<Self>,
    ) {
        // Supersede any composer already in the reader slot first.
        self.release_reader_compose();
        let (id, init) = self.build_compose_init(account_id, prefill, false, true);
        let controller = self.spawn_compose(init, sender);
        let widget = controller.widget();
        self.reader_compose_revealer.set_child(Some(widget));
        self.reader_compose_revealer.set_reveal_child(true);
        controller.emit(ComposeInput::FocusEditor);
        self.reader_compose = Some(ReaderCompose { id, controller, window: None });
    }

    /// Detach the reader's inline composer from the reader slot. If it was popped
    /// out to a window it lives on independently; if inline, ask it to save-if-
    /// dirty and let it drain closed.
    fn release_reader_compose(&mut self) {
        let Some(r) = self.reader_compose.take() else {
            return;
        };
        match r.window {
            Some(window) => {
                self.composers.push(ComposeHost { id: r.id, controller: r.controller, window });
            }
            None => {
                self.reader_compose_revealer.set_reveal_child(false);
                self.reader_compose_revealer.set_child(None::<&gtk::Widget>);
                r.controller.emit(ComposeInput::SaveDraftIfDirty);
                self.draining_composers.push((r.id, r.controller));
            }
        }
    }

    /// Promote the reader's inline pane to a window, or collapse a window back
    /// inline — reparenting the live pane so the editor state survives the move.
    fn toggle_compose_window(&mut self, id: u32, sender: &ComponentSender<Self>) {
        let Some(mut r) = self.reader_compose.take() else {
            return;
        };
        if r.id != id {
            self.reader_compose = Some(r);
            return;
        }
        let widget = r.controller.widget().clone();
        match r.window.take() {
            None => {
                // inline → window: unparent from the revealer, then host in a window.
                self.reader_compose_revealer.set_reveal_child(false);
                self.reader_compose_revealer.set_child(None::<&gtk::Widget>);
                let window = self.compose_window_host(&widget, id, sender);
                r.window = Some(window);
                r.controller.emit(ComposeInput::SetWindowed(true));
            }
            Some(window) => {
                // window → inline: unparent from the window, drop it back in place.
                window.set_content(None::<&gtk::Widget>);
                window.destroy();
                self.reader_compose_revealer.set_child(Some(&widget));
                self.reader_compose_revealer.set_reveal_child(true);
                r.controller.emit(ComposeInput::SetWindowed(false));
            }
        }
        r.controller.emit(ComposeInput::FocusEditor);
        self.reader_compose = Some(r);
    }

    /// Tear down a composer by id (from a Close output or a window's close-request).
    fn close_compose(&mut self, id: u32) {
        if let Some(pos) = self.composers.iter().position(|h| h.id == id) {
            let host = self.composers.remove(pos);
            host.window.set_content(None::<&gtk::Widget>);
            host.window.destroy();
            return;
        }
        if self.reader_compose.as_ref().is_some_and(|r| r.id == id) {
            let r = self.reader_compose.take().unwrap();
            match r.window {
                Some(window) => {
                    window.set_content(None::<&gtk::Widget>);
                    window.destroy();
                }
                None => {
                    self.reader_compose_revealer.set_reveal_child(false);
                    self.reader_compose_revealer.set_child(None::<&gtk::Widget>);
                }
            }
            return;
        }
        self.draining_composers.retain(|(cid, _)| *cid != id);
    }

    /// Move a message to its account's folder of `kind` (archive/delete).
    /// Destination path for `kind` on an account: its existing folder of that kind,
    /// else a sensible default (the worker creates it on the server on first move).
    fn folder_path_for(&self, account_id: u32, kind: FolderKind) -> Option<String> {
        self.folders
            .get(&account_id)
            .and_then(|fs| fs.iter().find(|f| f.kind == kind))
            .map(|f| f.path.clone())
            .or_else(|| self.default_folder_path(account_id, kind))
    }

    /// Fold the rest of a conversation — messages from the account's other
    /// folders, chiefly the replies you sent — into the open one (#21).
    ///
    /// Ignored unless it answers the message still on screen: the lookup is
    /// asynchronous and the user may have moved on. Messages already in the
    /// conversation are skipped, so this is safe to apply more than once.
    fn merge_related(&mut self, account_id: u32, message_id: u32, messages: Vec<Message>) {
        let Some(current) = self.current.clone() else { return };
        if current.account_id != account_id || current.id != message_id || !self.threading {
            return;
        }
        let mut conv = if self.current_thread.is_empty() {
            vec![current.clone()]
        } else {
            self.current_thread.clone()
        };
        let messages = dedupe_label_copies(messages, &conv, current.folder_id);
        let mut added = Vec::new();
        for mut r in messages {
            if conv.iter().any(|m| m.folder_id == r.folder_id && m.uid == r.uid) {
                continue; // already in the conversation, from this folder's own index
            }
            let key = (r.account_id, r.folder_id, r.uid);
            r.id = match self.related_ids.get(&key) {
                Some(id) => *id,
                None => {
                    self.related_id_seq = self.related_id_seq.saturating_sub(1);
                    self.related_ids.insert(key, self.related_id_seq);
                    self.related_id_seq
                }
            };
            r.unread = false;
            if let Some(b) = self.body_cache.get(&(r.account_id, r.id)) {
                r.body = b.clone();
            }
            added.push(r);
        }
        if added.is_empty() {
            return;
        }
        conv.extend(added);
        conv.sort_by(|a, b| a.timestamp.cmp(&b.timestamp)); // oldest first, as the list threads
        // Late arrivals bring bodies of their own; give them the same grace
        // period so they join the first paint instead of causing a second.
        self.thread_opened_at = Some(std::time::Instant::now());
        self.current_thread = conv;
        let to_load: Vec<MissingBody> = self
            .current_thread
            .iter()
            .filter(|tm| tm.body.is_empty())
            .filter_map(|tm| self.resolve_folder_path(tm).map(|p| (tm.account_id, tm.id, tm.uid, p)))
            .collect();
        for ((aid, path), items) in batch_bodies_by_folder(to_load) {
            self.send_to(aid, MailRequest::LoadBodies { items, path });
        }
    }

    /// Whether a message already lives in its account's Trash — where "delete"
    /// can't mean "move to Trash" any more, so it has to mean erase for good.
    fn in_trash(&self, m: &Message) -> bool {
        self.folders.get(&m.account_id).is_some_and(|fs| {
            fs.iter().any(|f| f.id == m.folder_id && f.kind == FolderKind::Trash)
        })
    }

    /// Delete `messages`: the ones still outside Trash are moved there, and any
    /// already in Trash are erased for good once the user confirms.
    fn delete_messages(&mut self, messages: Vec<Message>, sender: &ComponentSender<Self>) {
        let (purge, move_out): (Vec<Message>, Vec<Message>) =
            messages.into_iter().partition(|m| self.in_trash(m));
        match move_out.len() {
            0 => {}
            1 => self.move_to(move_out.into_iter().next().unwrap(), FolderKind::Trash),
            _ => self.apply_bulk_move(BulkAction::Delete, move_out),
        }
        if !purge.is_empty() {
            self.confirm_purge(purge, sender);
        }
    }

    /// Confirm erasing messages that are already in Trash — there's no undo, so
    /// this always asks first.
    fn confirm_purge(&self, messages: Vec<Message>, sender: &ComponentSender<Self>) {
        let n = messages.len();
        let heading = if n == 1 {
            "Delete this message permanently?".to_string()
        } else {
            format!("Delete {n} messages permanently?")
        };
        let body = if n == 1 {
            "It is already in Trash, so it will be erased from the server. This can’t be undone."
        } else {
            "They are already in Trash, so they will be erased from the server. \
             This can’t be undone."
        };
        let dialog = adw::MessageDialog::new(Some(&self.window), Some(&heading), Some(body));
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("delete", "Delete");
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
        let s = sender.clone();
        let messages = std::cell::RefCell::new(Some(messages));
        dialog.connect_response(None, move |_, resp| {
            if resp == "delete" {
                if let Some(msgs) = messages.borrow_mut().take() {
                    s.input(AppMsg::PurgeMessages(msgs));
                }
            }
        });
        dialog.present();
    }

    /// Erase messages from the server for good (flag `\Deleted` + EXPUNGE),
    /// grouped by source folder so each folder takes a single request.
    fn purge_messages(&mut self, messages: Vec<Message>) {
        let mut groups: HashMap<(u32, String), Vec<u32>> = HashMap::new();
        let mut removed_ids = Vec::with_capacity(messages.len());
        for m in &messages {
            let Some(src) = self.resolve_folder_path(m) else { continue };
            groups.entry((m.account_id, src)).or_default().push(m.uid);
            self.discard_message_local(m);
            removed_ids.push(m.id);
        }
        self.bulk_pending += groups.len();
        for ((account_id, path), uids) in groups {
            self.send_to(account_id, MailRequest::PurgeMessages { path, uids });
        }
        self.message_list.emit(MessageListInput::RemoveMany(removed_ids));
        self.push_unread_counts();
    }

    fn move_to(&mut self, m: Message, kind: FolderKind) {
        let Some(dest) = self.folder_path_for(m.account_id, kind) else {
            self.notifications.emit(NotifyInput::Push {
                text: format!("No {} folder available", kind_label(kind)),
                error: true,
                connectivity: false,
            });
            return;
        };
        self.move_to_path(m, dest);
    }

    /// Move a message to an explicit destination folder path (used by both the
    /// kind-based actions and drag-and-drop onto a folder).
    fn move_to_path(&mut self, m: Message, dest: String) {
        let Some(src) = self.resolve_folder_path(&m) else {
            return;
        };
        if src == dest {
            return; // already in that folder
        }
        self.send_to(m.account_id, MailRequest::MoveMessage { path: src, uid: m.uid, dest });
        self.discard_message(&m);
    }

    /// Move a dropped drag selection into `dest` on `dest_account`. Messages are
    /// grouped by source folder and each group moved in a single `MoveMessages`
    /// request, so dragging a multi-selection moves all of it (#23). IMAP can't
    /// move mail between accounts, so anything from another account — possible
    /// when the drag started in the unified inbox — stays put and is reported
    /// rather than silently dropped.
    fn drop_move(&mut self, dest_account: u32, dest: String, items: Vec<(u32, u32, u32, u32)>) {
        let mut groups: HashMap<String, Vec<u32>> = HashMap::new();
        let mut removed_ids: Vec<u32> = Vec::new();
        let mut foreign = 0usize;
        for (aid, fid, uid, id) in items {
            if aid != dest_account {
                foreign += 1;
                continue;
            }
            // Prefer the cached message: it knows its own source folder (in the
            // unified inbox that isn't the folder the row was listed under) and
            // lets the caches be cleaned up optimistically.
            let cached = self.find_cached_message(aid, id);
            let src = match &cached {
                Some(m) => self.resolve_folder_path(m),
                None => self
                    .folders
                    .get(&aid)
                    .and_then(|fs| fs.iter().find(|f| f.id == fid))
                    .map(|f| f.path.clone()),
            };
            let Some(src) = src else { continue };
            if src == dest {
                continue; // already in that folder
            }
            if let Some(m) = &cached {
                self.discard_message_local(m);
            }
            groups.entry(src).or_default().push(uid);
            removed_ids.push(id);
        }
        if foreign > 0 {
            self.notifications.emit(NotifyInput::Push {
                text: if foreign == 1 {
                    "One message stayed put — mail can't be moved between accounts".to_string()
                } else {
                    format!("{foreign} messages stayed put — mail can't be moved between accounts")
                },
                error: true,
                connectivity: false,
            });
        }
        if removed_ids.is_empty() {
            return;
        }
        self.bulk_pending += groups.len();
        for (src, uids) in groups {
            self.send_to(
                dest_account,
                MailRequest::MoveMessages { path: src, uids, dest: dest.clone() },
            );
        }
        self.message_list.emit(MessageListInput::RemoveMany(removed_ids));
        self.push_unread_counts();
    }

    /// The mailbox namespace prefix for an account (e.g. "INBOX." if its folders
    /// nest under INBOX, otherwise ""), derived from an existing sub-folder.
    fn folder_namespace(&self, account_id: u32) -> String {
        self.folders
            .get(&account_id)
            .map(|folders| {
                folders
                    .iter()
                    .filter(|f| f.kind != FolderKind::Inbox && !f.name.is_empty())
                    .find_map(|f| {
                        f.path
                            .strip_suffix(&f.name)
                            .filter(|p| !p.is_empty())
                            .map(|p| p.to_string())
                    })
                    .unwrap_or_default()
            })
            .unwrap_or_default()
    }

    /// A sensible destination path for a standard folder the account doesn't have
    /// yet (Archive/Trash/Junk), matching the account's folder namespace so the
    /// server creates it in the right place (e.g. "INBOX.Archive").
    fn default_folder_path(&self, account_id: u32, kind: FolderKind) -> Option<String> {
        let leaf = match kind {
            FolderKind::Archive => "Archive",
            FolderKind::Trash => "Trash",
            FolderKind::Junk => "Junk",
            FolderKind::Drafts => "Drafts",
            _ => return None,
        };
        self.folders.get(&account_id)?; // require folders to be loaded
        Some(format!("{}{leaf}", self.folder_namespace(account_id)))
    }

    /// Find a cached message by (account, id) for drag-and-drop moves.
    fn find_cached_message(&self, account_id: u32, id: u32) -> Option<Message> {
        for ((aid, _), msgs) in self.message_cache.iter() {
            if *aid == account_id {
                if let Some(m) = msgs.iter().find(|m| m.id == id) {
                    return Some(m.clone());
                }
            }
        }
        self.unified_by_account
            .get(&account_id)
            .and_then(|msgs| msgs.iter().find(|m| m.id == id).cloned())
    }

    /// Explain how to set up the system keyring (Secret Service) so passwords
    /// persist across restarts, and — on Linux Mint / Cinnamon — how to stop the
    /// keyring asking for an unlock password at every login.
    ///
    /// `problem` is true when this is shown because a save actually failed;
    /// false for the proactive one-time tip (which offers "Don't show again").
    fn show_keyring_help(&self, problem: bool) {
        let mint = crate::platform::is_mint_cinnamon();

        let heading = if problem {
            "Vireo couldn’t save your password"
        } else {
            "Keyring setup on Linux Mint"
        };

        let mut body = String::new();
        if problem {
            body.push_str(
                "Vireo stores account passwords in the system keyring (the Secret \
                 Service), never on disk. The keyring didn’t accept the password, so \
                 this account won’t stay signed in after you close Vireo.\n\n",
            );
        } else {
            body.push_str(
                "Vireo keeps your account passwords in the system keyring (the Secret \
                 Service) rather than on disk. On Linux Mint with Cinnamon the keyring \
                 sometimes needs a one-time setup so passwords persist — and so it \
                 doesn’t ask you to unlock it at every login.\n\n",
            );
        }

        if mint {
            body.push_str(
                "Set it up:\n\
                 1. Install the keyring tools if needed:\n\
                 \u{2003}sudo apt install gnome-keyring seahorse\n\
                 2. Open “Passwords and Keys” (Seahorse) and make sure a keyring named \
                 “Login” exists and is set as Default (right-click → Set as Default).\n\n\
                 Stop it asking for a password at each login — pick one:\n\
                 • Recommended: set the Login keyring’s password to match your user \
                 login password (right-click the Login keyring → Change Password), and \
                 log in with your password rather than using automatic login. The \
                 keyring then unlocks automatically when you log in.\n\
                 • Or, to remove the prompt entirely even with automatic login: set the \
                 Login keyring’s password to blank (Change Password → leave the new \
                 password empty). This is convenient, but your saved passwords are then \
                 stored unencrypted at rest — only do this on a machine you trust.",
            );
            if crate::platform::is_flatpak() {
                body.push_str(
                    "\n\nNote: run these steps on the host system (not inside the \
                     Flatpak) — Vireo uses whatever keyring your desktop provides.",
                );
            }
        } else {
            body.push_str(
                "Make sure a Secret Service keyring is installed, running, and \
                 unlocked — for example install “gnome-keyring” and “seahorse” \
                 (Passwords and Keys), then create a default “Login” keyring and set \
                 its password to your login password so it unlocks automatically.",
            );
        }

        let dialog = adw::MessageDialog::new(Some(&self.window), Some(heading), Some(&body));
        dialog.add_response("ok", "Got it");
        dialog.set_default_response(Some("ok"));
        // The proactive tip is a one-time thing: mark it seen once dismissed (by
        // any means) so it never nags again. A real save failure always shows.
        if !problem {
            dialog.connect_response(None, |_, _| config::dismiss_mint_keyring_help());
        }
        dialog.present();
    }

    /// Prompt for a new custom folder name and create it under `account_id`.
    fn prompt_new_folder(&self, account_id: u32, sender: &ComponentSender<Self>) {
        let dialog = adw::MessageDialog::new(
            Some(&self.window),
            Some("New Folder"),
            Some("Create a new folder for this account."),
        );
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("ok", "Create");
        dialog.set_default_response(Some("ok"));
        dialog.set_response_appearance("ok", adw::ResponseAppearance::Suggested);
        let entry = gtk::Entry::new();
        entry.set_placeholder_text(Some("Folder name"));
        entry.set_activates_default(true);
        dialog.set_extra_child(Some(&entry));
        let s = sender.clone();
        dialog.connect_response(None, move |_, resp| {
            if resp == "ok" {
                let name = entry.text().to_string();
                if !name.trim().is_empty() {
                    s.input(AppMsg::CreateFolder { account_id, name });
                }
            }
        });
        dialog.present();
    }

    /// Confirm deleting a custom folder (contents moved to Trash).
    fn confirm_delete_folder(
        &self,
        account_id: u32,
        name: String,
        path: String,
        sender: &ComponentSender<Self>,
    ) {
        let dialog = adw::MessageDialog::new(
            Some(&self.window),
            Some(&format!("Delete “{name}”?")),
            Some("Its messages are moved to Trash and the folder is removed."),
        );
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("delete", "Delete");
        dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
        let s = sender.clone();
        dialog.connect_response(None, move |_, resp| {
            if resp == "delete" {
                s.input(AppMsg::DeleteFolder {
                    account_id,
                    path: path.clone(),
                });
            }
        });
        dialog.present();
    }

    /// Mark `m` as spam: tag `$Junk` and move it to the account's Junk folder
    /// (so the server's spam filter can learn from it).
    fn mark_spam_msg(&mut self, m: Message) {
        let Some(src) = self.resolve_folder_path(&m) else {
            return;
        };
        let dest = self
            .folders
            .get(&m.account_id)
            .and_then(|fs| fs.iter().find(|f| f.kind == FolderKind::Junk))
            .map(|f| f.path.clone())
            .or_else(|| self.default_folder_path(m.account_id, FolderKind::Junk));
        let Some(dest) = dest else {
            self.notifications.emit(NotifyInput::Push {
                text: "No Junk folder available for this account".to_string(),
                error: true,
                connectivity: false,
            });
            return;
        };
        self.send_to(m.account_id, MailRequest::MarkSpam { path: src, uid: m.uid, dest });
        self.discard_message(&m);
    }

    fn mark_spam(&mut self) {
        if let Some(m) = self.current.clone() {
            self.mark_spam_msg(m);
        }
    }

    /// Mark every message in a folder as read: update server, caches, badges,
    /// and the displayed list.
    fn mark_folder_read(&mut self, account_id: u32, folder_id: u32) {
        let Some(path) = self
            .folders
            .get(&account_id)
            .and_then(|fs| fs.iter().find(|f| f.id == folder_id))
            .map(|f| f.path.clone())
        else {
            return;
        };
        // Optimistic in-memory update so the UI reacts instantly.
        if let Some(msgs) = self.message_cache.get_mut(&(account_id, folder_id)) {
            for m in msgs {
                m.unread = false;
            }
        }
        if self.inbox_of(account_id).map(|f| f.id) == Some(folder_id) {
            if let Some(msgs) = self.unified_by_account.get_mut(&account_id) {
                for m in msgs {
                    m.unread = false;
                }
            }
        }
        self.folder_unread.insert((account_id, folder_id), 0);
        self.send_to(account_id, MailRequest::MarkAllRead { folder_id, path });
        self.refresh_list_display();
        self.push_unread_counts();
    }

    /// Re-emit the currently-visible folder/unified list from the caches, so
    /// in-place changes (e.g. mark-all-read) show without a server round-trip.
    /// How many assembled conversations are kept. Each holds its messages'
    /// bodies, so this is a memory budget as much as a cache size.
    const THREAD_CACHE_MAX: usize = 8;

    /// Store the conversation on screen so returning to it is instant.
    fn remember_thread(&mut self) {
        let Some(key) = self.thread_key else { return };
        if self.current_thread.len() <= 1 {
            return;
        }
        if self.thread_cache.insert(key, self.current_thread.clone()).is_none() {
            self.thread_cache_order.push(key);
        }
        while self.thread_cache_order.len() > Self::THREAD_CACHE_MAX {
            let oldest = self.thread_cache_order.remove(0);
            self.thread_cache.remove(&oldest);
        }
    }

    /// Forget assembled conversations for an account — its mail has changed
    /// underneath them, so what they hold may no longer be the conversation.
    fn forget_threads(&mut self, account_id: u32) {
        self.thread_cache.retain(|(aid, _), _| *aid != account_id);
        self.thread_cache_order.retain(|(aid, _)| *aid != account_id);
    }

    /// Reply headers for every cached message, so the list can see that two
    /// replies in a folder answer the same message in another one.
    fn push_thread_links(&self) {
        // Newest first, and capped: threading the whole mailbox would otherwise
        // put every message's reply headers here, to be cloned on each sync and
        // walked on each rebuild. A conversation is joined by mail near it in
        // time, so the newest links are the ones that do the joining.
        let mut recent: Vec<&Message> = self
            .message_cache
            .values()
            .flatten()
            .filter(|m| !(m.message_id.is_empty() && m.references.is_empty()))
            .collect();
        recent.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        recent.truncate(THREAD_LINK_LIMIT);
        let mut links: Vec<(u32, String, String)> = recent
            .into_iter()
            .map(|m| (m.account_id, m.message_id.clone(), m.references.clone()))
            .collect();
        links.sort();
        links.dedup();
        self.message_list.emit(MessageListInput::SetThreadLinks(links));
    }

    /// Push every account's inbox slice to the list as one date-sorted run. The
    /// slices arrive independently (cache seed, then each account's load), so the
    /// whole merged list is re-emitted each time one of them changes.
    fn emit_unified(&self) {
        let mut merged: Vec<Message> =
            self.unified_by_account.values().flatten().cloned().collect();
        merged.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        self.message_list.emit(MessageListInput::SetMessages {
            title: "All Inboxes".into(),
            messages: merged,
        });
    }

    fn refresh_list_display(&self) {
        if self.unified {
            self.emit_unified();
        } else if let Some(sel) = self.selected.as_ref() {
            if let Some(msgs) = self.message_cache.get(&(sel.account_id, sel.folder_id)) {
                self.message_list.emit(MessageListInput::SetMessages {
                    title: sel.name.clone(),
                    messages: msgs.clone(),
                });
            }
        }
    }

    /// Modal contacts browser: pick a contact to start a new message to them.
    fn show_contacts_window(&self, sender: &ComponentSender<Self>) {
        let input = sender.input_sender().clone();
        crate::ui::contacts_browser::present(&self.window, move |contact| {
            let _ = input.send(AppMsg::ComposeTo(contact.email));
        });
    }

    /// Dialog to add an email to GNOME Contacts (choosing the address book).
    fn show_add_contact_dialog(&self, name: &str, email: &str, sender: &ComponentSender<Self>) {
        let books = crate::contacts::writable_books();
        if books.is_empty() || email.trim().is_empty() {
            self.notifications.emit(NotifyInput::Push {
                text: "No address book available to add contacts".to_string(),
                error: true,
                connectivity: false,
            });
            return;
        }

        let dialog = adw::MessageDialog::new(
            Some(&self.window),
            Some("Add to Contacts"),
            None,
        );
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("add", "Add");
        dialog.set_response_appearance("add", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("add"));
        dialog.set_close_response("cancel");

        let form = gtk::ListBox::new();
        form.add_css_class("boxed-list");
        form.set_selection_mode(gtk::SelectionMode::None);
        let name_row = adw::EntryRow::new();
        name_row.set_title("Name");
        name_row.set_text(name);
        let email_row = adw::EntryRow::new();
        email_row.set_title("Email");
        email_row.set_text(email);
        let book_row = adw::ComboRow::new();
        book_row.set_title("Address book");
        let labels: Vec<&str> = books.iter().map(|b| b.name.as_str()).collect();
        book_row.set_model(Some(&gtk::StringList::new(&labels)));
        form.append(&name_row);
        form.append(&email_row);
        form.append(&book_row);
        dialog.set_extra_child(Some(&form));

        let input = sender.input_sender().clone();
        dialog.connect_response(None, move |_, resp| {
            if resp != "add" {
                return;
            }
            let name = name_row.text().trim().to_string();
            let email = email_row.text().trim().to_string();
            let idx = book_row.selected() as usize;
            let Some(book) = books.get(idx).cloned() else {
                return;
            };
            if email.is_empty() {
                return;
            }
            // Writing talks to EDS over D-Bus (blocking) — do it off the UI thread.
            let input = input.clone();
            std::thread::spawn(move || {
                let result = crate::contacts::add_or_merge(&book.uid, &name, &email);
                let _ = input.send(AppMsg::ContactAdded(result));
            });
        });
        dialog.present();
    }

    /// Confirm and remove an account (drops its keyring password too).
    /// Open (or focus) the accounts window. When `add_new`, jump straight to the
    /// "add account" form — used by the empty-state "Add first account" button.
    fn open_accounts_window(&mut self, sender: &ComponentSender<Self>, add_new: bool) {
        // Already open? Bring it forward instead of opening another.
        if let Some(w) = self.accounts_win.as_ref().filter(|w| w.widget().is_visible()) {
            w.widget().present();
            if add_new {
                w.emit(crate::ui::accounts::AccountsInput::AddAccount);
            }
            return;
        }
        // Pass accounts in display order, with passwords prefilled from the keyring
        // so the editor shows them when editing.
        let order = self.ordered_emails();
        let mut accounts: Vec<AccountConfig> = Vec::new();
        for email in &order {
            if let Some(a) = self.config.iter().find(|c| &c.email == email) {
                accounts.push(a.clone());
            }
        }
        for c in &self.config {
            if !accounts.iter().any(|a| a.email == c.email) {
                accounts.push(c.clone());
            }
        }
        for a in &mut accounts {
            if a.password.is_empty() {
                a.password = config::load_password(&a.email).unwrap_or_default();
            }
            if a.smtp_separate && a.smtp_password.is_empty() {
                a.smtp_password = config::load_smtp_password(&a.email).unwrap_or_default();
            }
        }
        let win = AccountsWindow::builder()
            .transient_for(&self.window)
            .launch(accounts)
            .forward(sender.input_sender(), |out| match out {
                AccountsOutput::Saved { original_email, account } => {
                    AppMsg::AccountSaved { original_email, account }
                }
                AccountsOutput::Removed { email } => AppMsg::AccountRemoved { email },
                AccountsOutput::Reordered(emails) => AppMsg::AccountsReordered(emails),
                AccountsOutput::EnabledChanged { email, enabled } => {
                    AppMsg::AccountEnabledChanged { email, enabled }
                }
                AccountsOutput::ImportGoa(account) => AppMsg::ImportGoaAccount(account),
                AccountsOutput::Closed => AppMsg::CloseAccounts,
            });
        if add_new {
            win.emit(crate::ui::accounts::AccountsInput::AddAccount);
        }
        win.widget().present();
        self.accounts_win = Some(win);
    }

    fn confirm_remove_account(&self, account_id: u32, sender: &ComponentSender<Self>) {
        let Some(email) = self.email_of(account_id) else {
            return;
        };
        let label = self.account_label(account_id);
        let dialog = adw::MessageDialog::new(
            Some(&self.window),
            Some("Remove Account?"),
            Some(&format!(
                "Remove {label} from Vireo? Its saved password is deleted. \
                 Mail on the server is not affected."
            )),
        );
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("remove", "Remove");
        dialog.set_response_appearance("remove", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        let s = sender.clone();
        dialog.connect_response(None, move |_, resp| {
            if resp == "remove" {
                s.input(AppMsg::AccountRemoved { email: email.clone() });
            }
        });
        dialog.present();
    }

    /// A custom, scrollable About window: app identity up top (icon, name, a
    /// version chip, and a one-line description under it), then the feature
    /// sections laid out on the page itself, and project links.
    fn open_about(&self, sender: &ComponentSender<Self>) {
        let win = adw::Window::builder()
            .transient_for(&self.window)
            .modal(false)
            .title("About Vireo")
            .default_width(460)
            .default_height(640)
            .build();

        // A navigation stack so Release Notes / Changelog slide in (and back out)
        // within the same window instead of spawning separate ones.
        let nav = adw::NavigationView::new();

        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();
        let clamp = adw::Clamp::builder().maximum_size(420).build();
        let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        page.set_margin_top(18);
        page.set_margin_bottom(12);

        // Identity block.
        let icon = gtk::Image::from_icon_name(crate::APP_ID);
        icon.set_pixel_size(96);
        icon.set_margin_bottom(10);
        page.append(&icon);

        let name = gtk::Label::new(Some("Vireo"));
        name.add_css_class("title-1");
        page.append(&name);

        let version = gtk::Label::new(Some(env!("CARGO_PKG_VERSION")));
        version.add_css_class("about-version-chip");
        version.set_halign(gtk::Align::Center);
        version.set_margin_top(8);
        page.append(&version);

        // One-sentence description, directly under the version chip.
        let desc = gtk::Label::new(Some("A clean, fast, GNOME-native email client."));
        desc.set_wrap(true);
        desc.set_justify(gtk::Justification::Center);
        desc.add_css_class("dim-label");
        desc.set_margin_top(12);
        page.append(&desc);

        // Release notes: slide in as a sub-page of this window.
        let info = gtk::ListBox::new();
        info.add_css_class("boxed-list");
        info.set_selection_mode(gtk::SelectionMode::None);
        info.set_margin_top(20);

        let notes_row = adw::ActionRow::builder()
            .title("Release Notes")
            .subtitle(format!("What's new in {}", env!("CARGO_PKG_VERSION")))
            .activatable(true)
            .build();
        notes_row.add_suffix(&gtk::Image::from_icon_name("co.hyprlab.Vireo-go-next-symbolic"));
        {
            let nav = nav.clone();
            notes_row.connect_activated(move |_| nav.push_by_tag("notes"));
        }
        info.append(&notes_row);

        let changelog_row = adw::ActionRow::builder()
            .title("Changelog")
            .subtitle("Full version history")
            .activatable(true)
            .build();
        changelog_row.add_suffix(&gtk::Image::from_icon_name("co.hyprlab.Vireo-go-next-symbolic"));
        {
            let nav = nav.clone();
            changelog_row.connect_activated(move |_| nav.push_by_tag("changelog"));
        }
        info.append(&changelog_row);

        // Linux Mint (Cinnamon): re-open the one-time keyring setup tip. Shown only
        // where that tip applies, so Mint users who dismissed it can find it again.
        if crate::platform::is_mint_cinnamon() {
            let keyring_row = adw::ActionRow::builder()
                .title("Keyring Setup Help")
                .subtitle("Make account passwords persist on Linux Mint")
                .activatable(true)
                .build();
            keyring_row.add_suffix(&gtk::Image::from_icon_name("co.hyprlab.Vireo-go-next-symbolic"));
            let sender = sender.clone();
            keyring_row.connect_activated(move |_| {
                sender.input(AppMsg::ShowKeyringHelp { problem: false });
            });
            info.append(&keyring_row);
        }

        page.append(&info);

        // Project links. Each row shows its URL as a hover tooltip.
        let links_title = gtk::Label::new(Some("Project"));
        links_title.add_css_class("heading");
        links_title.set_halign(gtk::Align::Start);
        links_title.set_margin_top(20);
        links_title.set_margin_bottom(6);
        page.append(&links_title);

        let links = gtk::ListBox::new();
        links.add_css_class("boxed-list");
        links.set_selection_mode(gtk::SelectionMode::None);
        let mk_row = |title: &str, url: &str| -> adw::ActionRow {
            let row = adw::ActionRow::builder().title(title).activatable(true).build();
            row.set_tooltip_text(Some(url));
            row.add_suffix(&gtk::Image::from_icon_name("co.hyprlab.Vireo-adw-external-link-symbolic"));
            let u = url.to_string();
            row.connect_activated(move |_| crate::oauth::open_uri(&u));
            row
        };
        links.append(&mk_row("Website", "https://vireo.hyprlab.co"));
        links.append(&mk_row("Contact — hyprlab@proton.me", "mailto:hyprlab@proton.me"));
        links.append(&mk_row("Source Code", "https://github.com/hyprlab/vireo"));
        links.append(&mk_row("License (GNU AGPL v3)", "https://www.gnu.org/licenses/agpl-3.0.html"));

        // Buy Me a Coffee — with a coffee-cup glyph as its leading icon.
        let coffee = adw::ActionRow::builder()
            .title("Buy Me a Coffee")
            .activatable(true)
            .build();
        coffee.set_tooltip_text(Some("https://buymeacoffee.com/hyprlab"));
        let cup = gtk::Label::new(Some("☕"));
        cup.add_css_class("about-coffee");
        coffee.add_prefix(&cup);
        coffee.add_suffix(&gtk::Image::from_icon_name("co.hyprlab.Vireo-adw-external-link-symbolic"));
        coffee.connect_activated(move |_| crate::oauth::open_uri("https://buymeacoffee.com/hyprlab"));
        links.append(&coffee);
        page.append(&links);

        // Contributors — people outside Hyprlab whose patches are in the app.
        let thanks_title = gtk::Label::new(Some("Thanks"));
        thanks_title.add_css_class("heading");
        thanks_title.set_halign(gtk::Align::Start);
        thanks_title.set_margin_top(20);
        thanks_title.set_margin_bottom(6);
        page.append(&thanks_title);

        let thanks = gtk::ListBox::new();
        thanks.add_css_class("boxed-list");
        thanks.set_selection_mode(gtk::SelectionMode::None);
        for (name, handle, what) in CONTRIBUTORS {
            let row = adw::ActionRow::builder()
                .title(*name)
                .subtitle(format!("@{handle} — {what}"))
                .activatable(true)
                .build();
            let url = format!("https://github.com/{handle}");
            row.set_tooltip_text(Some(&url));
            row.add_suffix(&gtk::Image::from_icon_name("co.hyprlab.Vireo-adw-external-link-symbolic"));
            row.connect_activated(move |_| crate::oauth::open_uri(&url));
            thanks.append(&row);
        }
        page.append(&thanks);

        // Footer.
        let footer = gtk::Label::new(Some("© 2026 Hyprlab"));
        footer.add_css_class("dim-label");
        footer.add_css_class("caption");
        footer.set_wrap(true);
        footer.set_justify(gtk::Justification::Center);
        footer.set_margin_top(20);
        page.append(&footer);

        clamp.set_child(Some(&page));
        scroller.set_child(Some(&clamp));

        // The root page holds the identity + links; the sub-pages slide over it.
        let main_tv = adw::ToolbarView::new();
        let main_header = adw::HeaderBar::new();
        main_header.add_css_class("flat");
        main_tv.add_top_bar(&main_header);
        main_tv.set_content(Some(&scroller));
        nav.add(
            &adw::NavigationPage::builder()
                .title("About Vireo")
                .tag("main")
                .child(&main_tv)
                .build(),
        );
        nav.add(&release_notes_page());
        nav.add(&changelog_page());

        win.set_content(Some(&nav));
        win.present();
    }

    /// Star/unstar a message, updating the server, the list, and the reader.
    fn set_star(&mut self, m: &Message, starred: bool) {
        let Some(path) = self.resolve_folder_path(m) else {
            return;
        };
        self.send_to(m.account_id, MailRequest::SetFlagged { path, uid: m.uid, flagged: starred });
        self.message_list
            .emit(MessageListInput::SetStarred { id: m.id, starred });
        if let Some(cur) = self.current.as_mut() {
            if cur.id == m.id && cur.account_id == m.account_id {
                cur.starred = starred;
            }
        }
        if self.current.as_ref().is_some_and(|c| c.id == m.id && c.account_id == m.account_id) {
            let current = self.current.clone();
            self.show_message(current, false);
        }
        if let Some(p) = self.popouts.get(&(m.account_id, m.id)) {
            p.controller.emit(MessageWindowInput::SetStarred(starred));
        }
    }

    /// Mark a message read/unread, updating the server, list, badges, and reader.
    fn set_read(&mut self, m: &Message, read: bool) {
        // No-op if it's already in the requested state.
        if read != m.unread {
            return;
        }
        let Some(path) = self.resolve_folder_path(m) else {
            return;
        };
        self.send_to(m.account_id, MailRequest::SetSeen { path, uid: m.uid, seen: read });
        self.message_list
            .emit(MessageListInput::SetRead { id: m.id, read });
        self.set_cached_unread(m.account_id, m.id, !read);
        if let Some(n) = self.folder_unread.get_mut(&(m.account_id, m.folder_id)) {
            if read {
                *n = n.saturating_sub(1);
            } else {
                *n += 1;
            }
        }
        if let Some(cur) = self.current.as_mut() {
            if cur.id == m.id && cur.account_id == m.account_id {
                cur.unread = !read;
            }
        }
        // The reader shows a conversation's unread marks, so a message whose
        // state changed while its thread is open has to be told about it —
        // otherwise marking one unread does nothing visible until the thread is
        // opened again.
        let in_thread = self
            .current_thread
            .iter_mut()
            .filter(|tm| tm.id == m.id && tm.account_id == m.account_id)
            .fold(false, |_, tm| {
                tm.unread = !read;
                true
            });
        if in_thread {
            self.remember_thread();
        }
        if in_thread && self.current_thread.len() > 1 {
            if read {
                self.show_thread();
            } else {
                // Marked unread deliberately: the reader keeps the mark until
                // this conversation is opened afresh, so a message sitting in
                // view can't immediately undo it.
                self.message_view.emit(MessageViewInput::SuppressAutoRead {
                    account_id: m.account_id,
                    id: m.id,
                });
            }
        }
        self.push_unread_counts();
    }

    /// Fill a message's body from the cache if it isn't already loaded, so
    /// reply/forward from the context menu can quote it when available.
    fn with_cached_body(&self, mut m: Message) -> Message {
        if m.body.is_empty() {
            if let Some(b) = self.body_cache.get(&(m.account_id, m.id)) {
                m.body = b.clone();
            }
        }
        m
    }

    /// Remove a handled message from the list, caches, and badges. Clears the
    /// reader only if that message was the one open. Shared by archive/delete/spam.
    /// Apply a removing bulk action (archive/delete/spam) to many messages at once.
    /// Messages are grouped by (account, source folder) and each group is moved in a
    /// SINGLE `MoveMessages` request (one server-side UID MOVE) — far faster and more
    /// reliable than one request per message, which on a huge mailbox (e.g. Gmail's
    /// All Mail) is slow and drops moves when the connection blips. The list is
    /// updated once (`RemoveMany`); the spinner clears when every group's worker
    /// reports `BulkComplete`.
    fn apply_bulk_move(&mut self, action: BulkAction, messages: Vec<Message>) {
        let kind = match action {
            BulkAction::Archive => FolderKind::Archive,
            BulkAction::Delete => FolderKind::Trash,
            BulkAction::Spam => FolderKind::Junk,
            // Non-removing actions never reach here (handled inline).
            BulkAction::MarkRead | BulkAction::MarkUnread | BulkAction::Flag => return,
        };
        // (account, source path) → (dest path, uids). dest is per-account.
        let mut groups: HashMap<(u32, String), (String, Vec<u32>)> = HashMap::new();
        let mut removed_ids = Vec::with_capacity(messages.len());
        let mut missing_dest = false;
        for m in &messages {
            let Some(src) = self.resolve_folder_path(m) else { continue };
            let Some(dest) = self.folder_path_for(m.account_id, kind) else {
                missing_dest = true;
                continue;
            };
            if src == dest {
                continue;
            }
            groups
                .entry((m.account_id, src))
                .or_insert_with(|| (dest, Vec::new()))
                .1
                .push(m.uid);
            self.discard_message_local(m);
            removed_ids.push(m.id);
        }
        if missing_dest {
            self.notifications.emit(NotifyInput::Push {
                text: format!("No {} folder available for some messages", kind_label(kind)),
                error: true,
                connectivity: false,
            });
        }
        self.bulk_pending += groups.len();
        for ((account_id, src), (dest, uids)) in groups {
            self.send_to(account_id, MailRequest::MoveMessages { path: src, uids, dest });
        }
        self.message_list.emit(MessageListInput::RemoveMany(removed_ids));
        self.push_unread_counts();
    }

    /// Optimistic local cleanup when a message leaves the current folder: close its
    /// popout, drop it from the in-memory caches and the unread count, and clear the
    /// reader if it was the viewed message. Does NOT touch the list widget or push
    /// unread counts — the caller does that (single delete via `discard_message`;
    /// bulk via one `RemoveMany` + one push in `apply_bulk_move`).
    fn discard_message_local(&mut self, m: &Message) {
        self.forget_threads(m.account_id);
        if let Some(p) = self.popouts.get(&(m.account_id, m.id)) {
            p.window.close();
        }
        if let Some(msgs) = self.unified_by_account.get_mut(&m.account_id) {
            msgs.retain(|x| x.uid != m.uid);
        }
        if let Some(msgs) = self.message_cache.get_mut(&(m.account_id, m.folder_id)) {
            msgs.retain(|x| x.uid != m.uid);
        }
        if m.unread {
            if let Some(n) = self.folder_unread.get_mut(&(m.account_id, m.folder_id)) {
                *n = n.saturating_sub(1);
            }
        }
        if self.current.as_ref().is_some_and(|c| c.id == m.id && c.account_id == m.account_id) {
            // Drop the reader's view state, but DON'T blank it here: the list's
            // Remove handler advances to the next message (or emits SelectionCleared
            // when the folder is empty), which drives the reader. Blanking now would
            // flash "No message selected" before the next one loads.
            self.current = None;
            self.current_thread.clear();
            self.attachments.clear();
            self.sync_attachment_drawer();
            self.attachments_loading = false;
            self.attachments_available = false;
        }
    }

    fn discard_message(&mut self, m: &Message) {
        self.discard_message_local(m);
        self.message_list.emit(MessageListInput::Remove(m.id));
        self.push_unread_counts();
    }

    /// Whether a sender address matches the blacklist (exact address, or a bare
    /// domain entry matching the sender's domain or any subdomain of it).
    fn is_blacklisted(&self, addr: &str) -> bool {
        let a = addr.trim().to_lowercase();
        if a.is_empty() {
            return false;
        }
        let domain = a.rsplit('@').next().unwrap_or("");
        self.blacklist.iter().any(|entry| {
            if entry.contains('@') {
                a == *entry
            } else {
                domain == entry.as_str() || domain.ends_with(&format!(".{entry}"))
            }
        })
    }

    /// Move any blacklisted senders in an inbox sync to Trash, returning the rest.
    fn apply_blacklist(
        &self,
        account_id: u32,
        folder_id: u32,
        messages: Vec<Message>,
    ) -> Vec<Message> {
        if self.blacklist.is_empty()
            || self.inbox_of(account_id).map(|f| f.id) != Some(folder_id)
        {
            return messages;
        }
        let folders = self.folders.get(&account_id);
        let trash = folders
            .and_then(|fs| fs.iter().find(|f| f.kind == FolderKind::Trash))
            .map(|f| f.path.clone());
        let src = folders
            .and_then(|fs| fs.iter().find(|f| f.id == folder_id))
            .map(|f| f.path.clone());
        let mut kept = Vec::with_capacity(messages.len());
        for m in messages {
            if self.is_blacklisted(&m.from_addr) {
                if let (Some(trash), Some(src)) = (&trash, &src) {
                    self.send_to(account_id, MailRequest::MoveMessage {
                        path: src.clone(),
                        uid: m.uid,
                        dest: trash.clone(),
                    });
                }
            } else {
                kept.push(m);
            }
        }
        kept
    }

    /// Re-sync every inbox so a newly-blacklisted sender's existing mail is
    /// caught and deleted by [`apply_blacklist`].
    fn sweep_blacklisted(&self) {
        let reqs: Vec<(u32, u32, String)> = self
            .accounts
            .iter()
            .filter_map(|a| self.inbox_of(a.id).map(|f| (a.id, f.id, f.path.clone())))
            .collect();
        for (account_id, folder_id, path) in reqs {
            self.send_to(account_id, MailRequest::LoadMessages { folder_id, path });
        }
    }

    /// The IMAP folder path a message lives in (its account's folder by id).
    fn resolve_folder_path(&self, m: &Message) -> Option<String> {
        self.folders
            .get(&m.account_id)?
            .iter()
            .find(|f| f.id == m.folder_id)
            .map(|f| f.path.clone())
    }

    /// An account's Inbox folder, if known.
    fn inbox_of(&self, account_id: u32) -> Option<&Folder> {
        self.folders
            .get(&account_id)?
            .iter()
            .find(|f| f.kind == FolderKind::Inbox)
    }

    /// Server-side unread count for an account's inbox.
    fn inbox_unread(&self, account_id: u32) -> u32 {
        self.inbox_of(account_id)
            .and_then(|inbox| self.folder_unread.get(&(account_id, inbox.id)))
            .copied()
            .unwrap_or(0)
    }

    /// Mark a cached message read in every list that holds it, so unread badges
    /// update immediately without waiting for the next server sync.
    fn mark_cached_read(&mut self, account_id: u32, message_id: u32) {
        self.set_cached_unread(account_id, message_id, false);
    }

    /// Set a cached message's unread flag in every list that holds it.
    fn set_cached_unread(&mut self, account_id: u32, message_id: u32, unread: bool) {
        for ((aid, _), msgs) in self.message_cache.iter_mut() {
            if *aid == account_id {
                if let Some(m) = msgs.iter_mut().find(|m| m.id == message_id) {
                    m.unread = unread;
                }
            }
        }
        if let Some(msgs) = self.unified_by_account.get_mut(&account_id) {
            if let Some(m) = msgs.iter_mut().find(|m| m.id == message_id) {
                m.unread = unread;
            }
        }
    }

    /// Title for the message-list pane header.
    fn pane_title(&self) -> &str {
        if self.unified {
            "All Inboxes"
        } else {
            self.selected.as_ref().map(|s| s.name.as_str()).unwrap_or("Mailbox")
        }
    }
}

/// The About window's "Release Notes" page, rendered from the single source of
/// truth — `RELEASE_NOTES.md` at the repo root, which is also used verbatim for
/// the GitHub release, so the notes stay identical everywhere.
fn release_notes_page() -> adw::NavigationPage {
    notes_page("Release Notes", "notes", include_str!("../RELEASE_NOTES.md"))
}

/// The About window's "Changelog" page, from the centralized `CHANGELOG.md` — so
/// the version history updates everywhere from one file.
fn changelog_page() -> adw::NavigationPage {
    notes_page("Changelog", "changelog", include_str!("../CHANGELOG.md"))
}

/// Inline Markdown → Pango markup: `**bold**`, `*italic*`, `` `code` `` and
/// `[text](url)`. Everything outside a marker is escaped, so the source may
/// contain `&` or `<` safely. Emphasis nests (a bold link, a link with code in
/// its text); an unclosed marker stays the literal character it is.
fn md_inline(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let joined = |from: usize, to: usize| -> String { chars[from..to].iter().collect() };
    // Where `needle` next starts, at or after `from`.
    let find = |from: usize, needle: &[char]| -> Option<usize> {
        if needle.len() > chars.len() {
            return None;
        }
        (from..=chars.len() - needle.len()).find(|&i| chars[i..i + needle.len()] == *needle)
    };

    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        // Code spans first: their contents are literal, never re-parsed.
        if chars[i] == '`' {
            if let Some(end) = find(i + 1, &['`']) {
                out.push_str("<tt>");
                out.push_str(&gtk::glib::markup_escape_text(&joined(i + 1, end)));
                out.push_str("</tt>");
                i = end + 1;
                continue;
            }
        } else if chars[i..].starts_with(&['*', '*']) {
            if let Some(end) = find(i + 2, &['*', '*']) {
                out.push_str("<b>");
                out.push_str(&md_inline(&joined(i + 2, end)));
                out.push_str("</b>");
                i = end + 2;
                continue;
            }
        } else if chars[i] == '*' {
            if let Some(end) = find(i + 1, &['*']) {
                out.push_str("<i>");
                out.push_str(&md_inline(&joined(i + 1, end)));
                out.push_str("</i>");
                i = end + 1;
                continue;
            }
        } else if chars[i] == '[' {
            if let Some(close) = find(i + 1, &[']']) {
                if chars.get(close + 1) == Some(&'(') {
                    if let Some(paren) = find(close + 2, &[')']) {
                        out.push_str(&format!(
                            "<a href=\"{}\">{}</a>",
                            gtk::glib::markup_escape_text(&joined(close + 2, paren)),
                            md_inline(&joined(i + 1, close)),
                        ));
                        i = paren + 1;
                        continue;
                    }
                }
            }
        }
        out.push_str(&gtk::glib::markup_escape_text(&chars[i].to_string()));
        i += 1;
    }
    out
}

/// A wrapped label carrying inline Markdown. Links are opened through the app's
/// own URI handler rather than GTK's default, which has no portal under Flatpak.
fn md_label(text: &str, classes: &[&str]) -> gtk::Label {
    let label = gtk::Label::new(None);
    label.set_markup(&md_inline(text));
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_xalign(0.0);
    label.set_halign(gtk::Align::Start);
    for class in classes {
        label.add_css_class(class);
    }
    label.connect_activate_link(|_, uri| {
        crate::oauth::open_uri(uri);
        gtk::glib::Propagation::Stop
    });
    label
}

/// Render Markdown as a column of widgets rather than one long label: a bullet
/// gets its own column so wrapped lines align under the text instead of running
/// back under the bullet, and each heading carries its own spacing. Handles
/// headings, bullets (nested one level), indented continuation paragraphs, and
/// the inline syntax in [`md_inline`].
fn md_column(md: &str) -> gtk::Box {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let mut first = true;

    for raw in md.lines() {
        let line = raw.trim_end();
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        // Blank lines carry no meaning here: spacing comes from each block's
        // own margins, so a stray one can't open a gap.
        if trimmed.is_empty() {
            continue;
        }
        // The page's header bar already shows the document's title.
        if trimmed.starts_with("# ") {
            continue;
        }

        let widget: gtk::Widget = if let Some(rest) = trimmed.strip_prefix("### ") {
            let label = md_label(rest, &["heading"]);
            label.set_margin_top(if first { 0 } else { 14 });
            label.into()
        } else if let Some(rest) = trimmed.strip_prefix("## ") {
            let label = md_label(rest, &["title-4"]);
            label.set_margin_top(if first { 0 } else { 22 });
            label.set_margin_bottom(2);
            label.into()
        } else if let Some(rest) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            row.set_margin_top(6);
            row.set_margin_start(if indent >= 2 { 18 } else { 0 });
            let bullet = gtk::Label::new(Some(if indent >= 2 { "◦" } else { "•" }));
            bullet.set_valign(gtk::Align::Start);
            bullet.add_css_class("dim-label");
            row.append(&bullet);
            let text = md_label(rest, &[]);
            text.set_hexpand(true);
            row.append(&text);
            row.into()
        } else {
            // An indented paragraph continues the bullet above it, so it lines up
            // with that bullet's text rather than the page margin.
            let label = md_label(trimmed, &[]);
            label.set_margin_top(8);
            label.set_margin_start(if indent >= 2 { 26 } else { 0 });
            label.into()
        };

        column.append(&widget);
        first = false;
    }
    column
}

/// Build a scrollable About sub-page from Markdown for the navigation stack,
/// reachable by `tag`. Pushed pages get a back button and slide animation from
/// the parent `NavigationView`.
fn notes_page(title: &str, tag: &str, md: &str) -> adw::NavigationPage {
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();
    let clamp = adw::Clamp::builder().maximum_size(460).build();

    let column = md_column(md);
    column.set_margin_top(18);
    column.set_margin_bottom(24);
    column.set_margin_start(18);
    column.set_margin_end(18);

    clamp.set_child(Some(&column));
    scroller.set_child(Some(&clamp));

    let tv = adw::ToolbarView::new();
    tv.add_top_bar(&adw::HeaderBar::new());
    tv.set_content(Some(&scroller));

    adw::NavigationPage::builder()
        .title(title)
        .tag(tag)
        .child(&tv)
        .build()
}

/// One single-key shortcut. Gmail-compatible where Gmail and the request agree
/// (issue #5); where they differ, the request wins — `a` archives here, rather
/// than replying to all as it does in Gmail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shortcut {
    NextMessage,
    PrevMessage,
    OpenMessage,
    BackToList,
    NextInThread,
    PrevInThread,
    ToggleSelect,
    Reply,
    ReplyAll,
    Forward,
    Archive,
    Delete,
    Spam,
    Star,
    ToggleRead,
    Compose,
    Search,
    Shortcuts,
}

/// The shortcut for a key press, if any. `shift` distinguishes `r` from `R`.
fn shortcut_for(key: gtk::gdk::Key, shift: bool) -> Option<Shortcut> {
    use gtk::gdk::Key;
    let action = match key {
        Key::j | Key::Down => Shortcut::NextMessage,
        Key::k | Key::Up => Shortcut::PrevMessage,
        Key::l | Key::Right => Shortcut::OpenMessage,
        Key::h | Key::Left | Key::u => Shortcut::BackToList,
        Key::w => Shortcut::NextInThread,
        Key::b => Shortcut::PrevInThread,
        Key::x => Shortcut::ToggleSelect,
        Key::r if !shift => Shortcut::Reply,
        Key::R => Shortcut::ReplyAll,
        Key::f => Shortcut::Forward,
        Key::a => Shortcut::Archive,
        Key::d => Shortcut::Delete,
        Key::exclam => Shortcut::Spam,
        Key::s => Shortcut::Star,
        Key::m => Shortcut::ToggleRead,
        Key::c => Shortcut::Compose,
        Key::slash => Shortcut::Search,
        Key::question => Shortcut::Shortcuts,
        _ => return None,
    };
    Some(action)
}

/// Every shortcut with its key and description, for the reference window.
const SHORTCUT_HELP: &[(&str, &[(&str, &str)])] = &[
    (
        "Move around",
        &[
            ("j  or  ↓", "Next message"),
            ("k  or  ↑", "Previous message"),
            ("l  or  →", "Open the selected message"),
            ("h  or  ←  or  u", "Back to the message list"),
            ("w", "Next message in the conversation"),
            ("b", "Previous message in the conversation"),
            ("/", "Search"),
        ],
    ),
    (
        "Act on a message",
        &[
            ("r", "Reply"),
            ("R", "Reply to all"),
            ("f", "Forward"),
            ("a", "Archive"),
            ("d", "Delete"),
            ("!", "Mark as spam"),
            ("s", "Star or unstar"),
            ("m", "Mark read or unread"),
            ("x", "Select this row (for a bulk action)"),
        ],
    ),
    (
        "Everything else",
        &[
            ("c", "Compose"),
            ("Esc", "Back out of a reply and return to the list"),
            ("Ctrl+P", "Print the message you are reading"),
            ("Ctrl+Shift+P", "Preview it as a PDF first"),
            ("?", "This list"),
        ],
    ),
];

/// Whether a keystroke should be left to the widget that has focus. Typing in a
/// search field, an address row or the composer must never archive mail, and the
/// message view is a web view that handles its own keys (find, scrolling).
fn focus_takes_keys(window: &adw::ApplicationWindow) -> bool {
    focus_matches(window, true)
}

/// Whether focus is in something being typed into. Narrower than
/// [`focus_takes_keys`]: Escape means "back out" while reading a message, but in
/// a search field it means "clear the search", which is the field's business.
fn focus_is_text(window: &adw::ApplicationWindow) -> bool {
    focus_matches(window, false)
}

fn focus_matches(window: &adw::ApplicationWindow, include_web_view: bool) -> bool {
    let Some(focus) = gtk::prelude::GtkWindowExt::focus(window) else {
        return false;
    };
    let mut node = Some(focus);
    while let Some(widget) = node {
        if widget.is::<gtk::Editable>() || widget.is::<gtk::TextView>() {
            return true;
        }
        if include_web_view && widget.is::<webkit6::WebView>() {
            return true;
        }
        node = widget.parent();
    }
    false
}

/// Whether to serve the built-in sample/demo data (for screenshots). Off unless
/// `VIREO_DEMO` is set, so removing all real accounts leaves the app blank.
fn demo_mode() -> bool {
    std::env::var_os("VIREO_DEMO").is_some()
}

/// What [`reconcile_goa`] changed: accounts dropped outright (their GOA account
/// is gone), and whether any account was paused or resumed because its Mail
/// service was toggled in GNOME Settings.
#[derive(Default)]
struct GoaReconcile {
    removed: Vec<String>,
    paused_changed: bool,
}

/// Reconcile imported accounts against GNOME Online Accounts: drop the ones
/// whose GOA account no longer exists, and pause — rather than remove — the ones
/// whose Mail service is switched off there, restoring their previous enabled
/// state when it comes back on. Pausing keeps every local setting (label,
/// colour, signature, sidebar state) intact. `live` is a snapshot the caller
/// obtained while GOA was reachable — when it isn't, skip reconciliation
/// entirely, so a momentarily-unavailable GOA never wipes imported accounts.
fn reconcile_goa(config: &mut Vec<AccountConfig>, live: &crate::goa::GoaLiveState) -> GoaReconcile {
    let mut outcome = GoaReconcile::default();
    config.retain(|c| match &c.goa_id {
        Some(id) if !live.account_ids.contains(id) => {
            outcome.removed.push(c.email.clone());
            false
        }
        _ => true,
    });
    for c in config.iter_mut() {
        let Some(id) = &c.goa_id else { continue };
        let mail_disabled = live.disabled_mail_ids.contains(id);
        if mail_disabled && !c.goa_mail_disabled {
            c.goa_mail_disabled = true;
            c.goa_enabled_before_mail_disabled = c.enabled;
            c.enabled = false;
            outcome.paused_changed = true;
        } else if !mail_disabled && c.goa_mail_disabled {
            c.goa_mail_disabled = false;
            c.enabled = c.goa_enabled_before_mail_disabled;
            outcome.paused_changed = true;
        }
    }
    outcome
}

/// Label for the spinner shown while a large bulk action is applied.
fn bulk_busy_label(action: BulkAction, n: usize) -> String {
    let verb = match action {
        BulkAction::Archive => "Archiving",
        BulkAction::Delete => "Deleting",
        BulkAction::Spam => "Moving to Spam",
        BulkAction::MarkRead | BulkAction::MarkUnread | BulkAction::Flag => "Updating",
    };
    format!("{verb} {n} messages…")
}

/// Trim the sidebar header in the icon-only rail so it no longer forces a minimum
/// width wider than the rail: hide the (redundant) window-control buttons and tag
/// the header so its Compose/Menu buttons shrink to fit (see `.rail-header` in the
/// stylesheet). The reader pane's header still carries the window's close button,
/// so nothing becomes unreachable.
fn set_sidebar_header_compact(
    header: &adw::HeaderBar,
    title: &gtk::Label,
    menu: &gtk::MenuButton,
    compact: bool,
) {
    // Reparenting the menu button is only valid as a transition: called twice
    // with the same value, the second `remove` would target a widget that is no
    // longer a packed child.
    if header.has_css_class("rail-header") == compact {
        return;
    }
    header.set_show_start_title_buttons(!compact);
    header.set_show_end_title_buttons(!compact);
    // In the rail there is no title to show, so the menu button takes the title
    // slot — the only position a header bar centres — instead of hugging the
    // right edge of an 80px strip. Both widgets are held by the model, so the
    // one being displaced survives being unparented here.
    if compact {
        header.add_css_class("rail-header");
        header.remove(menu);
        header.set_title_widget(Some(menu));
    } else {
        header.remove_css_class("rail-header");
        header.set_title_widget(Some(title));
        header.pack_end(menu);
    }
    title.set_visible(!compact);
}

fn open_attachment(att: &Attachment) {
    let dir = std::env::temp_dir().join("vireo-attachments");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let safe = att.name.replace(['/', '\\'], "_");
    let path = dir.join(&safe);
    if std::fs::write(&path, &att.data).is_ok() {
        let uri = format!("file://{}", path.display());
        let _ = gtk::gio::AppInfo::launch_default_for_uri(
            &uri,
            None::<&gtk::gio::AppLaunchContext>,
        );
    }
}

/// Ask for a folder and write every attachment into it.
fn save_all_attachments(atts: Vec<Attachment>, parent: Option<adw::ApplicationWindow>) {
    let dialog = gtk::FileDialog::new();
    dialog.set_title("Save All Attachments");
    dialog.select_folder(parent.as_ref(), gtk::gio::Cancellable::NONE, move |res| {
        if let Ok(folder) = res {
            if let Some(dir) = folder.path() {
                for att in &atts {
                    let safe = att.name.replace(['/', '\\'], "_");
                    let _ = std::fs::write(dir.join(&safe), &att.data);
                }
            }
        }
    });
}

/// Register the app icon so windows and dialogs can find it by name.
///
/// Vireo's toolbar/list icons are shipped inside the binary as a GResource
/// (registered in `main`), so they no longer depend on the host icon theme.
/// GTK auto-adds the bundle's resource path (`/co/hyprlab/Vireo/icons`) to the
/// default theme; we add it explicitly too, so lookups work even if that
/// convention ever changes.
fn register_icons() {
    if let Some(display) = gtk::gdk::Display::default() {
        let theme = gtk::IconTheme::for_display(&display);
        theme.add_resource_path("/co/hyprlab/Vireo/icons");
        // Dev-only: lets the window/about app icon resolve when running from the
        // source tree (uninstalled). Silently ignored on installed systems.
        theme.add_search_path(concat!(env!("CARGO_MANIFEST_DIR"), "/data/icons"));
    }
    gtk::Window::set_default_icon_name(crate::APP_ID);
}

fn map_event(account_id: u32, event: WorkerEvent) -> AppMsg {
    match event {
        WorkerEvent::BulkComplete => AppMsg::BulkComplete,
        WorkerEvent::Related { message_id, messages } => {
            AppMsg::Related { account_id, message_id, messages }
        }
        WorkerEvent::Account(a) => AppMsg::SetAccount(a),
        WorkerEvent::Folders(folders) => AppMsg::SetFolders { account_id, folders },
        WorkerEvent::Messages { folder_id, messages } => {
            AppMsg::Messages { account_id, folder_id, messages }
        }
        WorkerEvent::MessagesAppend { folder_id, messages } => {
            AppMsg::MessagesAppend { account_id, folder_id, messages }
        }
        WorkerEvent::Gallery { items } => AppMsg::GalleryItems { account_id, items },
        WorkerEvent::BackfillDone { folder_id } => AppMsg::BackfillDone { account_id, folder_id },
        WorkerEvent::FolderUnread { folder_id, unread } => {
            AppMsg::FolderUnread { account_id, folder_id, unread }
        }
        WorkerEvent::RefsRepaired { folder_id } => {
            AppMsg::RefsRepaired { account_id, folder_id }
        }
        WorkerEvent::Body { message_id, path, body } => {
            AppMsg::Body { account_id, message_id, path, body }
        }
        WorkerEvent::SenderChecked { message_id, check } => {
            AppMsg::SenderChecked { account_id, message_id, check: Box::new(check) }
        }
        WorkerEvent::Source { text, .. } => AppMsg::Source { text },
        WorkerEvent::Attachments { message_id, items } => {
            AppMsg::Attachments { account_id, message_id, items }
        }
        WorkerEvent::AttachmentsPending { message_id } => {
            AppMsg::AttachmentsPending { account_id, message_id }
        }
        WorkerEvent::NoAttachments { message_id } => {
            AppMsg::NoAttachments { account_id, message_id }
        }
        WorkerEvent::HasAttachments { message_id } => {
            AppMsg::HasAttachments { account_id, message_id }
        }
        WorkerEvent::Sent => AppMsg::Sent { account_id },
        WorkerEvent::Outbox { items } => AppMsg::OutboxItems { account_id, items },
        WorkerEvent::Notice(text) => AppMsg::Notice(text),
        WorkerEvent::DraftSaved => AppMsg::DraftSaved,
        WorkerEvent::Status(text) => AppMsg::Status { account_id, text },
        WorkerEvent::Error { text, connectivity } => {
            AppMsg::Error { account_id, text, connectivity }
        }
    }
}

fn reply_prefill(m: &Message) -> ComposePrefill {
    let subject = if m.subject.to_lowercase().starts_with("re:") {
        m.subject.clone()
    } else {
        format!("Re: {}", m.subject)
    };
    let text = message_text(&m.body);
    let attribution = format!("On {}, {} wrote:", m.date, m.from_name);
    ComposePrefill {
        to: m.from_addr.clone(),
        cc: String::new(),
        subject,
        body_html: quote_block(&attribution, &text),
        // What makes this a reply rather than a new conversation: In-Reply-To
        // names the parent, References carries the chain it belongs to.
        in_reply_to: m.message_id.clone(),
        references: thread_chain(m),
        ..Default::default()
    }
}

/// The References chain for a reply to `m`: whatever `m` already referenced,
/// with `m` itself appended, de-duplicated and in order.
fn thread_chain(m: &Message) -> String {
    let mut chain: Vec<&str> = Vec::new();
    for id in m.references.split_whitespace().chain(std::iter::once(m.message_id.as_str())) {
        if !id.is_empty() && !chain.contains(&id) {
            chain.push(id);
        }
    }
    chain.join(" ")
}

/// Reply-all: To = original sender; Cc = every other recipient (original To +
/// Cc) minus the sender and our own address, de-duplicated.
fn reply_all_prefill(m: &Message, self_email: &str) -> ComposePrefill {
    let mut prefill = reply_prefill(m);
    let self_l = self_email.to_lowercase();
    let from_l = m.from_addr.to_lowercase();
    let mut cc: Vec<String> = Vec::new();
    for list in [m.to.as_str(), m.cc.as_str()] {
        for addr in list.split(',') {
            let a = addr.trim();
            let al = a.to_lowercase();
            if a.is_empty() || al == self_l || al == from_l {
                continue;
            }
            if !cc.iter().any(|x| x.eq_ignore_ascii_case(a)) {
                cc.push(a.to_string());
            }
        }
    }
    prefill.cc = cc.join(", ");
    prefill
}

fn forward_prefill(m: &Message) -> ComposePrefill {
    let subject = if m.subject.to_lowercase().starts_with("fwd:") {
        m.subject.clone()
    } else {
        format!("Fwd: {}", m.subject)
    };
    let text = message_text(&m.body);
    let header = format!(
        "---------- Forwarded message ----------\nFrom: {} <{}>\nDate: {}\nSubject: {}",
        m.from_name, m.from_addr, m.date, m.subject
    );
    ComposePrefill {
        to: String::new(),
        cc: String::new(),
        subject,
        body_html: quote_block(&header, &text),
        ..Default::default()
    }
}

/// Build the HTML quoted block (attribution line + blockquote) for a reply or
/// forward, from plain text so no scripts/remote content leak into the editor.
fn quote_block(attribution: &str, text: &str) -> String {
    let esc = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('\n', "<br>")
    };
    format!(
        "<p class=\"vireo-quote-attr\">{}</p><blockquote>{}</blockquote>",
        esc(attribution),
        esc(text)
    )
}

/// A readable plain-text rendering of a message body, which may be HTML. Used to
/// build safe quoted replies/forwards (no scripts, styles or remote content).
pub fn message_text(body: &str) -> String {
    if !body.contains('<') {
        return body.trim().to_string();
    }
    let mut s = strip_block(body, "script");
    s = strip_block(&s, "style");
    s = strip_block(&s, "head");
    // Turn common block/line elements into newlines.
    for (tag, nl) in [
        ("<br>", "\n"), ("<br/>", "\n"), ("<br />", "\n"),
        ("</p>", "\n\n"), ("</div>", "\n"), ("</li>", "\n"),
        ("</tr>", "\n"), ("</h1>", "\n"), ("</h2>", "\n"), ("</h3>", "\n"),
    ] {
        s = s.replace(tag, nl);
        s = s.replace(&tag.to_uppercase(), nl);
    }
    // Strip remaining tags.
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    // Decode the handful of entities that matter for plain text.
    let out = out
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    // Collapse runs of blank lines.
    let mut result = String::new();
    let mut blanks = 0;
    for line in out.lines() {
        if line.trim().is_empty() {
            blanks += 1;
            if blanks <= 1 {
                result.push('\n');
            }
        } else {
            blanks = 0;
            result.push_str(line.trim_end());
            result.push('\n');
        }
    }
    result.trim().to_string()
}

/// Remove `<tag>…</tag>` blocks (case-insensitive) from HTML.
fn strip_block(html: &str, tag: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut out = String::new();
    let mut i = 0;
    while i < html.len() {
        if lower[i..].starts_with(&open) {
            if let Some(rel) = lower[i..].find(&close) {
                i += rel + close.len();
                continue;
            } else {
                break; // unterminated — drop the rest
            }
        }
        let ch = html[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn kind_label(kind: FolderKind) -> &'static str {
    match kind {
        FolderKind::Archive => "archive",
        FolderKind::Trash => "trash",
        _ => "destination",
    }
}

/// Every Message-ID that identifies a conversation: the messages' own ids plus
/// the ones they reference. This is what the cache is searched by to find the
/// parts of the thread filed in other folders.
fn thread_ids(msgs: &[Message]) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    let mut push = |id: &str| {
        if !id.is_empty() && !ids.iter().any(|x| x == id) {
            ids.push(id.to_string());
        }
    };
    for m in msgs {
        push(&m.message_id);
        for r in m.references.split_whitespace() {
            push(r);
        }
    }
    ids
}

/// Flatten every folder's indexed messages into one pool for cross-folder search.
/// The map is keyed by `(account_id, folder_id)`, so a flat concatenation already
/// spans every folder of every account with no duplicates.
fn build_search_pool(cache: &HashMap<(u32, u32), Vec<Message>>) -> Vec<Message> {
    cache.values().flatten().cloned().collect()
}

/// A conversation member still waiting for its body:
/// `(account_id, message_id, uid, folder_path)`.
type MissingBody = (u32, u32, u32, String);

/// One `LoadBodies` request: the `(account_id, folder_path)` it goes to, and the
/// `(message_id, uid)` of every member it covers.
type BodyBatch = ((u32, String), Vec<(u32, u32)>);

/// Whether two rows are the same mail stored twice, rather than two messages.
///
/// Message-ID is the identity, but it is written by whoever sent the mail and
/// spam reuses one across unrelated messages — so sender and timestamp have to
/// agree as well before one copy is allowed to stand for another. Gmail's label
/// copies agree on all three. Subject is deliberately left out: a re-decoded
/// encoded-word can rewrite it under one label and not another.
fn same_mail(a: &Message, b: &Message) -> bool {
    !a.message_id.is_empty()
        && a.message_id == b.message_id
        && a.from_addr == b.from_addr
        && a.timestamp == b.timestamp
}

/// Keep one copy of each message, dropping the rest of its labels.
///
/// Gmail exposes labels as IMAP folders, so a single message sits in INBOX,
/// All Mail and Important at once — three folder/UID pairs carrying one
/// Message-ID. A conversation that dedupes on those pairs alone therefore shows
/// every copy: a six-message thread renders as eighteen. On a real cache 1210 of
/// this account's 1212 messages are labelled that way, so this is the normal
/// case for Gmail, not an edge one.
///
/// The copy from the folder the reader is showing wins, so the "from folder X"
/// label and the actions on a message refer to the mail actually on screen. A
/// message with no Message-ID can't be matched this way and is left alone.
fn dedupe_label_copies(messages: Vec<Message>, conv: &[Message], shown_folder: u32) -> Vec<Message> {
    let mut out: Vec<Message> = Vec::new();
    for r in messages {
        if r.message_id.is_empty() {
            out.push(r);
            continue;
        }
        if conv.iter().any(|m| same_mail(m, &r)) {
            continue; // the conversation already holds this mail under some label
        }
        match out.iter().position(|m| same_mail(m, &r)) {
            Some(i) => {
                if out[i].folder_id != shown_folder && r.folder_id == shown_folder {
                    out[i] = r;
                }
            }
            None => out.push(r),
        }
    }
    out
}

/// Group a conversation's missing bodies into one request per (account, folder).
///
/// Every member asked for on its own is a round trip each, and each arrival
/// re-renders the reader; `LoadBodies` fetches a whole UID set at once. Members
/// are kept in the order given — the primary is first, so it is first in its
/// batch and renders first — and a conversation that spans folders (or, in the
/// unified inbox, accounts) splits into one batch per folder rather than losing
/// the members that don't fit the first one.
fn batch_bodies_by_folder(to_load: Vec<MissingBody>) -> Vec<BodyBatch> {
    let mut batches: Vec<BodyBatch> = Vec::new();
    for (account_id, message_id, uid, path) in to_load {
        match batches
            .iter_mut()
            .find(|((a, p), _)| *a == account_id && *p == path)
        {
            Some((_, items)) => items.push((message_id, uid)),
            None => batches.push(((account_id, path), vec![(message_id, uid)])),
        }
    }
    batches
}

/// Where to move the reader when its message disappeared from a sync (deleted or
/// moved on another device): whatever now sits in the slot it occupied.
///
/// `None` when that slot cannot be worked out, which is not the same as "start at
/// the top". This used to fall back to the folder's first message, and since
/// deleting prunes the message from the cache before the sync arrives, the
/// lookup fails on exactly that path — so a delete could throw the selection to
/// the top of the folder and scroll the list with it (#19).
fn next_after_vanish(
    previous: Option<&Vec<Message>>,
    messages: &[Message],
    cur_uid: u32,
) -> Option<(u32, u32)> {
    let idx = previous?.iter().position(|m| m.uid == cur_uid)?;
    messages
        .get(idx.min(messages.len().checked_sub(1)?))
        .map(|m| (m.account_id, m.id))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A conversation member as the related-lookup hands it over: what matters
    /// here is which folder it came from and what Message-ID it carries.
    fn labelled(folder_id: u32, uid: u32, message_id: &str) -> Message {
        Message {
            id: uid,
            account_id: 1,
            folder_id,
            uid,
            from_name: String::new(),
            from_addr: String::new(),
            to: String::new(),
            cc: String::new(),
            subject: String::new(),
            preview: String::new(),
            body: String::new(),
            date: String::new(),
            timestamp: 0,
            unread: false,
            starred: false,
            has_attachment: false,
            message_id: message_id.to_string(),
            references: String::new(),
        }
    }

    #[test]
    fn one_gmail_message_under_three_labels_joins_the_conversation_once() {
        // INBOX = 1, All Mail = 2, Important = 3 — the same mail in all three,
        // which is how 1210 of one real account's 1212 messages are stored.
        let related = vec![
            labelled(2, 500, "<a@example.com>"),
            labelled(3, 900, "<a@example.com>"),
            labelled(1, 42, "<a@example.com>"),
            labelled(2, 501, "<b@example.com>"),
        ];
        let kept = dedupe_label_copies(related, &[], 1);
        assert_eq!(kept.len(), 2, "two messages, not four copies");
        let a = kept.iter().find(|m| m.message_id == "<a@example.com>").unwrap();
        assert_eq!(
            (a.folder_id, a.uid),
            (1, 42),
            "the copy from the folder on screen wins, whatever order it arrived in"
        );
    }

    #[test]
    fn a_label_copy_of_a_message_already_shown_is_dropped() {
        let conv = vec![labelled(1, 42, "<a@example.com>")];
        let kept = dedupe_label_copies(vec![labelled(2, 500, "<a@example.com>")], &conv, 1);
        assert!(kept.is_empty(), "the conversation already holds this mail");
    }

    #[test]
    fn a_reused_message_id_is_not_treated_as_the_same_mail() {
        let mut spam = labelled(2, 900, "<a@example.com>");
        spam.from_addr = "spammer@fake.example".into();
        let mut mine = labelled(1, 42, "<a@example.com>");
        mine.from_addr = "me@real.example".into();

        let kept = dedupe_label_copies(vec![mine, spam], &[], 1);
        assert_eq!(kept.len(), 2, "a shared id from another sender is other mail");
    }

    #[test]
    fn messages_without_a_message_id_are_never_merged_together() {
        // Two distinct messages that carry no Message-ID must stay two: there is
        // nothing to match them on, and folding them would lose one.
        let kept = dedupe_label_copies(vec![labelled(1, 7, ""), labelled(1, 8, "")], &[], 1);
        assert_eq!(kept.len(), 2);
    }

    #[test]
    fn a_conversation_in_one_folder_becomes_a_single_fetch() {
        let batches = batch_bodies_by_folder(vec![
            (1, 10, 100, "INBOX".into()),
            (1, 11, 101, "INBOX".into()),
            (1, 12, 102, "INBOX".into()),
        ]);
        assert_eq!(batches.len(), 1, "one folder is one round trip");
        assert_eq!(batches[0].0, (1, "INBOX".to_string()));
        // Order is the thread's, so the primary is fetched and rendered first.
        assert_eq!(batches[0].1, vec![(10, 100), (11, 101), (12, 102)]);
    }

    #[test]
    fn members_from_other_folders_and_accounts_get_their_own_fetch() {
        // A conversation pulled together across Inbox and Sent, plus a second
        // account: batching must not drop the members that don't share the
        // first one's folder.
        let batches = batch_bodies_by_folder(vec![
            (1, 10, 100, "INBOX".into()),
            (1, 11, 55, "Sent".into()),
            (1, 12, 101, "INBOX".into()),
            (2, 13, 7, "INBOX".into()),
        ]);
        assert_eq!(batches.len(), 3);
        assert_eq!(batches[0], ((1, "INBOX".to_string()), vec![(10, 100), (12, 101)]));
        assert_eq!(batches[1], ((1, "Sent".to_string()), vec![(11, 55)]));
        assert_eq!(batches[2], ((2, "INBOX".to_string()), vec![(13, 7)]));
        let members: usize = batches.iter().map(|(_, items)| items.len()).sum();
        assert_eq!(members, 4, "every member is still requested");
    }

    #[test]
    fn a_vanished_message_never_throws_you_to_the_top() {
        let m = |uid| msg(1, 1, uid);
        let previous = vec![m(10), m(9), m(8), m(7)];
        let after = vec![m(10), m(9), m(7)];
        // The message that vanished sat third; whatever is third now takes over.
        assert_eq!(next_after_vanish(Some(&previous), &after, 8), Some((1, 7)));
        // It sat last: the new last row.
        let after = vec![m(10), m(9)];
        assert_eq!(next_after_vanish(Some(&previous), &after, 7), Some((1, 9)));
        // No slot to go by — deleting prunes the cache, so this is the delete
        // path — and the top of the folder is NOT the answer (#19).
        assert_eq!(next_after_vanish(Some(&previous), &after, 999), None);
        assert_eq!(next_after_vanish(None, &after, 8), None);
        // Nothing left at all.
        assert_eq!(next_after_vanish(Some(&previous), &[], 8), None);
    }

    #[test]
    fn single_key_map_matches_the_request() {
        use gtk::gdk::Key;
        // The keys issue #5 asked for, by name.
        assert_eq!(shortcut_for(Key::j, false), Some(Shortcut::NextMessage));
        assert_eq!(shortcut_for(Key::k, false), Some(Shortcut::PrevMessage));
        assert_eq!(shortcut_for(Key::l, false), Some(Shortcut::OpenMessage));
        assert_eq!(shortcut_for(Key::h, false), Some(Shortcut::BackToList));
        assert_eq!(shortcut_for(Key::r, false), Some(Shortcut::Reply));
        assert_eq!(shortcut_for(Key::a, false), Some(Shortcut::Archive));
        assert_eq!(shortcut_for(Key::x, false), Some(Shortcut::ToggleSelect));
        assert_eq!(shortcut_for(Key::d, false), Some(Shortcut::Delete));
        assert_eq!(shortcut_for(Key::w, false), Some(Shortcut::NextInThread));
        assert_eq!(shortcut_for(Key::b, false), Some(Shortcut::PrevInThread));
        // Arrows do what h/j/k/l do.
        assert_eq!(shortcut_for(Key::Down, false), Some(Shortcut::NextMessage));
        assert_eq!(shortcut_for(Key::Up, false), Some(Shortcut::PrevMessage));
        assert_eq!(shortcut_for(Key::Right, false), Some(Shortcut::OpenMessage));
        assert_eq!(shortcut_for(Key::Left, false), Some(Shortcut::BackToList));
        // Shift distinguishes reply from reply-all.
        assert_eq!(shortcut_for(Key::R, true), Some(Shortcut::ReplyAll));
        // Anything unmapped is left to the widget with focus.
        assert_eq!(shortcut_for(Key::z, false), None);
        assert_eq!(shortcut_for(Key::Return, false), None);
        assert_eq!(shortcut_for(Key::space, false), None);
    }

    #[test]
    fn every_shortcut_is_documented() {
        // The reference window is the only place the keys are written down, so a
        // new shortcut without a line there would be invisible.
        let documented: Vec<&str> = SHORTCUT_HELP
            .iter()
            .flat_map(|(_, keys)| keys.iter().map(|(key, _)| *key))
            .collect();
        for key in ["j  or  ↓", "r", "a", "d", "w", "b", "x", "?"] {
            assert!(documented.contains(&key), "{key} is not in the reference");
        }
        // Every documented line has a description.
        for (_, keys) in SHORTCUT_HELP {
            for (key, what) in *keys {
                assert!(!key.trim().is_empty() && !what.trim().is_empty());
            }
        }
    }

    #[test]
    fn md_inline_renders_the_syntax_the_changelog_uses() {
        assert_eq!(md_inline("**bold**"), "<b>bold</b>");
        assert_eq!(md_inline("routed *every* port"), "routed <i>every</i> port");
        assert_eq!(md_inline("`set_visible`"), "<tt>set_visible</tt>");
        assert_eq!(
            md_inline("[Chris](https://github.com/chrispouliot)"),
            "<a href=\"https://github.com/chrispouliot\">Chris</a>"
        );
        // Emphasis nests, as in the changelog's bolded contributor links.
        assert_eq!(
            md_inline("**[Chris](https://example.com)**"),
            "<b><a href=\"https://example.com\">Chris</a></b>"
        );
    }

    #[test]
    fn md_inline_escapes_markup_and_keeps_stray_markers() {
        // Pango would refuse the whole label if these went through unescaped.
        assert_eq!(md_inline("a < b & c"), "a &lt; b &amp; c");
        // A code span's contents are literal, never re-parsed as Markdown.
        assert_eq!(md_inline("`a <b> *c*`"), "<tt>a &lt;b&gt; *c*</tt>");
        // Unclosed markers stay the characters they are rather than eating the line.
        assert_eq!(md_inline("2 * 3 = 6"), "2 * 3 = 6");
        assert_eq!(md_inline("see [the docs"), "see [the docs");
    }

    #[test]
    fn changelog_and_release_notes_render_without_pango_errors() {
        // Pango refuses to render a label whose markup is malformed, blanking the
        // whole page — so every line of both documents has to parse. Link tags are
        // GtkLabel's own extension and are not known to `parse_markup`, so they
        // are lifted out first (which also asserts each one is closed).
        fn without_links(markup: &str) -> String {
            let mut out = String::new();
            let mut rest = markup;
            while let Some(open) = rest.find("<a href=\"") {
                out.push_str(&rest[..open]);
                let tail = &rest[open..];
                let close = tail.find('>').expect("link tag is closed");
                rest = &tail[close + 1..];
            }
            out.push_str(rest);
            out.replace("</a>", "")
        }

        for md in [
            include_str!("../CHANGELOG.md"),
            include_str!("../RELEASE_NOTES.md"),
        ] {
            for line in md.lines() {
                let markup = md_inline(line);
                assert!(
                    gtk::pango::parse_markup(&without_links(&markup), '\0').is_ok(),
                    "does not parse as Pango markup: {line}\n  -> {markup}"
                );
            }
        }
    }

    fn msg(account_id: u32, folder_id: u32, uid: u32) -> Message {
        Message {
            id: uid,
            account_id,
            folder_id,
            uid,
            from_name: String::new(),
            from_addr: String::new(),
            to: String::new(),
            cc: String::new(),
            subject: String::new(),
            preview: String::new(),
            body: String::new(),
            date: String::new(),
            timestamp: 0,
            unread: false,
            starred: false,
            has_attachment: false,
            message_id: String::new(),
            references: String::new(),
        }
    }

    #[test]
    fn thread_ids_collect_own_and_referenced_ids_once() {
        let mut a = msg(1, 1, 10);
        a.message_id = "root@x".into();
        let mut b = msg(1, 1, 11);
        b.message_id = "reply@x".into();
        b.references = "root@x parent@x".into();
        let ids = thread_ids(&[a, b]);
        assert_eq!(ids, vec!["root@x", "reply@x", "parent@x"]);
    }

    #[test]
    fn thread_ids_skips_messages_with_no_headers() {
        // Nothing to search the cache by — better an empty list than a lookup
        // that would match every message with an empty message_id.
        assert!(thread_ids(&[msg(1, 1, 10)]).is_empty());
    }

    #[test]
    fn search_pool_spans_every_folder_and_account() {
        let mut cache: HashMap<(u32, u32), Vec<Message>> = HashMap::new();
        cache.insert((1, 10), vec![msg(1, 10, 1), msg(1, 10, 2)]); // acct 1, inbox
        cache.insert((1, 11), vec![msg(1, 11, 3)]); // acct 1, archive
        cache.insert((2, 20), vec![msg(2, 20, 4), msg(2, 20, 5)]); // acct 2, inbox

        let pool = build_search_pool(&cache);
        assert_eq!(pool.len(), 5, "pool must include every folder's messages");

        let folders: std::collections::HashSet<(u32, u32)> =
            pool.iter().map(|m| (m.account_id, m.folder_id)).collect();
        assert_eq!(folders.len(), 3, "pool must span all three folders");
    }

    #[test]
    fn search_pool_is_empty_when_nothing_indexed() {
        assert!(build_search_pool(&HashMap::new()).is_empty());
    }
}
