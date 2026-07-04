//! Native OAuth2 (XOAUTH2) sign-in for accounts added directly in Veem.
//!
//! Runs the authorization-code flow with PKCE against the provider: opens the
//! system browser, captures the redirect on a loopback socket, and exchanges the
//! code for a refresh token. The refresh token is kept in the keyring; a fresh
//! access token is minted from it at connect time.
//!
//! Microsoft uses Veem's built-in OAuth client; Google's client is bundled into
//! official builds at compile time (otherwise use GNOME Online Accounts, or your
//! own client in `~/.config/veem/oauth.toml`). Advanced users can override any of
//! it or point "Custom OAuth" at another provider — see `provider_credentials`.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::config::OAuthSettings;

// Built-in OAuth client credentials.
//
// Microsoft is a public client (PKCE, no secret), and a client ID is just a
// public identifier, so it's embedded directly for one-click sign-in in every
// build.
//
// Google's client is deliberately kept OUT of the public source — GitHub push
// protection and Google's own secret scanning flag it, and Google may auto-revoke
// an exposed secret. Instead it's read at COMPILE TIME from env vars, so official
// / Flathub builds bundle Veem's Google app by setting `VEEM_GOOGLE_CLIENT_ID`
// and `VEEM_GOOGLE_CLIENT_SECRET` during the build, while a plain `cargo build`
// ships empty (Google sign-in then works via GNOME Online Accounts or a client
// the user supplies). Runtime overrides — `~/.config/veem/oauth.toml` or `VEEM_*`
// env vars at runtime — still take precedence; see `provider_credentials`.
const GOOGLE_CLIENT_ID: &str = match option_env!("VEEM_GOOGLE_CLIENT_ID") {
    Some(v) => v,
    None => "",
};
const GOOGLE_CLIENT_SECRET: &str = match option_env!("VEEM_GOOGLE_CLIENT_SECRET") {
    Some(v) => v,
    None => "",
};
const MICROSOFT_CLIENT_ID: &str = "47579d63-4785-4131-98bb-7f2a2a1a2c59";
const MICROSOFT_CLIENT_SECRET: &str = "";

/// The Veem app icon, embedded so the success page needs no external resources.
const ICON_PNG: &[u8] = include_bytes!("../data/icons/hicolor/256x256/apps/dev.veem.Veem.png");

