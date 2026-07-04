//! GNOME Contacts integration via Evolution Data Server (EDS).
//!
//! Reading is done straight from the EDS address-book SQLite caches (covers
//! both the local address book and CardDAV books, which EDS mirrors locally).
//! Writing goes through the EDS D-Bus API so changes are tracked by EDS and
//! synced back to CardDAV servers.

use std::collections::HashSet;
use std::path::PathBuf;

/// A name + email pair from the address book.
#[derive(Debug, Clone)]
pub struct Contact {
    pub name: String,
    pub email: String,
}

/// A recipient autocomplete suggestion (from Contacts or mail history).
#[derive(Debug, Clone)]
pub struct Suggestion {
    pub name: String,
    pub email: String,
    /// True when this came from the address book.
    pub from_contacts: bool,
    /// How often this address appears in mail history (0 for contacts-only).
    pub score: u32,
}

impl Suggestion {
    /// "Name <email>" for inserting into a recipient field (email alone if no name).
    pub fn display(&self) -> String {
        if self.name.trim().is_empty() || self.name == self.email {
            self.email.clone()
        } else {
            format!("{} <{}>", self.name, self.email)
        }
    }

    /// Whether the suggestion matches a typed fragment (name or email).
    pub fn matches(&self, q: &str) -> bool {
        let q = q.to_lowercase();
        self.email.to_lowercase().contains(&q) || self.name.to_lowercase().contains(&q)
    }
}

/// Combined, de-duplicated recipient suggestions from Contacts and mail history
/// (addresses sent to / received from), excluding the user's own `exclude`
/// addresses. History frequency becomes each suggestion's `score` so frequently
/// used addresses — contact or not — can rank ahead.
pub fn suggestions(exclude: &[String]) -> Vec<Suggestion> {
    let exclude: HashSet<String> = exclude.iter().map(|e| e.to_lowercase()).collect();
    let mut map: std::collections::HashMap<String, Suggestion> = std::collections::HashMap::new();

    for c in read_contacts() {
        let key = c.email.to_lowercase();
        if exclude.contains(&key) {
            continue;
        }
        map.entry(key).or_insert(Suggestion {
            name: c.name,
            email: c.email,
            from_contacts: true,
            score: 0,
        });
    }

    if let Ok(cache) = crate::cache::Cache::open() {
        for (name, email, count) in cache.address_history() {
            let key = email.to_lowercase();
            if exclude.contains(&key) {
                continue;
            }
            map.entry(key)
                .and_modify(|s| s.score = s.score.max(count))
                .or_insert(Suggestion { name, email, from_contacts: false, score: count });
        }
    }

    let mut out: Vec<Suggestion> = map.into_values().collect();
    // Default order: most-used first, then name (the field filter re-ranks live).
    out.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out
}

/// A writable address book the user can add contacts to.
#[derive(Debug, Clone)]
pub struct Book {
    /// EDS source UID (what `OpenAddressBook` expects).
    pub uid: String,
    pub name: String,
}

fn data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("evolution/addressbook"))
}

fn cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("evolution/addressbook"))
}

/// Read every address email from the local + cached (CardDAV) EDS books.
pub fn read_contacts() -> Vec<Contact> {
    let mut out = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    // Local books: table `folder_id` + `folder_id_email_list`.
    if let Some(dir) = data_dir() {
        for db in find_dbs(&dir, "contacts.db") {
            read_book_db(
                &db,
                "SELECT f.full_name, e.value FROM folder_id f \
                 JOIN folder_id_email_list e ON f.uid = e.uid",
                &mut out,
                &mut seen,
            );
        }
    }
    // Cached books (CardDAV etc.): table `ECacheObjects` + `attrlist_email_list`.
    if let Some(dir) = cache_dir() {
        for db in find_dbs(&dir, "cache.db") {
            read_book_db(
                &db,
                "SELECT o.full_name, e.value FROM ECacheObjects o \
                 JOIN attrlist_email_list e ON o.ECacheUID = e.uid",
                &mut out,
                &mut seen,
            );
        }
    }
    out
}

fn find_dbs(root: &std::path::Path, file: &str) -> Vec<PathBuf> {
    let mut dbs = Vec::new();
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let db = entry.path().join(file);
            if db.is_file() {
                dbs.push(db);
            }
        }
    }
    dbs
}

