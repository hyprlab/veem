//! Settings window: privacy options (remote-content allowlist).
//!
//! Account credentials are managed in their own window (see `ui/accounts.rs`).

use adw::prelude::*;
use relm4::factory::FactoryVecDeque;
use relm4::prelude::*;

use crate::config::{AppTheme, ClockStyle, DateStyle, MessageTheme};

/// Initial data for the settings window.
#[derive(Debug)]
pub struct PrefInit {
    pub allowed_senders: Vec<String>,
    pub auto_remote_content: bool,
    pub show_remote_banner: bool,
    pub gravatar: bool,
    pub avatars: bool,
    pub sender_logos: bool,
    pub date_style: DateStyle,
    pub clock_style: ClockStyle,
    pub fetch_interval_secs: u64,
    pub push: bool,
    pub blacklist: Vec<String>,
    pub palette_collapse_secs: u64,
    pub threading: bool,
    pub threads_expanded: bool,
    /// Conversation card actions hide until the card is hovered.
    pub card_actions_hover: bool,
    /// With the ⋯ toggle off: card actions appear automatically on hover.
    pub card_actions_auto: bool,
    /// The list's Actions Palette opens on row hover (no ⋯ click).
    pub list_palette_hover: bool,
    /// "New message" composes inline over the reading pane (vs a window).
    pub compose_inline: bool,
    pub message_theme: MessageTheme,
    pub app_theme: AppTheme,
    pub notifications: bool,
    pub notification_content: bool,
    pub show_attachments: bool,
    pub sidebar_hover_expand: bool,
    pub preview_lines: u32,
    pub single_key_shortcuts: bool,
    pub run_in_background: bool,
    pub autostart: bool,
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
    senders: FactoryVecDeque<SenderRow>,
    sender_addrs: Vec<String>,
    blacklist: FactoryVecDeque<SenderRow>,
    blacklist_addrs: Vec<String>,
    /// Mirrors the notifications switch, so the "show sender and subject" row
    /// below it can grey out when nothing is being posted at all.
    notifications: bool,
}

#[derive(Debug)]
pub enum PrefInput {
    AddSenderText(String),
    RemoveSenderRow(String),
    ToggleShowRemoteBanner(bool),
    AddBlacklistText(String),
    RemoveBlacklistRow(String),
    ToggleAutoRemoteContent(bool),
    ToggleGravatar(bool),
    ToggleAvatars(bool),
    ToggleSenderLogos(bool),
    ChangeDateStyle(u32),
    ChangeClockStyle(u32),
    ToggleThreading(bool),
    ToggleThreadsExpanded(bool),
    ChangeCardActionsMode(u32),
    ToggleListPaletteHover(bool),
    ToggleComposeInline(bool),
    ChangeFetchInterval(u32),
    TogglePush(bool),
    ToggleNotifications(bool),
    ToggleNotificationContent(bool),
    ToggleShowAttachments(bool),
    ToggleSidebarHoverExpand(bool),
    ChangePreviewLines(u32),
    ToggleSingleKey(bool),
    ToggleRunInBackground(bool),
    ToggleAutostart(bool),
    ChangePaletteCollapse(u64),
    ChangeMessageTheme(u32),
    ChangeAppTheme(u32),
}

#[derive(Debug)]
pub enum PrefOutput {
    AddSender(String),
    RemoveSender(String),
    AddBlacklist(String),
    RemoveBlacklist(String),
    SetAutoRemoteContent(bool),
    SetShowRemoteBanner(bool),
    SetGravatar(bool),
    SetAvatars(bool),
    SetSenderLogos(bool),
    SetDateStyle(DateStyle),
    SetClockStyle(ClockStyle),
    SetThreading(bool),
    SetThreadsExpanded(bool),
    SetCardActionsMode { hover_toggle: bool, hover_auto: bool },
    SetListPaletteHover(bool),
    SetComposeInline(bool),
    SetFetchInterval(u64),
    SetPush(bool),
    SetNotifications(bool),
    SetNotificationContent(bool),
    SetShowAttachments(bool),
    SetSidebarHoverExpand(bool),
    SetAppTheme(AppTheme),
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
            set_default_width: 500,
            // Remembered vertical size (tall by default) — resizing sticks
            // across restarts via the save on close below.
            set_default_height: crate::config::load_prefs_height(),
            set_title: Some("Settings"),

            connect_close_request[sender] => move |w| {
                crate::config::save_prefs_height(w.height());
                let _ = sender.output(PrefOutput::Closed);
                gtk::glib::Propagation::Proceed
            },

