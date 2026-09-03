//! Accounts panel: manage all mail accounts (add / edit / remove / reorder).
//!
//! Not a window of its own: the panel is embedded behind the "Accounts" tab
//! of the combined Accounts & Preferences window (see `ui/preferences.rs`).
//! It uses an `AdwNavigationView` with two pages: a list of accounts (drag rows
//! to set the sidebar order) and a reusable editor form pushed on top.

use adw::prelude::*;
use relm4::prelude::*;

use crate::config::{split_identity, AccountConfig, AliasConfig, OAuthSettings, Protocol};
use crate::ui::rich_editor::{self, RichEditor};
use crate::ui::preferences::{SenderRow, SenderRowOutput};
use crate::worker::{self, ConnTest};

const DEFAULT_COLOR: &str = "#3584e4";

/// How an account signs in, chosen via the single Provider dropdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProviderKind {
    /// Manual IMAP/POP3 + password ("Other (IMAP/POP3)…").
    Manual,
    /// A known IMAP provider: password auth with auto-filled servers.
    Preset,
    /// Google OAuth (browser sign-in; falls back to GNOME Online Accounts).
    Google,
    /// Microsoft OAuth (browser sign-in).
    Microsoft,
    /// OAuth against a user-entered provider ("Custom (OAuth)…").
    CustomOAuth,
}

/// One entry in the Provider dropdown. It selects both the sign-in method and,
/// for `Preset`, the IMAP/SMTP servers to auto-fill. `hint` is shown as the row's
/// subtitle. Server fields are empty for non-`Preset` kinds (OAuth providers get
/// their servers from `crate::oauth::preset`; Manual/Custom are user-entered).
pub(crate) struct Provider {
    label: &'static str,
    kind: ProviderKind,
    imap_host: &'static str,
    imap_port: u16,
    smtp_host: &'static str,
    smtp_port: u16,
    hint: &'static str,
}

impl Provider {
    /// Wizard accessors (src/ui/welcome.rs): the fields stay private to this
    /// module, which owns the table's meaning.
    pub(crate) fn wizard_password_provider(&self) -> bool {
        self.is_password()
    }
    pub(crate) fn wizard_label(&self) -> &'static str {
        self.label
    }
    pub(crate) fn wizard_servers(&self) -> (&'static str, u16, &'static str, u16) {
        (self.imap_host, self.imap_port, self.smtp_host, self.smtp_port)
    }
    pub(crate) fn wizard_hint(&self) -> &'static str {
        self.hint
    }

    fn is_password(&self) -> bool {
        matches!(self.kind, ProviderKind::Manual | ProviderKind::Preset)
    }
    fn is_oauth(&self) -> bool {
        !self.is_password()
    }
    /// OAuth preset key for the built-in providers.
    fn oauth_name(&self) -> Option<&'static str> {
        match self.kind {
            ProviderKind::Google => Some("google"),
            ProviderKind::Microsoft => Some("microsoft"),
            _ => None,
        }
    }
}

const APP_PW: &str = "Requires an app-specific password (not your normal login password).";

/// The Provider dropdown, in display order. OAuth options first, then the major
/// app-password IMAP providers, then the two manual escape hatches. IMAP uses
/// SSL/TLS on 993; SMTP uses implicit TLS on 465 or STARTTLS on 587.
pub(crate) const PROVIDERS: &[Provider] = &[
    Provider { label: "Google (Gmail) — sign in", kind: ProviderKind::Google, imap_host: "", imap_port: 0, smtp_host: "", smtp_port: 0, hint: "Sign in with your browser — no password needed." },
    Provider { label: "Microsoft 365 / Outlook", kind: ProviderKind::Microsoft, imap_host: "", imap_port: 0, smtp_host: "", smtp_port: 0, hint: "Sign in through GNOME Online Accounts." },
    Provider { label: "iCloud", kind: ProviderKind::Preset, imap_host: "imap.mail.me.com", imap_port: 993, smtp_host: "smtp.mail.me.com", smtp_port: 587, hint: APP_PW },
    Provider { label: "Yahoo Mail", kind: ProviderKind::Preset, imap_host: "imap.mail.yahoo.com", imap_port: 993, smtp_host: "smtp.mail.yahoo.com", smtp_port: 465, hint: APP_PW },
    Provider { label: "Proton Mail (Bridge)", kind: ProviderKind::Preset, imap_host: "127.0.0.1", imap_port: 1143, smtp_host: "127.0.0.1", smtp_port: 1025, hint: "Requires Proton Mail Bridge running locally." },
    Provider { label: "Fastmail", kind: ProviderKind::Preset, imap_host: "imap.fastmail.com", imap_port: 993, smtp_host: "smtp.fastmail.com", smtp_port: 465, hint: APP_PW },
    Provider { label: "AOL Mail", kind: ProviderKind::Preset, imap_host: "imap.aol.com", imap_port: 993, smtp_host: "smtp.aol.com", smtp_port: 465, hint: APP_PW },
    Provider { label: "Zoho Mail", kind: ProviderKind::Preset, imap_host: "imap.zoho.com", imap_port: 993, smtp_host: "smtp.zoho.com", smtp_port: 465, hint: "" },
    Provider { label: "GMX", kind: ProviderKind::Preset, imap_host: "imap.gmx.com", imap_port: 993, smtp_host: "mail.gmx.com", smtp_port: 587, hint: "Enable POP/IMAP access in GMX settings first." },
    Provider { label: "Yandex Mail", kind: ProviderKind::Preset, imap_host: "imap.yandex.com", imap_port: 993, smtp_host: "smtp.yandex.com", smtp_port: 465, hint: APP_PW },
    Provider { label: "Mail.com", kind: ProviderKind::Preset, imap_host: "imap.mail.com", imap_port: 993, smtp_host: "smtp.mail.com", smtp_port: 587, hint: "" },
    Provider { label: "Custom (OAuth)…", kind: ProviderKind::CustomOAuth, imap_host: "", imap_port: 0, smtp_host: "", smtp_port: 0, hint: "Enter your provider's OAuth endpoints, then sign in." },
    Provider { label: "Other (IMAP/POP3)…", kind: ProviderKind::Manual, imap_host: "", imap_port: 0, smtp_host: "", smtp_port: 0, hint: "Enter your server details manually." },
];

/// Dropdown index of the "Other (IMAP/POP3)…" manual entry (the default).
fn manual_index() -> u32 {
    PROVIDERS
        .iter()
        .position(|p| p.kind == ProviderKind::Manual)
        .unwrap_or(0) as u32
}

/// The provider entry for a dropdown index (clamped to the manual default).
fn provider_at(idx: u32) -> &'static Provider {
    PROVIDERS
        .get(idx as usize)
        .unwrap_or(&PROVIDERS[manual_index() as usize])
}

pub struct AccountsWindow {
    /// Accounts in display order.
    accounts: Vec<AccountConfig>,
    /// Each account's live folder list (path, display name), for the editor's
    /// Special Folders combos — pushed by the app, keyed by account email.
    folders_by_email: std::collections::HashMap<String, Vec<(String, String)>>,
    /// Allowed-senders and blocklist rows (moved here from Settings).
    senders: relm4::factory::FactoryVecDeque<SenderRow>,
    sender_addrs: Vec<String>,
    blacklist: relm4::factory::FactoryVecDeque<SenderRow>,
    blacklist_addrs: Vec<String>,
    /// Filter rules (#47), managed on this tab.
    filter_rules: Vec<crate::config::FilterRule>,
    filters_list: Option<gtk::ListBox>,
    /// Paths behind the currently-open editor's folder combos (index 0 in the
    /// combo is "Automatic"; entry N here is combo index N + 1).
    folder_paths: Vec<String>,
    /// Index being edited; `None` while adding a new account.
    editing: Option<usize>,
    /// Emoji currently chosen in the editor (`None` → use initials).
    emoji: Option<String>,
    /// WYSIWYG editor for the account signature.
    sig_editor: RichEditor,
    /// The email value the label field currently mirrors, so the label auto-fills
    /// from the email until the user customizes it.
    label_synced: String,
    /// GNOME Online Accounts mail accounts available to import (not yet in Vireo).
    goa: Vec<crate::goa::GoaMailAccount>,
    /// Refresh token captured from a successful OAuth sign-in, applied on save.
    pending_oauth_refresh: Option<String>,
    /// The send-as aliases being edited for the account in the editor (#34).
    /// Committed to the account on Save.
    alias_edits: Vec<AliasConfig>,
    /// Index into `alias_edits` open in the alias dialog; `None` while adding.
    alias_editing: Option<usize>,
    /// The open alias editor dialog and its fields, if any.
    alias_dialog: Option<AliasDialog>,
}

/// The alias editor dialog (#34): a small modal for one send-as alias — its
/// identity, and optionally its own SMTP transport.
struct AliasDialog {
    window: adw::Window,
    name_row: adw::EntryRow,
    addr_row: adw::EntryRow,
    smtp_switch: adw::SwitchRow,
    host_row: adw::EntryRow,
    port_row: adw::EntryRow,
    user_row: adw::EntryRow,
    pass_row: adw::PasswordEntryRow,
    test_btn: gtk::Button,
    test_result: gtk::Label,
}

#[derive(Debug)]
pub enum AccountsInput {
    /// The app's live folder lists per account email (for Special Folders).
    SetFolderChoices(std::collections::HashMap<String, Vec<(String, String)>>),
    AddAccount,
    EditAccount(usize),
    /// Open the editor for the account with this address (the sidebar's
    /// "Account Settings…"), leaving another account's editor if one is up.
    EditAccountByEmail(String),
    /// The email field changed — mirror it into the (auto-filled) label field.
    EmailChanged,
    MoveRow { from: usize, to: usize },
    /// Enable/disable an account from the list toggle.
    ToggleEnabled { index: usize, enabled: bool },
    /// Enable/disable the account currently open in the editor (GOA group toggle).
    ToggleCurrentEnabled(bool),
    /// Import a GNOME Online Account (by index into `goa`) into Vireo.
    ImportGoa(usize),
    /// The provider dropdown changed — adapt the form (servers vs. OAuth).
    ProviderChanged,
    /// Start the OAuth browser sign-in flow.
    OAuthSignIn,
    /// Open GNOME Settings → Online Accounts (the Google path).
    OpenOnlineAccounts,
    SetEmoji(String),
    ClearEmoji,
    TestConnection,
    Save,
    /// Second phase of Save, once the signature HTML has been read from the editor.
    SaveWithSig(String),
    /// Clicked "Remove Account" — ask for confirmation first.
    RemoveCurrent,
    /// Confirmed in the dialog — actually remove the account being edited.
    ConfirmRemove,
    /// Open the alias dialog to add a new send-as alias (#34).
    AliasAdd,
    /// Open the alias dialog on an existing alias (by index into `alias_edits`).
    AliasEdit(usize),
    /// Remove an alias from the list being edited.
    AliasRemove(usize),
    /// The alias dialog's Save button.
    AliasDialogSave,
    /// The alias dialog's Test button: try its SMTP server and credentials.
    AliasDialogTest,
    /// The alias dialog was closed (Cancel, Esc, or after a save).
    AliasDialogClosed,
    /// Allow list / blocklist / filters (moved here from Settings).
    AddSenderText(String),
    RemoveSenderRow(String),
    AddBlacklistText(String),
    RemoveBlacklistRow(String),
    AddFilter,
    RemoveFilter(usize),
    FilterAdded(crate::config::FilterRule),
}

#[derive(Debug)]
pub enum AccountsOutput {
    /// `original_email` is `Some` (the pre-edit email) when editing, `None` when adding.
    Saved {
        original_email: Option<String>,
        account: Box<AccountConfig>,
    },
    Removed { email: String },
    /// New display order, as account emails.
    Reordered(Vec<String>),
    /// An account was enabled/disabled from the list.
    EnabledChanged { email: String, enabled: bool },
    /// Import a GNOME Online Account into Vireo (with its credentials).
    ImportGoa(Box<AccountConfig>),
    /// The editor subpage opened (true) or closed (false) — the combined
    /// settings window hides its shared header while it is open.
    EditorOpen(bool),
    /// Mail-hygiene changes, routed to the same app handlers Settings used.
    AddSender(String),
    RemoveSender(String),
    AddBlacklist(String),
    RemoveBlacklist(String),
    SetFilters(Vec<crate::config::FilterRule>),
}

