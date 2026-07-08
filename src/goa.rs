//! GNOME Online Accounts (GOA) integration.
//!
//! Reads mail-capable accounts configured in GNOME Settings → Online Accounts via
//! the `org.gnome.OnlineAccounts` D-Bus service (session bus), so the user can
//! enable them in Veem without re-entering server settings. Password-based
//! providers (generic IMAP/SMTP) have their password retrieved from GOA and stored
//! in Veem's keyring on import; OAuth2 providers (Gmail, Microsoft) authenticate
//! with a GOA-issued access token (XOAUTH2) fetched fresh at connect time.

use std::collections::HashMap;

use zbus::zvariant::{OwnedObjectPath, OwnedValue};

use crate::config::{AccountConfig, Protocol};

const GOA_DEST: &str = "org.gnome.OnlineAccounts";
const GOA_PATH: &str = "/org/gnome/OnlineAccounts";
const IFACE_ACCOUNT: &str = "org.gnome.OnlineAccounts.Account";
const IFACE_MAIL: &str = "org.gnome.OnlineAccounts.Mail";
const IFACE_PASSWORD: &str = "org.gnome.OnlineAccounts.PasswordBased";
const IFACE_OAUTH2: &str = "org.gnome.OnlineAccounts.OAuth2Based";

/// A mail account discovered in GNOME Online Accounts.
#[derive(Debug, Clone)]
pub struct GoaMailAccount {
    /// GOA account id (e.g. "account_1699…").
    pub id: String,
    /// D-Bus object path of the account.
    pub path: String,
    pub email: String,
    pub name: String,
    pub provider: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub imap_user: String,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_user: String,
    pub smtp_separate: bool,
    /// Whether GOA can hand us a password (generic IMAP).
    pub password_based: bool,
    /// Whether the provider uses OAuth2 — imported accounts authenticate with a
    /// GOA-issued token (XOAUTH2) fetched fresh at connect time.
    pub oauth2: bool,
}

impl GoaMailAccount {
    /// Turn a discovered GOA account into a Veem [`AccountConfig`]. Pass the
    /// password for password-based providers, or `oauth = true` for OAuth ones
    /// (the token is fetched from GOA at connect time).
    pub fn to_config(&self, password: String, oauth: bool) -> AccountConfig {
        AccountConfig {
            name: if self.name.trim().is_empty() {
                self.email.clone()
            } else {
                self.name.clone()
            },
            email: self.email.clone(),
            protocol: Protocol::Imap,
            imap_host: self.imap_host.clone(),
            imap_port: self.imap_port,
            smtp_host: self.smtp_host.clone(),
            smtp_port: self.smtp_port,
            username: if self.imap_user.is_empty() {
                self.email.clone()
            } else {
                self.imap_user.clone()
            },
            password,
            smtp_separate: self.smtp_separate,
            smtp_username: self.smtp_user.clone(),
            smtp_password: String::new(),
            color: None,
            emoji: None,
            signature: None,
            signature_html: false,
            label: None,
            enabled: true,
            goa_id: Some(self.id.clone()),
            oauth,
            oauth_settings: None,
            oauth_refresh: String::new(),
        }
    }
}

fn get_str(map: &HashMap<String, OwnedValue>, key: &str) -> String {
    map.get(key)
        .and_then(|v| <&str>::try_from(v).ok())
        .unwrap_or_default()
        .to_string()
}

fn get_bool(map: &HashMap<String, OwnedValue>, key: &str) -> bool {
    map.get(key).and_then(|v| bool::try_from(v).ok()).unwrap_or(false)
}

type ManagedObjects =
    HashMap<OwnedObjectPath, HashMap<String, HashMap<String, OwnedValue>>>;

/// List mail-capable GNOME Online Accounts. Returns an empty list if GOA isn't
/// running or has no mail accounts — never errors into the UI.
pub fn list_mail_accounts() -> Vec<GoaMailAccount> {
    match try_list() {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!("GOA discovery skipped: {e}");
            Vec::new()
        }
    }
}

