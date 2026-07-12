//! Local SQLite cache for folders, message summaries, and bodies.
//!
//! The cache lets the app show mail instantly on startup, read offline, and
//! avoid re-fetching message bodies. Each per-account worker opens its own
//! connection (so the `!Send` `rusqlite::Connection` never crosses threads);
//! WAL mode keeps concurrent access from contending. Everything is keyed by
//! account so two accounts can both have an "INBOX". It is strictly
//! best-effort: any error is logged and degrades to "no cache".

use std::time::Duration;

use rusqlite::{params, Connection};

use crate::models::{Attachment, Folder, FolderKind, Message};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS folders (
    account_id INTEGER NOT NULL,
    path       TEXT    NOT NULL,
    name       TEXT    NOT NULL,
    kind       INTEGER NOT NULL,
    unread     INTEGER NOT NULL,
    ord        INTEGER NOT NULL,
    PRIMARY KEY (account_id, path)
);
CREATE TABLE IF NOT EXISTS messages (
    account_id     INTEGER NOT NULL,
    folder_path    TEXT    NOT NULL,
    uid            INTEGER NOT NULL,
    from_name      TEXT    NOT NULL,
    from_addr      TEXT    NOT NULL,
    recipients     TEXT    NOT NULL DEFAULT '',
    cc             TEXT    NOT NULL DEFAULT '',
    subject        TEXT    NOT NULL,
    date           TEXT    NOT NULL,
    ts             INTEGER NOT NULL,
    unread         INTEGER NOT NULL,
    starred        INTEGER NOT NULL,
    has_attachment INTEGER NOT NULL,
    message_id     TEXT    NOT NULL DEFAULT '',
    references_    TEXT    NOT NULL DEFAULT '',
    PRIMARY KEY (account_id, folder_path, uid)
);
CREATE TABLE IF NOT EXISTS bodies (
    account_id  INTEGER NOT NULL,
    folder_path TEXT NOT NULL,
    uid         INTEGER NOT NULL,
    body        TEXT NOT NULL,
    PRIMARY KEY (account_id, folder_path, uid)
);
CREATE TABLE IF NOT EXISTS attachments (
    account_id  INTEGER NOT NULL,
    folder_path TEXT NOT NULL,
    uid         INTEGER NOT NULL,
    idx         INTEGER NOT NULL,
    name        TEXT NOT NULL,
    data        BLOB NOT NULL,
    PRIMARY KEY (account_id, folder_path, uid, idx)
);
CREATE TABLE IF NOT EXISTS addresses (
    email TEXT PRIMARY KEY,
    name  TEXT NOT NULL DEFAULT '',
    count INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS attachments_checked (
    account_id  INTEGER NOT NULL,
    folder_path TEXT NOT NULL,
    uid         INTEGER NOT NULL,
    PRIMARY KEY (account_id, folder_path, uid)
);
";

/// Bump when the table layout changes; older rows are dropped on open.
/// v8: bodies are re-rendered with clickable links, so cached bodies (which
/// `LoadBody` serves without re-fetching) must be dropped and rebuilt on open.
/// v9: re-decode subjects cached as raw RFC 2047 encoded-words by builds that
/// aborted on over-long encoded-words (e.g. Mailchimp newsletters).
const SCHEMA_VERSION: i64 = 9;

/// The first version whose table *layout* matches the current `SCHEMA`. At or
/// above this, an upgrade only needs to drop the derived caches, not the
/// expensive message index (which would force a whole-mailbox re-sync).
const LAYOUT_VERSION: i64 = 6;

pub struct Cache {
    conn: Connection,
}

impl Cache {
    /// Open (creating if needed) the cache DB at `~/.local/share/veem/cache.db`.
    pub fn open() -> rusqlite::Result<Cache> {
        let path = dirs::data_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("veem");
        let _ = std::fs::create_dir_all(&path);
        let conn = Connection::open(path.join("cache.db"))?;

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap_or(0);
        // Upgrading in place (layout current, message index preserved) rather
        // than wiping and re-syncing the whole mailbox.
        let upgrading_index = (LAYOUT_VERSION..SCHEMA_VERSION).contains(&version);
        if version < LAYOUT_VERSION {
            let _ = conn.execute_batch(
                "DROP TABLE IF EXISTS folders;\
                 DROP TABLE IF EXISTS messages;\
                 DROP TABLE IF EXISTS bodies;\
                 DROP TABLE IF EXISTS attachments;\
                 DROP TABLE IF EXISTS attachments_checked;",
            );
        } else if version < SCHEMA_VERSION {
            // The layout is current but `bodies` holds HTML rendered by an older
            // build. `LoadBody` serves that cache without ever re-fetching, so a
            // stale entry would survive forever — drop it and let it re-render on
            // next open. The message index is kept: it's expensive to rebuild.
            let _ = conn.execute_batch("DROP TABLE IF EXISTS bodies;");
        }
        conn.execute_batch(SCHEMA)?;
        if upgrading_index {
            Self::redecode_encoded_subjects(&conn);
        }
        let _ = conn.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION};"));

        // Concurrency: multiple account workers share this file.
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        let _ = conn.busy_timeout(Duration::from_secs(5));

        Ok(Cache { conn })
    }

    /// One-time upgrade fix: earlier builds cached a message's subject verbatim
    /// when the RFC 2047 decoder aborted on an over-long encoded-word (a single
    /// `=?utf-8?Q?…?=` far past the 75-char limit, as Mailchimp emits). Now that
    /// [`crate::worker::decode_header`] decodes those, re-decode any subject
    /// still stored as a raw encoded-word — in place, so no re-sync is needed.
    fn redecode_encoded_subjects(conn: &Connection) {
        let rows: Vec<(u32, String, u32, String)> = {
            let Ok(mut stmt) = conn.prepare(
                "SELECT account_id, folder_path, uid, subject FROM messages \
                 WHERE subject LIKE '%=?%?=%'",
            ) else {
                return;
            };
            let Ok(mapped) = stmt.query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            }) else {
                return;
            };
            mapped.flatten().collect()
        };
        for (account_id, folder_path, uid, subject) in rows {
            let decoded = crate::worker::decode_header(subject.as_bytes());
            if decoded != subject {
                let _ = conn.execute(
                    "UPDATE messages SET subject = ?1 \
                     WHERE account_id = ?2 AND folder_path = ?3 AND uid = ?4",
                    params![decoded, account_id, folder_path, uid],
                );
            }
        }
    }

    pub fn load_folders(&self, account_id: u32) -> Vec<Folder> {
        let run = || -> rusqlite::Result<Vec<Folder>> {
            let mut stmt = self.conn.prepare(
                "SELECT path, name, kind, unread FROM folders WHERE account_id = ?1 ORDER BY ord",
            )?;
            let rows = stmt.query_map([account_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, u32>(3)?,
                ))
            })?;
            let mut folders = Vec::new();
            for (i, r) in rows.enumerate() {
                let (path, name, kind, unread) = r?;
                folders.push(Folder {
                    id: i as u32 + 1,
                    account_id,
                    name,
                    path,
                    kind: kind_from_i64(kind),
                    unread,
                });
            }
            Ok(folders)
        };
        run().unwrap_or_else(|e| {
            tracing::warn!("cache load_folders failed: {e}");
            Vec::new()
        })
    }

    pub fn save_folders(&self, account_id: u32, folders: &[Folder]) {
        let run = || -> rusqlite::Result<()> {
            let tx = self.conn.unchecked_transaction()?;
            tx.execute("DELETE FROM folders WHERE account_id = ?1", [account_id])?;
            for (i, f) in folders.iter().enumerate() {
                tx.execute(
                    "INSERT INTO folders (account_id, path, name, kind, unread, ord)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![account_id, f.path, f.name, kind_to_i64(f.kind), f.unread, i as i64],
                )?;
            }
            tx.commit()
        };
        if let Err(e) = run() {
            tracing::warn!("cache save_folders failed: {e}");
        }
    }

    pub fn load_messages(&self, account_id: u32, folder_path: &str, folder_id: u32) -> Vec<Message> {
        let run = || -> rusqlite::Result<Vec<Message>> {
            let mut stmt = self.conn.prepare(
                "SELECT uid, from_name, from_addr, subject, date, ts, unread, starred, has_attachment, recipients, cc, message_id, references_
                 FROM messages WHERE account_id = ?1 AND folder_path = ?2 ORDER BY uid DESC",
            )?;
            let rows = stmt.query_map(params![account_id, folder_path], |row| {
                let uid: u32 = row.get(0)?;
                Ok(Message {
                    id: uid,
                    account_id,
                    folder_id,
                    uid,
                    from_name: row.get(1)?,
                    from_addr: row.get(2)?,
                    to: row.get(9)?,
                    cc: row.get(10)?,
                    subject: row.get(3)?,
                    preview: String::new(),
                    body: String::new(),
                    date: row.get(4)?,
                    timestamp: row.get(5)?,
                    unread: row.get(6)?,
                    starred: row.get(7)?,
                    has_attachment: row.get(8)?,
                    message_id: row.get(11)?,
                    references: row.get(12)?,
                })
            })?;
            rows.collect()
        };
        run().unwrap_or_else(|e| {
            tracing::warn!("cache load_messages failed: {e}");
            Vec::new()
        })
    }

    pub fn save_messages(&self, account_id: u32, folder_path: &str, messages: &[Message]) {
        let run = || -> rusqlite::Result<()> {
            let tx = self.conn.unchecked_transaction()?;
            tx.execute(
                "DELETE FROM messages WHERE account_id = ?1 AND folder_path = ?2",
                params![account_id, folder_path],
            )?;
            for m in messages {
                tx.execute(
                    "INSERT INTO messages
                     (account_id, folder_path, uid, from_name, from_addr, subject, date, ts, unread, starred, has_attachment, recipients, cc, message_id, references_)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                    params![
                        account_id, folder_path, m.uid, m.from_name, m.from_addr, m.subject,
                        m.date, m.timestamp, m.unread, m.starred, m.has_attachment, m.to, m.cc,
                        m.message_id, m.references
                    ],
                )?;
            }
            tx.commit()
        };
        if let Err(e) = run() {
            tracing::warn!("cache save_messages failed: {e}");
        }
    }

    /// Insert-or-replace message summaries without clearing the folder first.
    /// Used to grow the search index (fast first page + background backfill)
    /// without wiping already-indexed messages.
    pub fn upsert_messages(&self, account_id: u32, folder_path: &str, messages: &[Message]) {
        let run = || -> rusqlite::Result<()> {
            let tx = self.conn.unchecked_transaction()?;
            for m in messages {
                tx.execute(
                    "INSERT OR REPLACE INTO messages
                     (account_id, folder_path, uid, from_name, from_addr, subject, date, ts, unread, starred, has_attachment, recipients, cc, message_id, references_)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                    params![
                        account_id, folder_path, m.uid, m.from_name, m.from_addr, m.subject,
                        m.date, m.timestamp, m.unread, m.starred, m.has_attachment, m.to, m.cc,
                        m.message_id, m.references
                    ],
                )?;
            }
            tx.commit()
        };
        if let Err(e) = run() {
            tracing::warn!("cache upsert_messages failed: {e}");
        }
    }

    /// The set of message UIDs already cached for a folder (for backfill diffing).
    pub fn cached_uids(&self, account_id: u32, folder_path: &str) -> std::collections::HashSet<u32> {
        let run = || -> rusqlite::Result<std::collections::HashSet<u32>> {
            let mut stmt = self.conn.prepare(
                "SELECT uid FROM messages WHERE account_id = ?1 AND folder_path = ?2",
            )?;
            let rows = stmt.query_map(params![account_id, folder_path], |row| row.get::<_, u32>(0))?;
            rows.collect()
        };
        run().unwrap_or_default()
    }

    pub fn load_body(&self, account_id: u32, folder_path: &str, uid: u32) -> Option<String> {
        self.conn
            .query_row(
                "SELECT body FROM bodies WHERE account_id = ?1 AND folder_path = ?2 AND uid = ?3",
                params![account_id, folder_path, uid],
                |row| row.get::<_, String>(0),
            )
            .ok()
    }

    pub fn save_body(&self, account_id: u32, folder_path: &str, uid: u32, body: &str) {
        if let Err(e) = self.conn.execute(
            "INSERT OR REPLACE INTO bodies (account_id, folder_path, uid, body) VALUES (?1, ?2, ?3, ?4)",
            params![account_id, folder_path, uid, body],
        ) {
            tracing::warn!("cache save_body failed: {e}");
        }
    }

    /// Every cached attachment across an account's folders (newest message
    /// first), for the attachments gallery — excluding Drafts, Junk and Trash.
    /// Data is loaded only for files under `data_cap` bytes (so thumbnails/
    /// previews are instant); larger files carry `None` and are fetched on
    /// demand. Capped to `limit` rows.
    pub fn gallery_items(
        &self,
        account_id: u32,
        data_cap: u64,
        limit: u32,
    ) -> Vec<crate::models::GalleryItem> {
        // Drafts(3), Junk(5), Trash(6) — see `kind_to_i64`.
        let drafts = kind_to_i64(FolderKind::Drafts);
        let junk = kind_to_i64(FolderKind::Junk);
        let trash = kind_to_i64(FolderKind::Trash);
        let run = || -> rusqlite::Result<Vec<crate::models::GalleryItem>> {
            let mut stmt = self.conn.prepare(
                "SELECT a.uid, a.name, length(a.data), \
                        CASE WHEN length(a.data) <= ?2 THEN a.data ELSE NULL END, \
                        COALESCE(m.from_name, ''), COALESCE(m.subject, ''), COALESCE(m.ts, 0), \
                        a.folder_path \
                 FROM attachments a \
                 JOIN folders f ON f.account_id = a.account_id AND f.path = a.folder_path \
                 LEFT JOIN messages m \
                   ON m.account_id = a.account_id AND m.folder_path = a.folder_path AND m.uid = a.uid \
                 WHERE a.account_id = ?1 AND f.kind NOT IN (?3, ?4, ?5) \
                 ORDER BY m.ts DESC, a.uid DESC, a.idx ASC \
                 LIMIT ?6",
            )?;
            let rows = stmt.query_map(
                params![account_id, data_cap as i64, drafts, junk, trash, limit],
                |row| {
                    Ok(crate::models::GalleryItem {
                        account_id,
                        uid: row.get(0)?,
                        name: row.get(1)?,
                        size: row.get::<_, i64>(2)? as u64,
                        data: row.get(3)?,
                        from_name: row.get(4)?,
                        subject: row.get(5)?,
                        timestamp: row.get(6)?,
                        folder_path: row.get(7)?,
                    })
                },
            )?;
            rows.collect()
        };
        run().unwrap_or_else(|e| {
            tracing::warn!("cache gallery_items failed: {e}");
            Vec::new()
        })
    }

    pub fn load_attachments(&self, account_id: u32, folder_path: &str, uid: u32) -> Vec<Attachment> {
        let run = || -> rusqlite::Result<Vec<Attachment>> {
            let mut stmt = self.conn.prepare(
                "SELECT name, data FROM attachments
                 WHERE account_id = ?1 AND folder_path = ?2 AND uid = ?3 ORDER BY idx",
            )?;
            let rows = stmt.query_map(params![account_id, folder_path, uid], |row| {
                Ok(Attachment {
                    name: row.get(0)?,
                    data: row.get(1)?,
                })
            })?;
            rows.collect()
        };
        run().unwrap_or_else(|e| {
            tracing::warn!("cache load_attachments failed: {e}");
            Vec::new()
        })
    }

    pub fn save_attachments(
        &self,
        account_id: u32,
        folder_path: &str,
        uid: u32,
        items: &[Attachment],
    ) {
        let run = || -> rusqlite::Result<()> {
            let tx = self.conn.unchecked_transaction()?;
            tx.execute(
                "DELETE FROM attachments WHERE account_id = ?1 AND folder_path = ?2 AND uid = ?3",
                params![account_id, folder_path, uid],
            )?;
            for (i, a) in items.iter().enumerate() {
                tx.execute(
                    "INSERT INTO attachments (account_id, folder_path, uid, idx, name, data)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![account_id, folder_path, uid, i as i64, a.name, a.data],
                )?;
            }
            tx.commit()
        };
        if let Err(e) = run() {
            tracing::warn!("cache save_attachments failed: {e}");
        }
    }

    /// Mark a message's attachments as fetched (even if it turned out to have
    /// none), so the background prefetch never re-downloads it to re-check.
    pub fn mark_attachments_checked(&self, account_id: u32, folder_path: &str, uid: u32) {
        let _ = self.conn.execute(
            "INSERT OR IGNORE INTO attachments_checked (account_id, folder_path, uid) VALUES (?1, ?2, ?3)",
            params![account_id, folder_path, uid],
        );
    }

    /// Whether a message's attachments have already been fetched/checked.
    pub fn attachments_checked(&self, account_id: u32, folder_path: &str, uid: u32) -> bool {
        self.conn
            .query_row(
                "SELECT 1 FROM attachments_checked WHERE account_id = ?1 AND folder_path = ?2 AND uid = ?3",
                params![account_id, folder_path, uid],
                |_| Ok(()),
            )
            .is_ok()
    }

    pub fn set_unread(&self, account_id: u32, folder_path: &str, uid: u32, unread: bool) {
        let _ = self.conn.execute(
            "UPDATE messages SET unread = ?1 WHERE account_id = ?2 AND folder_path = ?3 AND uid = ?4",
            params![unread, account_id, folder_path, uid],
        );
    }

    /// Record recipients the user has sent to, so they autocomplete even before
    /// the Sent folder syncs. Each send bumps the address's frequency.
    pub fn record_addresses(&self, entries: &[(String, String)]) {
        for (name, email) in entries {
            let email = email.trim().to_lowercase();
            if email.is_empty() || !email.contains('@') {
                continue;
            }
            let _ = self.conn.execute(
                "INSERT INTO addresses(email, name, count) VALUES(?1, ?2, 1) \
                 ON CONFLICT(email) DO UPDATE SET count = count + 1, \
                   name = CASE WHEN excluded.name <> '' THEN excluded.name ELSE addresses.name END",
                params![email, name.trim()],
            );
        }
    }

    /// Aggregate every address seen in stored mail (senders received +
    /// recipients sent/recorded), as (name, email, frequency), most-frequent first.
    pub fn address_history(&self) -> Vec<(String, String, u32)> {
        use std::collections::HashMap;
        let mut counts: HashMap<String, (String, u32)> = HashMap::new();
        fn bump(counts: &mut HashMap<String, (String, u32)>, name: &str, email: &str, n: u32) {
            let email = email.trim();
            if email.is_empty() || !email.contains('@') {
                return;
            }
            let key = email.to_lowercase();
            let entry = counts.entry(key).or_insert_with(|| (String::new(), 0));
            entry.1 += n;
            if entry.0.is_empty() && !name.trim().is_empty() && name.trim() != email {
                entry.0 = name.trim().to_string();
            }
            if entry.0.is_empty() {
                entry.0 = email.to_string();
            }
        }

        if let Ok(mut stmt) = self
            .conn
            .prepare("SELECT from_name, from_addr, recipients, cc FROM messages")
        {
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0).unwrap_or_default(),
                    r.get::<_, String>(1).unwrap_or_default(),
                    r.get::<_, String>(2).unwrap_or_default(),
                    r.get::<_, String>(3).unwrap_or_default(),
                ))
            });
            if let Ok(rows) = rows {
                for (from_name, from_addr, to, cc) in rows.flatten() {
                    bump(&mut counts, &from_name, &from_addr, 1);
                    for list in [to, cc] {
                        for addr in list.split(',') {
                            bump(&mut counts, addr.trim(), addr.trim(), 1);
                        }
                    }
                }
            }
        }

        if let Ok(mut stmt) = self.conn.prepare("SELECT name, email, count FROM addresses") {
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0).unwrap_or_default(),
                    r.get::<_, String>(1).unwrap_or_default(),
                    r.get::<_, u32>(2).unwrap_or(0),
                ))
            });
            if let Ok(rows) = rows {
                for (name, email, c) in rows.flatten() {
                    bump(&mut counts, &name, &email, c);
                }
            }
        }

        let mut out: Vec<(String, String, u32)> = counts
            .into_iter()
            .map(|(email, (name, n))| (name, email, n))
            .collect();
        out.sort_by(|a, b| b.2.cmp(&a.2));
        out
    }

    pub fn mark_folder_read(&self, account_id: u32, folder_path: &str) {
        let _ = self.conn.execute(
            "UPDATE messages SET unread = 0 WHERE account_id = ?1 AND folder_path = ?2",
            params![account_id, folder_path],
        );
    }

    pub fn set_starred(&self, account_id: u32, folder_path: &str, uid: u32, starred: bool) {
        let _ = self.conn.execute(
            "UPDATE messages SET starred = ?1 WHERE account_id = ?2 AND folder_path = ?3 AND uid = ?4",
            params![starred, account_id, folder_path, uid],
        );
    }

    pub fn delete_message(&self, account_id: u32, folder_path: &str, uid: u32) {
        let _ = self.conn.execute(
            "DELETE FROM messages WHERE account_id = ?1 AND folder_path = ?2 AND uid = ?3",
            params![account_id, folder_path, uid],
        );
        let _ = self.conn.execute(
            "DELETE FROM bodies WHERE account_id = ?1 AND folder_path = ?2 AND uid = ?3",
            params![account_id, folder_path, uid],
        );
        let _ = self.conn.execute(
            "DELETE FROM attachments WHERE account_id = ?1 AND folder_path = ?2 AND uid = ?3",
            params![account_id, folder_path, uid],
        );
        let _ = self.conn.execute(
            "DELETE FROM attachments_checked WHERE account_id = ?1 AND folder_path = ?2 AND uid = ?3",
            params![account_id, folder_path, uid],
        );
    }
}

