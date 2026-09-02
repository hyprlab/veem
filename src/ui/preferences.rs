//! Settings window: privacy options (remote-content allowlist).
//!
//! Account credentials are managed in their own window (see `ui/accounts.rs`).

use adw::prelude::*;
use relm4::prelude::*;

use crate::config::{AppTheme, ClockStyle, DateStyle, MessageTheme};

/// Initial data for the settings window.
#[derive(Debug)]
pub struct PrefInit {
    pub auto_remote_content: bool,
    pub show_remote_banner: bool,
    pub gravatar: bool,
    pub avatars: bool,
    pub sender_logos: bool,
    pub date_style: DateStyle,
    pub clock_style: ClockStyle,
    pub fetch_interval_secs: u64,
    pub push: bool,
    pub palette_collapse_secs: u64,
    pub threading: bool,
    pub threads_expanded: bool,
    /// Reading pane shows conversations newest-message-first.
    pub thread_newest_first: bool,
    /// Reader always shows the recipients line under the sender.
    pub always_show_recipients: bool,
    /// Lone messages render as inset cards, like conversation messages.
    pub single_message_card: bool,
    /// Conversation rows may expand into their members in the message list.
    pub thread_expansion: bool,
    /// Deleting a whole selected conversation asks for confirmation.
    pub confirm_thread_delete: bool,
    /// Conversation card actions hide until the card is hovered.
    pub card_actions_hover: bool,
    /// With the ⋯ toggle off: card actions appear automatically on hover.
    pub card_actions_auto: bool,
    /// The list rows carry an Actions Palette line at all.
    pub list_palette: bool,
    /// The list's Actions Palette opens on row hover (no ⋯ click).
    pub list_palette_hover: bool,
    /// "New message" composes inline over the reading pane (vs a window).
    pub compose_inline: bool,
    pub paste_plain: bool,
    pub spellcheck: bool,
    pub spellcheck_langs: String,
    pub message_theme: MessageTheme,
    pub app_theme: AppTheme,
    pub notifications: bool,
    pub notification_content: bool,
    pub show_attachments: bool,
    pub show_contacts: bool,
    pub show_unified: bool,
    pub unified_chip: bool,
    pub chevrons_left: bool,
    pub console_mode: bool,
    pub read_mark: crate::config::ReadMark,
    pub sidebar_hover_expand: bool,
    pub preview_lines: u32,
    pub single_key_shortcuts: bool,
    pub run_in_background: bool,
    pub autostart: bool,
    /// The accounts panel (built by the AccountsWindow component), shown
    /// behind the window's "Accounts" tab.
    pub accounts_panel: gtk::Widget,
    /// Open showing the Accounts tab instead of Preferences.
    pub start_on_accounts: bool,
    /// The persisted "this window opens to" choice (true = Accounts).
    pub settings_open_accounts: bool,
}


/// App-chrome appearance options, in combo order.
const APP_THEMES: &[(&str, AppTheme)] = &[
    ("Follow system", AppTheme::System),
    ("Light", AppTheme::Light),
    ("Dark", AppTheme::Dark),
];

/// Message-content appearance options, in combo order.
const MESSAGE_THEMES: &[(&str, MessageTheme)] = &[
    ("Follow system", MessageTheme::System),
    ("Light", MessageTheme::Light),
    ("Dark", MessageTheme::Dark),
];

/// Date arrangements, in combo order. The examples are what each writes.
const DATE_STYLES: &[(&str, DateStyle)] = &[
    ("Follow system", DateStyle::System),
    ("Aug 23, 2026", DateStyle::MonthFirst),
    ("23 Aug 2026", DateStyle::DayFirst),
    ("2026 Aug 23", DateStyle::YearFirst),
];

/// Clock options, in combo order.
const CLOCK_STYLES: &[(&str, ClockStyle)] = &[
    ("Follow system", ClockStyle::System),
    ("12-hour (5:40 PM)", ClockStyle::Twelve),
    ("24-hour (17:40)", ClockStyle::TwentyFour),
];

/// Selectable mail-check intervals (label, seconds). 0 = manual only.
const FETCH_INTERVALS: &[(&str, u64)] = &[
    ("Manually", 0),
    ("Every minute", 60),
    ("Every 5 minutes", 300),
    ("Every 15 minutes", 900),
    ("Every 30 minutes", 1800),
];

// ---- Allowed-sender row -----------------------------------------------------

pub struct SenderRow {
    addr: String,
}

#[derive(Debug)]
pub enum SenderRowOutput {
    Remove(String),
}

#[relm4::factory(pub)]
impl FactoryComponent for SenderRow {
    type Init = String;
    type Input = ();
    type Output = SenderRowOutput;
    type CommandOutput = ();
    type ParentWidget = gtk::ListBox;

    view! {
        adw::ActionRow {
            set_title: &self.addr,
            add_suffix = &gtk::Button {
                set_icon_name: "co.hyprlab.Vireo-user-trash-symbolic",
                set_valign: gtk::Align::Center,
                set_tooltip_text: Some("Remove"),
                add_css_class: "flat",
                connect_clicked[sender, addr = self.addr.clone()] => move |_| {
                    let _ = sender.output(SenderRowOutput::Remove(addr.clone()));
                },
            },
        }
    }

    fn init_model(addr: Self::Init, _index: &DynamicIndex, _sender: FactorySender<Self>) -> Self {
        Self { addr }
    }
}

// ---- Preferences window -----------------------------------------------------

pub struct Preferences {
    /// Mirrors the notifications switch, so the "show sender and subject" row
    /// below it can grey out when nothing is being posted at all.
    notifications: bool,
    show_unified: bool,
    /// Mirrors the threading switch, so the "threaded message list" row below
    /// it can grey out when conversations aren't grouped at all.
    threading: bool,
    /// Mirrors the expandable-conversations switch — "expand by default" only
    /// means anything while conversations can expand in the list at all.
    thread_expansion: bool,
    /// Mirrors the list-palette switch, so the hover row under it can grey
    /// out when there is no palette to open.
    list_palette: bool,
    /// The Accounts/Preferences panel stack, for tab switching from update().
    panels_stack: Option<adw::ViewStack>,
    /// The shared header bar (view switcher); hidden while the accounts
    /// editor subpage is open, whose own header takes over.
    host_header: Option<adw::HeaderBar>,
}

