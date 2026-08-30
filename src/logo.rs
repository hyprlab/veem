//! Sender logos (opt-in): the brand's own site icon in place of coloured
//! initials, so mail from Apple, Amazon or PayPal is recognisable at a glance
//! (#30).
//!
//! The icon comes from the sender's own domain, in descending order of quality:
//! `apple-touch-icon.png` (180px, and what a site publishes when it cares how it
//! looks as an icon), then `favicon.ico`. No third-party service is involved and
//! no per-user identifier is sent, but the request does tell that domain your IP
//! address — which is exactly what blocking remote content avoids. So this is off
//! by default and gated behind a Preferences switch, as Gravatar is.
//!
//! One fetch per domain per session, remembered either way: a miss is cached too,
//! or every row from the same sender would ask again.
//!
//! Icons persist on disk (`~/.local/share/vireo/logos/<domain>.img`) so a
//! restart shows them without touching the network; a `.miss` marker remembers
//! a domain with nothing to give. Both go stale after a week: the next message
//! from that sender then re-asks the domain — a changed brand icon appears, a
//! domain that gained one is picked up — while the stale icon keeps showing in
//! the meantime.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

thread_local! {
    static CACHE: RefCell<HashMap<String, gtk::gdk::Texture>> = RefCell::new(HashMap::new());
    /// Domains with no usable icon, so they are asked once and not again.
    static MISSES: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    /// Domains whose weekly refresh has already been kicked off this session.
    static REFRESHED: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

/// How long a stored icon (or miss) is trusted before the domain is re-asked.
const REFRESH_AFTER: std::time::Duration = std::time::Duration::from_secs(7 * 24 * 60 * 60);

fn store_dir() -> Option<PathBuf> {
    let dir = crate::config::data_base()?.join("vireo").join("logos");
    let _ = std::fs::create_dir_all(&dir);
    Some(dir)
}

fn img_path(domain: &str) -> Option<PathBuf> {
    Some(store_dir()?.join(format!("{domain}.img")))
}

fn miss_path(domain: &str) -> Option<PathBuf> {
    Some(store_dir()?.join(format!("{domain}.miss")))
}

/// Whether the file at `path` exists and was written within [`REFRESH_AFTER`].
fn fresh(path: &PathBuf) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age < REFRESH_AFTER)
}

/// The registrable domain of an email address: the part that owns the brand.
///
/// Mail comes from `em6917.cloudways.com` or `mail.notifications.apple.com`, and
/// the icon lives at the domain those hang off. Two labels, or three when the
/// last two are a country-code pair like `co.uk`, which is a heuristic rather
/// than the public suffix list — close enough to point a favicon request at, and
/// wrong only for a handful of unusual suffixes.
pub fn domain_of(email: &str) -> Option<String> {
    let host = email.rsplit('@').next()?.trim().trim_end_matches('.');
    let host = host.to_ascii_lowercase();
    if host.is_empty() || !host.contains('.') || host.parse::<std::net::IpAddr>().is_ok() {
        return None;
    }
    let labels: Vec<&str> = host.split('.').filter(|l| !l.is_empty()).collect();
    if labels.len() < 2 {
        return None;
    }
    let tail = &labels[labels.len().saturating_sub(2)..];
    let take = if labels.len() > 2 && tail[0].len() <= 3 && tail[1].len() == 2 {
        3
    } else {
        2
    };
    Some(labels[labels.len() - take.min(labels.len())..].join("."))
}

/// A previously decoded logo for this sender's domain (main thread only).
/// Falls back to the on-disk copy — however old — so a restart shows icons
/// without a single network request; [`wants_refresh`] handles staleness.
pub fn cached(email: &str) -> Option<gtk::gdk::Texture> {
    let domain = domain_of(email)?;
    if let Some(tex) = CACHE.with(|c| c.borrow().get(&domain).cloned()) {
        return Some(tex);
    }
    let bytes = img_path(&domain).and_then(|p| std::fs::read(p).ok())?;
    let tex = decode(&bytes)?;
    CACHE.with(|c| {
        c.borrow_mut().insert(domain, tex.clone());
    });
    Some(tex)
}