/// Compare folder lists ignoring the volatile id, but including unread counts so
/// a changed count re-emits the list and refreshes the sidebar badges.
pub fn folders_equal(a: &[Folder], b: &[Folder]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| {
            x.path == y.path && x.name == y.name && x.kind == y.kind && x.unread == y.unread
        })
}

fn kind_to_i64(kind: FolderKind) -> i64 {
    match kind {
        FolderKind::Inbox => 0,
        FolderKind::Starred => 1,
        FolderKind::Sent => 2,
        FolderKind::Drafts => 3,
        FolderKind::Archive => 4,
        FolderKind::Junk => 5,
        FolderKind::Trash => 6,
        FolderKind::Custom => 7,
    }
}

fn kind_from_i64(v: i64) -> FolderKind {
    match v {
        0 => FolderKind::Inbox,
        1 => FolderKind::Starred,
        2 => FolderKind::Sent,
        3 => FolderKind::Drafts,
        4 => FolderKind::Archive,
        5 => FolderKind::Junk,
        6 => FolderKind::Trash,
        _ => FolderKind::Custom,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl Cache {
        fn in_memory() -> Cache {
            let conn = Connection::open_in_memory().unwrap();
            conn.execute_batch(SCHEMA).unwrap();
            Cache { conn }
        }
    }

    fn add_msg(c: &Cache, folder: &str, uid: u32, from: &str, subject: &str, ts: i64) {
        c.conn.execute(
            "INSERT INTO messages (account_id, folder_path, uid, from_name, from_addr, subject, date, ts, unread, starred, has_attachment) \
             VALUES (1, ?1, ?2, ?3, '', ?4, '', ?5, 0, 0, 1)",
            params![folder, uid, from, subject, ts],
        ).unwrap();
    }
    fn add_att(c: &Cache, folder: &str, uid: u32, idx: u32, name: &str, data: &[u8]) {
        c.conn.execute(
            "INSERT INTO attachments (account_id, folder_path, uid, idx, name, data) VALUES (1, ?1, ?2, ?3, ?4, ?5)",
            params![folder, uid, idx, name, data],
        ).unwrap();
    }
    fn add_folder(c: &Cache, path: &str, kind: FolderKind) {
        c.conn.execute(
            "INSERT INTO folders (account_id, path, name, kind, unread, ord) VALUES (1, ?1, ?1, ?2, 0, 0)",
            params![path, kind_to_i64(kind)],
        ).unwrap();
    }

    #[test]
    fn gallery_items_spans_folders_excluding_trash_junk_drafts() {
        let c = Cache::in_memory();
        add_folder(&c, "INBOX", FolderKind::Inbox);
        add_folder(&c, "Archive", FolderKind::Archive);
        add_folder(&c, "Trash", FolderKind::Trash);
        add_msg(&c, "INBOX", 1, "Alice", "Hi", 100);
        add_msg(&c, "Archive", 2, "Bob", "Report", 200);
        add_msg(&c, "Trash", 3, "Carol", "Old", 300);
        add_att(&c, "INBOX", 1, 0, "a.png", &[0u8; 4]); // small → data kept
        add_att(&c, "Archive", 2, 0, "big.bin", &[0u8; 20]); // over cap → data dropped
        add_att(&c, "Trash", 3, 0, "x.pdf", &[0u8; 4]); // Trash → excluded

        let items = c.gallery_items(1, 10, 50);
        assert_eq!(items.len(), 2, "inbox + archive, not trash");
        // Newest message first (ts DESC): Archive/Bob (200) before Inbox/Alice (100).
        assert_eq!(items[0].name, "big.bin");
        assert_eq!(items[0].folder_path, "Archive");
        assert_eq!(items[0].from_name, "Bob");
        assert!(items[0].data.is_none(), "over the cap → no bytes");
        assert_eq!(items[1].name, "a.png");
        assert_eq!(items[1].folder_path, "INBOX");
        assert_eq!(items[1].data.as_deref(), Some(&[0u8; 4][..]));
    }

    #[test]
    fn redecode_encoded_subjects_fixes_raw_encoded_words_in_place() {
        let c = Cache::in_memory();
        add_folder(&c, "INBOX", FolderKind::Inbox);
        // A subject an older build stored raw after aborting on the over-long word.
        let raw = "=?utf-8?Q?92=2Dyear=2Dold=20artist=20Sheila=20Hicks?=";
        add_msg(&c, "INBOX", 1, "Popova", raw, 100);
        add_msg(&c, "INBOX", 2, "Plain", "Already fine", 200);

        Cache::redecode_encoded_subjects(&c.conn);

        let subj: String = c
            .conn
            .query_row(
                "SELECT subject FROM messages WHERE uid = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(subj, "92-year-old artist Sheila Hicks");
        let plain: String = c
            .conn
            .query_row("SELECT subject FROM messages WHERE uid = 2", [], |r| r.get(0))
            .unwrap();
        assert_eq!(plain, "Already fine");
    }

    #[test]
    fn gallery_items_respects_limit() {
        let c = Cache::in_memory();
        add_folder(&c, "INBOX", FolderKind::Inbox);
        for uid in 1..=5 {
            add_msg(&c, "INBOX", uid, "X", "S", uid as i64);
            add_att(&c, "INBOX", uid, 0, "f.png", &[0u8; 2]);
        }
        assert_eq!(c.gallery_items(1, 10, 3).len(), 3);
    }
}