            #[wrap(Some)]
            set_content = &adw::ToolbarView {
                add_top_bar = &adw::HeaderBar {},

                #[wrap(Some)]
                set_content = &adw::PreferencesPage {
                    add = &adw::PreferencesGroup {
                        set_title: "Mail",

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

                        #[name = "show_attachments_row"]
                        adw::SwitchRow {
                            set_title: "Attachments in the sidebar",
                            set_subtitle: "Show a shortcut for browsing every account's attachments.",
                            connect_active_notify[sender] => move |row| {
                                sender.input(PrefInput::ToggleShowAttachments(row.is_active()));
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

                        #[name = "threading_row"]
                        adw::SwitchRow {
                            set_title: "Group messages by conversation",
                            set_subtitle: "Collapse replies into a single threaded conversation.",
                            connect_active_notify[sender] => move |row| {
                                sender.input(PrefInput::ToggleThreading(row.is_active()));
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

                        #[name = "avatars_row"]
                        adw::SwitchRow {
                            set_title: "Sender circles",
                            set_subtitle: "The coloured circle of initials beside each message, in \
                                           the list and above the message. Turning it off gives \
                                           the sender and subject more room.",
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

                        #[name = "threads_expanded_row"]
                        adw::SwitchRow {
                            set_title: "Expand conversations by default",
                            set_subtitle: "Show every message of a conversation in the list. \
                                           When off, conversations start collapsed to their \
                                           newest message.",
                            connect_active_notify[sender] => move |row| {
                                sender.input(PrefInput::ToggleThreadsExpanded(row.is_active()));
                            },
                        },

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

                        #[name = "card_actions_row"]
                        adw::ComboRow {
                            set_title: "Message card actions",
                            set_subtitle: "How each message's action icons show in the \
                                           reader, single or threaded.",
                            connect_selected_notify[sender] => move |row| {
                                sender.input(PrefInput::ChangeCardActionsMode(row.selected()));
                            },
                        },

                        #[name = "list_palette_hover_row"]
                        adw::SwitchRow {
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
                        set_title: "Appearance",

                        #[name = "app_theme_row"]
                        adw::ComboRow {
                            set_title: "Style",
                            set_subtitle: "The app itself. Message content has its own \
                                           setting under Reading.",
                            connect_selected_notify[sender] => move |row| {
                                sender.input(PrefInput::ChangeAppTheme(row.selected()));
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
                        set_title: "Reading",

                        #[name = "message_theme_row"]
                        adw::ComboRow {
                            set_title: "Message appearance",
                            set_subtitle: "Theme for email content only, not the app itself.",
                            connect_selected_notify[sender] => move |row| {
                                sender.input(PrefInput::ChangeMessageTheme(row.selected()));
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
                            set_subtitle: "Fills the sender circle with the brand's own site \
                                           icon, fetched from the sender's domain. That domain \
                                           learns your IP address, which is what blocking \
                                           remote content otherwise avoids.",
                            connect_active_notify[sender] => move |row| {
                                sender.input(PrefInput::ToggleSenderLogos(row.is_active()));
                            },
                        },

                    },

                    add = &adw::PreferencesGroup {
                        set_title: "Allowed Senders",
                        set_description: Some(
                            "Messages from these senders load remote content automatically."
                        ),

                        #[name = "add_sender_row"]
                        adw::EntryRow {
                            set_title: "Email address",
                            set_input_purpose: gtk::InputPurpose::Email,
                            // The + is the apply button, so there is only one way
                            // to add and it reads the same in both lists.
                            set_show_apply_button: false,
                            connect_entry_activated[sender] => move |row| {
                                sender.input(PrefInput::AddSenderText(row.text().to_string()));
                                row.set_text("");
                            },

                            add_suffix = &gtk::Button {
                                set_icon_name: "co.hyprlab.Vireo-list-add-symbolic",
                                set_tooltip_text: Some("Allow this sender"),
                                set_valign: gtk::Align::Center,
                                add_css_class: "flat",
                                connect_clicked[sender, add_sender_row] => move |_| {
                                    sender.input(PrefInput::AddSenderText(
                                        add_sender_row.text().to_string(),
                                    ));
                                    add_sender_row.set_text("");
                                },
                            },
                        },

                        #[local_ref]
                        senders_box -> gtk::ListBox {
                            add_css_class: "boxed-list",
                            add_css_class: "sender-list",
                            set_selection_mode: gtk::SelectionMode::None,
                        },
                    },

                    add = &adw::PreferencesGroup {
                        set_title: "Blacklist",
                        set_description: Some(
                            "Incoming mail from these senders is deleted automatically \
                             (moved to Trash). Enter an email address, or a whole domain \
                             like \"example.com\" to block every sender there."
                        ),

                        #[name = "add_blacklist_row"]
                        adw::EntryRow {
                            set_title: "Address or domain",
                            set_show_apply_button: false,
                            connect_entry_activated[sender] => move |row| {
                                sender.input(PrefInput::AddBlacklistText(row.text().to_string()));
                                row.set_text("");
                            },

                            add_suffix = &gtk::Button {
                                set_icon_name: "co.hyprlab.Vireo-list-add-symbolic",
                                set_tooltip_text: Some("Block this sender"),
                                set_valign: gtk::Align::Center,
                                add_css_class: "flat",
                                connect_clicked[sender, add_blacklist_row] => move |_| {
                                    sender.input(PrefInput::AddBlacklistText(
                                        add_blacklist_row.text().to_string(),
                                    ));
                                    add_blacklist_row.set_text("");
                                },
                            },
                        },

                        #[local_ref]
                        blacklist_box -> gtk::ListBox {
                            add_css_class: "boxed-list",
                            add_css_class: "sender-list",
                            set_selection_mode: gtk::SelectionMode::None,
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
        let senders = FactoryVecDeque::builder()
            .launch(gtk::ListBox::new())
            .forward(sender.input_sender(), |out| match out {
                SenderRowOutput::Remove(addr) => PrefInput::RemoveSenderRow(addr),
            });
        let blacklist = FactoryVecDeque::builder()
            .launch(gtk::ListBox::new())
            .forward(sender.input_sender(), |out| match out {
                SenderRowOutput::Remove(addr) => PrefInput::RemoveBlacklistRow(addr),
            });

        let mut model = Preferences {
            senders,
            sender_addrs: Vec::new(),
            blacklist,
            blacklist_addrs: Vec::new(),
            notifications: init.notifications,
        };

        {
            let mut guard = model.senders.guard();
            for addr in &init.allowed_senders {
                model.sender_addrs.push(addr.clone());
                guard.push_back(addr.clone());
            }
        }
        {
            let mut guard = model.blacklist.guard();
            for addr in &init.blacklist {
                model.blacklist_addrs.push(addr.clone());
                guard.push_back(addr.clone());
            }
        }

        let senders_box = model.senders.widget();
        let blacklist_box = model.blacklist.widget();
        let widgets = view_output!();
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
        widgets.threading_row.set_active(init.threading);
        widgets.threads_expanded_row.set_active(init.threads_expanded);
        widgets.card_actions_row.set_model(Some(&gtk::StringList::new(&[
            "Hidden behind a \u{22ef} toggle",
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
        widgets.list_palette_hover_row.set_active(init.list_palette_hover);
        widgets.compose_inline_row.set_active(init.compose_inline);

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
                let _ = sender.output(PrefOutput::SetThreading(on));
            }
            PrefInput::ToggleThreadsExpanded(on) => {
                let _ = sender.output(PrefOutput::SetThreadsExpanded(on));
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
            PrefInput::ToggleListPaletteHover(on) => {
                let _ = sender.output(PrefOutput::SetListPaletteHover(on));
            }
            PrefInput::ToggleComposeInline(on) => {
                let _ = sender.output(PrefOutput::SetComposeInline(on));
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
            PrefInput::ToggleShowAttachments(on) => {
                let _ = sender.output(PrefOutput::SetShowAttachments(on));
            }
            PrefInput::ToggleSidebarHoverExpand(on) => {
                let _ = sender.output(PrefOutput::SetSidebarHoverExpand(on));
            }
            PrefInput::ToggleSingleKey(on) => {
                let _ = sender.output(PrefOutput::SetSingleKey(on));
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
            PrefInput::ChangeMessageTheme(index) => {
                let theme = MESSAGE_THEMES
                    .get(index as usize)
                    .map(|(_, t)| *t)
                    .unwrap_or_default();
                let _ = sender.output(PrefOutput::SetMessageTheme(theme));
            }
            PrefInput::AddSenderText(text) => {
                let addr = text.trim().to_lowercase();
                if !addr.is_empty() && !self.sender_addrs.contains(&addr) {
                    self.sender_addrs.push(addr.clone());
                    self.senders.guard().push_back(addr.clone());
                    let _ = sender.output(PrefOutput::AddSender(addr));
                }
            }

            PrefInput::RemoveSenderRow(addr) => {
                if let Some(pos) = self.sender_addrs.iter().position(|s| *s == addr) {
                    self.sender_addrs.remove(pos);
                    self.senders.guard().remove(pos);
                    let _ = sender.output(PrefOutput::RemoveSender(addr));
                }
            }

            PrefInput::ToggleShowRemoteBanner(on) => {
                let _ = sender.output(PrefOutput::SetShowRemoteBanner(on));
            }
            PrefInput::AddBlacklistText(text) => {
                let addr = text.trim().to_lowercase();
                if !addr.is_empty() && !self.blacklist_addrs.contains(&addr) {
                    self.blacklist_addrs.push(addr.clone());
                    self.blacklist.guard().push_back(addr.clone());
                    let _ = sender.output(PrefOutput::AddBlacklist(addr));
                }
            }

            PrefInput::RemoveBlacklistRow(addr) => {
                if let Some(pos) = self.blacklist_addrs.iter().position(|s| *s == addr) {
                    self.blacklist_addrs.remove(pos);
                    self.blacklist.guard().remove(pos);
                    let _ = sender.output(PrefOutput::RemoveBlacklist(addr));
                }
            }
        }
    }
}
