//! Account configuration.
//!
//! Account metadata (name, email, servers, username) lives in
//! `~/.config/vireo/accounts.toml`. Passwords are kept in the system keyring
//! (secret-service, e.g. gnome-keyring) — never written to disk. The `password`
//! field is read from the TOML if present (older configs / manual setup) and
//! migrated into the keyring on first use, then stripped from the file.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Write a config file that only its owner can read, without ever having
/// existed as anything else.
///
/// `fs::write` then `set_permissions` leaves a window in which the file sits on
/// disk with whatever the umask allowed — brief, but these files carry
/// hostnames, usernames, OAuth client secrets and correspondent lists. Creating
/// with the mode already set closes it.
fn write_private(path: &std::path::Path, contents: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path)?;
    f.write_all(contents.as_bytes())?;
    // An existing file keeps its old mode through `OpenOptions::mode`, which
    // only applies at creation — so tighten regardless.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// XDG base directories, with one twist: the beta channel running inside its
/// own Flatpak sandbox (app ID co.hyprlab.Vireo.Beta) would get an empty
/// `~/.var/app/co.hyprlab.Vireo.Beta` tree of its own — so it redirects to the
/// STABLE app's tree instead, sharing accounts, settings and the mail cache
/// with the stable install. The keyring service name is identical across
/// channels, so credentials are shared through the same redirection-free path.
/// Outside Flatpak (host builds) both channels already share `~/.config/vireo`
/// et al., so no redirection is needed or done.
fn shared_base(own: fn() -> Option<PathBuf>, sub: &str) -> Option<PathBuf> {
    if cfg!(feature = "beta")
        && std::env::var("FLATPAK_ID").is_ok_and(|id| id == "co.hyprlab.Vireo.Beta")
    {
        return Some(dirs::home_dir()?.join(".var/app/co.hyprlab.Vireo").join(sub));
    }
    own()
}

pub fn config_base() -> Option<PathBuf> {
    shared_base(dirs::config_dir, "config")
}

pub fn cache_base() -> Option<PathBuf> {
    shared_base(dirs::cache_dir, "cache")
}

pub fn data_base() -> Option<PathBuf> {
    shared_base(dirs::data_dir, "data")
}

/// Service name used for keyring entries; password items are keyed by email.
const KEYRING_SERVICE: &str = "co.hyprlab.Vireo";

/// Incoming-mail protocol for an account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    #[default]
    Imap,
    Pop3,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AccountConfig {
    pub name: String,
    pub email: String,
    /// Incoming-mail protocol (IMAP or POP3).
    #[serde(default)]
    pub protocol: Protocol,
    /// Incoming server host (IMAP or POP3, per `protocol`).
    pub imap_host: String,
    /// Incoming server port (IMAP or POP3, per `protocol`).
    #[serde(default = "default_imap_port")]
    pub imap_port: u16,
    /// SMTP server. If empty, derived from `imap_host` (imap.* → smtp.*).
    #[serde(default)]
    pub smtp_host: String,
    #[serde(default = "default_smtp_port")]
    pub smtp_port: u16,
    pub username: String,
    /// Read from TOML if present (legacy/manual), but never written back —
    /// passwords belong in the keyring. Usually empty after the first run.
    #[serde(default, skip_serializing)]
    pub password: String,
    /// Use distinct SMTP credentials instead of the IMAP ones.
    #[serde(default)]
    pub smtp_separate: bool,
    /// SMTP username (used only when `smtp_separate`).
    #[serde(default)]
    pub smtp_username: String,
    /// SMTP password — kept in the keyring (separate entry), never on disk.
    #[serde(default, skip_serializing)]
    pub smtp_password: String,
    /// Sidebar avatar background colour ("#rrggbb"). Falls back to the auto accent.
    #[serde(default)]
    pub color: Option<String>,
    /// Sidebar avatar emoji; when absent, the account-name initials are shown.
    #[serde(default)]
    pub emoji: Option<String>,
    /// Composition signature appended to new messages from this account.
    #[serde(default)]
    pub signature: Option<String>,
    /// Whether `signature` is HTML (vs. plain text).
    #[serde(default)]
    pub signature_html: bool,
    /// How this account is labelled in the UI (e.g. the All Inboxes view).
    /// When unset, the email address is shown.
    #[serde(default)]
    pub label: Option<String>,
    /// Send-as aliases: extra From identities offered in the composer (#34).
    /// Older configs stored these as plain "Name <address>" strings; both forms
    /// are accepted on load, and saved back as tables.
    #[serde(default, deserialize_with = "deserialize_aliases")]
    pub aliases: Vec<AliasConfig>,
    /// Whether the account is active. Disabled accounts stay configured but don't
    /// connect, sync, or appear in the sidebar.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// When imported from GNOME Online Accounts, the GOA account id (so its
    /// settings/credentials trace back to the system account).
    #[serde(default)]
    pub goa_id: Option<String>,
    /// Set while this GOA account's Mail service is switched off in GNOME
    /// Settings: the account is paused (`enabled` forced off) rather than
    /// removed, so its local settings survive until Mail comes back on.
    #[serde(default)]
    pub goa_mail_disabled: bool,
    /// What `enabled` was when the Mail pause began; restored when it ends.
    #[serde(default = "default_enabled")]
    pub goa_enabled_before_mail_disabled: bool,
    /// Authenticate with OAuth2 (XOAUTH2) instead of a stored password. The token
    /// comes from GOA (`goa_id`) or, for accounts added directly in Vireo, from
    /// refreshing `oauth_settings` with the keyring-stored refresh token.
    #[serde(default)]
    pub oauth: bool,
    /// OAuth2 endpoints/client for a natively-added OAuth account (no GOA).
    #[serde(default)]
    pub oauth_settings: Option<OAuthSettings>,
    /// OAuth2 refresh token — kept in the keyring, never on disk. Transient in
    /// memory (like `password`); stored on save.
    #[serde(default, skip_serializing)]
    pub oauth_refresh: String,
}