/// Whether a GOA account's mail runs over the Microsoft Graph API: the
/// "Microsoft 365" (`ms_graph`) provider has no IMAP — its token is
/// Graph-scoped — so the imported account uses [`Protocol::Graph`] (issue #36).
fn goa_uses_graph(g: &crate::goa::GoaMailAccount) -> bool {
    g.oauth2 && g.provider_type == "ms_graph"
}

/// GNOME Online Accounts mail accounts not (properly) configured in Vireo.
/// A configured entry counts when it has an IMAP host or runs over Graph — an
/// entry with neither is a broken pre-#36 Microsoft 365 import, so its GOA
/// account is offered again and re-importing repairs it. Accounts GOA can
/// neither serve IMAP nor Graph mail for can never connect and aren't listed.
fn importable_goa_accounts(configured: &[AccountConfig]) -> Vec<crate::goa::GoaMailAccount> {
    crate::goa::list_mail_accounts()
        .into_iter()
        .filter(|g| {
            !configured.iter().any(|a| {
                a.email.eq_ignore_ascii_case(&g.email)
                    && (!a.imap_host.trim().is_empty()
                        || a.protocol == crate::config::Protocol::Graph)
            })
        })
        .filter(|g| !g.imap_host.is_empty() || goa_uses_graph(g))
        .collect()
}

/// Background command results for the editor.
#[derive(Debug)]
pub enum AccountsCmd {
    /// Test-connection result.
    Test(ConnTest),
    /// OAuth sign-in result: the refresh token, or an error message.
    OAuth(Result<String, String>),
    /// Alias SMTP test result (#34).
    AliasTested(Result<(), String>),
}

/// Everything the Accounts panel needs at launch: the accounts themselves
/// plus the mail-hygiene lists that live on this tab (filters, allow list,
/// blocklist).
pub struct AccountsInit {
    pub accounts: Vec<AccountConfig>,
    pub allowed_senders: Vec<String>,
    pub blacklist: Vec<String>,
    pub filters: Vec<crate::config::FilterRule>,
}

#[relm4::component(pub)]
impl Component for AccountsWindow {
    type Init = AccountsInit;
    type Input = AccountsInput;
    type Output = AccountsOutput;
    type CommandOutput = AccountsCmd;

