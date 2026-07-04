//! Gravatar support (opt-in). Builds the Gravatar URL from an email's MD5,
//! fetches the image over HTTPS (blocking — call from a relm4 command, off the
//! main thread), and caches decoded textures on the main thread so rows and the
//! reader don't refetch the same sender.
//!
//! Privacy note: enabling Gravatar sends a hash of each sender's email to a
//! third party (Gravatar/Automattic). It is therefore off by default and gated
//! behind a Preferences toggle.

use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static CACHE: RefCell<HashMap<String, gtk::gdk::Texture>> = RefCell::new(HashMap::new());
}

fn key(email: &str) -> String {
    email.trim().to_lowercase()
}

/// A previously decoded texture for this sender, if cached (main thread only).
pub fn cached(email: &str) -> Option<gtk::gdk::Texture> {
    CACHE.with(|c| c.borrow().get(&key(email)).cloned())
}

fn store(email: &str, tex: gtk::gdk::Texture) {
    CACHE.with(|c| {
        c.borrow_mut().insert(key(email), tex);
    });
}

fn gravatar_url(email: &str) -> String {
    let digest = md5::compute(key(email));
    // d=404 → no image when the sender has no Gravatar, so we fall back to initials.
    format!("https://www.gravatar.com/avatar/{digest:x}?s=80&d=404")
}

/// Blocking HTTPS fetch of the Gravatar image bytes. Returns `None` if the
/// sender has no Gravatar (404) or on any error. Call off the main thread.
pub fn fetch(email: &str) -> Option<Vec<u8>> {
    use std::io::Read;
    let resp = ureq::get(&gravatar_url(email)).call().ok()?;
    let mut buf = Vec::new();
    resp.into_reader()
        .take(2_000_000)
        .read_to_end(&mut buf)
        .ok()?;
    (!buf.is_empty()).then_some(buf)
}

/// Decode image bytes into a texture and cache it under `email` (main thread).
pub fn decode_and_cache(email: &str, bytes: &[u8]) -> Option<gtk::gdk::Texture> {
    let glib_bytes = gtk::glib::Bytes::from(bytes);
    let tex = gtk::gdk::Texture::from_bytes(&glib_bytes).ok()?;
    store(email, tex.clone());
    Some(tex)
}