/// A send-as alias (#34): an extra From identity the composer offers. By
/// default the mail still leaves through the account's own SMTP; an alias may
/// instead carry its own SMTP transport (host, credentials), so mail sent as
/// the alias goes out through the alias's provider — the forwarded-mailbox
/// setup where e.g. Gmail would otherwise rewrite the sender.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AliasConfig {
    /// The From identity: "Name <address>" or a bare address.
    pub identity: String,
    /// The alias's own SMTP server; empty = send through the account's SMTP.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub smtp_host: String,
    #[serde(default = "default_smtp_port", skip_serializing_if = "is_default_smtp_port")]
    pub smtp_port: u16,
    /// SMTP username (used only when `smtp_host` is set).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub smtp_username: String,
    /// SMTP password — kept in the keyring (keyed by account + alias address),
    /// never on disk.
    #[serde(default, skip_serializing)]
    pub smtp_password: String,
}

impl Default for AliasConfig {
    fn default() -> Self {
        Self {
            identity: String::new(),
            smtp_host: String::new(),
            smtp_port: default_smtp_port(),
            smtp_username: String::new(),
            smtp_password: String::new(),
        }
    }
}

impl AliasConfig {
    /// The bare address inside `identity`.
    pub fn address(&self) -> String {
        split_identity(&self.identity).1
    }

    /// Whether mail sent as this alias leaves through the alias's own SMTP
    /// server rather than the account's.
    pub fn has_own_smtp(&self) -> bool {
        !self.smtp_host.trim().is_empty()
    }
}

fn is_default_smtp_port(port: &u16) -> bool {
    *port == default_smtp_port()
}

/// "Name <addr>" or a bare address → (name, addr).
pub fn split_identity(s: &str) -> (String, String) {
    match s.split_once('<') {
        Some((n, rest)) => (
            n.trim().trim_matches('"').to_string(),
            rest.trim_end_matches('>').trim().to_string(),
        ),
        None => (String::new(), s.trim().to_string()),
    }
}

/// Aliases were plain strings before they could carry their own SMTP; accept
/// either form so existing configs keep loading.
fn deserialize_aliases<'de, D>(deserializer: D) -> Result<Vec<AliasConfig>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Entry {
        Plain(String),
        Full(AliasConfig),
    }
    Ok(Vec::<Entry>::deserialize(deserializer)?
        .into_iter()
        .map(|e| match e {
            Entry::Plain(identity) => AliasConfig { identity, ..AliasConfig::default() },
            Entry::Full(alias) => alias,
        })
        .collect())
}

/// OAuth2 client configuration for an account added directly in Vireo.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct OAuthSettings {
    pub auth_url: String,
    pub token_url: String,
    pub client_id: String,
    /// Optional; public/installed clients often have none.
    #[serde(default)]
    pub client_secret: String,
    pub scopes: String,
}

fn default_enabled() -> bool {
    true
}

impl AccountConfig {
    /// The account's UI label: the custom label, or the email address.
    pub fn display_label(&self) -> String {
        match self.label.as_deref() {
            Some(l) if !l.trim().is_empty() => l.to_string(),
            _ => self.email.clone(),
        }
    }
}

fn default_imap_port() -> u16 {
    993
}

fn default_smtp_port() -> u16 {
    587
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct ConfigFile {
    #[serde(default)]
    accounts: Vec<AccountConfig>,
}

/// Path to the accounts config file (`~/.config/vireo/accounts.toml`).
pub fn path() -> Option<PathBuf> {
    Some(config_base()?.join("vireo").join("accounts.toml"))
}

/// Returns the configured accounts, or `None` if there is no usable config
/// (missing file, parse error, or empty list) — in which case the app falls
/// back to the offline sample backend.
pub fn load() -> Option<Vec<AccountConfig>> {
    let path = path()?;
    let text = std::fs::read_to_string(&path).ok()?;
    match toml::from_str::<ConfigFile>(&text) {
        Ok(cfg) if !cfg.accounts.is_empty() => {
            tracing::info!("loaded {} account(s) from {}", cfg.accounts.len(), path.display());
            Some(cfg.accounts)
        }
        Ok(_) => None,
        Err(e) => {
            tracing::error!("failed to parse {}: {e}", path.display());
            None
        }
    }
}

/// Write account metadata to disk (no passwords) and store each password in the
/// keyring.
///
/// Passwords live only in the keyring, so an in-memory `AccountConfig` loaded
/// from disk has an empty `password`. We must NEVER store an empty password —
/// doing so would wipe the keyring entry of any account that wasn't just edited.
pub fn save(accounts: &[AccountConfig]) -> std::io::Result<()> {
    write_config(accounts)?;
    for account in accounts {
        if !account.password.is_empty() {
            if let Err(e) = store_password(&account.email, &account.password) {
                tracing::error!("could not store password for {}: {e}", account.email);
            }
        }
        // Same empty-guard for the separate SMTP password.
        if account.smtp_separate && !account.smtp_password.is_empty() {
            if let Err(e) = store_smtp_password(&account.email, &account.smtp_password) {
                tracing::error!("could not store SMTP password for {}: {e}", account.email);
            }
        }
        // And for each alias that sends through its own SMTP (#34).
        for alias in &account.aliases {
            if alias.has_own_smtp() && !alias.smtp_password.is_empty() {
                if let Err(e) = store_alias_smtp_password(
                    &account.email,
                    &alias.address(),
                    &alias.smtp_password,
                ) {
                    tracing::error!(
                        "could not store SMTP password for alias {} of {}: {e}",
                        alias.address(),
                        account.email
                    );
                }
            }
        }
        // OAuth refresh token (never overwrite a stored one with an empty value).
        if !account.oauth_refresh.is_empty() {
            if let Err(e) = store_oauth_refresh(&account.email, &account.oauth_refresh) {
                tracing::error!("could not store OAuth token for {}: {e}", account.email);
            }
        }
    }
    Ok(())
}

/// Rewrite the config file from the current accounts (dropping any plaintext
/// passwords still on disk). Used after migrating a legacy password.
pub fn strip_passwords_on_disk() {
    if let Some(accounts) = load() {
        if let Err(e) = write_config(&accounts) {
            tracing::warn!("could not rewrite config without passwords: {e}");
        }
    }
}

fn write_config(accounts: &[AccountConfig]) -> std::io::Result<()> {
    use std::io::{Error, ErrorKind};

    let path = path().ok_or_else(|| Error::new(ErrorKind::NotFound, "no config directory"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }

    let file = ConfigFile {
        accounts: accounts.to_vec(),
    };
    let toml =
        toml::to_string_pretty(&file).map_err(|e| Error::new(ErrorKind::InvalidData, e))?;
    write_private(&path, &toml)?;

    tracing::info!("saved {} account(s) to {}", accounts.len(), path.display());
    Ok(())
}

// ---------------------------------------------------------------------------
// Keyring (secret-service)
// ---------------------------------------------------------------------------

fn keyring_entry(key: &str) -> keyring::Result<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, key)
}