#[derive(Debug)]
pub enum PrefInput {
    ToggleShowRemoteBanner(bool),
    ToggleAutoRemoteContent(bool),
    ToggleGravatar(bool),
    ToggleAvatars(bool),
    ToggleSenderLogos(bool),
    ChangeDateStyle(u32),
    ChangeClockStyle(u32),
    ToggleThreading(bool),
    ToggleThreadsExpanded(bool),
    ToggleThreadNewestFirst(bool),
    ToggleAlwaysShowRecipients(bool),
    ToggleSingleMessageCard(bool),
    ToggleThreadExpansion(bool),
    ToggleConfirmThreadDelete(bool),
    ChangeCardActionsMode(u32),
    ToggleListPalette(bool),
    ToggleListPaletteHover(bool),
    ToggleComposeInline(bool),
    TogglePastePlain(bool),
    ToggleSpellcheck(bool),
    SpellLangsEdited(String),
    ChangeFetchInterval(u32),
    TogglePush(bool),
    ToggleNotifications(bool),
    ToggleNotificationContent(bool),
    ToggleAttachmentsRow(bool),
    ToggleContactsRow(bool),
    ToggleShowUnified(bool),
    ToggleUnifiedChip(bool),
    ChangeChevronSide(u32),
    ToggleSidebarHoverExpand(bool),
    ChangePreviewLines(u32),
    ToggleSingleKey(bool),
    ToggleConsoleMode(bool),
    ChangeReadMark(u32),
    ExportSettings,
    ImportSettings,
    ToggleRunInBackground(bool),
    ToggleAutostart(bool),
    ChangePaletteCollapse(u64),
    ChangeMessageTheme(u32),
    ChangeAppTheme(u32),
    ChangeSettingsOpen(u32),
    /// Switch the window to the Accounts panel (true) or Preferences (false).
    ShowAccounts(bool),
    /// The accounts editor subpage opened/closed — hide/show the shared
    /// header so the editor's own header takes over the window.
    EditorOpen(bool),
}

#[derive(Debug)]
pub enum PrefOutput {
    SetAutoRemoteContent(bool),
    SetShowRemoteBanner(bool),
    SetGravatar(bool),
    SetAvatars(bool),
    SetSenderLogos(bool),
    SetDateStyle(DateStyle),
    SetClockStyle(ClockStyle),
    SetThreading(bool),
    SetThreadsExpanded(bool),
    SetThreadNewestFirst(bool),
    SetAlwaysShowRecipients(bool),
    SetSingleMessageCard(bool),
    SetThreadExpansion(bool),
    SetConfirmThreadDelete(bool),
    SetCardActionsMode { hover_toggle: bool, hover_auto: bool },
    SetListPalette(bool),
    SetListPaletteHover(bool),
    SetComposeInline(bool),
    SetPastePlain(bool),
    SetSpellcheck(bool),
    SetSpellcheckLangs(String),
    SetFetchInterval(u64),
    SetPush(bool),
    SetNotifications(bool),
    SetNotificationContent(bool),
    SetAttachmentsRow(bool),
    SetContactsRow(bool),
    SetShowUnified(bool),
    SetUnifiedChip(bool),
    SetChevronsLeft(bool),
    SetConsoleMode(bool),
    SetReadMark(crate::config::ReadMark),
    ExportSettings,
    ImportSettings,
    SetSidebarHoverExpand(bool),
    SetAppTheme(AppTheme),
    /// The "this window opens to" choice changed (true = Accounts).
    SetSettingsOpenAccounts(bool),
    SetPreviewLines(u32),
    SetSingleKey(bool),
    SetRunInBackground(bool),
    SetAutostart(bool),
    SetPaletteCollapse(u64),
    SetMessageTheme(MessageTheme),
    Closed,
}

#[relm4::component(pub)]
impl Component for Preferences {
    type Init = PrefInit;
    type Input = PrefInput;
    type Output = PrefOutput;
    type CommandOutput = ();