/// Branded sign-in success page. `__ICON__` is replaced with the app icon (as a
/// data URI) at runtime by [`success_page`]. Self-contained (inline CSS/SVG,
/// system fonts) so it renders offline and adapts to light/dark.
const SUCCESS_TEMPLATE: &str = r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Veem — Signed in</title>
<style>
  :root { color-scheme: light dark; --bg1:#0b1220; --bg2:#0e1526; --glow:rgba(53,132,228,.28);
          --card:rgba(255,255,255,.045); --stroke:rgba(255,255,255,.09); --shadow:rgba(0,0,0,.5);
          --fg:#eef2f8; --muted:#9fb0cc; --faint:#7688a6; }
  @media (prefers-color-scheme: light) {
    :root { --bg1:#f4f7fc; --bg2:#e9eef7; --glow:rgba(53,132,228,.16);
            --card:rgba(255,255,255,.82); --stroke:rgba(24,38,64,.08); --shadow:rgba(40,66,110,.16);
            --fg:#182338; --muted:#546482; --faint:#7a89a6; }
  }
  * { box-sizing:border-box; margin:0; padding:0; }
  html,body { height:100%; }
  body {
    display:grid; place-items:center; padding:24px;
    font-family:"Cantarell","Inter",-apple-system,system-ui,"Segoe UI",sans-serif;
    color:var(--fg);
    background:
      radial-gradient(1000px 560px at 50% -12%, var(--glow), transparent 70%),
      linear-gradient(155deg, var(--bg1), var(--bg2));
  }
  .card {
    width:min(440px, 92vw); text-align:center;
    padding:44px 38px 34px; border-radius:24px;
    background:var(--card); border:1px solid var(--stroke);
    box-shadow:0 30px 80px var(--shadow);
    -webkit-backdrop-filter:blur(22px); backdrop-filter:blur(22px);
    animation:rise .5s cubic-bezier(.2,.8,.2,1) both;
  }
  @keyframes rise { from { opacity:0; transform:translateY(14px) scale(.98); } }
  .hero { position:relative; width:92px; margin:0 auto 22px;
          animation:pop .5s .12s cubic-bezier(.2,1.4,.4,1) both; }
  .hero img { width:92px; height:92px; border-radius:22px; display:block;
              box-shadow:0 16px 40px rgba(0,0,0,.4); }
  @keyframes pop { from { transform:scale(.4); opacity:0; } }
  .check { position:absolute; right:-5px; bottom:-5px; width:33px; height:33px; border-radius:50%;
           display:grid; place-items:center;
           background:linear-gradient(135deg,#34c759,#2ba24b);
           box-shadow:0 6px 16px rgba(40,167,69,.45), 0 0 0 4px var(--card); }
  .check svg { width:17px; height:17px; }
  .check svg path { stroke-dasharray:30; stroke-dashoffset:30; animation:draw .45s .45s ease forwards; }
  @keyframes draw { to { stroke-dashoffset:0; } }
  .brand { font-size:19px; font-weight:800; letter-spacing:-.005em;
           color:#4f9bff; margin-bottom:6px; }
  h1 { font-size:25px; font-weight:800; letter-spacing:-.01em; margin-bottom:10px; }
  p  { color:var(--muted); font-size:15px; line-height:1.6; }
  .hint { margin-top:26px; font-size:12.5px; color:var(--faint); }
</style>
</head>
<body>
  <main class="card">
    <div class="hero">
      <img src="data:image/png;base64,__ICON__" alt="Veem">
      <span class="check">
        <svg viewBox="0 0 24 24" fill="none" stroke="#fff" stroke-width="3.2"
             stroke-linecap="round" stroke-linejoin="round"><path d="M5 12.5l4.2 4.3L19 6.8"/></svg>
      </span>
    </div>
    <div class="brand">Veem</div>
    <h1>You&rsquo;re signed in</h1>
    <p>Your account is connected. You can close this tab and head back to Veem.</p>
    <div class="hint">It&rsquo;s safe to close this window.</div>
  </main>
</body>
</html>"##;

/// Build the success page with the app icon embedded as a data URI.
fn success_page() -> String {
    SUCCESS_TEMPLATE.replace("__ICON__", &base64_encode(ICON_PNG))
}

/// Standard base64 encoding (no dependency), for the inline icon data URI.
fn base64_encode(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 { T[((n >> 6) & 63) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[(n & 63) as usize] as char } else { '=' });
    }
    out
}

/// Server presets for a known provider (endpoints + IMAP/SMTP hosts). Client
/// credentials come from [`provider_credentials`], not here.
pub struct Preset {
    pub auth_url: &'static str,
    pub token_url: &'static str,
    pub scopes: &'static str,
    pub imap_host: &'static str,
    pub imap_port: u16,
    pub smtp_host: &'static str,
    pub smtp_port: u16,
}

/// Preset for a provider key ("google" / "microsoft"), or `None` for custom.
pub fn preset(provider: &str) -> Option<Preset> {
    match provider {
        "google" => Some(Preset {
            auth_url: "https://accounts.google.com/o/oauth2/v2/auth",
            token_url: "https://oauth2.googleapis.com/token",
            scopes: "https://mail.google.com/",
            imap_host: "imap.gmail.com",
            imap_port: 993,
            smtp_host: "smtp.gmail.com",
            smtp_port: 465,
        }),
        "microsoft" => Some(Preset {
            auth_url: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
            token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token",
            scopes: "https://outlook.office.com/IMAP.AccessAsUser.All \
                     https://outlook.office.com/SMTP.Send offline_access",
            imap_host: "outlook.office365.com",
            imap_port: 993,
            smtp_host: "smtp.office365.com",
            smtp_port: 587,
        }),
        _ => None,
    }
}

/// Resolve a provider's OAuth client credentials `(client_id, client_secret)`,
/// preferring the user's own over the built-in fallback. Order:
///   1. Environment: `VEEM_GOOGLE_CLIENT_ID` / `VEEM_GOOGLE_CLIENT_SECRET`
///      (or `VEEM_MICROSOFT_*`).
///   2. `~/.config/veem/oauth.toml` — `[google]` / `[microsoft]` with
///      `client_id` and `client_secret`. Keeps secrets out of the source repo.
///   3. The built-in Thunderbird fallback.
///
/// A user override only takes effect once complete (both id and secret), so a
/// half-configured `oauth.toml` (client_id but no secret yet) keeps working on the
/// fallback instead of breaking sign-in.
pub fn provider_credentials(provider: &str) -> (String, String) {
    // `secret_required`: Google desktop clients always have a secret and its token
    // endpoint demands it, so the override only counts as complete with both. Azure
    // desktop apps are public clients (PKCE, no secret), so a client_id alone is a
    // valid override there.
    let (env_id, env_secret, default_id, default_secret, secret_required) = match provider {
        "google" => (
            "VEEM_GOOGLE_CLIENT_ID",
            "VEEM_GOOGLE_CLIENT_SECRET",
            GOOGLE_CLIENT_ID,
            GOOGLE_CLIENT_SECRET,
            true,
        ),
        "microsoft" => (
            "VEEM_MICROSOFT_CLIENT_ID",
            "VEEM_MICROSOFT_CLIENT_SECRET",
            MICROSOFT_CLIENT_ID,
            MICROSOFT_CLIENT_SECRET,
            false,
        ),
        _ => ("", "", "", "", true),
    };

    let usable = |id: String, secret: String| -> Option<(String, String)> {
        let ok = !id.trim().is_empty() && (!secret_required || !secret.trim().is_empty());
        ok.then_some((id, secret))
    };

    if let Some(c) = usable(
        std::env::var(env_id).unwrap_or_default(),
        std::env::var(env_secret).unwrap_or_default(),
    ) {
        return c;
    }
    if let Some((id, secret)) = creds_from_file(provider) {
        if let Some(c) = usable(id, secret) {
            return c;
        }
    }
    (default_id.to_string(), default_secret.to_string())
}

#[derive(Deserialize, Default)]
struct OAuthFile {
    #[serde(default)]
    google: Option<FileCreds>,
    #[serde(default)]
    microsoft: Option<FileCreds>,
}

#[derive(Deserialize, Default)]
struct FileCreds {
    #[serde(default)]
    client_id: String,
    #[serde(default)]
    client_secret: String,
}

fn creds_from_file(provider: &str) -> Option<(String, String)> {
    let path = dirs::config_dir()?.join("veem").join("oauth.toml");
    let text = std::fs::read_to_string(path).ok()?;
    let file: OAuthFile = toml::from_str(&text).ok()?;
    let creds = match provider {
        "google" => file.google,
        "microsoft" => file.microsoft,
        _ => None,
    }?;
    Some((creds.client_id, creds.client_secret))
}

#[derive(Deserialize)]
struct TokenResponse {
    #[serde(default)]
    access_token: String,
    #[serde(default)]
    refresh_token: String,
}

/// Result of a completed sign-in (the refresh token to persist).
pub struct FlowResult {
    pub refresh_token: String,
}

/// Run the interactive authorization-code + PKCE flow (blocking — call off the
/// UI thread). Opens the browser and waits for the loopback redirect.
pub fn run_flow(settings: &OAuthSettings) -> Result<FlowResult, String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    // Microsoft (Entra) only ignores the port when matching *localhost* loopback
    // redirects — a random-port 127.0.0.1 URI would need the exact port registered,
    // which we can't do. Google (and others) accept the 127.0.0.1 literal. The
    // listener is on 127.0.0.1 either way; browsers resolve localhost to it.
    let host = if settings.token_url.contains("microsoftonline") {
        "localhost"
    } else {
        "127.0.0.1"
    };
    let redirect = format!("http://{host}:{port}/");

    // PKCE "plain" (no crypto dep): challenge == verifier.
    let verifier = random_token(64);
    let state = random_token(24);
    let auth_url = format!(
        "{base}?response_type=code&client_id={cid}&redirect_uri={redir}&scope={scope}\
         &code_challenge={chal}&code_challenge_method=plain&state={state}\
         &access_type=offline&prompt=consent",
        base = settings.auth_url,
        cid = pct(&settings.client_id),
        redir = pct(&redirect),
        scope = pct(&settings.scopes),
        chal = pct(&verifier),
        state = pct(&state),
    );

    // Open the system browser (Linux/GNOME).
    let _ = std::process::Command::new("xdg-open").arg(&auth_url).spawn();

    // Wait for the redirect (with a timeout so a cancelled sign-in doesn't hang).
    let code = wait_for_code(&listener, &state)?;

    // Exchange the code for tokens.
    let mut form: Vec<(&str, &str)> = vec![
        ("grant_type", "authorization_code"),
        ("code", &code),
        ("redirect_uri", &redirect),
        ("client_id", &settings.client_id),
        ("code_verifier", &verifier),
    ];
    if !settings.client_secret.is_empty() {
        form.push(("client_secret", &settings.client_secret));
    }
    let token: TokenResponse = ureq::post(&settings.token_url)
        .send_form(&form)
        .map_err(|e| e.to_string())?
        .into_json()
        .map_err(|e| e.to_string())?;

    if token.refresh_token.is_empty() {
        return Err("the provider did not return a refresh token".into());
    }
    Ok(FlowResult {
        refresh_token: token.refresh_token,
    })
}

/// Mint a fresh access token from a stored refresh token (blocking).
pub fn refresh_access_token(settings: &OAuthSettings, refresh_token: &str) -> Result<String, String> {
    let mut form: Vec<(&str, &str)> = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", &settings.client_id),
    ];
    if !settings.client_secret.is_empty() {
        form.push(("client_secret", &settings.client_secret));
    }
    let token: TokenResponse = ureq::post(&settings.token_url)
        .send_form(&form)
        .map_err(|e| e.to_string())?
        .into_json()
        .map_err(|e| e.to_string())?;
    if token.access_token.is_empty() {
        return Err("no access token in refresh response".into());
    }
    Ok(token.access_token)
}

/// Accept the browser redirect and return the authorization code, validating the
/// anti-CSRF state. Times out after 5 minutes.
fn wait_for_code(listener: &TcpListener, expected_state: &str) -> Result<String, String> {
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(300);
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_nonblocking(false).ok();
                let mut buf = [0u8; 8192];
                let n = stream.read(&mut buf).map_err(|e| e.to_string())?;
                let req = String::from_utf8_lossy(&buf[..n]);
                let line = req.lines().next().unwrap_or("");
                let (code, state) = parse_redirect(line);

                let body = success_page();
                let _ = stream.write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\
                         Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                    .as_bytes(),
                );

                if state.as_deref() != Some(expected_state) {
                    return Err("sign-in state mismatch (possible CSRF)".into());
                }
                return code.ok_or_else(|| "no authorization code returned".to_string());
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() > deadline {
                    return Err("sign-in timed out".into());
                }
                std::thread::sleep(Duration::from_millis(150));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// Pull `code` and `state` out of an HTTP request line ("GET /?code=…&state=… HTTP/1.1").
fn parse_redirect(request_line: &str) -> (Option<String>, Option<String>) {
    let target = request_line.split_whitespace().nth(1).unwrap_or("");
    let query = target.split_once('?').map(|(_, q)| q).unwrap_or("");
    let mut code = None;
    let mut state = None;
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            let value = pct_decode(v);
            match k {
                "code" => code = Some(value),
                "state" => state = Some(value),
                _ => {}
            }
        }
    }
    (code, state)
}

const UNRESERVED: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";

/// A random URL-safe token of `len` unreserved characters (from `/dev/urandom`).
fn random_token(len: usize) -> String {
    let mut bytes = vec![0u8; len];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = f.read_exact(&mut bytes);
    }
    bytes
        .iter()
        .map(|b| UNRESERVED[(*b as usize) % UNRESERVED.len()] as char)
        .collect()
}

/// Percent-encode a query value (encode everything but the unreserved set).
fn pct(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if UNRESERVED.contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Minimal percent-decode for redirect query values.
fn pct_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hi = (bytes[i + 1] as char).to_digit(16);
                let lo = (bytes[i + 2] as char).to_digit(16);
                if let (Some(h), Some(l)) = (hi, lo) {
                    out.push((h * 16 + l) as u8);
                    i += 3;
                    continue;
                }
                out.push(b'%');
                i += 1;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}