/// Keyring key for an account's separate SMTP password.
fn smtp_key(email: &str) -> String {
    format!("smtp:{email}")
}

/// Keyring key for an alias's own SMTP password (#34). The alias address is
/// lowercased so lookups can't miss on letter case.
fn alias_smtp_key(email: &str, alias_addr: &str) -> String {
    format!("smtp-alias:{email}:{}", alias_addr.to_lowercase())
}

/// Keyring key for a natively-added OAuth account's refresh token.
fn oauth_key(email: &str) -> String {
    format!("oauth:{email}")
}

pub fn store_oauth_refresh(email: &str, token: &str) -> keyring::Result<()> {
    keyring_entry(&oauth_key(email))?.set_password(token)
}

pub fn load_oauth_refresh(email: &str) -> Option<String> {
    load_key(&oauth_key(email))
}

pub fn store_password(email: &str, password: &str) -> keyring::Result<()> {
    keyring_entry(email)?.set_password(password)
}

pub fn load_password(email: &str) -> Option<String> {
    load_key(email)
}

pub fn store_smtp_password(email: &str, password: &str) -> keyring::Result<()> {
    keyring_entry(&smtp_key(email))?.set_password(password)
}

pub fn load_smtp_password(email: &str) -> Option<String> {
    load_key(&smtp_key(email))
}

pub fn store_alias_smtp_password(
    email: &str,
    alias_addr: &str,
    password: &str,
) -> keyring::Result<()> {
    keyring_entry(&alias_smtp_key(email, alias_addr))?.set_password(password)
}

pub fn load_alias_smtp_password(email: &str, alias_addr: &str) -> Option<String> {
    load_key(&alias_smtp_key(email, alias_addr))
}

pub fn delete_alias_smtp_password(email: &str, alias_addr: &str) {
    delete_key(&alias_smtp_key(email, alias_addr));
}

/// Drop every keyring entry an account owns: its password(s), OAuth token, and
/// each alias's own SMTP password. Prefer this over bare [`delete_password`]
/// whenever the account's config is still at hand — the alias entries are keyed
/// by address, which only the config knows.
pub fn delete_account_secrets(account: &AccountConfig) {
    delete_password(&account.email);
    for alias in &account.aliases {
        delete_alias_smtp_password(&account.email, &alias.address());
    }
}

fn load_key(key: &str) -> Option<String> {
    match keyring_entry(key).and_then(|e| e.get_password()) {
        Ok(password) => Some(password),
        Err(keyring::Error::NoEntry) => load_legacy_key(key),
        Err(e) => {
            tracing::warn!("could not read keyring entry for {key}: {e}");
            None
        }
    }
}

/// Keyring service name used before the 1.6.0 rename (Veem → Vireo).
const LEGACY_KEYRING_SERVICE: &str = "com.getveem.Veem";

/// Fall back to an entry stored under the pre-rename service, moving it to the
/// current service so accounts added as Veem keep working after the rename.
fn load_legacy_key(key: &str) -> Option<String> {
    let old = keyring::Entry::new(LEGACY_KEYRING_SERVICE, key).ok()?;
    let password = old.get_password().ok()?;
    if let Ok(new) = keyring_entry(key) {
        if new.set_password(&password).is_ok() {
            let _ = old.delete_credential();
        }
    }
    Some(password)
}

pub fn delete_password(email: &str) {
    delete_key(email);
    // Also drop the account's separate SMTP password and OAuth token, if any.
    delete_key(&smtp_key(email));
    delete_key(&oauth_key(email));
}