fn read_book_db(path: &std::path::Path, query: &str, out: &mut Vec<Contact>, seen: &mut HashSet<String>) {
    use rusqlite::OpenFlags;
    let Ok(conn) = rusqlite::Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    ) else {
        return;
    };
    let Ok(mut stmt) = conn.prepare(query) else {
        return;
    };
    let rows = stmt.query_map([], |row| {
        let name: Option<String> = row.get(0).ok();
        let email: String = row.get(1)?;
        Ok((name.unwrap_or_default(), email))
    });
    if let Ok(rows) = rows {
        for (name, email) in rows.flatten() {
            let email = email.trim().to_string();
            if email.is_empty() || !email.contains('@') {
                continue;
            }
            let key = email.to_lowercase();
            if seen.insert(key) {
                let name = if name.trim().is_empty() { email.clone() } else { name };
                out.push(Contact { name, email });
            }
        }
    }
}

/// Address books the user can add contacts to (local + cached/CardDAV).
pub fn writable_books() -> Vec<Book> {
    let mut books = Vec::new();
    if let Some(dir) = data_dir() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            if !entry.path().join("contacts.db").is_file() {
                continue;
            }
            let folder = entry.file_name().to_string_lossy().to_string();
            // The built-in local book's source UID is "system-address-book".
            let uid = if folder == "system" {
                "system-address-book".to_string()
            } else {
                folder
            };
            let name = source_display_name(&uid).unwrap_or_else(|| "On This Computer".to_string());
            books.push(Book { uid, name });
        }
    }
    if let Some(dir) = cache_dir() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            if !entry.path().join("cache.db").is_file() {
                continue;
            }
            let uid = entry.file_name().to_string_lossy().to_string();
            let name = source_display_name(&uid).unwrap_or_else(|| "CardDAV Address Book".to_string());
            books.push(Book { uid, name });
        }
    }
    books
}

/// Best-effort display name from the EDS source file for a book UID.
fn source_display_name(uid: &str) -> Option<String> {
    let path = dirs::config_dir()?.join(format!("evolution/sources/{uid}.source"));
    let text = std::fs::read_to_string(path).ok()?;
    text.lines()
        .find_map(|l| l.strip_prefix("DisplayName="))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

// ---------------------------------------------------------------------------
// Writing (EDS D-Bus)
// ---------------------------------------------------------------------------

/// Discover the versioned AddressBook factory bus name (e.g. `…AddressBook10`).
fn factory_dest() -> Option<String> {
    let dir = std::path::Path::new("/usr/share/dbus-1/services");
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.contains("AddressBook") && name.ends_with(".service") {
            if let Ok(text) = std::fs::read_to_string(entry.path()) {
                if let Some(n) = text.lines().find_map(|l| l.strip_prefix("Name=")) {
                    let n = n.trim();
                    if n.contains("AddressBook") {
                        return Some(n.to_string());
                    }
                }
            }
        }
    }
    None
}

const FACTORY_PATH: &str = "/org/gnome/evolution/dataserver/AddressBookFactory";
const FACTORY_IFACE: &str = "org.gnome.evolution.dataserver.AddressBookFactory";
const BOOK_IFACE: &str = "org.gnome.evolution.dataserver.AddressBook";

/// Open a book by source UID; returns (bus_name, object_path) for the book.
fn open_book(
    conn: &zbus::blocking::Connection,
    dest: &str,
    uid: &str,
) -> Result<(String, String), String> {
    let reply = conn
        .call_method(Some(dest), FACTORY_PATH, Some(FACTORY_IFACE), "OpenAddressBook", &(uid,))
        .map_err(|e| e.to_string())?;
    // Returns (object_path, bus_name).
    let (object_path, bus_name): (String, String) =
        reply.body().deserialize().map_err(|e| e.to_string())?;
    conn.call_method(Some(bus_name.as_str()), object_path.as_str(), Some(BOOK_IFACE), "Open", &())
        .map_err(|e| e.to_string())?;
    Ok((bus_name, object_path))
}