/// Whether the stored icon for this sender's domain is a week old — time to
/// look again in the background while the old one keeps showing. Says yes at
/// most once a session per domain, so a screenful of rows from one sender
/// doesn't fan out into a fetch per row.
pub fn wants_refresh(email: &str) -> bool {
    let Some(domain) = domain_of(email) else {
        return false;
    };
    let due = img_path(&domain).is_some_and(|p| p.exists() && !fresh(&p));
    due && REFRESHED.with(|r| r.borrow_mut().insert(domain))
}

/// Whether this domain has already been asked about and had nothing to give.
/// A miss remembered on disk expires after a week, so a domain that gains an
/// icon is eventually found.
pub fn known_missing(email: &str) -> bool {
    match domain_of(email) {
        Some(domain) => {
            MISSES.with(|m| m.borrow().contains(&domain))
                || miss_path(&domain).is_some_and(|p| fresh(&p))
        }
        // Nothing to look up counts as answered.
        None => true,
    }
}

/// Blocking fetch of a domain's icon. Call off the main thread.
///
/// A fresh on-disk copy answers without touching the network; otherwise the
/// domain is asked and the answer stored — icon or miss. When the network
/// fails with a stale copy in hand, the stale copy stands (and stays due for
/// refresh, so it is retried later).
pub fn fetch(email: &str) -> Option<Vec<u8>> {
    let domain = domain_of(email)?;
    let img = img_path(&domain);
    if let Some(p) = img.as_ref().filter(|p| fresh(p)) {
        if let Ok(bytes) = std::fs::read(p) {
            return Some(bytes);
        }
    }
    if miss_path(&domain).is_some_and(|p| fresh(&p)) {
        return None;
    }
    for url in candidate_urls(&domain) {
        if let Some(bytes) = get(&url) {
            if let Some(p) = img.as_ref() {
                let _ = std::fs::write(p, &bytes);
            }
            if let Some(p) = miss_path(&domain) {
                let _ = std::fs::remove_file(p);
            }
            return Some(bytes);
        }
    }
    if let Some(p) = img.as_ref().filter(|p| p.exists()) {
        // Nothing new, but yesterday's icon beats initials.
        return std::fs::read(p).ok();
    }
    if let Some(p) = miss_path(&domain) {
        let _ = std::fs::write(p, b"");
    }
    None
}

/// Where a site publishes its icon, best first.
fn candidate_urls(domain: &str) -> Vec<String> {
    [
        format!("https://{domain}/apple-touch-icon.png"),
        format!("https://www.{domain}/apple-touch-icon.png"),
        format!("https://{domain}/favicon.ico"),
        format!("https://www.{domain}/favicon.ico"),
    ]
    .to_vec()
}

fn get(url: &str) -> Option<Vec<u8>> {
    use std::io::Read;
    let resp = ureq::get(url)
        .timeout(std::time::Duration::from_secs(5))
        .call()
        .ok()?;
    // Some sites answer every icon path with their home page and a 200 — pm.me
    // sends 347KB of HTML for `/favicon.ico`. Ask what it is before reading it.
    if !is_image_type(resp.content_type()) {
        return None;
    }
    let mut buf = Vec::new();
    resp.into_reader()
        .take(1_000_000)
        .read_to_end(&mut buf)
        .ok()?;
    // And a backstop for the ones that mislabel it.
    (!buf.is_empty() && !looks_like_markup(&buf)).then_some(buf)
}

/// Whether a response claims to be an image. An empty or unknown type is
/// allowed through — plenty of servers send `application/octet-stream` for an
/// `.ico`, and the bytes are checked either way.
fn is_image_type(content_type: &str) -> bool {
    let ct = content_type.trim().to_ascii_lowercase();
    let ct = ct.split(';').next().unwrap_or("").trim();
    ct.is_empty() || ct.starts_with("image/") || ct == "application/octet-stream"
}

/// Whether these bytes are a web page rather than an image — some sites answer a
/// missing icon with their home page and a 200.
fn looks_like_markup(bytes: &[u8]) -> bool {
    let head: String = bytes
        .iter()
        .take(64)
        .map(|b| *b as char)
        .collect::<String>()
        .trim_start()
        .to_ascii_lowercase();
    head.starts_with("<!doctype") || head.starts_with("<html") || head.starts_with("<?xml")
}