fn delete_key(key: &str) {
    if let Ok(entry) = keyring_entry(key) {
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(e) => tracing::warn!("could not delete keyring entry for {key}: {e}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Privacy settings (remote-content allowlist)
// ---------------------------------------------------------------------------

/// The app's own theme: follow the system, or force light/dark regardless.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AppTheme {
    #[default]
    System,
    Light,
    Dark,
}

/// How email message content is themed, independent of the app UI theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageTheme {
    /// Follow the system / app light-dark preference.
    #[default]
    System,
    Light,
    Dark,
}

impl MessageTheme {
    /// Forced dark flag for message content, or `None` to follow the system.
    pub fn dark_override(self) -> Option<bool> {
        match self {
            MessageTheme::System => None,
            MessageTheme::Light => Some(false),
            MessageTheme::Dark => Some(true),
        }
    }
}

/// How dates are written: the system's own arrangement, or one the user picked
/// regardless of it (#32).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DateStyle {
    /// Follow the locale: its field order, month names and separators.
    #[default]
    System,
    /// Aug 23, 2026
    MonthFirst,
    /// 23 Aug 2026
    DayFirst,
    /// 2026 Aug 23
    YearFirst,
}

/// Whether the clock runs to 12 or 24, or follows the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ClockStyle {
    #[default]
    System,
    /// 5:40 PM
    Twelve,
    /// 17:40
    TwentyFour,
}

#[derive(Debug, Deserialize, Serialize)]
struct PrivacyFile {
    #[serde(default)]
    allowed_senders: Vec<String>,
    /// Whether remote content (images, trackers) is auto-loaded for every new
    /// message, not just those from allowed senders. Off by default, since
    /// remote content can be used to track when and where a message is read.
    #[serde(default)]
    auto_remote_content: bool,
    /// Whether to load sender avatars from Gravatar (off by default — it sends
    /// a hash of each sender's email to a third party).
    #[serde(default)]
    gravatar: bool,
    /// Whether the coloured sender circles are drawn in the message list and the
    /// reader (#29 — they cost horizontal room on a small screen).
    #[serde(default = "default_avatars")]
    avatars: bool,
    /// Whether a sender's site icon may be fetched to fill their circle (#30).
    #[serde(default)]
    sender_logos: bool,
    /// How dates are written (#32).
    #[serde(default)]
    date_style: DateStyle,
    /// Whether the clock runs to 12 or 24 (#32).
    #[serde(default)]
    clock_style: ClockStyle,
    /// Seconds between automatic mail checks; 0 = manual only.
    #[serde(default = "default_fetch_interval")]
    fetch_interval_secs: u64,
    /// Whether to use IMAP IDLE push for instant new-mail delivery.
    #[serde(default = "default_push")]
    push: bool,
    /// Addresses or whole domains whose incoming mail is auto-deleted (to Trash).
    /// Stored lowercased; a bare domain like "spam.com" matches any sender there.
    #[serde(default)]
    blacklist: Vec<String>,
    /// Seconds the message-list Actions Palette stays open after the cursor
    /// leaves it before auto-collapsing. (A prior `palette_delay_ms` setting in
    /// milliseconds is intentionally not migrated — its meaning has changed.)
    #[serde(default = "default_palette_collapse")]
    palette_collapse_secs: u64,
    /// Group messages into conversation threads in the list.
    #[serde(default = "default_threading")]
    threading: bool,
    /// Whether conversation threads start expanded in the message list
    /// (collapsed to their newest message by default).
    #[serde(default)]
    threads_expanded: bool,
    /// How email content is themed (independent of the app UI theme).
    #[serde(default)]
    message_theme: MessageTheme,
    /// Whether to post desktop notifications (new mail, error alerts).
    #[serde(default = "default_notifications")]
    notifications: bool,
    /// Whether new-mail notifications name the sender and subject. On by
    /// default — that is what makes them useful — but GNOME draws notifications
    /// on the lock screen, so turning it off is worth offering.
    #[serde(default = "default_notification_content")]
    notification_content: bool,
    /// Whether the sidebar shows the "Attachments" row (the gallery of every
    /// account's attachments).
    #[serde(default = "default_show_attachments")]
    show_attachments: bool,
    /// Whether a conversation card's action icons stay hidden until the card
    /// is hovered (expanded via their ⋯ toggle). Off = always shown, unless
    /// `card_actions_auto` shows them automatically on hover.
    #[serde(default = "default_card_actions_hover")]
    card_actions_hover: bool,
    /// With the ⋯ toggle off: show the action icons automatically while the
    /// card is hovered (rather than always).
    #[serde(default = "default_card_actions_auto")]
    card_actions_auto: bool,
    /// Whether the message list's Actions Palette opens on row hover, without
    /// needing the ⋯ click.
    #[serde(default)]
    list_palette_hover: bool,
    /// Whether "New message" opens inline over the reading pane (like a
    /// reply) rather than in its own window.
    #[serde(default = "default_compose_inline")]
    compose_inline: bool,
    /// Hovering the icon rail (narrow-window or user-collapsed) floats the
    /// full sidebar out over the panes without needing the expand button.
    #[serde(default)]
    sidebar_hover_expand: bool,
    /// The app chrome's theme: follow the system, or force light/dark.
    #[serde(default)]
    app_theme: AppTheme,
    /// Lines of message text shown under the subject in the list: 0 turns the
    /// preview off entirely, and stops it being fetched.
    #[serde(default = "default_preview_lines")]
    preview_lines: u32,
    /// Single-key shortcuts (j/k, r, a, d…) without a modifier. Off by default:
    /// a stray keystroke shouldn't archive mail for someone who never asked.
    #[serde(default)]
    single_key_shortcuts: bool,
    /// Keep running after the window is closed, so new mail still arrives and
    /// notifies. Off by default: closing a window is expected to quit.
    #[serde(default)]
    run_in_background: bool,
    /// Start at login (only meaningful with `run_in_background`).
    #[serde(default)]
    autostart: bool,
    /// Whether to say anything at all when remote content is blocked. Off hides
    /// the banner; it never changes what is blocked, only whether you're told.
    #[serde(default = "default_show_remote_banner")]
    show_remote_banner: bool,
}

