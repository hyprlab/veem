//! Accounts window: manage all mail accounts (add / edit / remove / reorder).
//!
//! A standalone window (opened from the main menu, separate from Preferences).
//! It uses an `AdwNavigationView` with two pages: a list of accounts (drag rows
//! to set the sidebar order) and a reusable editor form pushed on top.

use adw::prelude::*;
use relm4::prelude::*;

use crate::config::{AccountConfig, OAuthSettings, Protocol};
use crate::ui::rich_editor::{self, RichEditor};
use crate::worker::{self, ConnTest};

const DEFAULT_COLOR: &str = "#3584e4";

pub struct AccountsWindow {
    /// Accounts in display order.
    accounts: Vec<AccountConfig>,
    /// Index being edited; `None` while adding a new account.
    editing: Option<usize>,
    /// Emoji currently chosen in the editor (`None` → use initials).
    emoji: Option<String>,
    /// WYSIWYG editor for the account signature.
    sig_editor: RichEditor,
    /// The email value the label field currently mirrors, so the label auto-fills
    /// from the email until the user customizes it.
    label_synced: String,
    /// GNOME Online Accounts mail accounts available to import (not yet in Veem).
    goa: Vec<crate::goa::GoaMailAccount>,
    /// Refresh token captured from a successful OAuth sign-in, applied on save.
    pending_oauth_refresh: Option<String>,
}

#[derive(Debug)]
pub enum AccountsInput {
    AddAccount,
    EditAccount(usize),
    /// The email field changed — mirror it into the (auto-filled) label field.
    EmailChanged,
    MoveRow { from: usize, to: usize },
    /// Enable/disable an account from the list toggle.
    ToggleEnabled { index: usize, enabled: bool },
    /// Enable/disable the account currently open in the editor (GOA group toggle).
    ToggleCurrentEnabled(bool),
    /// Import a GNOME Online Account (by index into `goa`) into Veem.
    ImportGoa(usize),
    /// The authentication-method dropdown changed (Password / OAuth provider).
    AuthMethodChanged,
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
    /// Import a GNOME Online Account into Veem (with its credentials).
    ImportGoa(Box<AccountConfig>),
    Closed,
}

/// Background command results for the editor.
#[derive(Debug)]
pub enum AccountsCmd {
    /// Test-connection result.
    Test(ConnTest),
    /// OAuth sign-in result: the refresh token, or an error message.
    OAuth(Result<String, String>),
}

#[relm4::component(pub)]
impl Component for AccountsWindow {
    type Init = Vec<AccountConfig>;
    type Input = AccountsInput;
    type Output = AccountsOutput;
    type CommandOutput = AccountsCmd;