/// What happened when adding an email to Contacts.
#[derive(Debug)]
pub enum AddOutcome {
    /// A new contact was created.
    Created,
    /// The email was merged into an existing contact (carries its name).
    Merged(String),
    /// The email was already in the address book (carries the contact's name).
    AlreadyPresent(String),
}

/// Add `email` (with `name`) to Contacts, detecting duplicates and merging:
/// - if the email already exists anywhere, do nothing;
/// - else if a contact with the same name exists in the book, append the email;
/// - else create a new contact. Blocking — call from a background thread.
pub fn add_or_merge(book_uid: &str, name: &str, email: &str) -> Result<AddOutcome, String> {
    // 1. Already present (in any book)? Don't create a duplicate.
    let email_l = email.trim().to_lowercase();
    if let Some(existing) = read_contacts()
        .into_iter()
        .find(|c| c.email.to_lowercase() == email_l)
    {
        return Ok(AddOutcome::AlreadyPresent(existing.name));
    }

    let dest = factory_dest().ok_or("Evolution Data Server is not available")?;
    let conn = zbus::blocking::Connection::session().map_err(|e| e.to_string())?;
    let (bus, path) = open_book(&conn, &dest, book_uid)?;

    // 2. A contact with the same name in this book → merge the email into it.
    if !name.trim().is_empty() {
        let query = format!("(is \"full_name\" \"{}\")", sexpr_escape(name.trim()));
        let uids = book_get_uids(&conn, &bus, &path, &query)?;
        if let Some(uid) = uids.first() {
            let vcard = book_get_contact(&conn, &bus, &path, uid)?;
            let merged = add_email_to_vcard(&vcard, email.trim());
            book_call(&conn, &bus, &path, "ModifyContacts", &(vec![merged], 0u32))?;
            return Ok(AddOutcome::Merged(name.trim().to_string()));
        }
    }

    // 3. Create a new contact.
    book_call(
        &conn,
        &bus,
        &path,
        "CreateContacts",
        &(vec![vcard_for(name, email)], 0u32),
    )?;
    Ok(AddOutcome::Created)
}

fn book_call<B>(
    conn: &zbus::blocking::Connection,
    bus: &str,
    path: &str,
    method: &str,
    body: &B,
) -> Result<zbus::Message, String>
where
    B: serde::Serialize + zbus::zvariant::DynamicType,
{
    conn.call_method(Some(bus), path, Some(BOOK_IFACE), method, body)
        .map_err(|e| e.to_string())
}

fn book_get_uids(
    conn: &zbus::blocking::Connection,
    bus: &str,
    path: &str,
    query: &str,
) -> Result<Vec<String>, String> {
    let reply = book_call(conn, bus, path, "GetContactListUids", &(query,))?;
    let (uids,): (Vec<String>,) = reply.body().deserialize().map_err(|e| e.to_string())?;
    Ok(uids)
}

fn book_get_contact(
    conn: &zbus::blocking::Connection,
    bus: &str,
    path: &str,
    uid: &str,
) -> Result<String, String> {
    let reply = book_call(conn, bus, path, "GetContact", &(uid,))?;
    let (vcard,): (String,) = reply.body().deserialize().map_err(|e| e.to_string())?;
    Ok(vcard)
}

/// Insert an EMAIL line into an existing vCard, just before END:VCARD.
fn add_email_to_vcard(vcard: &str, email: &str) -> String {
    let line = format!("EMAIL;TYPE=INTERNET:{email}\r\n");
    match vcard.rfind("END:VCARD") {
        Some(pos) => {
            let mut s = String::with_capacity(vcard.len() + line.len());
            s.push_str(&vcard[..pos]);
            if !s.ends_with('\n') {
                s.push_str("\r\n");
            }
            s.push_str(&line);
            s.push_str(&vcard[pos..]);
            s
        }
        None => vcard.to_string(),
    }
}

/// Escape a value for an EDS S-expression string literal.
fn sexpr_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn vcard_for(name: &str, email: &str) -> String {
    let name = if name.trim().is_empty() { email } else { name.trim() };
    // Minimal vCard 3.0; EDS assigns the UID.
    format!(
        "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:{name}\r\nN:;{name};;;\r\n\
         EMAIL;TYPE=INTERNET:{email}\r\nEND:VCARD\r\n"
    )
}