fn default_fetch_interval() -> u64 {
    300
}

fn default_push() -> bool {
    true
}

fn default_threading() -> bool {
    true
}

fn default_show_remote_banner() -> bool {
    true
}

fn default_palette_collapse() -> u64 {
    5
}

fn default_notification_content() -> bool {
    true
}

fn default_notifications() -> bool {
    true
}

fn default_show_attachments() -> bool {
    true
}

fn default_card_actions_hover() -> bool {
    true
}

fn default_card_actions_auto() -> bool {
    true
}

fn default_compose_inline() -> bool {
    true
}

fn default_preview_lines() -> u32 {
    1
}

fn default_avatars() -> bool {
    true
}

impl Default for PrivacyFile {
    fn default() -> Self {
        Self {
            allowed_senders: Vec::new(),
            auto_remote_content: false,
            show_remote_banner: default_show_remote_banner(),
            gravatar: false,
            avatars: default_avatars(),
            sender_logos: false,
            date_style: DateStyle::default(),
            clock_style: ClockStyle::default(),
            fetch_interval_secs: default_fetch_interval(),
            push: default_push(),
            blacklist: Vec::new(),
            palette_collapse_secs: default_palette_collapse(),
            threading: default_threading(),
            threads_expanded: false,
            message_theme: MessageTheme::default(),
            notifications: default_notifications(),
            notification_content: default_notification_content(),
            show_attachments: default_show_attachments(),
            card_actions_hover: default_card_actions_hover(),
            card_actions_auto: default_card_actions_auto(),
            list_palette_hover: false,
            compose_inline: default_compose_inline(),
            sidebar_hover_expand: false,
            app_theme: AppTheme::default(),
            preview_lines: default_preview_lines(),
            single_key_shortcuts: false,
            run_in_background: false,
            autostart: false,
        }
    }
}

fn privacy_path() -> Option<PathBuf> {
    Some(config_base()?.join("vireo").join("privacy.toml"))
}

fn load_privacy() -> PrivacyFile {
    let Some(path) = privacy_path() else {
        return PrivacyFile::default();
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return PrivacyFile::default();
    };
    toml::from_str::<PrivacyFile>(&text).unwrap_or_default()
}

/// Senders whose messages may auto-load remote content. Stored lowercased.
pub fn load_allowed_senders() -> Vec<String> {
    load_privacy().allowed_senders
}

/// Whether remote content is auto-loaded for every new message.
pub fn load_auto_remote_content() -> bool {
    load_privacy().auto_remote_content
}

pub fn load_show_remote_banner() -> bool {
    load_privacy().show_remote_banner
}

/// Whether Gravatar avatar loading is enabled.
pub fn load_gravatar() -> bool {
    load_privacy().gravatar
}

/// Whether the sender circles are shown in the list and the reader.
pub fn load_avatars() -> bool {
    load_privacy().avatars
}

/// Whether sender logos are fetched from senders' own domains.
pub fn load_sender_logos() -> bool {
    load_privacy().sender_logos
}

/// How dates are written, and on what clock.
pub fn load_date_format() -> (DateStyle, ClockStyle) {
    let p = load_privacy();
    (p.date_style, p.clock_style)
}

/// Seconds between automatic mail checks (0 = manual only).
pub fn load_fetch_interval() -> u64 {
    load_privacy().fetch_interval_secs
}

/// Whether IMAP IDLE push is enabled.
pub fn load_push() -> bool {
    load_privacy().push
}

/// Senders/domains whose incoming mail is auto-deleted. Stored lowercased.
pub fn load_blacklist() -> Vec<String> {
    load_privacy().blacklist
}

/// Seconds the message-list Actions Palette stays open after the cursor leaves it.
pub fn load_palette_collapse() -> u64 {
    load_privacy().palette_collapse_secs
}

/// Whether messages are grouped into conversation threads.
pub fn load_threading() -> bool {
    load_privacy().threading
}

/// Whether conversation threads start expanded (collapsed by default).
pub fn load_threads_expanded() -> bool {
    load_privacy().threads_expanded
}

/// How email message content is themed.
pub fn load_message_theme() -> MessageTheme {
    load_privacy().message_theme
}

/// Whether desktop notifications (new mail, error alerts) are enabled.
pub fn load_notifications() -> bool {
    load_privacy().notifications
}

/// Whether new-mail notifications may name the sender and subject.
pub fn load_notification_content() -> bool {
    load_privacy().notification_content
}

/// Whether the sidebar shows the "Attachments" row.
pub fn load_show_attachments() -> bool {
    load_privacy().show_attachments
}

/// Whether conversation card actions hide until hovered.
pub fn load_card_actions_hover() -> bool {
    load_privacy().card_actions_hover
}

/// With the ⋯ toggle off: whether card actions appear automatically on hover.
pub fn load_card_actions_auto() -> bool {
    load_privacy().card_actions_auto
}

/// Whether the list's Actions Palette opens on row hover (no ⋯ click).
pub fn load_list_palette_hover() -> bool {
    load_privacy().list_palette_hover
}

/// Whether "New message" composes inline over the reading pane.
pub fn load_compose_inline() -> bool {
    load_privacy().compose_inline
}

pub fn load_sidebar_hover_expand() -> bool {
    load_privacy().sidebar_hover_expand
}

pub fn load_app_theme() -> AppTheme {
    load_privacy().app_theme
}

/// Lines of message text shown under the subject in the list; 0 means previews
/// are off. Clamped in case the file was edited by hand.
pub fn load_preview_lines() -> u32 {
    load_privacy().preview_lines.min(3)
}

/// Whether single-key (modifier-free) shortcuts are enabled.
pub fn load_single_key_shortcuts() -> bool {
    load_privacy().single_key_shortcuts
}