    view! {
        adw::Bin {
            #[wrap(Some)]
            #[name = "nav"]
            set_child = &adw::NavigationView {

                // ---- list page ----
                add = &adw::NavigationPage {
                    set_title: "Accounts",
                    set_tag: Some("list"),

                    #[wrap(Some)]
                    set_child = &adw::ToolbarView {
                        // No header of its own: the combined settings window's
                        // shared header (with the view switcher) sits above.

                        #[wrap(Some)]
                        set_content = &adw::PreferencesPage {
                            add = &adw::PreferencesGroup {
                                set_title: "Mail Accounts",
                                set_description: Some(
                                    "Drag to set the order they appear in the sidebar."
                                ),

                                #[name = "accounts_list"]
                                gtk::ListBox {
                                    add_css_class: "boxed-list",
                                    set_selection_mode: gtk::SelectionMode::None,
                                    connect_row_activated[sender] => move |_, row| {
                                        sender.input(AccountsInput::EditAccount(row.index() as usize));
                                    },
                                },
                            },

                            #[name = "goa_group"]
                            add = &adw::PreferencesGroup {
                                set_title: "GNOME Online Accounts",
                                set_description: Some(
                                    "Mail accounts from GNOME Settings. Toggle one on to \
                                     use it in Vireo."
                                ),
                                set_visible: false,

                                #[name = "goa_list"]
                                gtk::ListBox {
                                    add_css_class: "boxed-list",
                                    set_selection_mode: gtk::SelectionMode::None,
                                },
                            },

                            add = &adw::PreferencesGroup {
                                gtk::Button {
                                    set_label: "Add Account",
                                    add_css_class: "suggested-action",
                                    add_css_class: "pill",
                                    set_halign: gtk::Align::Center,
                                    connect_clicked => AccountsInput::AddAccount,
                                },
                            },

                            // Mail hygiene (moved from Settings): filters,
                            // the remote-content allow list, the blocklist.
                            add = &adw::PreferencesGroup {
                                set_title: "Filters",
                                set_description: Some(
                                    "File incoming mail into folders automatically, by \
                                     sender, subject or recipients. Applied to each \
                                     account's Inbox as Vireo syncs it."
                                ),
                                #[wrap(Some)]
                                set_header_suffix = &gtk::Button {
                                    set_label: "Add Filter…",
                                    set_valign: gtk::Align::Center,
                                    add_css_class: "flat",
                                    connect_clicked => AccountsInput::AddFilter,
                                },

                                #[name = "filters_list"]
                                gtk::ListBox {
                                    add_css_class: "boxed-list",
                                    set_selection_mode: gtk::SelectionMode::None,
                                },
                            },

                            add = &adw::PreferencesGroup {
                                set_title: "Allowed Senders",
                                set_description: Some(
                                    "Messages from these senders load remote content \
                                     automatically."
                                ),

                                #[name = "add_sender_row"]
                                adw::EntryRow {
                                    set_title: "Email address",
                                    set_input_purpose: gtk::InputPurpose::Email,
                                    set_show_apply_button: false,
                                    connect_entry_activated[sender] => move |row| {
                                        sender.input(AccountsInput::AddSenderText(row.text().to_string()));
                                        row.set_text("");
                                    },

                                    add_suffix = &gtk::Button {
                                        set_icon_name: "co.hyprlab.Vireo-list-add-symbolic",
                                        set_tooltip_text: Some("Allow this sender"),
                                        set_valign: gtk::Align::Center,
                                        add_css_class: "flat",
                                        connect_clicked[sender, add_sender_row] => move |_| {
                                            sender.input(AccountsInput::AddSenderText(
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
                                    "Incoming mail from these senders is deleted \
                                     automatically (moved to Trash). Enter an email \
                                     address, or a whole domain like \"example.com\" \
                                     to block every sender there."
                                ),

                                #[name = "add_blacklist_row"]
                                adw::EntryRow {
                                    set_title: "Address or domain",
                                    set_show_apply_button: false,
                                    connect_entry_activated[sender] => move |row| {
                                        sender.input(AccountsInput::AddBlacklistText(row.text().to_string()));
                                        row.set_text("");
                                    },

                                    add_suffix = &gtk::Button {
                                        set_icon_name: "co.hyprlab.Vireo-list-add-symbolic",
                                        set_tooltip_text: Some("Block this sender"),
                                        set_valign: gtk::Align::Center,
                                        add_css_class: "flat",
                                        connect_clicked[sender, add_blacklist_row] => move |_| {
                                            sender.input(AccountsInput::AddBlacklistText(
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
                },

                // ---- editor page ----
                add = &adw::NavigationPage {
                    set_title: "Account",
                    set_tag: Some("editor"),

                    #[wrap(Some)]
                    set_child = &adw::ToolbarView {
                        add_top_bar = &adw::HeaderBar {
                            set_show_end_title_buttons: false,
                            pack_end = &gtk::Button {
                                set_label: "Save",
                                add_css_class: "suggested-action",
                                connect_clicked => AccountsInput::Save,
                            },
                        },

                        #[wrap(Some)]
                        set_content = &adw::PreferencesPage {
                            // GNOME Online Accounts owns this account's servers and
                            // credentials; Vireo only mirrors them. Saying so where
                            // the greyed-out fields are is worth more than leaving
                            // the user to work out why they can't type.
                            // GNOME Online Accounts owns this account's servers and
                            // credentials; Vireo only mirrors them, and can hide it
                            // locally. Both facts belong together, above the fields
                            // they explain.
                            #[name = "goa_banner"]
                            add = &adw::PreferencesGroup {
                                set_visible: false,
                                set_title: "GNOME Online Account",

                                #[name = "goa_enabled_row"]
                                adw::SwitchRow {
                                    set_title: "Show in Vireo",
                                    set_subtitle: "Switching this off returns the account to the \
                                                   import list — it stays in GNOME Online Accounts.",
                                    connect_active_notify[sender] => move |row| {
                                        sender.input(AccountsInput::ToggleCurrentEnabled(row.is_active()));
                                    },
                                },

                                gtk::Box {
                                    set_orientation: gtk::Orientation::Vertical,
                                    set_spacing: 12,
                                    set_halign: gtk::Align::Start,
                                    set_margin_top: 12,

                                    gtk::Label {
                                        set_label: "This account is managed by GNOME Online Accounts.\nIts address, servers and password are changed in Settings \u{2192} Online Accounts.",
                                        set_xalign: 0.0,
                                        set_halign: gtk::Align::Start,
                                        set_wrap: true,
                                        add_css_class: "dim-label",
                                    },

                                    gtk::Button {
                                        set_label: "Open Online Accounts\u{2026}",
                                        set_halign: gtk::Align::Start,
                                        connect_clicked => AccountsInput::OpenOnlineAccounts,
                                    },
                                },
                            },

                            add = &adw::PreferencesGroup {
                                set_title: "Mail Account",

                                // Pick the provider first; the rest of the form
                                // adapts (server fields vs. OAuth sign-in).
                                #[name = "provider_row"]
                                adw::ComboRow {
                                    set_title: "Provider",
                                    set_subtitle: "Choose your email provider.",
                                    connect_selected_notify => AccountsInput::ProviderChanged,
                                },
                                #[name = "name_row"]
                                adw::EntryRow { set_title: "Display Name" },
                                #[name = "email_row"]
                                adw::EntryRow {
                                    set_title: "Email Address",
                                    set_input_purpose: gtk::InputPurpose::Email,
                                },
                                #[name = "protocol_row"]
                                adw::ComboRow {
                                    set_title: "Incoming Protocol",
                                },
                                #[name = "host_row"]
                                adw::EntryRow { set_title: "Incoming Server" },
                                #[name = "port_row"]
                                adw::EntryRow {
                                    set_title: "Port (IMAP 993 / POP3 995)",
                                    set_input_purpose: gtk::InputPurpose::Digits,
                                },
                                #[name = "smtp_row"]
                                adw::EntryRow { set_title: "SMTP Server (optional)" },
                                #[name = "smtp_port_row"]
                                adw::EntryRow {
                                    set_title: "SMTP Port (default 587)",
                                    set_input_purpose: gtk::InputPurpose::Digits,
                                },
                                #[name = "user_row"]
                                adw::EntryRow { set_title: "Username" },
                                #[name = "pass_row"]
                                adw::PasswordEntryRow { set_title: "Password" },

                                // ---- OAuth fields (shown when Authentication is an OAuth option) ----
                                #[name = "oauth_client_id_row"]
                                adw::EntryRow {
                                    set_title: "OAuth Client ID",
                                    set_visible: false,
                                },
                                #[name = "oauth_secret_row"]
                                adw::PasswordEntryRow {
                                    set_title: "OAuth Client Secret (optional)",
                                    set_visible: false,
                                },
                                #[name = "oauth_auth_url_row"]
                                adw::EntryRow {
                                    set_title: "Authorization URL",
                                    set_visible: false,
                                },
                                #[name = "oauth_token_url_row"]
                                adw::EntryRow {
                                    set_title: "Token URL",
                                    set_visible: false,
                                },
                                #[name = "oauth_scope_row"]
                                adw::EntryRow {
                                    set_title: "Scopes (space-separated)",
                                    set_visible: false,
                                },
                                #[name = "oauth_signin_btn"]
                                gtk::Button {
                                    set_label: "Sign In with Browser",
                                    set_halign: gtk::Align::Start,
                                    set_margin_top: 16,
                                    set_visible: false,
                                    add_css_class: "suggested-action",
                                    connect_clicked => AccountsInput::OAuthSignIn,
                                },
                                #[name = "oauth_status"]
                                gtk::Label {
                                    set_visible: false,
                                    set_halign: gtk::Align::Start,
                                    set_xalign: 0.0,
                                    set_wrap: true,
                                },

                                // Shown for Google/Microsoft when no built-in/own OAuth
                                // client is available: point the user at GNOME Online
                                // Accounts (the only sign-in path for these providers).
                                #[name = "goa_hint"]
                                gtk::Box {
                                    set_orientation: gtk::Orientation::Vertical,
                                    set_spacing: 12,
                                    set_margin_top: 8,
                                    set_visible: false,

                                    gtk::Label {
                                        set_wrap: true,
                                        set_xalign: 0.0,
                                        add_css_class: "dim-label",
                                        set_label: "Google and Microsoft sign-in use GNOME Online \
                                            Accounts.\n\n\
                                            1. Open Online Accounts and sign in there.\n\
                                            2. Come back to Vireo and reopen this window — the \
                                            account then appears under “GNOME Online \
                                            Accounts” at the top of this window. Enable it there.",
                                    },
                                    gtk::Button {
                                        set_label: "Open Online Accounts…",
                                        set_halign: gtk::Align::Start,
                                        add_css_class: "suggested-action",
                                        connect_clicked => AccountsInput::OpenOnlineAccounts,
                                    },
                                },

                                #[name = "smtp_separate_row"]
                                adw::SwitchRow {
                                    set_title: "Separate SMTP credentials",
                                    set_subtitle: "Use a different username and password for \
                                                   sending. Off = use the credentials above.",
                                },
                                #[name = "smtp_user_row"]
                                adw::EntryRow { set_title: "SMTP Username" },
                                #[name = "smtp_pass_row"]
                                adw::PasswordEntryRow { set_title: "SMTP Password" },

                                gtk::Box {
                                    set_orientation: gtk::Orientation::Vertical,
                                    set_spacing: 6,
                                    set_margin_top: 16,

                                    #[name = "test_btn"]
                                    gtk::Button {
                                        set_label: "Test Connection",
                                        set_halign: gtk::Align::Start,
                                        connect_clicked => AccountsInput::TestConnection,
                                    },
                                    #[name = "test_result"]
                                    gtk::Label {
                                        set_visible: false,
                                        set_halign: gtk::Align::Start,
                                        set_xalign: 0.0,
                                        set_wrap: true,
                                    },
                                },
                            },

                            add = &adw::PreferencesGroup {
                                set_title: "Appearance",
                                set_description: Some(
                                    "How this account is shown in the sidebar and \
                                     the All Inboxes view."
                                ),

                                #[name = "label_row"]
                                adw::EntryRow {
                                    set_title: "Label (defaults to email address)",
                                },

                                adw::ActionRow {
                                    set_title: "Circle color",
                                    #[name = "color_btn"]
                                    add_suffix = &gtk::ColorDialogButton {
                                        set_valign: gtk::Align::Center,
                                        set_dialog: &gtk::ColorDialog::new(),
                                    },
                                },

                                adw::ActionRow {
                                    set_title: "Emoji",
                                    set_subtitle: "Optional — shown instead of initials",

                                    #[name = "emoji_btn"]
                                    add_suffix = &gtk::MenuButton {
                                        set_valign: gtk::Align::Center,
                                        set_label: "Add",
                                        #[wrap(Some)]
                                        set_popover = &gtk::EmojiChooser {
                                            connect_emoji_picked[sender] => move |_, text| {
                                                sender.input(AccountsInput::SetEmoji(text.to_string()));
                                            },
                                        },
                                    },
                                    add_suffix = &gtk::Button {
                                        set_valign: gtk::Align::Center,
                                        set_label: "Use initials",
                                        set_tooltip_text: Some("Show name initials instead of an emoji"),
                                        connect_clicked => AccountsInput::ClearEmoji,
                                    },
                                },
                            },

                            // Send-as aliases (#34): extra From identities the
                            // composer offers, and replies to an alias answer
                            // from it. Each alias sends through this account's
                            // SMTP, or — for a forwarded mailbox whose provider
                            // would rewrite the sender — through its own.
                            add = &adw::PreferencesGroup {
                                set_title: "Send-as aliases",
                                set_description: Some(
                                    "Extra addresses this account can send as. An alias \
                                     can use this account's SMTP server, or bring its own."
                                ),

                                #[wrap(Some)]
                                set_header_suffix = &gtk::Button {
                                    set_label: "Add Alias…",
                                    set_valign: gtk::Align::Center,
                                    add_css_class: "flat",
                                    connect_clicked => AccountsInput::AliasAdd,
                                },

                                #[name = "aliases_list"]
                                gtk::ListBox {
                                    add_css_class: "boxed-list",
                                    set_selection_mode: gtk::SelectionMode::None,
                                    connect_row_activated[sender] => move |_, row| {
                                        sender.input(AccountsInput::AliasEdit(row.index() as usize));
                                    },
                                },
                            },

                            // Per-account push override (#91): some servers
                            // mishandle IDLE, and one bad account shouldn't
                            // cost the good ones their instant delivery.
                            add = &adw::PreferencesGroup {
                                set_title: "Syncing",

                                #[name = "push_row"]
                                adw::ComboRow {
                                    set_title: "Instant new mail (IMAP push)",
                                    set_subtitle: "Turn off for servers that stall on push connections.",
                                },
                            },

                            // Manual special-folder mapping (#82): for servers
                            // whose Sent/Trash/… aren't detected, pin each role
                            // to one of the account's real folders.
                            add = &adw::PreferencesGroup {
                                set_title: "Special Folders",
                                set_description: Some(
                                    "Where sent, deleted and junk mail goes. Automatic                                      follows the server's own markings; pick a folder                                      when a role isn't detected or lands wrong."
                                ),

                                #[name = "folder_sent_row"]
                                adw::ComboRow { set_title: "Sent" },
                                #[name = "folder_drafts_row"]
                                adw::ComboRow { set_title: "Drafts" },
                                #[name = "folder_trash_row"]
                                adw::ComboRow { set_title: "Trash" },
                                #[name = "folder_junk_row"]
                                adw::ComboRow { set_title: "Junk" },
                                #[name = "folder_archive_row"]
                                adw::ComboRow { set_title: "Archive" },
                            },

                            add = &adw::PreferencesGroup {
                                set_title: "Signature",
                                set_description: Some(
                                    "Appended to new messages sent from this account."
                                ),

                                #[name = "sig_holder"]
                                gtk::Box {
                                    set_orientation: gtk::Orientation::Vertical,
                                    set_height_request: 180,
                                    set_margin_top: 6,
                                },
                            },

                            // For a GOA-imported account this removes it from
                            // Vireo only — it stays in GNOME Online Accounts and
                            // returns to the import list.
                            #[name = "remove_group"]
                            add = &adw::PreferencesGroup {
                                gtk::Button {
                                    set_label: "Remove Account",
                                    add_css_class: "destructive-action",
                                    set_halign: gtk::Align::Center,
                                    connect_clicked => AccountsInput::RemoveCurrent,
                                },
                            },

                            add = &adw::PreferencesGroup {
                                gtk::Label {
                                    set_wrap: true,
                                    set_xalign: 0.0,
                                    add_css_class: "dim-label",
                                    add_css_class: "caption",
                                    set_label: "Your password is stored in the system keyring \
                                                (secret-service), never in plain text on disk.",
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
        let goa = importable_goa_accounts(&init.accounts);

        let senders = relm4::factory::FactoryVecDeque::builder()
            .launch(gtk::ListBox::new())
            .forward(sender.input_sender(), |out| match out {
                SenderRowOutput::Remove(addr) => AccountsInput::RemoveSenderRow(addr),
            });
        let blacklist = relm4::factory::FactoryVecDeque::builder()
            .launch(gtk::ListBox::new())
            .forward(sender.input_sender(), |out| match out {
                SenderRowOutput::Remove(addr) => AccountsInput::RemoveBlacklistRow(addr),
            });

        let mut model = AccountsWindow {
            accounts: init.accounts,
            editing: None,
            emoji: None,
            sig_editor: RichEditor::new(""),
            label_synced: String::new(),
            goa,
            pending_oauth_refresh: None,
            alias_edits: Vec::new(),
            alias_editing: None,
            alias_dialog: None,
            folders_by_email: std::collections::HashMap::new(),
            folder_paths: Vec::new(),
            senders,
            sender_addrs: Vec::new(),
            blacklist,
            blacklist_addrs: Vec::new(),
            filter_rules: init.filters,
            filters_list: None,
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
        model.filters_list = Some(widgets.filters_list.clone());
        model.rebuild_filter_rows(&sender);
        widgets.sig_holder.append(&model.sig_editor.widget);
        model.rebuild_account_list(&widgets.accounts_list, &sender);
        model.rebuild_goa_list(&widgets.goa_list, &sender);
        widgets.goa_group.set_visible(!model.goa.is_empty());
        widgets
            .protocol_row
            .set_model(Some(&gtk::StringList::new(&["IMAP", "POP3"])));

        // The Provider dropdown picks both the sign-in method and (for known
        // providers) the servers. The default popup ellipsizes items; a factory
        // whose labels don't lets the list widen to the full option text.
        let provider_labels: Vec<&str> = PROVIDERS.iter().map(|p| p.label).collect();
        widgets
            .provider_row
            .set_model(Some(&gtk::StringList::new(&provider_labels)));
        widgets.provider_row.set_list_factory(Some(&non_ellipsizing_factory()));

        // Push override choices mirror AccountConfig::push (None / Some(true)
        // / Some(false), in that order).
        widgets
            .push_row
            .set_model(Some(&gtk::StringList::new(&["Follow Settings", "On", "Off"])));
        widgets.push_row.set_list_factory(Some(&non_ellipsizing_factory()));

        // Show the SMTP credential fields only when the toggle is on.
        widgets
            .smtp_separate_row
            .bind_property("active", &widgets.smtp_user_row, "visible")
            .sync_create()
            .build();
        widgets
            .smtp_separate_row
            .bind_property("active", &widgets.smtp_pass_row, "visible")
            .sync_create()
            .build();

        // Auto-fill the label from the email as it's typed (until customized).
        let es = sender.clone();
        widgets.email_row.connect_changed(move |_| es.input(AccountsInput::EmailChanged));

        // Tell the combined settings window when the editor subpage is up —
        // every way in or out (Save, back button, swipe) lands here.
        {
            let s = sender.output_sender().clone();
            widgets.nav.connect_visible_page_notify(move |nav| {
                let editor = nav
                    .visible_page()
                    .and_then(|p| p.tag())
                    .is_some_and(|tag| tag == "editor");
                let _ = s.send(AccountsOutput::EditorOpen(editor));
            });
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
            AccountsInput::AddAccount => {
                self.editing = None;
                self.emoji = None;
                self.label_synced = String::new();
                self.pending_oauth_refresh = None;
                self.close_alias_dialog();
                self.alias_edits.clear();
                self.rebuild_alias_list(&widgets.aliases_list, &sender);
                clear_editor(widgets);
                self.populate_folder_combos(widgets, None);
                set_connection_editable(widgets, true);
                widgets.goa_banner.set_visible(false);
                self.apply_provider(widgets);
                self.sig_editor.set_html("");
                widgets.color_btn.set_rgba(&parse_color(DEFAULT_COLOR));
                widgets.emoji_btn.set_label("Add");
                widgets.remove_group.set_visible(false);
                // A prior GOA edit may have hidden the provider picker.
                widgets.provider_row.set_visible(true);
                widgets.nav.push_by_tag("editor");
            }

            AccountsInput::EditAccountByEmail(email) => {
                let Some(i) = self.accounts.iter().position(|a| a.email == email) else {
                    return;
                };
                let editor_up = widgets
                    .nav
                    .visible_page()
                    .and_then(|p| p.tag())
                    .is_some_and(|tag| tag == "editor");
                if editor_up {
                    if self.editing == Some(i) {
                        return;
                    }
                    // Another account's editor is up: back to the list first,
                    // since a page can't be pushed while it is in the stack.
                    widgets.nav.pop();
                }
                sender.input(AccountsInput::EditAccount(i));
            }

            AccountsInput::EditAccount(i) => {
                let Some(acc) = self.accounts.get(i).cloned() else {
                    return;
                };
                self.editing = Some(i);
                self.pending_oauth_refresh = None;
                self.close_alias_dialog();
                self.alias_edits = acc.aliases.clone();
                self.rebuild_alias_list(&widgets.aliases_list, &sender);
                fill_editor(widgets, &acc);
                self.populate_folder_combos(widgets, Some(&acc));
                self.apply_provider(widgets);
                // Label mirrors the email until customized.
                self.label_synced = acc.email.clone();
                self.sig_editor
                    .set_html(&rich_editor::signature_to_html(acc.signature.as_deref().unwrap_or("")));
                widgets
                    .color_btn
                    .set_rgba(&parse_color(acc.color.as_deref().unwrap_or(DEFAULT_COLOR)));
                self.emoji = acc.emoji.clone();
                widgets.emoji_btn.set_label(self.emoji.as_deref().unwrap_or("Add"));
                // GOA accounts: no "Remove" (it lives in the system) — offer an
                // enable/disable toggle and a shortcut to Online Accounts instead.
                let is_goa = acc.goa_id.is_some();
                set_connection_editable(widgets, !is_goa);
                widgets.goa_banner.set_visible(is_goa);
                // GOA accounts get the same Remove flow — it removes the account
                // from Vireo only (back to the import list); GNOME keeps it.
                widgets.remove_group.set_visible(true);
                // GNOME owns a GOA account's connection outright, so the server
                // and credential section isn't shown at all — only what Vireo
                // owns (name, label, colour, signature, aliases) plus the email
                // for identification. `apply_provider` re-shows what applies the
                // next time a native account or the add-form opens the editor.
                widgets.provider_row.set_visible(!is_goa);
                if is_goa {
                    for w in [
                        widgets.protocol_row.upcast_ref::<gtk::Widget>(),
                        widgets.host_row.upcast_ref(),
                        widgets.port_row.upcast_ref(),
                        widgets.smtp_row.upcast_ref(),
                        widgets.smtp_port_row.upcast_ref(),
                        widgets.user_row.upcast_ref(),
                        widgets.pass_row.upcast_ref(),
                        widgets.smtp_separate_row.upcast_ref(),
                        widgets.smtp_user_row.upcast_ref(),
                        widgets.smtp_pass_row.upcast_ref(),
                        widgets.test_btn.upcast_ref(),
                        widgets.oauth_signin_btn.upcast_ref(),
                        widgets.oauth_status.upcast_ref(),
                        widgets.goa_hint.upcast_ref(),
                    ] {
                        w.set_visible(false);
                    }
                    // The Google/Microsoft "use GNOME" guidance hid these; a GOA
                    // account edits its display fields right here.
                    widgets.name_row.set_visible(true);
                    widgets.email_row.set_visible(true);
                }
                widgets.goa_enabled_row.set_active(acc.enabled);
                // While Mail is switched off in GNOME Settings the account is
                // paused from there, not from here — say so where the toggle is.
                widgets.goa_enabled_row.set_sensitive(!acc.goa_mail_disabled);
                widgets.goa_enabled_row.set_subtitle(if acc.goa_mail_disabled {
                    "Paused: Mail is switched off for this account in GNOME Settings \u{2192} Online Accounts."
                } else {
                    "Switching this off returns the account to the import list — it \
                     stays in GNOME Online Accounts."
                });
                widgets.nav.push_by_tag("editor");
            }

            AccountsInput::EmailChanged => {
                let email = trimmed(&widgets.email_row);
                let label = widgets.label_row.text().to_string();
                // Mirror while the label is still tracking the email (or empty);
                // once the user types a custom label, stop.
                if label.is_empty() || label == self.label_synced {
                    widgets.label_row.set_text(&email);
                }
                self.label_synced = email;
            }

            AccountsInput::MoveRow { from, to } => {
                if from < self.accounts.len() {
                    let acc = self.accounts.remove(from);
                    let to = to.min(self.accounts.len());
                    self.accounts.insert(to, acc);
                    self.rebuild_account_list(&widgets.accounts_list, &sender);
                    let emails = self.accounts.iter().map(|a| a.email.clone()).collect();
                    let _ = sender.output(AccountsOutput::Reordered(emails));
                }
            }

            AccountsInput::ToggleEnabled { index, enabled } => {
                // A GNOME Online Account switched off here isn't paused — it is
                // un-imported: it drops out of Vireo (the config entry and its
                // stored copies go) and returns to the "GNOME Online Accounts"
                // list below, ready to import again. The account itself stays
                // in GNOME untouched.
                if !enabled && self.accounts.get(index).is_some_and(|a| a.goa_id.is_some()) {
                    self.unimport_goa(index, widgets, &sender);
                    return;
                }
                if let Some(acc) = self.accounts.get_mut(index) {
                    if acc.enabled != enabled {
                        acc.enabled = enabled;
                        let email = acc.email.clone();
                        let _ = sender.output(AccountsOutput::EnabledChanged { email, enabled });
                    }
                }
            }

            AccountsInput::ToggleCurrentEnabled(enabled) => {
                if let Some(i) = self.editing {
                    // Same un-import semantics as the list toggle; the editor
                    // page closes since its account is no longer in Vireo.
                    if !enabled && self.accounts.get(i).is_some_and(|a| a.goa_id.is_some()) {
                        self.unimport_goa(i, widgets, &sender);
                        widgets.nav.pop();
                        return;
                    }
                    if let Some(acc) = self.accounts.get_mut(i) {
                        if acc.enabled != enabled {
                            acc.enabled = enabled;
                            let email = acc.email.clone();
                            let _ = sender.output(AccountsOutput::EnabledChanged { email, enabled });
                            self.rebuild_account_list(&widgets.accounts_list, &sender);
                        }
                    }
                }
            }

            AccountsInput::ImportGoa(index) => {
                if let Some(g) = self.goa.get(index).cloned() {
                    // Password-based providers: pull the password now. OAuth
                    // providers: authenticate with a GOA token at connect time.
                    // Either way the worker asks GOA again when it connects, so an
                    // account still works if this read comes back empty.
                    let (password, oauth) = if g.password_based {
                        (crate::goa::mail_passwords(&g.id).0.unwrap_or_default(), false)
                    } else {
                        (String::new(), true)
                    };
                    let account = g.to_config(password, oauth);
                    self.goa.remove(index);
                    self.accounts.push(account.clone());
                    self.rebuild_account_list(&widgets.accounts_list, &sender);
                    self.rebuild_goa_list(&widgets.goa_list, &sender);
                    widgets.goa_group.set_visible(!self.goa.is_empty());
                    let _ = sender.output(AccountsOutput::ImportGoa(Box::new(account)));
                }
            }

            AccountsInput::SetEmoji(text) => {
                widgets.emoji_btn.set_label(&text);
                self.emoji = Some(text);
            }

            AccountsInput::ClearEmoji => {
                self.emoji = None;
                widgets.emoji_btn.set_label("Add");
            }

            AccountsInput::TestConnection => {
                let account = read_account(widgets, self.emoji.clone());
                widgets.test_btn.set_sensitive(false);
                widgets.test_result.set_visible(true);
                widgets.test_result.set_css_classes(&["dim-label"]);
                widgets.test_result.set_label("Testing…");
                sender.oneshot_command(async move {
                    let r = tokio::task::spawn_blocking(move || {
                        worker::test_connection_blocking(account)
                    })
                    .await
                    .unwrap_or_else(|_| ConnTest {
                        incoming: Err("test could not run".into()),
                        smtp: Err("test could not run".into()),
                    });
                    AccountsCmd::Test(r)
                });
            }

            AccountsInput::ProviderChanged => {
                // Editing a GOA account: the provider dropdown is hidden and the
                // connection section deliberately not shown — but filling the
                // editor sets the dropdown, whose notify lands here right after
                // EditAccount and would undo that. GNOME owns those fields;
                // leave them hidden.
                let editing_goa = self
                    .editing
                    .and_then(|i| self.accounts.get(i))
                    .is_some_and(|a| a.goa_id.is_some());
                if !editing_goa {
                    self.apply_provider(widgets);
                }
            }

            AccountsInput::OAuthSignIn => {
                let settings = self.oauth_settings_from_form(widgets);
                if settings.client_id.trim().is_empty()
                    || settings.auth_url.is_empty()
                    || settings.token_url.is_empty()
                {
                    widgets.oauth_status.set_visible(true);
                    widgets.oauth_status.set_css_classes(&["error"]);
                    widgets
                        .oauth_status
                        .set_label("Enter a client ID (and endpoints for a custom provider) first");
                    return;
                }
                widgets.oauth_signin_btn.set_sensitive(false);
                widgets.oauth_status.set_visible(true);
                widgets.oauth_status.set_css_classes(&["dim-label"]);
                widgets
                    .oauth_status
                    .set_label("Opening browser… complete sign-in there.");
                sender.oneshot_command(async move {
                    let r = tokio::task::spawn_blocking(move || {
                        crate::oauth::run_flow(&settings).map(|f| f.refresh_token)
                    })
                    .await
                    .unwrap_or_else(|_| Err("sign-in task failed".into()));
                    AccountsCmd::OAuth(r)
                });
            }

            AccountsInput::OpenOnlineAccounts => open_online_accounts(),

            AccountsInput::Save => {
                // Pull the signature HTML out of the editor first (async), then
                // finish saving in SaveWithSig.
                let s = sender.clone();
                self.sig_editor
                    .extract_html(move |html| s.input(AccountsInput::SaveWithSig(html)));
            }

            AccountsInput::SetFolderChoices(map) => {
                self.folders_by_email = map;
            }
            AccountsInput::SaveWithSig(sig_html) => {
                widgets.host_row.remove_css_class("error");
                let mut account = read_account(widgets, self.emoji.clone());
                account.aliases = self.alias_edits.clone();
                account.folder_roles = self.read_folder_roles(widgets);
                let sig = sig_html.trim();
                account.signature = if signature_is_empty(sig) {
                    None
                } else {
                    Some(sig_html.clone())
                };
                account.signature_html = true;

                // Editing preserves the enabled state; GOA accounts keep their
                // (GOA-driven) OAuth mechanism regardless of the Authentication combo.
                let editing_orig = self.editing.and_then(|i| self.accounts.get(i)).cloned();
                if let Some(orig) = &editing_orig {
                    account.enabled = orig.enabled;
                    account.goa_id = orig.goa_id.clone();
                    account.goa_mail_disabled = orig.goa_mail_disabled;
                    account.goa_enabled_before_mail_disabled =
                        orig.goa_enabled_before_mail_disabled;
                    if orig.goa_id.is_some() {
                        account.oauth = orig.oauth;
                        account.oauth_settings = orig.oauth_settings.clone();
                        // GNOME Online Accounts is the source of truth for these;
                        // Vireo keeps only what it owns (display name, signature,
                        // colour, emoji, label).
                        account.email = orig.email.clone();
                        account.protocol = orig.protocol;
                        account.imap_host = orig.imap_host.clone();
                        account.imap_port = orig.imap_port;
                        account.smtp_host = orig.smtp_host.clone();
                        account.smtp_port = orig.smtp_port;
                        account.username = orig.username.clone();
                        account.password = orig.password.clone();
                        account.smtp_separate = orig.smtp_separate;
                        account.smtp_username = orig.smtp_username.clone();
                        account.smtp_password = orig.smtp_password.clone();
                    }
                }

                // Native account: authentication comes from the provider dropdown.
                let is_oauth = provider_at(widgets.provider_row.selected()).is_oauth();
                if account.goa_id.is_none() {
                    if is_oauth {
                        account.oauth = true;
                        account.oauth_settings = Some(self.oauth_settings_from_form(widgets));
                        if account.username.trim().is_empty() {
                            account.username = account.email.clone();
                        }
                        // A fresh sign-in supplies a refresh token; otherwise keep
                        // the one already in the keyring (edit without re-signing).
                        if let Some(rt) = self.pending_oauth_refresh.clone() {
                            account.oauth_refresh = rt;
                        }
                    } else {
                        account.oauth = false;
                        account.oauth_settings = None;
                    }
                }

                // Validation.
                let oauth_ready = if account.oauth && account.goa_id.is_none() {
                    let has_client = account
                        .oauth_settings
                        .as_ref()
                        .is_some_and(|s| !s.client_id.trim().is_empty());
                    let signed_in = self.pending_oauth_refresh.is_some()
                        || editing_orig.as_ref().is_some_and(|o| o.oauth);
                    if !has_client || !signed_in {
                        widgets.oauth_status.set_visible(true);
                        widgets.oauth_status.set_css_classes(&["error"]);
                        widgets
                            .oauth_status
                            .set_label("Enter a client ID and sign in before saving");
                    }
                    has_client && signed_in
                } else {
                    true
                };
                let password_ok = account.oauth || !account.password.is_empty();
                // A GOA account's connection fields were all restored from the
                // original above (GNOME owns them; a Graph account rightly has
                // no IMAP host at all) — validating them would only block the
                // fields Vireo does own: label, signature, colour, aliases.
                let is_goa_edit = account.goa_id.is_some();
                if !is_goa_edit
                    && (account.imap_host.is_empty()
                        || account.username.is_empty()
                        || !password_ok
                        || !oauth_ready
                        || (account.smtp_separate
                            && (account.smtp_username.is_empty()
                                || account.smtp_password.is_empty())))
                {
                    widgets.host_row.add_css_class("error");
                    return;
                }
                self.pending_oauth_refresh = None;

                let original_email = self
                    .editing
                    .and_then(|i| self.accounts.get(i))
                    .map(|a| a.email.clone());
                match self.editing {
                    Some(i) if i < self.accounts.len() => self.accounts[i] = account.clone(),
                    _ => self.accounts.push(account.clone()),
                }
                self.rebuild_account_list(&widgets.accounts_list, &sender);
                let _ = sender.output(AccountsOutput::Saved {
                    original_email,
                    account: Box::new(account),
                });
                widgets.nav.pop();
            }

            AccountsInput::RemoveCurrent => {
                // Confirm before this destructive, keyring-clearing action.
                let Some(i) = self.editing else { return };
                let Some(account) = self.accounts.get(i) else { return };
                let name = if account.name.trim().is_empty() {
                    account.email.clone()
                } else {
                    account.name.clone()
                };
                let body = if account.goa_id.is_some() {
                    format!(
                        "Remove {name} from Vireo? It stays in GNOME Online Accounts \
                         and can be imported again from the list below. Mail on the \
                         server is not affected."
                    )
                } else {
                    format!(
                        "Remove {name} from Vireo? Its saved password is deleted from \
                         the keyring. Mail on the server is not affected."
                    )
                };
                // The panel is embedded — dialogs parent to whatever window
                // it currently sits in (the combined settings window).
                let host = root.root().and_downcast::<gtk::Window>();
                let dialog =
                    adw::MessageDialog::new(host.as_ref(), Some("Remove Account?"), Some(&body));
                dialog.add_response("cancel", "Cancel");
                dialog.add_response("remove", "Remove");
                dialog.set_response_appearance("remove", adw::ResponseAppearance::Destructive);
                dialog.set_default_response(Some("cancel"));
                dialog.set_close_response("cancel");
                let s = sender.clone();
                dialog.connect_response(None, move |_, resp| {
                    if resp == "remove" {
                        s.input(AccountsInput::ConfirmRemove);
                    }
                });
                dialog.present();
            }
            AccountsInput::ConfirmRemove => {
                if let Some(i) = self.editing {
                    if i < self.accounts.len() {
                        let email = self.accounts[i].email.clone();
                        self.accounts.remove(i);
                        self.rebuild_account_list(&widgets.accounts_list, &sender);
                        let _ = sender.output(AccountsOutput::Removed { email });
                        // A removed GOA import returns to the list below (and a
                        // repaired-away broken entry re-offers its GOA account).
                        self.goa = importable_goa_accounts(&self.accounts);
                        self.rebuild_goa_list(&widgets.goa_list, &sender);
                        widgets.goa_group.set_visible(!self.goa.is_empty());
                    }
                }
                widgets.nav.pop();
            }

            AccountsInput::AliasAdd => {
                self.alias_editing = None;
                self.open_alias_dialog(root, &AliasConfig::default(), &sender);
            }

            AccountsInput::AliasEdit(i) => {
                let Some(alias) = self.alias_edits.get(i).cloned() else {
                    return;
                };
                self.alias_editing = Some(i);
                self.open_alias_dialog(root, &alias, &sender);
            }

            AccountsInput::AliasRemove(i) => {
                if i < self.alias_edits.len() {
                    // The keyring entry (if the alias had its own SMTP) is
                    // dropped on Save, when the removal actually takes effect.
                    self.alias_edits.remove(i);
                    self.rebuild_alias_list(&widgets.aliases_list, &sender);
                }
            }

            AccountsInput::AliasDialogSave => {
                let Some(d) = self.alias_dialog.as_ref() else { return };
                for row in [&d.addr_row, &d.host_row, &d.user_row] {
                    row.remove_css_class("error");
                }
                d.pass_row.remove_css_class("error");

                let name = trimmed(&d.name_row);
                let addr = trimmed(&d.addr_row);
                let own_smtp = d.smtp_switch.is_active();
                let host = trimmed(&d.host_row);
                let port: u16 = trimmed(&d.port_row).parse().unwrap_or(587);
                let user = trimmed(&d.user_row);
                let pass = d.pass_row.text().to_string();

                // The address must look like one, and must not shadow the
                // account's own address or another alias — the send path picks
                // an alias's transport by matching the From address, so a
                // duplicate would make "which server sends this?" ambiguous.
                let account_email = trimmed(&widgets.email_row);
                let duplicate = addr.eq_ignore_ascii_case(&account_email)
                    || self.alias_edits.iter().enumerate().any(|(i, a)| {
                        Some(i) != self.alias_editing
                            && a.address().eq_ignore_ascii_case(&addr)
                    });
                let mut bad = false;
                if addr.is_empty() || !addr.contains('@') || duplicate {
                    d.addr_row.add_css_class("error");
                    bad = true;
                }
                if own_smtp {
                    if host.is_empty() {
                        d.host_row.add_css_class("error");
                        bad = true;
                    }
                    if user.is_empty() {
                        d.user_row.add_css_class("error");
                        bad = true;
                    }
                    if pass.is_empty() {
                        d.pass_row.add_css_class("error");
                        bad = true;
                    }
                }
                if bad {
                    return;
                }

                let alias = AliasConfig {
                    identity: if name.is_empty() {
                        addr.clone()
                    } else {
                        format!("{name} <{addr}>")
                    },
                    smtp_host: if own_smtp { host } else { String::new() },
                    smtp_port: port,
                    smtp_username: if own_smtp { user } else { String::new() },
                    smtp_password: if own_smtp { pass } else { String::new() },
                };
                match self.alias_editing {
                    Some(i) if i < self.alias_edits.len() => self.alias_edits[i] = alias,
                    _ => self.alias_edits.push(alias),
                }
                self.alias_editing = None;
                self.close_alias_dialog();
                self.rebuild_alias_list(&widgets.aliases_list, &sender);
            }

            AccountsInput::AliasDialogTest => {
                let Some(d) = self.alias_dialog.as_ref() else { return };
                let alias = AliasConfig {
                    identity: trimmed(&d.addr_row),
                    smtp_host: trimmed(&d.host_row),
                    smtp_port: trimmed(&d.port_row).parse().unwrap_or(587),
                    smtp_username: trimmed(&d.user_row),
                    smtp_password: d.pass_row.text().to_string(),
                };
                let email = trimmed(&widgets.email_row);
                d.test_btn.set_sensitive(false);
                d.test_result.set_visible(true);
                d.test_result.set_css_classes(&["dim-label"]);
                d.test_result.set_label("Testing…");
                sender.oneshot_command(async move {
                    let r = tokio::task::spawn_blocking(move || {
                        worker::test_alias_smtp_blocking(email, alias)
                    })
                    .await
                    .unwrap_or_else(|_| Err("test could not run".into()));
                    AccountsCmd::AliasTested(r)
                });
            }

            AccountsInput::AliasDialogClosed => {
                // The notification is queued behind the close, so by the time it
                // arrives a replacement dialog may already be open — only clear
                // state when the dialog we track is really the one that closed.
                if self.alias_dialog.as_ref().is_none_or(|d| !d.window.is_visible()) {
                    self.alias_dialog = None;
                    self.alias_editing = None;
                }
            }

            AccountsInput::AddSenderText(text) => {
                let addr = text.trim().to_lowercase();
                if !addr.is_empty() && !self.sender_addrs.contains(&addr) {
                    self.sender_addrs.push(addr.clone());
                    self.senders.guard().push_back(addr.clone());
                    let _ = sender.output(AccountsOutput::AddSender(addr));
                }
            }
            AccountsInput::RemoveSenderRow(addr) => {
                if let Some(pos) = self.sender_addrs.iter().position(|a| *a == addr) {
                    self.sender_addrs.remove(pos);
                    self.senders.guard().remove(pos);
                    let _ = sender.output(AccountsOutput::RemoveSender(addr));
                }
            }
            AccountsInput::AddBlacklistText(text) => {
                let addr = text.trim().to_lowercase();
                if !addr.is_empty() && !self.blacklist_addrs.contains(&addr) {
                    self.blacklist_addrs.push(addr.clone());
                    self.blacklist.guard().push_back(addr.clone());
                    let _ = sender.output(AccountsOutput::AddBlacklist(addr));
                }
            }
            AccountsInput::RemoveBlacklistRow(addr) => {
                if let Some(pos) = self.blacklist_addrs.iter().position(|a| *a == addr) {
                    self.blacklist_addrs.remove(pos);
                    self.blacklist.guard().remove(pos);
                    let _ = sender.output(AccountsOutput::RemoveBlacklist(addr));
                }
            }
            AccountsInput::AddFilter => {
                self.open_filter_dialog(&sender);
            }
            AccountsInput::RemoveFilter(i) => {
                if i < self.filter_rules.len() {
                    self.filter_rules.remove(i);
                    self.rebuild_filter_rows(&sender);
                    let _ =
                        sender.output(AccountsOutput::SetFilters(self.filter_rules.clone()));
                }
            }
            AccountsInput::FilterAdded(rule) => {
                self.filter_rules.push(rule);
                self.rebuild_filter_rows(&sender);
                let _ = sender.output(AccountsOutput::SetFilters(self.filter_rules.clone()));
            }
        }
    }

    fn update_cmd_with_view(
        &mut self,
        widgets: &mut Self::Widgets,
        result: AccountsCmd,
        _sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match result {
            AccountsCmd::Test(result) => {
                let line = |label: &str, r: &Result<(), String>| match r {
                    Ok(()) => format!("✓ {label}: connected"),
                    Err(e) => format!("✗ {label}: {e}"),
                };
                let incoming_label =
                    if widgets.protocol_row.selected() == 1 { "POP3" } else { "IMAP" };
                let text = format!(
                    "{}\n{}",
                    line(incoming_label, &result.incoming),
                    line("SMTP", &result.smtp),
                );
                let class = if result.incoming.is_ok() && result.smtp.is_ok() {
                    "success"
                } else {
                    "error"
                };
                widgets.test_result.set_label(&text);
                widgets.test_result.set_css_classes(&[class]);
                widgets.test_btn.set_sensitive(true);
            }
            AccountsCmd::OAuth(result) => {
                widgets.oauth_signin_btn.set_sensitive(true);
                widgets.oauth_status.set_visible(true);
                match result {
                    Ok(refresh) => {
                        self.pending_oauth_refresh = Some(refresh);
                        widgets.oauth_status.set_css_classes(&["success"]);
                        widgets.oauth_status.set_label("✓ Signed in — save the account to finish");
                    }
                    Err(e) => {
                        widgets.oauth_status.set_css_classes(&["error"]);
                        widgets.oauth_status.set_label(&format!("Sign-in failed: {e}"));
                    }
                }
            }
            AccountsCmd::AliasTested(result) => {
                let Some(d) = self.alias_dialog.as_ref() else { return };
                d.test_btn.set_sensitive(true);
                d.test_result.set_visible(true);
                match result {
                    Ok(()) => {
                        d.test_result.set_css_classes(&["success"]);
                        d.test_result.set_label("✓ SMTP: connected");
                    }
                    Err(e) => {
                        d.test_result.set_css_classes(&["error"]);
                        d.test_result.set_label(&format!("✗ SMTP: {e}"));
                    }
                }
            }
        }
    }
}

impl AccountsWindow {
    /// Rebuild the editor's send-as alias list from `alias_edits` (#34).
    fn rebuild_alias_list(&self, list: &gtk::ListBox, sender: &ComponentSender<Self>) {
        while let Some(child) = list.first_child() {
            list.remove(&child);
        }
        // An empty boxed-list draws as a bare frame; hide it until there is a row.
        list.set_visible(!self.alias_edits.is_empty());

        for (i, alias) in self.alias_edits.iter().enumerate() {
            let row = adw::ActionRow::new();
            row.set_activatable(true);
            row.set_title(&gtk::glib::markup_escape_text(&alias.identity));
            row.set_subtitle(&gtk::glib::markup_escape_text(&if alias.has_own_smtp() {
                format!("Sends through {}", alias.smtp_host)
            } else {
                "Sends through this account".to_string()
            }));

            // A dim pencil says "activate to edit"; the trash button removes.
            let edit = gtk::Image::from_icon_name("co.hyprlab.Vireo-document-edit-symbolic");
            edit.add_css_class("dim-label");
            row.add_suffix(&edit);

            let remove = gtk::Button::from_icon_name("co.hyprlab.Vireo-user-trash-symbolic");
            remove.set_valign(gtk::Align::Center);
            remove.add_css_class("flat");
            remove.set_tooltip_text(Some("Remove this alias"));
            let ri = sender.input_sender().clone();
            remove.connect_clicked(move |_| {
                let _ = ri.send(AccountsInput::AliasRemove(i));
            });
            row.add_suffix(&remove);

            list.append(&row);
        }
    }

    /// Close the alias dialog, if one is open.
    fn close_alias_dialog(&mut self) {
        if let Some(d) = self.alias_dialog.take() {
            d.window.close();
        }
    }

    /// Open the modal alias editor (#34), prefilled from `alias`.
    fn open_alias_dialog(
        &mut self,
        root: &adw::Bin,
        alias: &AliasConfig,
        sender: &ComponentSender<Self>,
    ) {
        self.close_alias_dialog();

        let window = adw::Window::builder()
            .modal(true)
            .default_width(440)
            .title(if self.alias_editing.is_some() { "Edit Alias" } else { "Add Alias" })
            .build();
        // Parent to whatever window the embedded panel currently sits in.
        window.set_transient_for(root.root().and_downcast::<gtk::Window>().as_ref());

        let cancel = gtk::Button::with_label("Cancel");
        let save = gtk::Button::with_label("Save");
        save.add_css_class("suggested-action");
        let header = adw::HeaderBar::builder()
            .show_start_title_buttons(false)
            .show_end_title_buttons(false)
            .build();
        header.pack_start(&cancel);
        header.pack_end(&save);

        let (name, addr) = split_identity(&alias.identity);
        let identity_group = adw::PreferencesGroup::new();
        identity_group.set_description(Some(
            "The composer's From menu offers this address, and replies to \
             mail sent to it answer from it.",
        ));
        let name_row = adw::EntryRow::builder().title("Display name (optional)").build();
        name_row.set_text(&name);
        let addr_row = adw::EntryRow::builder().title("Email address").build();
        addr_row.set_text(&addr);
        identity_group.add(&name_row);
        identity_group.add(&addr_row);

        let smtp_group = adw::PreferencesGroup::new();
        smtp_group.set_title("Sending");
        let smtp_switch = adw::SwitchRow::builder()
            .title("Own SMTP server")
            .subtitle(
                "Send through the alias's own mail provider, with its own \
                 sign-in — instead of this account's server. Needed when the \
                 account's provider (e.g. Gmail) rewrites the sender address.",
            )
            .build();
        smtp_switch.set_active(alias.has_own_smtp());
        let host_row = adw::EntryRow::builder().title("SMTP server").build();
        host_row.set_text(&alias.smtp_host);
        let port_row = adw::EntryRow::builder().title("SMTP port").build();
        port_row.set_text(&alias.smtp_port.to_string());
        let user_row = adw::EntryRow::builder().title("SMTP username").build();
        user_row.set_text(&alias.smtp_username);
        let pass_row = adw::PasswordEntryRow::builder().title("SMTP password").build();
        pass_row.set_text(&alias.smtp_password);
        smtp_group.add(&smtp_switch);
        smtp_group.add(&host_row);
        smtp_group.add(&port_row);
        smtp_group.add(&user_row);
        smtp_group.add(&pass_row);

        let test_btn = gtk::Button::with_label("Test SMTP");
        test_btn.set_halign(gtk::Align::Start);
        let test_result = gtk::Label::new(None);
        test_result.set_visible(false);
        test_result.set_halign(gtk::Align::Start);
        test_result.set_xalign(0.0);
        test_result.set_wrap(true);
        let test_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
        test_box.append(&test_btn);
        test_box.append(&test_result);

        // The SMTP fields (and their test) only apply with the switch on.
        for target in [
            host_row.upcast_ref::<gtk::Widget>(),
            port_row.upcast_ref(),
            user_row.upcast_ref(),
            pass_row.upcast_ref(),
            test_box.upcast_ref(),
        ] {
            smtp_switch
                .bind_property("active", target, "visible")
                .sync_create()
                .build();
        }

        let content = gtk::Box::new(gtk::Orientation::Vertical, 24);
        content.set_margin_top(24);
        content.set_margin_bottom(24);
        content.set_margin_start(24);
        content.set_margin_end(24);
        content.append(&identity_group);
        content.append(&smtp_group);
        content.append(&test_box);

        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .max_content_height(640)
            .child(&content)
            .build();
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.set_content(Some(&scroller));
        window.set_content(Some(&toolbar));

        let w = window.clone();
        cancel.connect_clicked(move |_| w.close());
        let s = sender.input_sender().clone();
        save.connect_clicked(move |_| {
            let _ = s.send(AccountsInput::AliasDialogSave);
        });
        let s = sender.input_sender().clone();
        test_btn.connect_clicked(move |_| {
            let _ = s.send(AccountsInput::AliasDialogTest);
        });
        let s = sender.input_sender().clone();
        window.connect_close_request(move |_| {
            let _ = s.send(AccountsInput::AliasDialogClosed);
            gtk::glib::Propagation::Proceed
        });

        window.present();
        self.alias_dialog = Some(AliasDialog {
            window,
            name_row,
            addr_row,
            smtp_switch,
            host_row,
            port_row,
            user_row,
            pass_row,
            test_btn,
            test_result,
        });
    }

    /// Rebuild the draggable account list.
    /// Fill the editor's Special Folders combos (#82) for `acc` (None = a new
    /// account, whose folders aren't known yet): "Automatic" plus the account's
    /// live folder list, with any saved assignment selected.
    fn populate_folder_combos(&mut self, widgets: &AccountsWindowWidgets, acc: Option<&AccountConfig>) {
        let choices = acc
            .and_then(|a| self.folders_by_email.get(&a.email))
            .cloned()
            .unwrap_or_default();
        let mut labels: Vec<&str> = vec!["Automatic"];
        labels.extend(choices.iter().map(|(_, display)| display.as_str()));
        self.folder_paths = choices.iter().map(|(path, _)| path.clone()).collect();
        for (role, row) in [
            ("sent", &widgets.folder_sent_row),
            ("drafts", &widgets.folder_drafts_row),
            ("trash", &widgets.folder_trash_row),
            ("junk", &widgets.folder_junk_row),
            ("archive", &widgets.folder_archive_row),
        ] {
            row.set_model(Some(&gtk::StringList::new(&labels)));
            row.set_list_factory(Some(&non_ellipsizing_factory()));
            let selected = acc
                .and_then(|a| a.folder_roles.get(role))
                .and_then(|path| self.folder_paths.iter().position(|p| p == path))
                .map(|i| i as u32 + 1)
                .unwrap_or(0);
            row.set_selected(selected);
        }
    }

    /// The Special Folders combos' current assignments: role → folder path,
    /// omitting everything left on Automatic.
    fn read_folder_roles(
        &self,
        widgets: &AccountsWindowWidgets,
    ) -> std::collections::BTreeMap<String, String> {
        let mut roles = std::collections::BTreeMap::new();
        for (role, row) in [
            ("sent", &widgets.folder_sent_row),
            ("drafts", &widgets.folder_drafts_row),
            ("trash", &widgets.folder_trash_row),
            ("junk", &widgets.folder_junk_row),
            ("archive", &widgets.folder_archive_row),
        ] {
            let sel = row.selected();
            if sel > 0 {
                if let Some(path) = self.folder_paths.get(sel as usize - 1) {
                    roles.insert(role.to_string(), path.clone());
                }
            }
        }
        roles
    }

    fn rebuild_account_list(&self, list: &gtk::ListBox, sender: &ComponentSender<Self>) {
        while let Some(child) = list.first_child() {
            list.remove(&child);
        }

        for (pos, acc) in self.accounts.iter().enumerate() {
            let row = gtk::ListBoxRow::new();
            row.set_activatable(true);

            let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            hbox.add_css_class("account-list-row");

            let handle = gtk::Image::from_icon_name("co.hyprlab.Vireo-list-drag-handle-symbolic");
            handle.add_css_class("dim-label");
            hbox.append(&handle);

            let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
            vbox.set_hexpand(true);
            vbox.set_valign(gtk::Align::Center);
            let name = gtk::Label::new(Some(&display_name(acc)));
            name.set_halign(gtk::Align::Start);
            name.set_ellipsize(gtk::pango::EllipsizeMode::End);
            name.add_css_class("account-name");
            let email = gtk::Label::new(Some(&acc.email));
            email.set_halign(gtk::Align::Start);
            email.set_ellipsize(gtk::pango::EllipsizeMode::End);
            email.add_css_class("account-email");
            vbox.append(&name);
            vbox.append(&email);
            hbox.append(&vbox);

            // Source badge: is this account from GNOME Online Accounts, or added
            // directly in Vireo?
            let from_goa = acc.goa_id.is_some();
            let badge =
                gtk::Label::new(Some(if from_goa { "GNOME Online Account" } else { "Vireo" }));
            badge.set_valign(gtk::Align::Center);
            badge.add_css_class("account-source-badge");
            if from_goa {
                badge.add_css_class("goa");
                badge.set_tooltip_text(Some("Imported from GNOME Online Accounts"));
            } else {
                badge.set_tooltip_text(Some("Added directly in Vireo"));
            }
            hbox.append(&badge);

            // Enable/disable toggle. Disabled accounts stay configured but don't
            // sync or appear in the sidebar.
            let toggle = gtk::Switch::new();
            toggle.set_valign(gtk::Align::Center);
            if acc.goa_mail_disabled {
                toggle.set_tooltip_text(Some(
                    "Paused: Mail is switched off for this account in GNOME Settings",
                ));
                toggle.set_sensitive(false);
            } else {
                toggle.set_tooltip_text(Some("Enable this account"));
            }
            toggle.set_active(acc.enabled);
            let ti = sender.input_sender().clone();
            let tpos = pos;
            toggle.connect_state_set(move |_, state| {
                let _ = ti.send(AccountsInput::ToggleEnabled { index: tpos, enabled: state });
                gtk::glib::Propagation::Proceed
            });
            hbox.append(&toggle);

            let next = gtk::Image::from_icon_name("co.hyprlab.Vireo-go-next-symbolic");
            next.add_css_class("dim-label");
            hbox.append(&next);

            row.set_child(Some(&hbox));

            // Drag to reorder.
            let drag = gtk::DragSource::new();
            drag.set_actions(gtk::gdk::DragAction::MOVE);
            let from = pos as u32;
            drag.connect_prepare(move |_, _, _| {
                Some(gtk::gdk::ContentProvider::for_value(&from.to_value()))
            });
            row.add_controller(drag);

            let drop = gtk::DropTarget::new(gtk::glib::Type::U32, gtk::gdk::DragAction::MOVE);
            let to = pos;
            let input = sender.input_sender().clone();
            drop.connect_drop(move |_, value, _, _| {
                if let Ok(from) = value.get::<u32>() {
                    let _ = input.send(AccountsInput::MoveRow {
                        from: from as usize,
                        to,
                    });
                    true
                } else {
                    false
                }
            });
            row.add_controller(drop);

            list.append(&row);
        }
    }

    /// Un-import a GNOME Online Account: drop it from Vireo (the app removes
    /// the config entry and its stored copies) and return it to the "GNOME
    /// Online Accounts" import list below. The account stays in GNOME.
    fn unimport_goa(
        &mut self,
        index: usize,
        widgets: &AccountsWindowWidgets,
        sender: &ComponentSender<Self>,
    ) {
        if index >= self.accounts.len() {
            return;
        }
        let email = self.accounts[index].email.clone();
        self.accounts.remove(index);
        let _ = sender.output(AccountsOutput::Removed { email });
        // Re-query GOA so the account's row reappears in the import list fresh.
        self.goa = importable_goa_accounts(&self.accounts);
        self.rebuild_account_list(&widgets.accounts_list, sender);
        self.rebuild_goa_list(&widgets.goa_list, sender);
        widgets.goa_group.set_visible(!self.goa.is_empty());
    }

    /// Populate the "GNOME Online Accounts" list with importable mail accounts.
    fn rebuild_goa_list(&self, list: &gtk::ListBox, sender: &ComponentSender<Self>) {
        while let Some(child) = list.first_child() {
            list.remove(&child);
        }
        for (pos, g) in self.goa.iter().enumerate() {
            let row = adw::ActionRow::new();
            row.set_title(&g.email);
            let mut subtitle = if g.provider.is_empty() {
                "Mail".to_string()
            } else {
                g.provider.clone()
            };
            // OAuth providers (Gmail, Microsoft 365) sign in with a token from
            // GNOME.
            if g.oauth2 && !g.password_based {
                subtitle.push_str(" · sign-in via GNOME");
            }
            row.set_subtitle(&subtitle);

            let toggle = gtk::Switch::new();
            toggle.set_valign(gtk::Align::Center);
            toggle.set_active(false);
            toggle.set_tooltip_text(Some("Use this account in Vireo"));
            let ti = sender.input_sender().clone();
            let tpos = pos;
            toggle.connect_state_set(move |_, state| {
                if state {
                    let _ = ti.send(AccountsInput::ImportGoa(tpos));
                }
                gtk::glib::Propagation::Proceed
            });
            row.add_suffix(&toggle);
            list.append(&row);
        }
    }

    /// Show/hide credential rows based on the Authentication combo, and pre-fill
    /// server settings for known OAuth providers.
    /// Adapt the editor to the selected provider: show server + credential fields
    /// for password providers, the OAuth sign-in for OAuth providers, and fill in
    /// the servers for known providers.
    fn apply_provider(&self, widgets: &AccountsWindowWidgets) {
        let p = provider_at(widgets.provider_row.selected());
        let is_password = p.is_password();
        let is_oauth = p.is_oauth();
        let is_custom = matches!(p.kind, ProviderKind::CustomOAuth);
        // Google or Microsoft with no built-in (or user-supplied) OAuth client:
        // there's nothing to sign in with, so guide the user to GNOME Online
        // Accounts instead. Microsoft always lands here since its embedded
        // client was removed (issue #36) — mail runs over GOA + Graph.
        let needs_goa = p
            .oauth_name()
            .filter(|_| matches!(p.kind, ProviderKind::Google | ProviderKind::Microsoft))
            .is_some_and(|n| crate::oauth::provider_credentials(n).0.trim().is_empty());
        // Google/Microsoft servers come from the built-in preset (hidden). Custom
        // OAuth still needs its server addresses and client details entered.
        let show_servers = is_password || is_custom;

        widgets.provider_row.set_subtitle(p.hint);

        // Server/credential fields (password or Custom-OAuth manual servers).
        widgets.protocol_row.set_visible(is_password);
        widgets.host_row.set_visible(show_servers);
        widgets.port_row.set_visible(show_servers);
        widgets.smtp_row.set_visible(show_servers);
        widgets.smtp_port_row.set_visible(show_servers);
        widgets.user_row.set_visible(is_password);
        widgets.pass_row.set_visible(is_password);
        widgets.smtp_separate_row.set_visible(is_password);
        widgets.test_btn.set_visible(is_password);
        if is_oauth {
            widgets.smtp_separate_row.set_active(false);
        }

        // OAuth: the user just signs in. Google with no client falls back to the
        // GNOME Online Accounts panel, which replaces the sign-in + identity fields.
        widgets.name_row.set_visible(!needs_goa);
        widgets.email_row.set_visible(!needs_goa);
        widgets.goa_hint.set_visible(needs_goa);
        widgets.oauth_signin_btn.set_visible(is_oauth && !needs_goa);
        widgets.oauth_client_id_row.set_visible(is_custom);
        widgets.oauth_secret_row.set_visible(is_custom);
        widgets.oauth_auth_url_row.set_visible(is_custom);
        widgets.oauth_token_url_row.set_visible(is_custom);
        widgets.oauth_scope_row.set_visible(is_custom);
        if !is_oauth || needs_goa {
            widgets.oauth_status.set_visible(false);
        }

        // Auto-fill IMAP/SMTP: known password providers from the preset table,
        // Google/Microsoft from the OAuth preset (filled but hidden, so the saved
        // account still carries the right servers). Manual/Custom are left alone.
        let servers = match p.kind {
            ProviderKind::Preset => Some((p.imap_host, p.imap_port, p.smtp_host, p.smtp_port)),
            ProviderKind::Google | ProviderKind::Microsoft => crate::oauth::preset(p.oauth_name().unwrap())
                .map(|o| (o.imap_host, o.imap_port, o.smtp_host, o.smtp_port)),
            ProviderKind::Manual | ProviderKind::CustomOAuth => None,
        };
        if let Some((ih, ip, sh, sp)) = servers {
            widgets.protocol_row.set_selected(0); // IMAP
            widgets.host_row.set_text(ih);
            widgets.port_row.set_text(&ip.to_string());
            widgets.smtp_row.set_text(sh);
            widgets.smtp_port_row.set_text(&sp.to_string());
        }
    }

    /// Build the OAuth client config from the form. Google/Microsoft use built-in
    /// endpoints + credentials; "Custom OAuth" uses the user-entered fields.
    fn oauth_settings_from_form(&self, widgets: &AccountsWindowWidgets) -> OAuthSettings {
        let provider = provider_at(widgets.provider_row.selected()).oauth_name();
        if let Some(name) = provider {
            let p = crate::oauth::preset(name).unwrap();
            let (client_id, client_secret) = crate::oauth::provider_credentials(name);
            OAuthSettings {
                auth_url: p.auth_url.to_string(),
                token_url: p.token_url.to_string(),
                client_id,
                client_secret,
                scopes: p.scopes.to_string(),
            }
        } else {
            OAuthSettings {
                auth_url: trimmed(&widgets.oauth_auth_url_row),
                token_url: trimmed(&widgets.oauth_token_url_row),
                client_id: trimmed(&widgets.oauth_client_id_row),
                client_secret: widgets.oauth_secret_row.text().to_string(),
                scopes: trimmed(&widgets.oauth_scope_row),
            }
        }
    }
}

/// A list-item factory whose labels never ellipsize, so a `ComboRow` popup grows
/// to fit its longest option instead of truncating it.
/// Open GNOME Settings → Online Accounts. Uses D-Bus app activation so it works
/// both natively and inside a Flatpak (with `--talk-name=org.gnome.Settings`);
/// falls back to the CLI on non-GNOME/older setups.
fn open_online_accounts() {
    if activate_online_accounts_panel().is_err() {
        let _ = std::process::Command::new("gnome-control-center")
            .arg("online-accounts")
            .spawn();
    }
}

fn activate_online_accounts_panel() -> Result<(), gtk::glib::Error> {
    let conn = gtk::gio::bus_get_sync(gtk::gio::BusType::Session, gtk::gio::Cancellable::NONE)?;
    // org.freedesktop.Application.ActivateAction(action: s, parameter: av, a{sv}).
    // GNOME Settings' "launch-panel" action takes a (sav): (panel_id, extra_args).
    let panel = ("online-accounts", Vec::<gtk::glib::Variant>::new()).to_variant();
    let params: Vec<gtk::glib::Variant> = vec![panel];
    let platform: std::collections::HashMap<String, gtk::glib::Variant> =
        std::collections::HashMap::new();
    let args = ("launch-panel", params, platform).to_variant();
    conn.call_sync(
        Some("org.gnome.Settings"),
        "/org/gnome/Settings",
        "org.freedesktop.Application",
        "ActivateAction",
        Some(&args),
        None,
        gtk::gio::DBusCallFlags::NONE,
        -1,
        gtk::gio::Cancellable::NONE,
    )?;
    Ok(())
}

fn non_ellipsizing_factory() -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        if let Some(item) = item.downcast_ref::<gtk::ListItem>() {
            let label = gtk::Label::new(None);
            label.set_xalign(0.0);
            label.set_ellipsize(gtk::pango::EllipsizeMode::None);
            label.set_margin_start(6);
            label.set_margin_end(6);
            item.set_child(Some(&label));
        }
    });
    factory.connect_bind(|_, item| {
        if let Some(item) = item.downcast_ref::<gtk::ListItem>() {
            let text = item
                .item()
                .and_downcast::<gtk::StringObject>()
                .map(|o| o.string())
                .unwrap_or_default();
            if let Some(label) = item.child().and_downcast::<gtk::Label>() {
                label.set_label(&text);
            }
        }
    });
    factory
}

fn display_name(acc: &AccountConfig) -> String {
    if acc.name.trim().is_empty() {
        acc.email.clone()
    } else {
        acc.name.clone()
    }
}

/// Build an `AccountConfig` from the current editor form values.
fn read_account(widgets: &AccountsWindowWidgets, emoji: Option<String>) -> AccountConfig {
    let protocol = if widgets.protocol_row.selected() == 1 {
        Protocol::Pop3
    } else {
        Protocol::Imap
    };
    let default_port = if protocol == Protocol::Pop3 { 995 } else { 993 };
    AccountConfig {
        name: trimmed(&widgets.name_row),
        email: trimmed(&widgets.email_row),
        protocol,
        imap_host: trimmed(&widgets.host_row),
        imap_port: trimmed(&widgets.port_row).parse().unwrap_or(default_port),
        smtp_host: trimmed(&widgets.smtp_row),
        smtp_port: trimmed(&widgets.smtp_port_row).parse().unwrap_or(587),
        username: trimmed(&widgets.user_row),
        password: widgets.pass_row.text().to_string(),
        smtp_separate: widgets.smtp_separate_row.is_active(),
        smtp_username: trimmed(&widgets.smtp_user_row),
        smtp_password: widgets.smtp_pass_row.text().to_string(),
        color: Some(crate::color::to_hex(&widgets.color_btn.rgba())),
        emoji,
        // Filled in by SaveWithSig from the rich-text editor.
        signature: None,
        signature_html: true,
        // Only store a custom label; blank or same-as-email falls back to email.
        label: {
            let l = trimmed(&widgets.label_row);
            let email = trimmed(&widgets.email_row);
            if l.is_empty() || l == email {
                None
            } else {
                Some(l)
            }
        },
        // The alias list is model state (alias_edits), assigned by SaveWithSig.
        aliases: Vec::new(),
        // Defaults for a new account; preserved from the original when editing.
        enabled: true,
        goa_id: None,
        goa_mail_disabled: false,
        goa_enabled_before_mail_disabled: true,
        oauth: false,
        oauth_settings: None,
        oauth_refresh: String::new(),
        push: match widgets.push_row.selected() {
            1 => Some(true),
            2 => Some(false),
            _ => None,
        },
        // Assigned by SaveWithSig from the Special Folders combos.
        folder_roles: Default::default(),
    }
}

/// Whether the editor's HTML is effectively empty (no visible content).
fn signature_is_empty(html: &str) -> bool {
    let stripped: String = {
        let mut out = String::new();
        let mut in_tag = false;
        for c in html.chars() {
            match c {
                '<' => in_tag = true,
                '>' => in_tag = false,
                _ if !in_tag => out.push(c),
                _ => {}
            }
        }
        out
    };
    stripped.replace("&nbsp;", " ").trim().is_empty()
}

fn fill_editor(widgets: &AccountsWindowWidgets, acc: &AccountConfig) {
    widgets.name_row.set_text(&acc.name);
    widgets.email_row.set_text(&acc.email);
    // Reflect the account's provider in the dropdown (OAuth by endpoint, known
    // password providers by server, otherwise "Other (IMAP/POP3)…").
    widgets.provider_row.set_selected(provider_index_for_account(acc));
    widgets
        .protocol_row
        .set_selected(if acc.protocol == Protocol::Pop3 { 1 } else { 0 });
    widgets.host_row.set_text(&acc.imap_host);
    widgets.port_row.set_text(&acc.imap_port.to_string());
    widgets.smtp_row.set_text(&acc.smtp_host);
    widgets.smtp_port_row.set_text(&acc.smtp_port.to_string());
    widgets.user_row.set_text(&acc.username);
    widgets.pass_row.set_text(&acc.password);
    widgets.smtp_separate_row.set_active(acc.smtp_separate);
    widgets.smtp_user_row.set_text(&acc.smtp_username);
    widgets.smtp_pass_row.set_text(&acc.smtp_password);
    widgets.push_row.set_selected(match acc.push {
        None => 0,
        Some(true) => 1,
        Some(false) => 2,
    });
    // Show the effective label (custom, or the email address).
    widgets
        .label_row
        .set_text(acc.label.as_deref().unwrap_or(&acc.email));

    // OAuth client detail fields. GOA accounts (goa_id set) can't be
    // re-authenticated here, so they show as password (their mechanism is kept on
    // save); natively-added OAuth accounts show their client details.
    let s = (acc.goa_id.is_none() && acc.oauth)
        .then_some(acc.oauth_settings.as_ref())
        .flatten();
    widgets.oauth_client_id_row.set_text(s.map(|s| s.client_id.as_str()).unwrap_or(""));
    widgets.oauth_secret_row.set_text(s.map(|s| s.client_secret.as_str()).unwrap_or(""));
    widgets.oauth_auth_url_row.set_text(s.map(|s| s.auth_url.as_str()).unwrap_or(""));
    widgets.oauth_token_url_row.set_text(s.map(|s| s.token_url.as_str()).unwrap_or(""));
    widgets.oauth_scope_row.set_text(s.map(|s| s.scopes.as_str()).unwrap_or(""));
    widgets.oauth_status.set_visible(false);
    widgets.oauth_signin_btn.set_sensitive(true);

    // Signature is loaded into the rich-text editor by the caller.
    widgets.test_result.set_visible(false);
    widgets.test_btn.set_sensitive(true);
}

/// Grey out everything GNOME Online Accounts owns.
///
/// An account imported from GOA takes its address, servers, protocol and
/// credentials from the system; editing them here would either be overwritten
/// the next time GOA is read, or quietly disagree with what the rest of the
/// desktop uses. What stays editable is what Vireo owns: the sender's display
/// name, signature, colour, emoji and label.
fn set_connection_editable(widgets: &AccountsWindowWidgets, editable: bool) {
    for row in [
        widgets.email_row.upcast_ref::<gtk::Widget>(),
        widgets.host_row.upcast_ref(),
        widgets.port_row.upcast_ref(),
        widgets.smtp_row.upcast_ref(),
        widgets.smtp_port_row.upcast_ref(),
        widgets.user_row.upcast_ref(),
        widgets.pass_row.upcast_ref(),
        widgets.smtp_user_row.upcast_ref(),
        widgets.smtp_pass_row.upcast_ref(),
    ] {
        row.set_sensitive(editable);
    }
    widgets.provider_row.set_sensitive(editable);
    widgets.protocol_row.set_sensitive(editable);
    widgets.smtp_separate_row.set_sensitive(editable);
    // OAuth client details belong to a natively-added account; a GOA one gets its
    // tokens from the system.
    for row in [
        widgets.oauth_client_id_row.upcast_ref::<gtk::Widget>(),
        widgets.oauth_secret_row.upcast_ref(),
        widgets.oauth_auth_url_row.upcast_ref(),
        widgets.oauth_token_url_row.upcast_ref(),
        widgets.oauth_scope_row.upcast_ref(),
    ] {
        row.set_sensitive(editable);
    }
    widgets.oauth_signin_btn.set_sensitive(editable);
    // Testing stays available: it is read-only, and confirming that the imported
    // settings actually connect is exactly what someone would want here.
}

fn clear_editor(widgets: &AccountsWindowWidgets) {
    widgets.name_row.set_text("");
    widgets.email_row.set_text("");
    widgets.provider_row.set_selected(manual_index());
    widgets.protocol_row.set_selected(0);
    widgets.host_row.set_text("");
    widgets.port_row.set_text("993");
    widgets.smtp_row.set_text("");
    widgets.smtp_port_row.set_text("587");
    widgets.user_row.set_text("");
    widgets.pass_row.set_text("");
    widgets.smtp_separate_row.set_active(false);
    widgets.push_row.set_selected(0);
    widgets.smtp_user_row.set_text("");
    widgets.smtp_pass_row.set_text("");
    widgets.label_row.set_text("");
    widgets.oauth_client_id_row.set_text("");
    widgets.oauth_secret_row.set_text("");
    widgets.oauth_auth_url_row.set_text("");
    widgets.oauth_token_url_row.set_text("");
    widgets.oauth_scope_row.set_text("");
    widgets.oauth_status.set_visible(false);
    widgets.test_result.set_visible(false);
    widgets.test_btn.set_sensitive(true);
}

fn trimmed(row: &impl IsA<gtk::Editable>) -> String {
    row.text().trim().to_string()
}

/// Dropdown index of the first provider entry of a given kind (the manual entry
/// if none — shouldn't happen for kinds present in the table).
fn kind_index(kind: ProviderKind) -> u32 {
    PROVIDERS
        .iter()
        .position(|p| p.kind == kind)
        .map(|i| i as u32)
        .unwrap_or_else(manual_index)
}

/// Dropdown index of the `Preset` provider whose incoming server matches `host`,
/// or the manual entry when nothing matches.
fn preset_index_for_host(host: &str) -> u32 {
    let host = host.trim().to_ascii_lowercase();
    if host.is_empty() {
        return manual_index();
    }
    PROVIDERS
        .iter()
        .position(|p| p.kind == ProviderKind::Preset && p.imap_host.eq_ignore_ascii_case(&host))
        .map(|i| i as u32)
        .unwrap_or_else(manual_index)
}

/// Dropdown index reflecting an existing account: its OAuth provider (by token
/// endpoint) for native OAuth accounts, otherwise the matching password provider.
fn provider_index_for_account(acc: &AccountConfig) -> u32 {
    if acc.goa_id.is_none() && acc.oauth {
        let kind = match acc.oauth_settings.as_ref() {
            Some(s) if s.token_url.contains("googleapis") => ProviderKind::Google,
            Some(s) if s.token_url.contains("microsoftonline") => ProviderKind::Microsoft,
            _ => ProviderKind::CustomOAuth,
        };
        return kind_index(kind);
    }
    preset_index_for_host(&acc.imap_host)
}

fn parse_color(hex: &str) -> gtk::gdk::RGBA {
    gtk::gdk::RGBA::parse(hex).unwrap_or_else(|_| gtk::gdk::RGBA::new(0.21, 0.52, 0.89, 1.0))
}

#[cfg(test)]
mod tests {
    use super::{manual_index, preset_index_for_host, provider_at, ProviderKind, PROVIDERS};

    #[test]
    fn known_host_maps_to_its_own_entry() {
        let idx = preset_index_for_host("imap.mail.me.com");
        assert_eq!(provider_at(idx).label, "iCloud");
        // Case-insensitive.
        assert_eq!(preset_index_for_host("IMAP.FASTMAIL.COM"), preset_index_for_host("imap.fastmail.com"));
        assert_eq!(provider_at(preset_index_for_host("imap.fastmail.com")).label, "Fastmail");
    }

    #[test]
    fn unknown_or_empty_host_falls_back_to_manual() {
        assert_eq!(preset_index_for_host("mail.example.org"), manual_index());
        assert_eq!(preset_index_for_host(""), manual_index());
        assert_eq!(preset_index_for_host("  "), manual_index());
        assert_eq!(provider_at(manual_index()).kind, ProviderKind::Manual);
    }

    #[test]
    fn removed_password_providers_are_gone() {
        // Gmail/Hotmail no longer work with a password — only via OAuth.
        for p in PROVIDERS {
            if p.kind == ProviderKind::Preset {
                assert_ne!(p.imap_host, "imap.gmail.com");
                assert_ne!(p.imap_host, "outlook.office365.com");
            }
        }
    }

    #[test]
    fn provider_table_is_well_formed() {
        // Distinct labels; presets have sane servers; exactly one Manual entry.
        let mut labels: Vec<&str> = PROVIDERS.iter().map(|p| p.label).collect();
        let n = labels.len();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), n, "provider labels must be unique");
        assert_eq!(PROVIDERS.iter().filter(|p| p.kind == ProviderKind::Manual).count(), 1);
        for p in PROVIDERS {
            if p.kind == ProviderKind::Preset {
                assert!(!p.imap_host.is_empty() && !p.smtp_host.is_empty(), "{}", p.label);
                assert!(p.imap_port > 0 && p.smtp_port > 0, "{}", p.label);
                // Each preset's host round-trips to its own entry.
                assert_eq!(provider_at(preset_index_for_host(p.imap_host)).label, p.label);
            }
        }
    }
}

impl AccountsWindow {
    /// Human labels for the filter enums, shared by rows and the dialog.
    fn field_label(f: crate::config::FilterField) -> &'static str {
        use crate::config::FilterField::*;
        match f {
            FromAddress => "From address",
            FromName => "From name",
            Subject => "Subject",
            Recipients => "To or Cc",
        }
    }
    fn match_label(m: crate::config::FilterMatch) -> &'static str {
        use crate::config::FilterMatch::*;
        match m {
            Contains => "contains",
            Equals => "is exactly",
            StartsWith => "starts with",
            EndsWith => "ends with",
        }
    }

    /// Re-render the Filters group's rule rows.
    fn rebuild_filter_rows(&self, sender: &ComponentSender<Self>) {
        let Some(list) = &self.filters_list else { return };
        while let Some(row) = list.first_child() {
            list.remove(&row);
        }
        list.set_visible(!self.filter_rules.is_empty());
        for (i, r) in self.filter_rules.iter().enumerate() {
            let row = adw::ActionRow::new();
            row.set_title(&format!(
                "{} {} \u{201c}{}\u{201d}",
                Self::field_label(r.field),
                Self::match_label(r.matcher),
                r.value,
            ));
            let dest = self
                .folders_by_email
                .get(&r.account_email)
                .and_then(|fs| fs.iter().find(|(p, _)| *p == r.dest_path))
                .map(|(_, name)| name.clone())
                .unwrap_or_else(|| r.dest_path.clone());
            row.set_subtitle(&format!("{} \u{2192} {}", r.account_email, dest));
            let rm = gtk::Button::from_icon_name("co.hyprlab.Vireo-user-trash-symbolic");
            rm.add_css_class("flat");
            rm.set_valign(gtk::Align::Center);
            rm.set_tooltip_text(Some("Remove filter"));
            let s = sender.clone();
            rm.connect_clicked(move |_| s.input(AccountsInput::RemoveFilter(i)));
            row.add_suffix(&rm);
            list.append(&row);
        }
    }

    /// The Add Filter dialog: account, field, match, value, destination.
    fn open_filter_dialog(&self, sender: &ComponentSender<Self>) {
        use crate::config::{FilterField, FilterMatch, FilterRule};
        let emails: Vec<String> = self.accounts.iter().map(|a| a.email.clone()).collect();
        if emails.is_empty() {
            return;
        }
        let parent = relm4::main_application().active_window();
        let dialog =
            adw::MessageDialog::new(parent.as_ref(), Some("Add Filter"), None);
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("add", "Add Filter");
        dialog.set_response_appearance("add", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("add"));

        let form = gtk::ListBox::new();
        form.add_css_class("boxed-list");
        form.set_selection_mode(gtk::SelectionMode::None);

        let account_row = adw::ComboRow::new();
        account_row.set_title("Account");
        let email_refs: Vec<&str> = emails.iter().map(|s| s.as_str()).collect();
        account_row.set_model(Some(&gtk::StringList::new(&email_refs)));

        let field_row = adw::ComboRow::new();
        field_row.set_title("Where");
        field_row.set_model(Some(&gtk::StringList::new(&[
            "From address",
            "From name",
            "Subject",
            "To or Cc",
        ])));

        let match_row = adw::ComboRow::new();
        match_row.set_title("Match");
        match_row.set_model(Some(&gtk::StringList::new(&[
            "contains",
            "is exactly",
            "starts with",
            "ends with",
        ])));

        let value_row = adw::EntryRow::new();
        value_row.set_title("Text to match");

        let dest_row = adw::ComboRow::new();
        dest_row.set_title("Move to");
        // The destination list follows the chosen account.
        let folders = std::rc::Rc::new(self.folders_by_email.clone());
        let emails_rc = std::rc::Rc::new(emails.clone());
        let dest_paths = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let fill_dest = {
            let dest_row = dest_row.clone();
            let folders = folders.clone();
            let emails_rc = emails_rc.clone();
            let dest_paths = dest_paths.clone();
            move |idx: usize| {
                let empty = Vec::new();
                let list =
                    emails_rc.get(idx).and_then(|e| folders.get(e)).unwrap_or(&empty);
                let names: Vec<&str> = list.iter().map(|(_, n)| n.as_str()).collect();
                dest_row.set_model(Some(&gtk::StringList::new(&names)));
                *dest_paths.borrow_mut() = list.iter().map(|(p, _)| p.clone()).collect();
            }
        };
        fill_dest(0);
        {
            let fill_dest = fill_dest.clone();
            account_row.connect_selected_notify(move |row| fill_dest(row.selected() as usize));
        }

        form.append(&account_row);
        form.append(&field_row);
        form.append(&match_row);
        form.append(&value_row);
        form.append(&dest_row);
        dialog.set_extra_child(Some(&form));

        let s = sender.clone();
        dialog.connect_response(Some("add"), move |_, _| {
            let value = value_row.text().trim().to_string();
            let paths = dest_paths.borrow();
            let (Some(email), Some(dest)) = (
                emails.get(account_row.selected() as usize),
                paths.get(dest_row.selected() as usize),
            ) else {
                return;
            };
            if value.is_empty() {
                return;
            }
            let rule = FilterRule {
                account_email: email.clone(),
                field: match field_row.selected() {
                    0 => FilterField::FromAddress,
                    1 => FilterField::FromName,
                    2 => FilterField::Subject,
                    _ => FilterField::Recipients,
                },
                matcher: match match_row.selected() {
                    0 => FilterMatch::Contains,
                    1 => FilterMatch::Equals,
                    2 => FilterMatch::StartsWith,
                    _ => FilterMatch::EndsWith,
                },
                value,
                dest_path: dest.clone(),
            };
            s.input(AccountsInput::FilterAdded(rule));
        });
        dialog.present();
    }
}