    view! {
        adw::Window {
            set_modal: false,
            set_default_width: 480,
            set_default_height: 620,
            set_title: Some("Accounts"),

            connect_close_request[sender] => move |_| {
                let _ = sender.output(AccountsOutput::Closed);
                gtk::glib::Propagation::Proceed
            },

            #[wrap(Some)]
            #[name = "nav"]
            set_content = &adw::NavigationView {

                // ---- list page ----
                add = &adw::NavigationPage {
                    set_title: "Accounts",
                    set_tag: Some("list"),

                    #[wrap(Some)]
                    set_child = &adw::ToolbarView {
                        add_top_bar = &adw::HeaderBar {},

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
                                     use it in Veem."
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
                            add = &adw::PreferencesGroup {
                                set_title: "Mail Account",

                                // Pick how to sign in first; the rest of the form
                                // adapts to the choice.
                                #[name = "auth_row"]
                                adw::ComboRow {
                                    set_title: "Authentication",
                                    connect_selected_notify => AccountsInput::AuthMethodChanged,
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

                                // Shown for Google when no built-in/own OAuth client
                                // is available: point the user at GNOME Online Accounts.
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
                                        set_label: "Google sign-in uses GNOME Online Accounts.\n\n\
                                            1. Open Online Accounts and sign in with Google.\n\
                                            2. Come back to Veem and reopen this window — your \
                                            Google account then appears under “GNOME Online \
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

                            // GOA-imported accounts: you can't meaningfully "remove"
                            // one from Veem while it still lives in GNOME Online
                            // Accounts — so disable it here, or open GOA to change it.
                            #[name = "goa_manage_group"]
                            add = &adw::PreferencesGroup {
                                set_visible: false,
                                set_title: "GNOME Online Account",
                                set_description: Some(
                                    "This account comes from GNOME Online Accounts. Turn it off \
                                     to hide it in Veem without touching your system; to edit or \
                                     remove it, open Online Accounts."
                                ),

                                #[name = "goa_enabled_row"]
                                adw::SwitchRow {
                                    set_title: "Enabled in Veem",
                                    connect_active_notify[sender] => move |row| {
                                        sender.input(AccountsInput::ToggleCurrentEnabled(row.is_active()));
                                    },
                                },

                                gtk::Button {
                                    set_label: "Open Online Accounts…",
                                    set_halign: gtk::Align::Center,
                                    set_margin_top: 12,
                                    connect_clicked => AccountsInput::OpenOnlineAccounts,
                                },
                            },

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
        // GNOME Online Accounts mail accounts not already configured in Veem.
        let goa: Vec<crate::goa::GoaMailAccount> = crate::goa::list_mail_accounts()
            .into_iter()
            .filter(|g| !init.iter().any(|a| a.email.eq_ignore_ascii_case(&g.email)))
            .collect();

        let model = AccountsWindow {
            accounts: init,
            editing: None,
            emoji: None,
            sig_editor: RichEditor::new(""),
            label_synced: String::new(),
            goa,
            pending_oauth_refresh: None,
        };

        let widgets = view_output!();
        widgets.sig_holder.append(&model.sig_editor.widget);
        model.rebuild_account_list(&widgets.accounts_list, &sender);
        model.rebuild_goa_list(&widgets.goa_list, &sender);
        widgets.goa_group.set_visible(!model.goa.is_empty());
        widgets
            .protocol_row
            .set_model(Some(&gtk::StringList::new(&["IMAP", "POP3"])));
        widgets.auth_row.set_model(Some(&gtk::StringList::new(&[
            "IMAP/POP3 Password",
            "Google OAuth",
            "Microsoft OAuth (experimental)",
            "Custom OAuth (experimental)",
        ])));
        // The default dropdown popup ellipsizes items; use a factory whose labels
        // don't, so the list widens to show the full option text.
        widgets.auth_row.set_list_factory(Some(&non_ellipsizing_factory()));

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
                clear_editor(widgets);
                self.apply_auth_visibility(widgets);
                self.sig_editor.set_html("");
                widgets.color_btn.set_rgba(&parse_color(DEFAULT_COLOR));
                widgets.emoji_btn.set_label("Add");
                widgets.remove_group.set_visible(false);
                widgets.goa_manage_group.set_visible(false);
                widgets.nav.push_by_tag("editor");
            }

            AccountsInput::EditAccount(i) => {
                let Some(acc) = self.accounts.get(i).cloned() else {
                    return;
                };
                self.editing = Some(i);
                self.pending_oauth_refresh = None;
                fill_editor(widgets, &acc);
                self.apply_auth_visibility(widgets);
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
                widgets.remove_group.set_visible(!is_goa);
                widgets.goa_manage_group.set_visible(is_goa);
                widgets.goa_enabled_row.set_active(acc.enabled);
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
                    let (password, oauth) = if g.password_based {
                        (crate::goa::fetch_password(&g.path, &g.id).unwrap_or_default(), false)
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

            AccountsInput::AuthMethodChanged => {
                self.apply_auth_visibility(widgets);
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

            AccountsInput::SaveWithSig(sig_html) => {
                widgets.host_row.remove_css_class("error");
                let mut account = read_account(widgets, self.emoji.clone());
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
                    if orig.goa_id.is_some() {
                        account.oauth = orig.oauth;
                        account.oauth_settings = orig.oauth_settings.clone();
                    }
                }

                // Native account: authentication comes from the combo.
                let auth_idx = widgets.auth_row.selected();
                if account.goa_id.is_none() {
                    if auth_idx != 0 {
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
                if account.imap_host.is_empty()
                    || account.username.is_empty()
                    || !password_ok
                    || !oauth_ready
                    || (account.smtp_separate
                        && (account.smtp_username.is_empty() || account.smtp_password.is_empty()))
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
                let dialog = adw::MessageDialog::new(
                    Some(root),
                    Some("Remove Account?"),
                    Some(&format!(
                        "Remove {name} from Veem? Its saved password is deleted from \
                         the keyring. Mail on the server is not affected."
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
                    }
                }
                widgets.nav.pop();
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
        }
    }
}

impl AccountsWindow {
    /// Rebuild the draggable account list.
    fn rebuild_account_list(&self, list: &gtk::ListBox, sender: &ComponentSender<Self>) {
        while let Some(child) = list.first_child() {
            list.remove(&child);
        }

        for (pos, acc) in self.accounts.iter().enumerate() {
            let row = gtk::ListBoxRow::new();
            row.set_activatable(true);

            let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            hbox.add_css_class("account-list-row");

            let handle = gtk::Image::from_icon_name("list-drag-handle-symbolic");
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

            // Enable/disable toggle. Disabled accounts stay configured but don't
            // sync or appear in the sidebar.
            let toggle = gtk::Switch::new();
            toggle.set_valign(gtk::Align::Center);
            toggle.set_tooltip_text(Some("Enable this account"));
            toggle.set_active(acc.enabled);
            let ti = sender.input_sender().clone();
            let tpos = pos;
            toggle.connect_state_set(move |_, state| {
                let _ = ti.send(AccountsInput::ToggleEnabled { index: tpos, enabled: state });
                gtk::glib::Propagation::Proceed
            });
            hbox.append(&toggle);

            let next = gtk::Image::from_icon_name("go-next-symbolic");
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
            // OAuth providers (Gmail, Microsoft) sign in with a token from GNOME.
            if g.oauth2 && !g.password_based {
                subtitle.push_str(" · sign-in via GNOME");
            }
            row.set_subtitle(&subtitle);

            let toggle = gtk::Switch::new();
            toggle.set_valign(gtk::Align::Center);
            toggle.set_active(false);
            toggle.set_tooltip_text(Some("Use this account in Veem"));
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
    fn apply_auth_visibility(&self, widgets: &AccountsWindowWidgets) {
        let idx = widgets.auth_row.selected();
        let is_password = idx == 0;
        let is_oauth = idx != 0;
        let is_custom = idx == 3;
        // Google with no built-in (or user-supplied) OAuth client: there's nothing
        // to sign in with, so guide the user to GNOME Online Accounts instead.
        let google_needs_goa =
            idx == 1 && crate::oauth::provider_credentials("google").0.trim().is_empty();
        // Google/Microsoft: servers come from the built-in preset, so hide all the
        // server/credential plumbing — the user only needs email + sign-in. Custom
        // OAuth still needs the server addresses (and its own client details).
        let show_servers = is_password || is_custom;

        // Google now goes through GNOME Online Accounts (stable); only the native
        // Microsoft/Custom OAuth flows are still flagged experimental.
        let experimental = idx == 2 || idx == 3;
        widgets
            .auth_row
            .set_subtitle(if experimental { "OAuth sign-in is experimental" } else { "" });

        // Server/credential fields.
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

        // Microsoft/Custom use built-in or entered credentials — the user just
        // signs in. Google with no client falls back to the GNOME Online Accounts
        // panel, which replaces the sign-in button and the identity fields.
        widgets.name_row.set_visible(!google_needs_goa);
        widgets.email_row.set_visible(!google_needs_goa);
        widgets.goa_hint.set_visible(google_needs_goa);
        widgets.oauth_signin_btn.set_visible(is_oauth && !google_needs_goa);
        widgets.oauth_client_id_row.set_visible(is_custom);
        widgets.oauth_secret_row.set_visible(is_custom);
        widgets.oauth_auth_url_row.set_visible(is_custom);
        widgets.oauth_token_url_row.set_visible(is_custom);
        widgets.oauth_scope_row.set_visible(is_custom);
        if !is_oauth || google_needs_goa {
            widgets.oauth_status.set_visible(false);
        }

        // Pre-fill IMAP/SMTP for known providers.
        let preset = match idx {
            1 => crate::oauth::preset("google"),
            2 => crate::oauth::preset("microsoft"),
            _ => None,
        };
        if let Some(p) = preset {
            widgets.host_row.set_text(p.imap_host);
            widgets.port_row.set_text(&p.imap_port.to_string());
            widgets.smtp_row.set_text(p.smtp_host);
            widgets.smtp_port_row.set_text(&p.smtp_port.to_string());
        }
    }

    /// Build the OAuth client config from the form. Google/Microsoft use built-in
    /// endpoints + credentials; "Custom OAuth" uses the user-entered fields.
    fn oauth_settings_from_form(&self, widgets: &AccountsWindowWidgets) -> OAuthSettings {
        let provider = match widgets.auth_row.selected() {
            1 => Some("google"),
            2 => Some("microsoft"),
            _ => None,
        };
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
        // Defaults for a new account; preserved from the original when editing.
        enabled: true,
        goa_id: None,
        oauth: false,
        oauth_settings: None,
        oauth_refresh: String::new(),
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
    // Show the effective label (custom, or the email address).
    widgets
        .label_row
        .set_text(acc.label.as_deref().unwrap_or(&acc.email));

    // Authentication method + OAuth fields. GOA accounts (goa_id set) can't be
    // re-authenticated here, so they show as Password (their mechanism is kept on
    // save); natively-added OAuth accounts show their provider + client details.
    let (auth_idx, s) = match (&acc.oauth_settings, acc.goa_id.is_none() && acc.oauth) {
        (Some(s), true) => {
            let idx = if s.token_url.contains("googleapis") {
                1
            } else if s.token_url.contains("microsoftonline") {
                2
            } else {
                3
            };
            (idx, Some(s))
        }
        _ => (0u32, None),
    };
    widgets.auth_row.set_selected(auth_idx);
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

fn clear_editor(widgets: &AccountsWindowWidgets) {
    widgets.name_row.set_text("");
    widgets.email_row.set_text("");
    widgets.protocol_row.set_selected(0);
    widgets.host_row.set_text("");
    widgets.port_row.set_text("993");
    widgets.smtp_row.set_text("");
    widgets.smtp_port_row.set_text("587");
    widgets.user_row.set_text("");
    widgets.pass_row.set_text("");
    widgets.smtp_separate_row.set_active(false);
    widgets.smtp_user_row.set_text("");
    widgets.smtp_pass_row.set_text("");
    widgets.label_row.set_text("");
    widgets.auth_row.set_selected(0);
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

fn parse_color(hex: &str) -> gtk::gdk::RGBA {
    gtk::gdk::RGBA::parse(hex).unwrap_or_else(|_| gtk::gdk::RGBA::new(0.21, 0.52, 0.89, 1.0))
}