/// Whether Vireo keeps running once its window is closed.
pub fn load_run_in_background() -> bool {
    load_privacy().run_in_background
}

/// Whether Vireo starts at login (background running only).
pub fn load_autostart() -> bool {
    let p = load_privacy();
    p.run_in_background && p.autostart
}

/// Persist all app settings together (so no field is clobbered).
#[allow(clippy::too_many_arguments)]
pub fn save_privacy(
    senders: &[String],
    auto_remote_content: bool,
    gravatar: bool,
    avatars: bool,
    sender_logos: bool,
    date_style: DateStyle,
    clock_style: ClockStyle,
    fetch_interval_secs: u64,
    push: bool,
    blacklist: &[String],
    palette_collapse_secs: u64,
    threading: bool,
    threads_expanded: bool,
    message_theme: MessageTheme,
    notifications: bool,
    notification_content: bool,
    show_attachments: bool,
    card_actions_hover: bool,
    card_actions_auto: bool,
    list_palette_hover: bool,
    compose_inline: bool,
    preview_lines: u32,
    single_key_shortcuts: bool,
    run_in_background: bool,
    autostart: bool,
    show_remote_banner: bool,
    sidebar_hover_expand: bool,
    app_theme: AppTheme,
) {
    let Some(path) = privacy_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let file = PrivacyFile {
        allowed_senders: senders.to_vec(),
        auto_remote_content,
        gravatar,
        avatars,
        sender_logos,
        date_style,
        clock_style,
        fetch_interval_secs,
        push,
        blacklist: blacklist.to_vec(),
        palette_collapse_secs,
        threading,
        threads_expanded,
        message_theme,
        notifications,
        notification_content,
        show_attachments,
        card_actions_hover,
        card_actions_auto,
        list_palette_hover,
        compose_inline,
        preview_lines,
        single_key_shortcuts,
        run_in_background,
        autostart,
        show_remote_banner,
        sidebar_hover_expand,
        app_theme,
    };
    match toml::to_string_pretty(&file) {
        Ok(toml) => {
            if let Err(e) = write_private(&path, &toml) {
                tracing::warn!("could not save privacy settings: {e}");
            }
        }
        Err(e) => tracing::warn!("could not serialize privacy settings: {e}"),
    }
}

// ---------------------------------------------------------------------------
// Sidebar state (account display order + collapsed accounts), keyed by email
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize, Serialize)]
struct SidebarFile {
    /// Account emails in the user's preferred display order.
    #[serde(default)]
    order: Vec<String>,
    /// Account emails whose folder list is collapsed.
    #[serde(default)]
    collapsed: Vec<String>,
    /// Account emails whose custom-folders section is expanded (default hidden).
    #[serde(default)]
    folders_expanded: Vec<String>,
    /// Whether the whole sidebar is in icon-only (collapsed) mode.
    #[serde(default)]
    icon_only: bool,
    /// Collapsed folder-tree nodes, as "email\tpath" entries.
    #[serde(default)]
    tree_collapsed: Vec<String>,
}

fn sidebar_path() -> Option<PathBuf> {
    Some(config_base()?.join("vireo").join("sidebar.toml"))
}

/// Sidebar state persisted across restarts.
#[derive(Debug, Default)]
pub struct SidebarState {
    /// Account emails in display order.
    pub order: Vec<String>,
    /// Account emails whose folder list is collapsed.
    pub collapsed: Vec<String>,
    /// Account emails whose custom-folders section is expanded (default hidden).
    pub folders_expanded: Vec<String>,
    /// Whether the sidebar is in icon-only mode.
    pub icon_only: bool,
    /// Collapsed folder-tree nodes, as "email\tpath" entries.
    pub tree_collapsed: Vec<String>,
}

pub fn load_sidebar_state() -> SidebarState {
    let Some(path) = sidebar_path() else {
        return SidebarState::default();
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return SidebarState::default();
    };
    toml::from_str::<SidebarFile>(&text)
        .map(|s| SidebarState {
            order: s.order,
            collapsed: s.collapsed,
            folders_expanded: s.folders_expanded,
            icon_only: s.icon_only,
            tree_collapsed: s.tree_collapsed,
        })
        .unwrap_or_default()
}

pub fn save_sidebar_state(state: &SidebarState) {
    let Some(path) = sidebar_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let file = SidebarFile {
        order: state.order.clone(),
        collapsed: state.collapsed.clone(),
        folders_expanded: state.folders_expanded.clone(),
        icon_only: state.icon_only,
        tree_collapsed: state.tree_collapsed.clone(),
    };
    match toml::to_string_pretty(&file) {
        Ok(toml) => {
            if let Err(e) = write_private(&path, &toml) {
                tracing::warn!("could not save sidebar state: {e}");
            }
        }
        Err(e) => tracing::warn!("could not serialize sidebar state: {e}"),
    }
}

// ---------------------------------------------------------------------------
// Window state (size + maximized). Position/monitor can't be persisted on
// Wayland — the compositor owns window placement.
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize)]
struct WindowFile {
    width: i32,
    height: i32,
    #[serde(default)]
    maximized: bool,
}

impl Default for WindowFile {
    fn default() -> Self {
        Self { width: 1280, height: 840, maximized: false }
    }
}

fn window_path() -> Option<PathBuf> {
    Some(config_base()?.join("vireo").join("window.toml"))
}

/// Returns the saved `(width, height, maximized)`, or sensible defaults.
pub fn load_window_state() -> (i32, i32, bool) {
    let file = window_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| toml::from_str::<WindowFile>(&t).ok())
        .unwrap_or_default();
    // Guard against absurd/zero sizes from a bad file.
    let width = if file.width >= 360 { file.width } else { 1280 };
    let height = if file.height >= 300 { file.height } else { 840 };
    (width, height, file.maximized)
}