/// Decode icon bytes into a texture and cache them under the sender's domain.
///
/// `GdkTexture` reads PNG and JPEG; an `.ico` needs GdkPixbuf, which the platform
/// supplies loaders for. Failing to decode is remembered as a miss, so a domain
/// serving something unreadable is not asked on every row.
pub fn decode_and_cache(email: &str, bytes: &[u8]) -> Option<gtk::gdk::Texture> {
    let domain = domain_of(email)?;
    let tex = decode(bytes);
    match tex {
        Some(tex) => {
            CACHE.with(|c| {
                c.borrow_mut().insert(domain, tex.clone());
            });
            Some(tex)
        }
        None => {
            // Undecodable bytes: persist the miss (and drop the stored copy)
            // so the next session doesn't fetch and fail to decode them again.
            if let Some(p) = img_path(&domain) {
                let _ = std::fs::remove_file(p);
            }
            if let Some(p) = miss_path(&domain) {
                let _ = std::fs::write(p, b"");
            }
            remember_miss(&domain);
            None
        }
    }
}

/// Remember that a sender's domain has no usable icon.
pub fn remember_missing(email: &str) {
    if let Some(domain) = domain_of(email) {
        remember_miss(&domain);
    }
}

fn remember_miss(domain: &str) {
    MISSES.with(|m| {
        m.borrow_mut().insert(domain.to_string());
    });
}

fn decode(bytes: &[u8]) -> Option<gtk::gdk::Texture> {
    let glib_bytes = gtk::glib::Bytes::from(bytes);
    if let Ok(tex) = gtk::gdk::Texture::from_bytes(&glib_bytes) {
        return Some(tex);
    }
    // ICO, and anything else the platform has a pixbuf loader for.
    let stream = gtk::gio::MemoryInputStream::from_bytes(&glib_bytes);
    let pixbuf = gtk::gdk_pixbuf::Pixbuf::from_stream(&stream, gtk::gio::Cancellable::NONE).ok()?;
    Some(gtk::gdk::Texture::for_pixbuf(&pixbuf))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_brand_domain_is_found_under_its_sending_subdomains() {
        assert_eq!(domain_of("no-reply@apple.com").as_deref(), Some("apple.com"));
        assert_eq!(
            domain_of("news@mail.notifications.apple.com").as_deref(),
            Some("apple.com")
        );
        assert_eq!(
            domain_of("bounces+42933322-387e@em6917.cloudways.com").as_deref(),
            Some("cloudways.com")
        );
        // Country-code pairs keep three labels.
        assert_eq!(domain_of("a@mail.bbc.co.uk").as_deref(), Some("bbc.co.uk"));
        assert_eq!(domain_of("a@shop.example.com.au").as_deref(), Some("example.com.au"));
    }

    #[test]
    fn addresses_with_no_domain_to_ask_are_skipped() {
        assert_eq!(domain_of(""), None);
        assert_eq!(domain_of("someone"), None);
        assert_eq!(domain_of("someone@localhost"), None);
        // An IP literal is nobody's brand.
        assert_eq!(domain_of("a@192.168.1.1"), None);
        // Nothing to look up is treated as already answered, so no fetch is made.
        assert!(known_missing("someone"));
    }

    #[test]
    fn only_images_are_read() {
        assert!(is_image_type("image/png"));
        assert!(is_image_type("image/vnd.microsoft.icon; charset=utf-8"));
        assert!(is_image_type("image/x-icon"));
        // Servers that don't know what an .ico is get the benefit of the doubt;
        // the bytes are sniffed anyway.
        assert!(is_image_type(""));
        assert!(is_image_type("application/octet-stream"));
        // A home page is not an icon, and is not worth downloading to find out.
        assert!(!is_image_type("text/html; charset=utf-8"));
        assert!(!is_image_type("application/json"));
    }

    #[test]
    fn a_home_page_served_in_place_of_an_icon_is_rejected() {
        assert!(looks_like_markup(b"<!DOCTYPE html><html>"));
        assert!(looks_like_markup(b"  <html lang=\"en\">"));
        assert!(looks_like_markup(b"<?xml version=\"1.0\"?><svg"));
        assert!(!looks_like_markup(b"\x89PNG\r\n\x1a\n"));
        assert!(!looks_like_markup(b"\x00\x00\x01\x00"));
    }
}