fn try_list() -> Result<Vec<GoaMailAccount>, String> {
    let conn = zbus::blocking::Connection::session().map_err(|e| e.to_string())?;
    let reply = conn
        .call_method(
            Some(GOA_DEST),
            GOA_PATH,
            Some("org.freedesktop.DBus.ObjectManager"),
            "GetManagedObjects",
            &(),
        )
        .map_err(|e| e.to_string())?;
    let (objects,): (ManagedObjects,) = reply.body().deserialize().map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    for (path, ifaces) in objects.iter() {
        let (Some(account), Some(mail)) =
            (ifaces.get(IFACE_ACCOUNT), ifaces.get(IFACE_MAIL))
        else {
            continue;
        };
        // Skip accounts whose Mail service is turned off in GNOME Settings.
        if get_bool(account, "MailDisabled") {
            continue;
        }

        let imap_ssl = get_bool(mail, "ImapUseSsl");
        let smtp_ssl = get_bool(mail, "SmtpUseSsl");
        let smtp_tls = get_bool(mail, "SmtpUseTls");
        let email = {
            let e = get_str(mail, "EmailAddress");
            if e.is_empty() {
                get_str(account, "PresentationIdentity")
            } else {
                e
            }
        };
        if email.is_empty() {
            continue;
        }
        let imap_user = get_str(mail, "ImapUserName");
        let smtp_user = get_str(mail, "SmtpUserName");

        out.push(GoaMailAccount {
            id: get_str(account, "Id"),
            path: path.as_str().to_string(),
            email,
            name: {
                let n = get_str(mail, "Name");
                if n.is_empty() {
                    get_str(account, "PresentationIdentity")
                } else {
                    n
                }
            },
            provider: get_str(account, "ProviderName"),
            imap_host: get_str(mail, "ImapHost"),
            imap_port: if imap_ssl { 993 } else { 143 },
            imap_user: imap_user.clone(),
            smtp_host: get_str(mail, "SmtpHost"),
            smtp_port: if smtp_ssl {
                465
            } else if smtp_tls {
                587
            } else {
                587
            },
            smtp_user: smtp_user.clone(),
            smtp_separate: !smtp_user.is_empty() && smtp_user != imap_user,
            password_based: ifaces.contains_key(IFACE_PASSWORD),
            oauth2: ifaces.contains_key(IFACE_OAUTH2),
        });
    }
    Ok(out)
}

/// Live set of ids for accounts that currently exist in GNOME Online Accounts
/// (any account object, mail-capable or not — so merely disabling Mail doesn't
/// count as removal). Returns `None` when GOA / the session bus can't be reached —
/// callers MUST treat that as "unknown", not "no accounts", or they'd wrongly
/// prune every imported GOA account whenever GOA is momentarily unavailable.
pub fn live_account_ids() -> Option<std::collections::HashSet<String>> {
    let conn = zbus::blocking::Connection::session().ok()?;
    let reply = conn
        .call_method(
            Some(GOA_DEST),
            GOA_PATH,
            Some("org.freedesktop.DBus.ObjectManager"),
            "GetManagedObjects",
            &(),
        )
        .ok()?;
    let (objects,): (ManagedObjects,) = reply.body().deserialize().ok()?;
    Some(
        objects
            .values()
            .filter_map(|ifaces| ifaces.get(IFACE_ACCOUNT))
            .map(|account| get_str(account, "Id"))
            .filter(|id| !id.is_empty())
            .collect(),
    )
}

/// Watch GNOME Online Accounts for account/interface removals, invoking
/// `on_change` on each. Runs on a dedicated thread; silently no-ops if GOA or the
/// session bus is unavailable. Lets Veem prune accounts removed in GNOME Settings
/// without a restart.
pub fn watch_removals<F: Fn() + Send + 'static>(on_change: F) {
    let _ = std::thread::Builder::new()
        .name("goa-watch".into())
        .spawn(move || {
            if let Err(e) = watch_loop(&on_change) {
                tracing::debug!("GOA watch stopped: {e}");
            }
        });
}

fn watch_loop<F: Fn()>(on_change: &F) -> Result<(), Box<dyn std::error::Error>> {
    let conn = zbus::blocking::Connection::session()?;
    let om = zbus::blocking::fdo::ObjectManagerProxy::builder(&conn)
        .destination(GOA_DEST)?
        .path(GOA_PATH)?
        .build()?;
    // Blocks until GOA emits an InterfacesRemoved signal; ends if the bus closes.
    let mut removed = om.receive_interfaces_removed()?;
    for _ in removed.by_ref() {
        on_change();
    }
    Ok(())
}

/// Fetch a fresh OAuth2 access token for a GOA account (by id). GOA refreshes the
/// token as needed, so this always returns a currently-valid token. Blocking.
pub fn oauth_token(goa_id: &str) -> Option<String> {
    let conn = zbus::blocking::Connection::session().ok()?;
    let path = format!("/org/gnome/OnlineAccounts/Accounts/{goa_id}");
    let reply = conn
        .call_method(
            Some(GOA_DEST),
            path.as_str(),
            Some(IFACE_OAUTH2),
            "GetAccessToken",
            &(),
        )
        .ok()?;
    // GetAccessToken() -> (access_token: s, expires_in: i)
    let (token, _expires): (String, i32) = reply.body().deserialize().ok()?;
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

/// Fetch a GOA account's mail password (password-based providers only).
pub fn fetch_password(path: &str, goa_id: &str) -> Option<String> {
    let conn = zbus::blocking::Connection::session().ok()?;
    // GOA's GetPassword takes an id argument; the account id is the usual key.
    let reply = conn
        .call_method(
            Some(GOA_DEST),
            path,
            Some(IFACE_PASSWORD),
            "GetPassword",
            &(goa_id,),
        )
        .ok()?;
    let (password,): (String,) = reply.body().deserialize().ok()?;
    if password.is_empty() {
        None
    } else {
        Some(password)
    }
}