pub fn save_window_state(width: i32, height: i32, maximized: bool) {
    let Some(path) = window_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let file = WindowFile { width, height, maximized };
    if let Ok(toml) = toml::to_string_pretty(&file) {
        let _ = std::fs::write(&path, toml);
    }
}

// ---------------------------------------------------------------------------
// Keyring health check + one-time setup-help flag
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize, Serialize)]
struct StateFile {
    /// Set once the user dismisses the Linux Mint keyring setup tip.
    #[serde(default)]
    mint_keyring_help_dismissed: bool,
    /// In-message attachment drawer: collapsed (showing only its header).
    #[serde(default)]
    drawer_collapsed: bool,
    /// Expanded attachment-drawer height in px (the dragged split).
    #[serde(default = "default_drawer_height")]
    drawer_height: i32,
    /// Attachment drawer shows an alphabetical list instead of the thumbnail grid.
    #[serde(default)]
    drawer_list_view: bool,
    /// The drawer's list view sorts Z→A instead of A→Z.
    #[serde(default)]
    drawer_sort_desc: bool,
    /// Attachments gallery shows a table instead of the thumbnail grid.
    #[serde(default)]
    gallery_table_view: bool,
    /// Attachments gallery thumbnail cell width in px.
    #[serde(default = "default_gallery_thumb_width")]
    gallery_thumb_width: i32,
    /// Attachments gallery sort criterion (the sort dropdown's row index).
    #[serde(default)]
    gallery_sort: u32,
    /// Message-list pane width in px (#28 — it reset every launch).
    #[serde(default = "default_list_pane_width")]
    list_pane_width: i32,
    /// Auxiliary window heights, remembering the user's vertical resizes.
    #[serde(default = "default_aux_height")]
    prefs_height: i32,
    #[serde(default = "default_aux_height")]
    accounts_height: i32,
    #[serde(default = "default_about_height")]
    about_height: i32,
}

fn default_aux_height() -> i32 {
    720
}

fn default_about_height() -> i32 {
    740
}

fn default_list_pane_width() -> i32 {
    350
}

fn default_drawer_height() -> i32 {
    160
}

fn default_gallery_thumb_width() -> i32 {
    230
}

fn state_path() -> Option<PathBuf> {
    Some(config_base()?.join("vireo").join("state.toml"))
}

fn load_state() -> StateFile {
    state_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| toml::from_str::<StateFile>(&t).ok())
        .unwrap_or_default()
}

fn save_state(state: &StateFile) {
    let Some(path) = state_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(toml) = toml::to_string_pretty(state) {
        let _ = std::fs::write(&path, toml);
    }
}

/// Whether the one-time Mint keyring setup tip has already been dismissed.
pub fn mint_keyring_help_dismissed() -> bool {
    load_state().mint_keyring_help_dismissed
}

/// Persist that the user dismissed the Mint keyring setup tip ("Don't show again").
pub fn dismiss_mint_keyring_help() {
    let mut state = load_state();
    state.mint_keyring_help_dismissed = true;
    save_state(&state);
}

/// Persisted state of the in-message attachment drawer.
#[derive(Debug, Clone, Copy)]
pub struct DrawerState {
    /// Expanded content height in px.
    pub height: i32,
    /// Whether the drawer is collapsed to just its header.
    pub collapsed: bool,
    /// Thumbnail edge in px.
    pub thumb: i32,
    /// Show an alphabetical list instead of the thumbnail grid.
    pub list_view: bool,
    /// Sort the list Z→A instead of A→Z.
    pub sort_desc: bool,
}

impl Default for DrawerState {
    fn default() -> Self {
        Self { height: 160, collapsed: false, thumb: 56, list_view: false, sort_desc: false }
    }
}

/// Load the attachment drawer's remembered state. The collapsed flag, view
/// settings and dragged height are persisted; thumbnail size always starts at
/// its default.
pub fn load_drawer_state() -> DrawerState {
    let s = load_state();
    DrawerState {
        collapsed: s.drawer_collapsed,
        list_view: s.drawer_list_view,
        sort_desc: s.drawer_sort_desc,
        height: s.drawer_height.clamp(96, 4000),
        ..DrawerState::default()
    }
}

/// Persist whether the attachment drawer is collapsed.
pub fn save_drawer_collapsed(collapsed: bool) {
    let mut s = load_state();
    s.drawer_collapsed = collapsed;
    save_state(&s);
}

/// Persist the attachment drawer's expanded (dragged) height.
pub fn save_drawer_height(height: i32) {
    let mut s = load_state();
    s.drawer_height = height.clamp(96, 4000);
    save_state(&s);
}

/// Persist the attachment drawer's view mode (list vs. thumbnail grid).
pub fn save_drawer_list_view(list_view: bool) {
    let mut s = load_state();
    s.drawer_list_view = list_view;
    save_state(&s);
}

/// Persist the attachment drawer's list sort direction.
pub fn save_drawer_sort_desc(desc: bool) {
    let mut s = load_state();
    s.drawer_sort_desc = desc;
    save_state(&s);
}

/// The attachments gallery's remembered view settings:
/// (table view, thumbnail width px, sort index).
pub fn load_gallery_view() -> (bool, i32, u32) {
    let s = load_state();
    (
        s.gallery_table_view,
        s.gallery_thumb_width.clamp(140, 380),
        s.gallery_sort,
    )
}

/// Persist whether the attachments gallery shows the table view.
pub fn save_gallery_table_view(table: bool) {
    let mut s = load_state();
    s.gallery_table_view = table;
    save_state(&s);
}