    view! {
        adw::Window {
            set_modal: false,
            set_default_width: 564,
            // Remembered vertical size (tall by default) — resizing sticks
            // across restarts via the save on close below.
            set_default_height: crate::config::load_prefs_height(),
            set_title: Some("Settings"),

            connect_close_request[sender] => move |w| {
                crate::config::save_prefs_height(w.height());
                let _ = sender.output(PrefOutput::Closed);
                gtk::glib::Propagation::Proceed
            },

            // One window, two views: Accounts and Preferences, switched by a
            // view switcher (GNOME HIG) in the shared header bar. The
            // accounts panel (with its own sub-navigation) is built by the
            // AccountsWindow component and handed in via PrefInit; while its
            // editor subpage is open the shared header hides, so the editor's
            // own back/Save header takes over the window.
            #[wrap(Some)]
            set_content = &adw::ToolbarView {
                #[name = "host_header"]
                add_top_bar = &adw::HeaderBar {
                    #[wrap(Some)]
                    #[name = "switcher"]
                    set_title_widget = &adw::ViewSwitcher {
                        set_policy: adw::ViewSwitcherPolicy::Wide,
                    },
                },

                #[wrap(Some)]
                #[name = "panels_stack"]
                set_content = &adw::ViewStack {
                    #[name = "accounts_slot"]
                    add_titled[Some("accounts"), "Accounts"] = &adw::Bin {},

                    #[name = "prefs_page"]
                    add_titled[Some("preferences"), "Settings"] = &adw::PreferencesPage {
                    add = &adw::PreferencesGroup {
                        set_title: "General",

                        #[name = "fetch_row"]
                        adw::ComboRow {
                            set_title: "Check for new mail",
                            connect_selected_notify[sender] => move |row| {
                                sender.input(PrefInput::ChangeFetchInterval(row.selected()));
                            },
                        },

                        #[name = "push_row"]
                        adw::SwitchRow {
                            set_title: "Instant new mail (IMAP push)",
                            set_subtitle: "Uses IMAP IDLE to receive messages the moment they arrive.",
                            connect_active_notify[sender] => move |row| {
                                sender.input(PrefInput::TogglePush(row.is_active()));
                            },
                        },
                    },

                    add = &adw::PreferencesGroup {
                        set_title: "Notifications",

                        #[name = "notifications_row"]
                        adw::SwitchRow {
                            set_title: "Desktop notifications",
                            set_subtitle: "Show system notifications for new mail and error alerts when Vireo isn't focused.",
                            connect_active_notify[sender] => move |row| {
                                sender.input(PrefInput::ToggleNotifications(row.is_active()));
                            },
                        },

                        #[name = "notification_content_row"]
                        adw::SwitchRow {
                            #[watch]
                            set_sensitive: model.notifications,
                            set_title: "Show sender and subject",
                            set_subtitle: "Name who wrote and what about in the notification. Turn this off to keep both off the lock screen.",
                            connect_active_notify[sender] => move |row| {
                                sender.input(PrefInput::ToggleNotificationContent(row.is_active()));
                            },
                        },
                    },

                    add = &adw::PreferencesGroup {
                        set_title: "Sidebar",

                        #[name = "show_unified_row"]
                        adw::SwitchRow {
                            set_title: "All Inboxes",
                            set_subtitle: "A unified inbox combining every account, at the top \
                                           of the sidebar. Only shown with more than one \
                                           account.",
                            connect_active_notify[sender] => move |row| {
                                sender.input(PrefInput::ToggleShowUnified(row.is_active()));
                            },
                        },

                        #[name = "unified_chip_row"]
                        adw::SwitchRow {
                            #[watch]
                            set_sensitive: model.show_unified,
                            set_title: "All Inboxes unread count",
                            set_subtitle: "Show the combined unread chip next to All Inboxes \
                                           while its per-account list is folded up.",
                            connect_active_notify[sender] => move |row| {
                                sender.input(PrefInput::ToggleUnifiedChip(row.is_active()));
                            },
                        },

                        #[name = "chevron_side_row"]
                        adw::ComboRow {
                            set_title: "Chevron placement",
                            set_subtitle: "Which side of All Inboxes and the account rows \
                                           their expand/collapse chevrons sit on.",
                            connect_selected_notify[sender] => move |row| {
                                sender.input(PrefInput::ChangeChevronSide(row.selected()));
                            },
                        },

                        #[name = "show_attachments_row"]
                        adw::SwitchRow {
                            set_title: "Attachments in the sidebar",
                            set_subtitle: "A shortcut for browsing every account's attachments, \
                                           pinned at the bottom of the sidebar.",
                            connect_active_notify[sender] => move |row| {
                                sender.input(PrefInput::ToggleAttachmentsRow(row.is_active()));
                            },
                        },

                        #[name = "show_contacts_row"]
                        adw::SwitchRow {
                            set_title: "Contacts in the sidebar",
                            set_subtitle: "A shortcut that opens your contacts, pinned at the \
                                           bottom of the sidebar.",
                            connect_active_notify[sender] => move |row| {
                                sender.input(PrefInput::ToggleContactsRow(row.is_active()));
                            },
                        },

                        #[name = "sidebar_hover_expand_row"]
                        adw::SwitchRow {
                            set_title: "Expand the sidebar on hover",
                            set_subtitle: "Whenever the sidebar is collapsed to its icon rail, \
                                           hovering it floats the full sidebar out over the \
                                           panes; it folds back a moment after the pointer \
                                           leaves.",
                            connect_active_notify[sender] => move |row| {
                                sender.input(PrefInput::ToggleSidebarHoverExpand(row.is_active()));
                            },
                        },
                    },

                    add = &adw::PreferencesGroup {
                        set_title: "Message List",

                        #[name = "avatars_row"]
                        adw::SwitchRow {
                            set_title: "Sender avatars",
                            set_subtitle: "The sender's avatar beside each message, in the list \
                                           and above the message. Turning it off gives the \
                                           sender and subject more room.",
                            connect_active_notify[sender] => move |row| {
                                sender.input(PrefInput::ToggleAvatars(row.is_active()));
                            },
                        },

                        #[name = "preview_lines_row"]
                        adw::ComboRow {
                            set_title: "Preview lines",
                            set_subtitle: "How much of each message to show under its subject. \
                                           Off also stops previews being downloaded.",
                            connect_selected_notify[sender] => move |row| {
                                sender.input(PrefInput::ChangePreviewLines(row.selected()));
                            },
                        },

                        #[name = "list_palette_row"]
                        adw::SwitchRow {
                            set_title: "Actions Palette in the message list",
                            set_subtitle: "The \u{22ef} action row under each message summary. \
                                           Turning it off returns its space to the row; \
                                           messages are still acted on from their cards and \
                                           the right-click menu.",
                            connect_active_notify[sender] => move |row| {
                                sender.input(PrefInput::ToggleListPalette(row.is_active()));
                            },
                        },

                        #[name = "list_palette_hover_row"]
                        adw::SwitchRow {
                            #[watch]
                            set_sensitive: model.list_palette,
                            set_title: "Open the Actions Palette on hover",
                            set_subtitle: "The message list's \u{22ef} palette slides open \
                                           by itself while the pointer rests on a row.",
                            connect_active_notify[sender] => move |row| {
                                sender.input(PrefInput::ToggleListPaletteHover(row.is_active()));
                            },
                        },

                        #[name = "palette_collapse_row"]
                        adw::SpinRow {
                            set_title: "Actions Palette timeout",
                            set_subtitle: "Seconds an actions palette stays open after the \
                                           cursor leaves it — the list's and the message \
                                           cards' alike.",
                            connect_value_notify[sender] => move |row| {
                                sender.input(PrefInput::ChangePaletteCollapse(row.value() as u64));
                            },
                        },
                    },

                    add = &adw::PreferencesGroup {
                        set_title: "Conversations",

                        #[name = "threading_row"]
                        adw::SwitchRow {
                            set_title: "Group messages by conversation",
                            set_subtitle: "Collapse replies into a single threaded conversation.",
                            connect_active_notify[sender] => move |row| {
                                sender.input(PrefInput::ToggleThreading(row.is_active()));
                            },
                        },

                        #[name = "thread_expansion_row"]
                        adw::SwitchRow {
                            #[watch]
                            set_sensitive: model.threading,
                            set_title: "Expandable conversations",
                            set_subtitle: "Allow a conversation to expand/collapse its messages \
                                           in the list. When off, the row keeps its count chip \
                                           but the messages are displayed only as cards in the \
                                           reading pane.",
                            connect_active_notify[sender] => move |row| {
                                sender.input(PrefInput::ToggleThreadExpansion(row.is_active()));
                            },
                        },

                        #[name = "threads_expanded_row"]
                        adw::SwitchRow {
                            #[watch]
                            set_sensitive: model.threading && model.thread_expansion,
                            set_title: "Expand conversations by default",
                            set_subtitle: "Show every message of a conversation in the list. \
                                           When off, conversations start collapsed to their \
                                           newest message.",
                            connect_active_notify[sender] => move |row| {
                                sender.input(PrefInput::ToggleThreadsExpanded(row.is_active()));
                            },
                        },

                        #[name = "thread_newest_first_row"]
                        adw::SwitchRow {
                            set_title: "Newest message first",
                            set_subtitle: "Show a conversation's latest message at the top of \
                                           the reading pane. Off reads oldest to newest, \
                                           downward.",
                            connect_active_notify[sender] => move |row| {
                                sender.input(PrefInput::ToggleThreadNewestFirst(row.is_active()));
                            },
                        },

                        #[name = "confirm_thread_delete_row"]
                        adw::SwitchRow {
                            set_title: "Confirm conversation deletion",
                            set_subtitle: "Warn before deleting when a whole conversation is \
                                           selected, since every message in the thread goes \
                                           with it.",
                            connect_active_notify[sender] => move |row| {
                                sender.input(PrefInput::ToggleConfirmThreadDelete(row.is_active()));
                            },
                        },
                    },

                    add = &adw::PreferencesGroup {
                        set_title: "Reading",

                        #[name = "read_mark_row"]
                        adw::ComboRow {
                            set_title: "Mark as read",
                            set_subtitle: "When an opened message counts as read. \
                                           Conversations mark each message as it \
                                           comes into view.",
                            connect_selected_notify[sender] => move |row| {
                                sender.input(PrefInput::ChangeReadMark(row.selected()));
                            },
                        },

                        #[name = "message_theme_row"]
                        adw::ComboRow {
                            set_title: "Message appearance",
                            set_subtitle: "Theme for email content only, not the app itself.",
                            connect_selected_notify[sender] => move |row| {
                                sender.input(PrefInput::ChangeMessageTheme(row.selected()));
                            },
                        },

                        #[name = "card_actions_row"]
                        adw::ComboRow {
                            set_title: "Message card actions",
                            set_subtitle: "How each message's action icons show in the \
                                           reader, single or threaded.",
                            connect_selected_notify[sender] => move |row| {
                                sender.input(PrefInput::ChangeCardActionsMode(row.selected()));
                            },
                        },

                        #[name = "single_message_card_row"]
                        adw::SwitchRow {
                            set_title: "Single messages as cards",
                            set_subtitle: "Show a lone message as an inset card with the same \
                                           border as a conversation's messages. Off fills the \
                                           pane edge to edge.",
                            connect_active_notify[sender] => move |row| {
                                sender.input(PrefInput::ToggleSingleMessageCard(row.is_active()));
                            },
                        },

                        #[name = "always_show_recipients_row"]
                        adw::SwitchRow {
                            set_title: "Always show recipients",
                            set_subtitle: "Show who each message went to under its sender, \
                                           without clicking the recipients chip. With one \
                                           recipient the chip is dropped entirely.",
                            connect_active_notify[sender] => move |row| {
                                sender.input(PrefInput::ToggleAlwaysShowRecipients(row.is_active()));
                            },
                        },
                    },

                    add = &adw::PreferencesGroup {
                        set_title: "Composing",

                        #[name = "compose_inline_row"]
                        adw::SwitchRow {
                            set_title: "Compose in the main window",
                            set_subtitle: "New message slides down over the reading pane, \
                                           like a reply — pop it out to a window from its \
                                           header. Off = open a separate window directly.",
                            connect_active_notify[sender] => move |row| {
                                sender.input(PrefInput::ToggleComposeInline(row.is_active()));
                            },
                        },

                        #[name = "paste_plain_row"]
                        adw::SwitchRow {
                            set_title: "Paste as plain text",
                            set_subtitle: "Pasting into a message strips the \
                                           clipboard's formatting. Off, a paste \
                                           keeps its formatting. Right-clicking \
                                           the editor always offers both.",
                            connect_active_notify[sender] => move |row| {
                                sender.input(PrefInput::TogglePastePlain(row.is_active()));
                            },
                        },
                    },

                    #[name = "spelling_group"]
                    add = &adw::PreferencesGroup {
                        set_title: "Spelling",
                        // The description is filled in at init with the
                        // dictionaries the app can actually see — checking a
                        // language without one silently checks nothing, so
                        // honesty about what is installed beats silence.

                        #[name = "spellcheck_row"]
                        adw::SwitchRow {
                            set_title: "Check spelling as you type",
                            set_subtitle: "Misspelled words in the message body \
                                           are underlined; right-click a word \
                                           for corrections.",
                            connect_active_notify[sender] => move |row| {
                                sender.input(PrefInput::ToggleSpellcheck(row.is_active()));
                            },
                        },

                        #[name = "spell_langs_row"]
                        adw::EntryRow {
                            set_title: "Languages (comma-separated, blank = system language)",
                            set_show_apply_button: true,
                            connect_apply[sender] => move |row| {
                                sender.input(PrefInput::SpellLangsEdited(row.text().to_string()));
                            },
                        },
                    },

                    add = &adw::PreferencesGroup {
                        // Rendered as Pango markup — a bare "&" breaks it.
                        set_title: "System &amp; Appearance",

                        #[name = "app_theme_row"]
                        adw::ComboRow {
                            set_title: "Style",
                            set_subtitle: "The app itself. Message content has its own \
                                           setting under Reading.",
                            connect_selected_notify[sender] => move |row| {
                                sender.input(PrefInput::ChangeAppTheme(row.selected()));
                            },
                        },

                        #[name = "settings_open_row"]
                        adw::ComboRow {
                            set_title: "This window opens to",
                            set_subtitle: "The view shown first when Settings \
                                           is opened from the menu.",
                            connect_selected_notify[sender] => move |row| {
                                sender.input(PrefInput::ChangeSettingsOpen(row.selected()));
                            },
                        },

                        #[name = "background_row"]
                        adw::SwitchRow {
                            set_title: "Keep running in the background",
                            set_subtitle: "Closing the window hides it instead of quitting, so new \
                                           mail still arrives. Vireo then appears under Background \
                                           Apps in the system menu, where it can be quit.",
                            connect_active_notify[sender] => move |row| {
                                sender.input(PrefInput::ToggleRunInBackground(row.is_active()));
                            },
                        },

                        #[name = "autostart_row"]
                        adw::SwitchRow {
                            set_title: "Start at login",
                            set_subtitle: "Start checking for mail when you log in. Vireo starts \
                                           without a window and waits in the system menu.",
                            connect_active_notify[sender] => move |row| {
                                sender.input(PrefInput::ToggleAutostart(row.is_active()));
                            },
                        },

                        #[name = "single_key_row"]
                        adw::SwitchRow {
                            set_title: "Single-key shortcuts",
                            set_subtitle: "Act on mail with one key and no modifier — j/k to move, \
                                           r to reply, a to archive. Press Ctrl+? for the full list.",
                            connect_active_notify[sender] => move |row| {
                                sender.input(PrefInput::ToggleSingleKey(row.is_active()));
                            },
                        },

                        #[name = "console_mode_row"]
                        adw::SwitchRow {
                            set_title: "Console mode",
                            set_subtitle: "A verbose live console in the status bar showing \
                                           everything Vireo is doing under the hood.",
                            connect_active_notify[sender] => move |row| {
                                sender.input(PrefInput::ToggleConsoleMode(row.is_active()));
                            },
                        },
                    },

                    add = &adw::PreferencesGroup {
                        set_title: "Backup",
                        set_description: Some(
                            "Accounts and preferences as one file. Passwords stay in the \
                             system keyring and are never exported."
                        ),

                        adw::ActionRow {
                            set_title: "Export settings",
                            set_activatable: true,
                            connect_activated => PrefInput::ExportSettings,
                            add_suffix = &gtk::Image {
                                set_icon_name: Some("co.hyprlab.Vireo-go-next-symbolic"),
                            },
                        },

                        adw::ActionRow {
                            set_title: "Import settings",
                            set_subtitle: "Replaces the current accounts and preferences in \
                                           place. Don't remove accounts first: removal also \
                                           deletes their keyring passwords, which no backup \
                                           carries.",
                            set_activatable: true,
                            connect_activated => PrefInput::ImportSettings,
                            add_suffix = &gtk::Image {
                                set_icon_name: Some("co.hyprlab.Vireo-go-next-symbolic"),
                            },
                        },
                    },

                    add = &adw::PreferencesGroup {
                        set_title: "Date and Time",
                        set_description: Some(
                            "By default dates follow the system's own arrangement — its \
                             field order, month names and clock. Choose a format here to \
                             use it whatever the system is set to."
                        ),

                        #[name = "date_style_row"]
                        adw::ComboRow {
                            set_title: "Date format",
                            connect_selected_notify[sender] => move |row| {
                                sender.input(PrefInput::ChangeDateStyle(row.selected()));
                            },
                        },

                        #[name = "clock_style_row"]
                        adw::ComboRow {
                            set_title: "Clock",
                            connect_selected_notify[sender] => move |row| {
                                sender.input(PrefInput::ChangeClockStyle(row.selected()));
                            },
                        },
                    },

                    add = &adw::PreferencesGroup {
                        set_title: "Privacy",
                        set_description: Some(
                            "Vireo collects no telemetry and sends no analytics. Remote \
                             content (images, trackers) is blocked by default. Allow it per \
                             message, trust a sender to always load it, or turn on \"Always \
                             load remote content\" below."
                        ),

                        #[name = "auto_remote_content_row"]
                        adw::SwitchRow {
                            set_title: "Always load remote content",
                            set_subtitle: "Show images and other remote content in every new \
                                           message without asking. Off by default, since \
                                           remote content can be used to track when and where \
                                           you read a message.",
                            connect_active_notify[sender] => move |row| {
                                sender.input(PrefInput::ToggleAutoRemoteContent(row.is_active()));
                            },
                        },

                        #[name = "show_remote_banner_row"]
                        adw::SwitchRow {
                            set_title: "Warn when remote content is blocked",
                            set_subtitle: "Shows the banner offering to load it. Turning this \
                                           off only hides the notice — remote content is still \
                                           blocked just the same.",
                            connect_active_notify[sender] => move |row| {
                                sender.input(PrefInput::ToggleShowRemoteBanner(row.is_active()));
                            },
                        },

                        #[name = "gravatar_row"]
                        adw::SwitchRow {
                            set_title: "Use Gravatar when a contact has no photo",
                            set_subtitle: "Local GNOME Contacts photos are always preferred. \
                                           Gravatar sends a hash of the sender's email.",
                            connect_active_notify[sender] => move |row| {
                                sender.input(PrefInput::ToggleGravatar(row.is_active()));
                            },
                        },

                        #[name = "sender_logos_row"]
                        adw::SwitchRow {
                            set_title: "Show sender logos",
                            set_subtitle: "Fills the sender's avatar with the brand's own site \
                                           icon, fetched from the sender's domain. That domain \
                                           learns your IP address, which is what blocking \
                                           remote content otherwise avoids.",
                            connect_active_notify[sender] => move |row| {
                                sender.input(PrefInput::ToggleSenderLogos(row.is_active()));
                            },
                        },

                    },

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
        let mut model = Preferences {
            notifications: init.notifications,
            show_unified: init.show_unified,
            threading: init.threading,
            thread_expansion: init.thread_expansion,
            list_palette: init.list_palette,
            panels_stack: None,
            host_header: None,
        };

        let widgets = view_output!();

        // Settings never truncates. AdwComboRow's DEFAULT item factory builds
        // the selected-value display with an ellipsizing label — and rebuilds
        // it on every selection change, so fixing the widget after the fact
        // doesn't stick ("Follow system" → "Follow s…"). Give every combo a
        // plain factory whose labels never ellipsize; the short row titles
        // yield the space instead.
        fn no_truncate(row: &adw::ComboRow) {
            let factory = gtk::SignalListItemFactory::new();
            factory.connect_setup(|_, item| {
                if let Some(item) = item.downcast_ref::<gtk::ListItem>() {
                    let label = gtk::Label::new(None);
                    label.set_xalign(0.0);
                    item.set_child(Some(&label));
                }
            });
            factory.connect_bind(|_, item| {
                let Some(item) = item.downcast_ref::<gtk::ListItem>() else { return };
                if let (Some(label), Some(s)) = (
                    item.child().and_downcast::<gtk::Label>(),
                    item.item().and_downcast::<gtk::StringObject>(),
                ) {
                    label.set_label(&s.string());
                }
            });
            row.set_factory(Some(&factory));
        }
        for row in [
            &widgets.fetch_row,
            &widgets.preview_lines_row,
            &widgets.message_theme_row,
            &widgets.card_actions_row,
            &widgets.app_theme_row,
            &widgets.settings_open_row,
            &widgets.date_style_row,
            &widgets.clock_style_row,
            &widgets.chevron_side_row,
        ] {
            no_truncate(row);
        }

        widgets.auto_remote_content_row.set_active(init.auto_remote_content);
        widgets.show_remote_banner_row.set_active(init.show_remote_banner);
        widgets.gravatar_row.set_active(init.gravatar);
        widgets.avatars_row.set_active(init.avatars);
        widgets.sender_logos_row.set_active(init.sender_logos);

        // Mail-check interval combo.
        let labels: Vec<&str> = FETCH_INTERVALS.iter().map(|(l, _)| *l).collect();
        widgets.fetch_row.set_model(Some(&gtk::StringList::new(&labels)));
        let selected = FETCH_INTERVALS
            .iter()
            .position(|(_, secs)| *secs == init.fetch_interval_secs)
            .unwrap_or(0);
        widgets.fetch_row.set_selected(selected as u32);
        widgets.push_row.set_active(init.push);
        widgets.notifications_row.set_active(init.notifications);
        widgets.notification_content_row.set_active(init.notification_content);
        widgets.show_attachments_row.set_active(init.show_attachments);
        widgets.show_contacts_row.set_active(init.show_contacts);
        widgets.show_unified_row.set_active(init.show_unified);
        widgets.unified_chip_row.set_active(init.unified_chip);
        widgets.chevron_side_row.set_model(Some(&gtk::StringList::new(&["Left", "Right"])));
        widgets.chevron_side_row.set_selected(if init.chevrons_left { 0 } else { 1 });
        widgets.sidebar_hover_expand_row.set_active(init.sidebar_hover_expand);
        let preview_labels = ["Off", "1 line", "2 lines", "3 lines"];
        widgets
            .preview_lines_row
            .set_model(Some(&gtk::StringList::new(&preview_labels)));
        widgets.preview_lines_row.set_selected(init.preview_lines.min(3));

        widgets.background_row.set_active(init.run_in_background);
        widgets.autostart_row.set_active(init.autostart);
        // Starting at login only means anything if Vireo stays running.
        widgets.autostart_row.set_sensitive(init.run_in_background);
        {
            let autostart_row = widgets.autostart_row.clone();
            widgets.background_row.connect_active_notify(move |row| {
                autostart_row.set_sensitive(row.is_active());
            });
        }
        widgets.single_key_row.set_active(init.single_key_shortcuts);
        widgets.console_mode_row.set_active(init.console_mode);
        widgets.read_mark_row.set_model(Some(&gtk::StringList::new(&[
            "When displayed",
            "After two seconds",
            "Manually",
        ])));
        no_truncate(&widgets.read_mark_row);
        widgets.read_mark_row.set_selected(match init.read_mark {
            crate::config::ReadMark::Shown => 0,
            crate::config::ReadMark::Delay => 1,
            crate::config::ReadMark::Manual => 2,
        });
        widgets.threading_row.set_active(init.threading);
        widgets.threads_expanded_row.set_active(init.threads_expanded);
        widgets.thread_newest_first_row.set_active(init.thread_newest_first);
        widgets.always_show_recipients_row.set_active(init.always_show_recipients);
        widgets.single_message_card_row.set_active(init.single_message_card);
        widgets.thread_expansion_row.set_active(init.thread_expansion);
        widgets.confirm_thread_delete_row.set_active(init.confirm_thread_delete);
        widgets.card_actions_row.set_model(Some(&gtk::StringList::new(&[
            "Hidden behind a toggle",
            "Shown while hovering",
            "Always visible",
        ])));
        widgets.card_actions_row.set_selected(if init.card_actions_hover {
            0
        } else if init.card_actions_auto {
            1
        } else {
            2
        });
        widgets.list_palette_row.set_active(init.list_palette);
        widgets.list_palette_hover_row.set_active(init.list_palette_hover);
        widgets.compose_inline_row.set_active(init.compose_inline);
        widgets.paste_plain_row.set_active(init.paste_plain);
        widgets.spellcheck_row.set_active(init.spellcheck);
        widgets.spell_langs_row.set_text(&init.spellcheck_langs);
        widgets.spelling_group.set_description(Some(&dictionary_summary()));

        // Date and clock combos.
        let date_labels: Vec<&str> = DATE_STYLES.iter().map(|(l, _)| *l).collect();
        widgets
            .date_style_row
            .set_model(Some(&gtk::StringList::new(&date_labels)));
        widgets.date_style_row.set_selected(
            DATE_STYLES
                .iter()
                .position(|(_, s)| *s == init.date_style)
                .unwrap_or(0) as u32,
        );
        let clock_labels: Vec<&str> = CLOCK_STYLES.iter().map(|(l, _)| *l).collect();
        widgets
            .clock_style_row
            .set_model(Some(&gtk::StringList::new(&clock_labels)));
        widgets.clock_style_row.set_selected(
            CLOCK_STYLES
                .iter()
                .position(|(_, s)| *s == init.clock_style)
                .unwrap_or(0) as u32,
        );

        // Message-content appearance combo.
        let app_theme_labels: Vec<&str> = APP_THEMES.iter().map(|(l, _)| *l).collect();
        widgets
            .app_theme_row
            .set_model(Some(&gtk::StringList::new(&app_theme_labels)));
        let app_theme_sel = APP_THEMES
            .iter()
            .position(|(_, t)| *t == init.app_theme)
            .unwrap_or(0);
        widgets.app_theme_row.set_selected(app_theme_sel as u32);

        let theme_labels: Vec<&str> = MESSAGE_THEMES.iter().map(|(l, _)| *l).collect();
        widgets
            .message_theme_row
            .set_model(Some(&gtk::StringList::new(&theme_labels)));
        let theme_sel = MESSAGE_THEMES
            .iter()
            .position(|(_, t)| *t == init.message_theme)
            .unwrap_or(0);
        widgets.message_theme_row.set_selected(theme_sel as u32);

        // Hover-palette delay spinner (0–3000ms, step 50).
        // Actions Palette timeout: 1–30 seconds.
        let adj = gtk::Adjustment::new(init.palette_collapse_secs as f64, 1.0, 30.0, 1.0, 5.0, 0.0);
        widgets.palette_collapse_row.set_adjustment(Some(&adj));

        widgets
            .settings_open_row
            .set_model(Some(&gtk::StringList::new(&["Settings", "Accounts"])));
        widgets
            .settings_open_row
            .set_selected(if init.settings_open_accounts { 1 } else { 0 });

        // The view switcher drives the panel stack; the pages carry icons so
        // the switcher shows the standard icon-and-label tabs.
        widgets.switcher.set_stack(Some(&widgets.panels_stack));
        widgets.accounts_slot.set_child(Some(&init.accounts_panel));
        widgets
            .panels_stack
            .page(&widgets.accounts_slot)
            .set_icon_name(Some("co.hyprlab.Vireo-avatar-default-symbolic"));
        widgets
            .panels_stack
            .page(&widgets.prefs_page)
            .set_icon_name(Some("co.hyprlab.Vireo-applications-system-symbolic"));
        widgets.panels_stack.set_visible_child_name(if init.start_on_accounts {
            "accounts"
        } else {
            "preferences"
        });
        model.panels_stack = Some(widgets.panels_stack.clone());
        model.host_header = Some(widgets.host_header.clone());

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match message {
            PrefInput::ToggleSenderLogos(on) => {
                let _ = sender.output(PrefOutput::SetSenderLogos(on));
            }
            PrefInput::ChangeDateStyle(i) => {
                if let Some((_, style)) = DATE_STYLES.get(i as usize) {
                    let _ = sender.output(PrefOutput::SetDateStyle(*style));
                }
            }
            PrefInput::ChangeClockStyle(i) => {
                if let Some((_, style)) = CLOCK_STYLES.get(i as usize) {
                    let _ = sender.output(PrefOutput::SetClockStyle(*style));
                }
            }
            PrefInput::ToggleAvatars(on) => {
                let _ = sender.output(PrefOutput::SetAvatars(on));
            }
            PrefInput::ToggleGravatar(on) => {
                let _ = sender.output(PrefOutput::SetGravatar(on));
            }
            PrefInput::ToggleAutoRemoteContent(on) => {
                let _ = sender.output(PrefOutput::SetAutoRemoteContent(on));
            }
            PrefInput::ToggleThreading(on) => {
                self.threading = on;
                let _ = sender.output(PrefOutput::SetThreading(on));
            }
            PrefInput::ToggleThreadExpansion(on) => {
                self.thread_expansion = on;
                let _ = sender.output(PrefOutput::SetThreadExpansion(on));
            }
            PrefInput::ToggleConfirmThreadDelete(on) => {
                let _ = sender.output(PrefOutput::SetConfirmThreadDelete(on));
            }
            PrefInput::ToggleThreadsExpanded(on) => {
                let _ = sender.output(PrefOutput::SetThreadsExpanded(on));
            }
            PrefInput::ToggleThreadNewestFirst(on) => {
                let _ = sender.output(PrefOutput::SetThreadNewestFirst(on));
            }
            PrefInput::ToggleAlwaysShowRecipients(on) => {
                let _ = sender.output(PrefOutput::SetAlwaysShowRecipients(on));
            }
            PrefInput::ToggleSingleMessageCard(on) => {
                let _ = sender.output(PrefOutput::SetSingleMessageCard(on));
            }
            PrefInput::ChangeCardActionsMode(index) => {
                let (hover_toggle, hover_auto) = match index {
                    0 => (true, false),
                    1 => (false, true),
                    _ => (false, false),
                };
                let _ = sender.output(PrefOutput::SetCardActionsMode {
                    hover_toggle,
                    hover_auto,
                });
            }
            PrefInput::ToggleListPalette(on) => {
                self.list_palette = on;
                let _ = sender.output(PrefOutput::SetListPalette(on));
            }
            PrefInput::ToggleListPaletteHover(on) => {
                let _ = sender.output(PrefOutput::SetListPaletteHover(on));
            }
            PrefInput::ToggleComposeInline(on) => {
                let _ = sender.output(PrefOutput::SetComposeInline(on));
            }
            PrefInput::TogglePastePlain(on) => {
                let _ = sender.output(PrefOutput::SetPastePlain(on));
            }
            PrefInput::ToggleSpellcheck(on) => {
                let _ = sender.output(PrefOutput::SetSpellcheck(on));
            }
            PrefInput::SpellLangsEdited(langs) => {
                let _ = sender.output(PrefOutput::SetSpellcheckLangs(langs));
            }
            PrefInput::ChangeFetchInterval(index) => {
                let secs = FETCH_INTERVALS
                    .get(index as usize)
                    .map(|(_, s)| *s)
                    .unwrap_or(0);
                let _ = sender.output(PrefOutput::SetFetchInterval(secs));
            }
            PrefInput::TogglePush(on) => {
                let _ = sender.output(PrefOutput::SetPush(on));
            }
            PrefInput::ToggleNotifications(on) => {
                self.notifications = on;
                let _ = sender.output(PrefOutput::SetNotifications(on));
            }
            PrefInput::ToggleNotificationContent(on) => {
                let _ = sender.output(PrefOutput::SetNotificationContent(on));
            }
            PrefInput::ToggleAttachmentsRow(on) => {
                let _ = sender.output(PrefOutput::SetAttachmentsRow(on));
            }
            PrefInput::ToggleShowUnified(on) => {
                self.show_unified = on;
                let _ = sender.output(PrefOutput::SetShowUnified(on));
            }
            PrefInput::ToggleUnifiedChip(on) => {
                let _ = sender.output(PrefOutput::SetUnifiedChip(on));
            }
            PrefInput::ChangeChevronSide(idx) => {
                let _ = sender.output(PrefOutput::SetChevronsLeft(idx == 0));
            }
            PrefInput::ToggleContactsRow(on) => {
                let _ = sender.output(PrefOutput::SetContactsRow(on));
            }
            PrefInput::ToggleSidebarHoverExpand(on) => {
                let _ = sender.output(PrefOutput::SetSidebarHoverExpand(on));
            }
            PrefInput::ToggleSingleKey(on) => {
                let _ = sender.output(PrefOutput::SetSingleKey(on));
            }
            PrefInput::ToggleConsoleMode(on) => {
                let _ = sender.output(PrefOutput::SetConsoleMode(on));
            }
            PrefInput::ChangeReadMark(idx) => {
                let policy = match idx {
                    1 => crate::config::ReadMark::Delay,
                    2 => crate::config::ReadMark::Manual,
                    _ => crate::config::ReadMark::Shown,
                };
                let _ = sender.output(PrefOutput::SetReadMark(policy));
            }
            PrefInput::ExportSettings => {
                let _ = sender.output(PrefOutput::ExportSettings);
            }
            PrefInput::ImportSettings => {
                let _ = sender.output(PrefOutput::ImportSettings);
            }
            PrefInput::ToggleRunInBackground(on) => {
                let _ = sender.output(PrefOutput::SetRunInBackground(on));
            }
            PrefInput::ToggleAutostart(on) => {
                let _ = sender.output(PrefOutput::SetAutostart(on));
            }
            PrefInput::ChangePreviewLines(index) => {
                // The combo lists Off, then 1, 2 and 3 lines — so the row index is
                // the number of lines.
                let _ = sender.output(PrefOutput::SetPreviewLines(index));
            }
            PrefInput::ChangePaletteCollapse(secs) => {
                let _ = sender.output(PrefOutput::SetPaletteCollapse(secs));
            }
            PrefInput::ChangeAppTheme(index) => {
                let theme = APP_THEMES
                    .get(index as usize)
                    .map(|(_, t)| *t)
                    .unwrap_or_default();
                let _ = sender.output(PrefOutput::SetAppTheme(theme));
            }
            PrefInput::ChangeSettingsOpen(index) => {
                let _ = sender.output(PrefOutput::SetSettingsOpenAccounts(index == 1));
            }
            PrefInput::ShowAccounts(accounts) => {
                if let Some(stack) = &self.panels_stack {
                    stack.set_visible_child_name(if accounts { "accounts" } else { "preferences" });
                }
            }
            PrefInput::EditorOpen(open) => {
                if let Some(header) = &self.host_header {
                    header.set_visible(!open);
                }
            }
            PrefInput::ChangeMessageTheme(index) => {
                let theme = MESSAGE_THEMES
                    .get(index as usize)
                    .map(|(_, t)| *t)
                    .unwrap_or_default();
                let _ = sender.output(PrefOutput::SetMessageTheme(theme));
            }
            PrefInput::ToggleShowRemoteBanner(on) => {
                let _ = sender.output(PrefOutput::SetShowRemoteBanner(on));
            }
        }
    }
}



/// What spell-checking can actually use: the hunspell dictionaries that
/// resolve to real files. Inside the Flatpak most of `/usr/share/hunspell` is
/// dangling symlinks until the locale extension carries the language, and a
/// language without a dictionary silently checks nothing — so the Spelling
/// group says out loud what is installed (#114).
fn dictionary_summary() -> String {
    let mut codes: std::collections::BTreeSet<String> = Default::default();
    for dir in ["/usr/share/hunspell", "/usr/share/myspell"] {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            let is_dic = p.extension().is_some_and(|x| x == "dic");
            // metadata() follows symlinks: a dangling one (locale extension
            // without that language) errs out and is rightly skipped.
            if is_dic && p.metadata().map(|m| m.is_file()).unwrap_or(false) {
                if let Some(s) = p.file_stem().and_then(|s| s.to_str()) {
                    codes.insert(s.to_string());
                }
            }
        }
    }
    if codes.is_empty() {
        return "No dictionaries are visible to the app. On Flatpak, add your \
                language with: flatpak config --set extra-languages <code>"
            .to_string();
    }
    let total = codes.len();
    let shown: Vec<String> = codes.into_iter().take(12).collect();
    let mut out = format!("Installed dictionaries: {}", shown.join(", "));
    if total > shown.len() {
        out.push_str(&format!(" and {} more", total - shown.len()));
    }
    out
}