/// Persist the attachments gallery's thumbnail width.
pub fn save_gallery_thumb_width(width: i32) {
    let mut s = load_state();
    s.gallery_thumb_width = width;
    save_state(&s);
}

/// Persist the attachments gallery's sort criterion (dropdown row index).
pub fn save_gallery_sort(sort: u32) {
    let mut s = load_state();
    s.gallery_sort = sort;
    save_state(&s);
}

/// Auxiliary window heights (Preferences / Accounts / About): they open tall
/// by default and remember the user's own vertical resize across restarts.
pub fn load_prefs_height() -> i32 {
    load_state().prefs_height.clamp(400, 4000)
}

pub fn save_prefs_height(height: i32) {
    let mut s = load_state();
    s.prefs_height = height.clamp(400, 4000);
    save_state(&s);
}

pub fn load_accounts_height() -> i32 {
    load_state().accounts_height.clamp(400, 4000)
}

pub fn save_accounts_height(height: i32) {
    let mut s = load_state();
    s.accounts_height = height.clamp(400, 4000);
    save_state(&s);
}

pub fn load_about_height() -> i32 {
    load_state().about_height.clamp(400, 4000)
}

pub fn save_about_height(height: i32) {
    let mut s = load_state();
    s.about_height = height.clamp(400, 4000);
    save_state(&s);
}

/// The message-list pane's remembered width (clamped to something sane).
pub fn load_list_pane_width() -> i32 {
    load_state().list_pane_width.clamp(324, 4000)
}

/// Persist the message-list pane's width (#28).
pub fn save_list_pane_width(width: i32) {
    let mut s = load_state();
    s.list_pane_width = width.clamp(324, 4000);
    save_state(&s);
}


#[cfg(test)]
mod tests {
    use super::{ConfigFile, PrivacyFile};

    #[test]
    fn plain_string_aliases_still_load() {
        // The pre-per-alias-SMTP format (#34): a bare array of identity strings.
        let cfg: ConfigFile = toml::from_str(
            r#"
            [[accounts]]
            name = "Ann"
            email = "ann@example.org"
            imap_host = "imap.example.org"
            username = "ann"
            aliases = ["Ann Work <ann@work.org>", "ann@shop.org"]
            "#,
        )
        .unwrap();
        let aliases = &cfg.accounts[0].aliases;
        assert_eq!(aliases.len(), 2);
        assert_eq!(aliases[0].identity, "Ann Work <ann@work.org>");
        assert_eq!(aliases[0].address(), "ann@work.org");
        assert!(!aliases[0].has_own_smtp(), "a plain alias rides the account's SMTP");
        assert_eq!(aliases[1].address(), "ann@shop.org");
    }

    #[test]
    fn aliases_can_carry_their_own_smtp() {
        let cfg: ConfigFile = toml::from_str(
            r#"
            [[accounts]]
            name = "Ann"
            email = "ann@example.org"
            imap_host = "imap.example.org"
            username = "ann"

            [[accounts.aliases]]
            identity = "Ann Work <ann@work.org>"
            smtp_host = "smtp.work.org"
            smtp_port = 465
            smtp_username = "ann@work.org"
            "#,
        )
        .unwrap();
        let alias = &cfg.accounts[0].aliases[0];
        assert!(alias.has_own_smtp());
        assert_eq!(alias.smtp_host, "smtp.work.org");
        assert_eq!(alias.smtp_port, 465);
        assert_eq!(alias.smtp_username, "ann@work.org");
        assert!(alias.smtp_password.is_empty(), "passwords live in the keyring");
    }

    #[test]
    fn alias_smtp_password_never_reaches_disk() {
        let mut cfg: ConfigFile = toml::from_str(
            r#"
            [[accounts]]
            name = "Ann"
            email = "ann@example.org"
            imap_host = "imap.example.org"
            username = "ann"

            [[accounts.aliases]]
            identity = "ann@work.org"
            smtp_host = "smtp.work.org"
            smtp_username = "ann"
            "#,
        )
        .unwrap();
        cfg.accounts[0].aliases[0].smtp_password = "hunter2".into();
        let out = toml::to_string_pretty(&cfg).unwrap();
        assert!(!out.contains("hunter2"), "password serialized to disk: {out}");
        // And the round trip keeps the alias's transport settings.
        let back: ConfigFile = toml::from_str(&out).unwrap();
        let alias = &back.accounts[0].aliases[0];
        assert_eq!(alias.smtp_host, "smtp.work.org");
        assert_eq!(alias.smtp_port, 587, "unwritten port falls back to the default");
    }

    #[test]
    fn preview_lines_default_to_one_and_stay_in_range() {
        // An older privacy.toml has no key at all.
        let p: PrivacyFile = toml::from_str("").unwrap();
        assert_eq!(p.preview_lines, 1);
        // Hand-edited nonsense must not make the list build rows of 40 lines, or
        // of none: the setting offers 1–3 and that is what it is worth honouring.
        // 0 is a real setting — previews off — but nothing above 3 is.
        for (written, expected) in [(0, 0), (1, 1), (3, 3), (99, 3)] {
            let p: PrivacyFile =
                toml::from_str(&format!("preview_lines = {written}")).unwrap();
            assert_eq!(p.preview_lines.min(3), expected, "for {written}");
        }
    }

    #[test]
    fn notifications_default_on_when_absent() {
        // An older privacy.toml with no `notifications` key opts in by default.
        let p: PrivacyFile = toml::from_str("").unwrap();
        assert!(p.notifications);
    }

    #[test]
    fn notifications_can_be_disabled() {
        let p: PrivacyFile = toml::from_str("notifications = false").unwrap();
        assert!(!p.notifications);
    }
}
