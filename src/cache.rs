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
CREATE TABLE IF NOT EXISTS refs_repair (
    account_id  INTEGER NOT NULL,
    folder_path TEXT    NOT NULL,
    next_uid    INTEGER NOT NULL,
    done        INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (account_id, folder_path)
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
    preview        TEXT    NOT NULL DEFAULT '',
    PRIMARY KEY (account_id, folder_path, uid)
);
CREATE TABLE IF NOT EXISTS bodies (
    account_id  INTEGER NOT NULL,
    folder_path TEXT NOT NULL,
    uid         INTEGER NOT NULL,
    body        TEXT NOT NULL,
    PRIMARY KEY (account_id, folder_path, uid)
);
CREATE TABLE IF NOT EXISTS sender_checks (
    account_id  INTEGER NOT NULL,
    folder_path TEXT NOT NULL,
    uid         INTEGER NOT NULL,
    trust       TEXT NOT NULL,
    summary     TEXT NOT NULL,
    findings    TEXT NOT NULL,
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
CREATE TABLE IF NOT EXISTS outbox (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id  INTEGER NOT NULL,
    from_addr   TEXT    NOT NULL,
    rcpts       TEXT    NOT NULL,
    recipients  TEXT    NOT NULL,
    subject     TEXT    NOT NULL,
    preview     TEXT    NOT NULL DEFAULT '',
    raw         BLOB    NOT NULL,
    sent_path   TEXT,
    queued_at   INTEGER NOT NULL,
    attempts    INTEGER NOT NULL DEFAULT 0,
    last_error  TEXT    NOT NULL DEFAULT ''
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
/// v10: bodies are re-rendered with `cid:` image references resolved to `data:`
/// URIs, so cached bodies showing a broken image must be dropped and rebuilt.
/// v11: sender authentication is computed from the raw message at the same
/// moment the body is rendered, so dropping `bodies` re-fetches both together
/// and every cached message gains a verdict.
/// v12: plain-text bodies no longer bake `padding:16px` into the cached
/// document — the reader injects the default inset at render time — so cached
/// bodies carrying the old baked padding must be dropped and re-rendered.
const SCHEMA_VERSION: i64 = 12;

/// Most Message-IDs one cross-folder conversation lookup matches against. A
/// thread's ancestry is short in practice, and the references half of that query
/// is a scan — this keeps a pathological References header (some mailing lists
/// accumulate hundreds) from turning a message-open into a long one.
const THREAD_ID_LIMIT: usize = 24;

/// How many messages one conversation may pull in from other folders. A
/// conversation is rendered as a single document with every member's body
/// inlined, so this bounds both the fetching and the rendering.
const THREAD_MEMBER_LIMIT: usize = 50;

/// (Adding a *new* table needs no bump: `SCHEMA` runs `CREATE TABLE IF NOT
/// EXISTS` on every open, so an existing cache gains it in place. A bump is for
/// changing or invalidating what is already stored — it costs users a re-render
/// or a re-sync.)
///
/// The first version whose table *layout* matches the current `SCHEMA`. At or
/// above this, an upgrade only needs to drop the derived caches, not the
/// expensive message index (which would force a whole-mailbox re-sync).
const LAYOUT_VERSION: i64 = 6;

pub struct Cache {
    conn: Connection,
}

/// Narrow an existing path's permissions to `mode`, if it exists.
///
/// Best-effort by design: a missing sidecar or a filesystem without Unix modes
/// is not a reason to refuse to open the cache.
#[cfg(unix)]
fn restrict(path: &std::path::Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    if path.exists() {
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
    }
}

#[cfg(not(unix))]
fn restrict(_path: &std::path::Path, _mode: u32) {}

impl Cache {
    /// Open (creating if needed) the cache DB at `~/.local/share/vireo/cache.db`.
    pub fn open() -> rusqlite::Result<Cache> {
        // No `temp_dir` fallback: this database holds message bodies, attachment
        // bytes and the harvested address book, and a shared world-writable
        // directory is the wrong place for any of it. Without a data dir there
        // is no acceptable location, so fail instead of picking a bad one.
        let path = crate::config::data_base()
            .ok_or_else(|| {
                rusqlite::Error::InvalidPath(std::path::PathBuf::from(
                    "no XDG data directory for the mail cache",
                ))
            })?
            .join("vireo");
        let _ = std::fs::create_dir_all(&path);
        restrict(&path, 0o700);
        let db = path.join("cache.db");
        let conn = Connection::open(&db)?;
        // The whole mailbox lives here, so it gets at least the care
        // `accounts.toml` gets.
        restrict(&db, 0o600);

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
                 DROP TABLE IF EXISTS sender_checks;\
                 DROP TABLE IF EXISTS attachments;\
                 DROP TABLE IF EXISTS attachments_checked;",
            );
        } else if version < SCHEMA_VERSION {
            // The layout is current but `bodies` holds HTML rendered by an older
            // build. `LoadBody` serves that cache without ever re-fetching, so a
            // stale entry would survive forever — drop it and let it re-render on
            // next open. The message index is kept: it's expensive to rebuild.
            let _ = conn
                .execute_batch("DROP TABLE IF EXISTS bodies; DROP TABLE IF EXISTS sender_checks;");
        }
        conn.execute_batch(SCHEMA)?;
        // `messages` predates the preview column, and `CREATE TABLE IF NOT
        // EXISTS` leaves an existing table alone. Add it in place rather than
        // dropping the index, which would cost every user a full re-sync; the
        // error when it is already there is the expected outcome, not a problem.
        let _ = conn.execute("ALTER TABLE messages ADD COLUMN preview TEXT NOT NULL DEFAULT ''", []);
        // Previews cached by a build that showed MIME machinery or a tracking
        // link instead of the message: a multipart's boundary ("--b2=_cipk…") or
        // the rendered link a marketing mail opens with ("( https://…"). Clearing
        // them shows nothing until the folder syncs again, which beats showing
        // either.
        let _ = conn.execute(
            "UPDATE messages SET preview = ''              WHERE preview LIKE '--%' OR preview LIKE '( http%'",
            [],
        );
        if upgrading_index {
            Self::redecode_encoded_subjects(&conn);
        }
        let _ = conn.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION};"));

        // Concurrency: multiple account workers share this file.
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        let _ = conn.busy_timeout(Duration::from_secs(5));

        // SQLite creates the WAL sidecars itself, and only once WAL mode is on —
        // hence here rather than beside the `Connection::open` above. They hold
        // the same message data mid-transaction, so they get the same mode. The
        // 0700 on the directory is what actually keeps other users out; this is
        // in case the cache is ever moved somewhere less private.
        for suffix in ["-wal", "-shm", "-journal"] {
            let mut side = db.clone().into_os_string();
            side.push(suffix);
            restrict(std::path::Path::new(&side), 0o600);
        }

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
                "SELECT uid, from_name, from_addr, subject, date, ts, unread, starred, has_attachment, recipients, cc, message_id, references_, preview
                 FROM messages WHERE account_id = ?1 AND folder_path = ?2 ORDER BY uid DESC",
            )?;
            let rows = stmt.query_map(params![account_id, folder_path], |row| {
                let uid: u32 = row.get(0)?;
                let mut m = Message {
                    id: uid,
                    account_id,
                    folder_id,
                    uid,
                    from_name: row.get(1)?,
                    from_addr: row.get(2)?,
                    to: row.get(9)?,
                    cc: row.get(10)?,
                    subject: row.get(3)?,
                    preview: row.get(13)?,
                    body: String::new(),
                    date: row.get(4)?,
                    timestamp: row.get(5)?,
                    unread: row.get(6)?,
                    starred: row.get(7)?,
                    has_attachment: row.get(8)?,
                    message_id: row.get(11)?,
                    references: row.get(12)?,
                };
                // Rows written before NUL-scrubbing existed may still carry
                // one; GTK labels abort on interior NULs.
                m.scrub_nuls();
                Ok(m)
            })?;
            rows.collect()
        };
        let mut messages = run().unwrap_or_else(|e| {
            tracing::warn!("cache load_messages failed: {e}");
            Vec::new()
        });
        // Correct false "has attachment" flags: a message we've already fetched
        // and found to hold no real attachments (e.g. iCloud marketing mail that
        // is multipart/mixed but only wraps inline `cid:` images) should not show
        // the paperclip, even though its summary flag said otherwise.
        let attachmentless = self.attachmentless_uids(account_id, folder_path);
        if !attachmentless.is_empty() {
            for m in messages.iter_mut() {
                if m.has_attachment && attachmentless.contains(&m.uid) {
                    m.has_attachment = false;
                }
            }
        }
        messages
    }

    /// UIDs whose attachments have been fetched (`attachments_checked`) but which
    /// turned out to hold no real attachments — used to correct false "has
    /// attachment" summary flags.
    pub fn attachmentless_uids(
        &self,
        account_id: u32,
        folder_path: &str,
    ) -> std::collections::HashSet<u32> {
        let run = || -> rusqlite::Result<std::collections::HashSet<u32>> {
            let mut stmt = self.conn.prepare(
                "SELECT ac.uid FROM attachments_checked ac
                 WHERE ac.account_id = ?1 AND ac.folder_path = ?2
                   AND NOT EXISTS (
                       SELECT 1 FROM attachments a
                       WHERE a.account_id = ac.account_id
                         AND a.folder_path = ac.folder_path
                         AND a.uid = ac.uid
                   )",
            )?;
            let rows = stmt.query_map(params![account_id, folder_path], |row| row.get::<_, u32>(0))?;
            rows.collect()
        };
        run().unwrap_or_default()
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
                // Upsert rather than REPLACE so an empty preview cannot erase one
                // already stored: the background backfill re-fetches summaries
                // without asking for a body slice, and every message it touched
                // lost its preview.
                tx.execute(
                    "INSERT INTO messages
                     (account_id, folder_path, uid, from_name, from_addr, subject, date, ts, unread, starred, has_attachment, recipients, cc, message_id, references_, preview)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
                     ON CONFLICT(account_id, folder_path, uid) DO UPDATE SET
                       from_name = excluded.from_name,
                       from_addr = excluded.from_addr,
                       subject = excluded.subject,
                       date = excluded.date,
                       ts = excluded.ts,
                       unread = excluded.unread,
                       starred = excluded.starred,
                       has_attachment = excluded.has_attachment,
                       recipients = excluded.recipients,
                       cc = excluded.cc,
                       message_id = excluded.message_id,
                       references_ = excluded.references_,
                       preview = CASE WHEN excluded.preview = '' THEN messages.preview ELSE excluded.preview END",
                    params![
                        account_id, folder_path, m.uid, m.from_name, m.from_addr, m.subject,
                        m.date, m.timestamp, m.unread, m.starred, m.has_attachment, m.to, m.cc,
                        m.message_id, m.references, m.preview
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

    /// Every cached message across the account's folders that belongs to the same
    /// conversation as `ids` — messages naming one of those ids as their own
    /// Message-ID (an ancestor), or referencing one (a descendant, e.g. the reply
    /// you sent, filed away in Sent).
    ///
    /// Returns each message with the folder path it lives in, since the caller
    /// needs that both to label it and to fetch its body. Cache-only: the point
    /// is to assemble a conversation without going near the network (#21).
    /// A message's age is not a factor — what it answers is written in its
    /// headers whenever it was sent. [`THREAD_MEMBER_LIMIT`] caps what one
    /// conversation can drag in, which is the bound that matters: every member
    /// found is a body the reader will load and render.
    pub fn messages_by_thread_ids(
        &self,
        account_id: u32,
        ids: &[String],
    ) -> Vec<(String, Message)> {
        let ids: Vec<&String> = ids.iter().filter(|i| !i.is_empty()).take(THREAD_ID_LIMIT).collect();
        if ids.is_empty() {
            return Vec::new();
        }
        // One placeholder per id, twice: matched against message_id, then looked
        // for inside the space-separated references list.
        let slots = |offset: usize| -> String {
            (0..ids.len()).map(|i| format!("?{}", offset + i + 1)).collect::<Vec<_>>().join(", ")
        };
        // Binds are pushed ids-then-padded-then-account, so the padded copies sit
        // at n+1..2n. Reading from n+2 shifted every one of these by a slot and
        // ran the last comparison against the *account id* — which, coerced to
        // text, matched every message whose References contained that digit. A
        // one-message lookup came back with thousands.
        let refs_match = (0..ids.len())
            .map(|i| format!("instr(' ' || references_ || ' ', ?{}) > 0", ids.len() + i + 1))
            .collect::<Vec<_>>()
            .join(" OR ");
        let sql = format!(
            "SELECT folder_path, uid, from_name, from_addr, subject, date, ts, unread, starred,                     has_attachment, recipients, cc, message_id, references_, preview              FROM messages              WHERE account_id = ?{account} AND (message_id IN ({in_list}) OR {refs_match})              ORDER BY ts DESC LIMIT ?{limit}",
            account = ids.len() * 2 + 1,
            limit = ids.len() * 2 + 2,
            in_list = slots(0),
            refs_match = refs_match,
        );
        let run = || -> rusqlite::Result<Vec<(String, Message)>> {
            let mut stmt = self.conn.prepare(&sql)?;
            // Bind order: the ids themselves, then each padded with spaces so a
            // substring match can't half-match a longer id, then the account.
            let padded: Vec<String> = ids.iter().map(|i| format!(" {i} ")).collect();
            let mut binds: Vec<&dyn rusqlite::ToSql> = Vec::new();
            for i in &ids {
                binds.push(*i as &dyn rusqlite::ToSql);
            }
            for p in &padded {
                binds.push(p as &dyn rusqlite::ToSql);
            }
            binds.push(&account_id as &dyn rusqlite::ToSql);
            let limit = THREAD_MEMBER_LIMIT as i64;
            binds.push(&limit as &dyn rusqlite::ToSql);
            let rows = stmt.query_map(binds.as_slice(), |row| {
                let uid: u32 = row.get(1)?;
                let mut m = Message {
                        id: uid,
                        account_id,
                        folder_id: 0, // filled in by the caller, which knows the ids
                        uid,
                        from_name: row.get(2)?,
                        from_addr: row.get(3)?,
                        to: row.get(10)?,
                        cc: row.get(11)?,
                        subject: row.get(4)?,
                        preview: row.get(14)?,
                        body: String::new(),
                        date: row.get(5)?,
                        timestamp: row.get(6)?,
                        unread: row.get(7)?,
                        starred: row.get(8)?,
                        has_attachment: row.get(9)?,
                        message_id: row.get(12)?,
                        references: row.get(13)?,
                };
                m.scrub_nuls();
                Ok((row.get::<_, String>(0)?, m))
            })?;
            rows.collect()
        };
        run().unwrap_or_else(|e| {
            tracing::warn!("cache messages_by_thread_ids failed: {e}");
            Vec::new()
        })
    }

    /// The next chunk of *replies* whose threading references are still the
    /// In-Reply-To-only ones an ENVELOPE fetch gives — a single id, or none.
    ///
    /// Replies only, and that is the whole economy of this: a message that opens
    /// a conversation has no References and never will, so asking the server for
    /// its header buys nothing. On a real 71k-message cache 68k rows hold a thin
    /// reference and only 3.9k of them are replies, so targeting them turns days
    /// of fetching into minutes.
    ///
    /// "None at all" cannot be told from a message that genuinely has no
    /// References, hence the `next_uid` watermark: each pass walks strictly
    /// downwards, so a message with nothing to find is visited once and never
    /// again.
    pub fn uids_needing_references(
        &self,
        account_id: u32,
        folder_path: &str,
        below_uid: u32,
        limit: usize,
    ) -> Vec<(u32, String)> {
        let run = || -> rusqlite::Result<Vec<(u32, String)>> {
            let mut stmt = self.conn.prepare(
                "SELECT uid, references_ FROM messages \
                 WHERE account_id = ?1 AND folder_path = ?2 AND uid < ?3 \
                   AND instr(trim(references_), ' ') = 0 \
                   AND (references_ <> '' OR subject LIKE 'Re:%' OR subject LIKE 'Fwd:%' \
                        OR subject LIKE 'Fw:%' OR subject LIKE 'Aw:%' OR subject LIKE 'Sv:%') \
                 ORDER BY uid DESC LIMIT ?4",
            )?;
            let rows = stmt.query_map(
                params![account_id, folder_path, below_uid, limit as i64],
                |row| Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?)),
            )?;
            rows.collect()
        };
        run().unwrap_or_default()
    }

    pub fn set_references(&self, account_id: u32, folder_path: &str, uid: u32, references: &str) {
        let _ = self.conn.execute(
            "UPDATE messages SET references_ = ?1 \
             WHERE account_id = ?2 AND folder_path = ?3 AND uid = ?4",
            params![references, account_id, folder_path, uid],
        );
    }

    /// How far down a folder's References repair has walked: the uid to look
    /// below next, and whether the pass has finished. A folder never touched
    /// starts at the top (`u32::MAX`), so the first chunk is its newest mail.
    pub fn refs_repair_state(&self, account_id: u32, folder_path: &str) -> (u32, bool) {
        self.conn
            .query_row(
                "SELECT next_uid, done FROM refs_repair \
                 WHERE account_id = ?1 AND folder_path = ?2",
                params![account_id, folder_path],
                |row| Ok((row.get::<_, u32>(0)?, row.get::<_, i64>(1)? != 0)),
            )
            .unwrap_or((u32::MAX, false))
    }

    pub fn set_refs_repair_state(
        &self,
        account_id: u32,
        folder_path: &str,
        next_uid: u32,
        done: bool,
    ) {
        let _ = self.conn.execute(
            "INSERT INTO refs_repair (account_id, folder_path, next_uid, done) \
             VALUES (?1, ?2, ?3, ?4) \
             ON CONFLICT(account_id, folder_path) DO UPDATE SET next_uid = ?3, done = ?4",
            params![account_id, folder_path, next_uid, done as i64],
        );
    }

    pub fn load_body(&self, account_id: u32, folder_path: &str, uid: u32) -> Option<String> {
        self.body_of(account_id, folder_path, uid).or_else(|| {
            self.sibling_copies(account_id, folder_path, uid)
                .into_iter()
                .find_map(|(p, u)| self.body_of(account_id, &p, u))
        })
    }

    fn body_of(&self, account_id: u32, folder_path: &str, uid: u32) -> Option<String> {
        self.conn
            .query_row(
                "SELECT body FROM bodies WHERE account_id = ?1 AND folder_path = ?2 AND uid = ?3",
                params![account_id, folder_path, uid],
                |row| row.get::<_, String>(0),
            )
            .ok()
    }

    /// Every other copy of this message in the account, as `(folder_path, uid)`.
    ///
    /// Gmail exposes labels as IMAP folders, so one message is stored several
    /// times over — INBOX, All Mail, Important — under a different UID in each.
    /// Everything keyed by (folder, uid) is therefore per-*copy*: a body read
    /// while the user was in All Mail is invisible to the INBOX copy, and its
    /// attachments would be downloaded a second time. They are the same mail, so
    /// a miss on one copy is answered from another instead of from the network.
    /// On a real cache 176 messages have a body under only some of their labels,
    /// and 31 have their attachments downloaded under only some.
    ///
    /// Copies must agree on sender and timestamp as well as Message-ID. The id is
    /// supposed to be unique, but it is written by whoever sent the mail, and
    /// spam and some list software reuse one across genuinely different messages
    /// — where serving one message's body for another would be worse than the
    /// cache miss this avoids. Gmail's own copies agree on all three (verified
    /// across a real 8300-message cache: no Message-ID there covers two
    /// different senders, subjects or timestamps), so the fallback still fires
    /// where it was meant to. Subject is deliberately not compared: a re-decoded
    /// encoded-word can rewrite it under one label and not another.
    fn sibling_copies(&self, account_id: u32, folder_path: &str, uid: u32) -> Vec<(String, u32)> {
        let run = || -> rusqlite::Result<Vec<(String, u32)>> {
            let mut stmt = self.conn.prepare(
                "SELECT m.folder_path, m.uid FROM messages m \
                 JOIN messages orig ON orig.account_id = ?1 \
                                   AND orig.folder_path = ?2 AND orig.uid = ?3 \
                 WHERE m.account_id = ?1 AND m.message_id != '' \
                   AND m.message_id = orig.message_id \
                   AND m.from_addr = orig.from_addr \
                   AND m.ts = orig.ts \
                   AND NOT (m.folder_path = ?2 AND m.uid = ?3)",
            )?;
            let rows = stmt.query_map(params![account_id, folder_path, uid], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?;
            rows.collect()
        };
        run().unwrap_or_else(|e| {
            tracing::warn!("cache sibling_copies failed: {e}");
            Vec::new()
        })
    }

    pub fn save_body(&self, account_id: u32, folder_path: &str, uid: u32, body: &str) {
        if let Err(e) = self.conn.execute(
            "INSERT OR REPLACE INTO bodies (account_id, folder_path, uid, body) VALUES (?1, ?2, ?3, ?4)",
            params![account_id, folder_path, uid, body],
        ) {
            tracing::warn!("cache save_body failed: {e}");
        }
    }

    /// The cached sender-authentication verdict for a message, if one was stored
    /// when its body was fetched.
    pub fn load_sender_check(
        &self,
        account_id: u32,
        folder_path: &str,
        uid: u32,
    ) -> Option<crate::models::SenderCheck> {
        self.sender_check_of(account_id, folder_path, uid).or_else(|| {
            self.sibling_copies(account_id, folder_path, uid)
                .into_iter()
                .find_map(|(p, u)| self.sender_check_of(account_id, &p, u))
        })
    }

    fn sender_check_of(
        &self,
        account_id: u32,
        folder_path: &str,
        uid: u32,
    ) -> Option<crate::models::SenderCheck> {
        self.conn
            .query_row(
                "SELECT trust, summary, findings FROM sender_checks \
                 WHERE account_id = ?1 AND folder_path = ?2 AND uid = ?3",
                params![account_id, folder_path, uid],
                |row| {
                    Ok(crate::models::SenderCheck {
                        trust: crate::models::SenderTrust::from_tag(&row.get::<_, String>(0)?),
                        summary: row.get(1)?,
                        findings: row
                            .get::<_, String>(2)?
                            .lines()
                            .filter(|l| !l.is_empty())
                            .map(str::to_string)
                            .collect(),
                    })
                },
            )
            .ok()
    }

    pub fn save_sender_check(
        &self,
        account_id: u32,
        folder_path: &str,
        uid: u32,
        check: &crate::models::SenderCheck,
    ) {
        if let Err(e) = self.conn.execute(
            "INSERT OR REPLACE INTO sender_checks \
             (account_id, folder_path, uid, trust, summary, findings) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                account_id,
                folder_path,
                uid,
                check.trust.as_tag(),
                check.summary,
                check.findings.join("\n")
            ],
        ) {
            tracing::warn!("cache save_sender_check failed: {e}");
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
        let own = self.attachments_of(account_id, folder_path, uid);
        if !own.is_empty() {
            return own;
        }
        // Downloaded under another of this message's labels: same mail, same
        // attachments, no reason to fetch them again.
        self.sibling_copies(account_id, folder_path, uid)
            .into_iter()
            .map(|(p, u)| self.attachments_of(account_id, &p, u))
            .find(|items| !items.is_empty())
            .unwrap_or_default()
    }

    fn attachments_of(&self, account_id: u32, folder_path: &str, uid: u32) -> Vec<Attachment> {
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

    // -----------------------------------------------------------------------
    // Outbox: messages that could not be sent yet
    // -----------------------------------------------------------------------

    /// Queue a message that failed to send. Returns its Outbox id.
    #[allow(clippy::too_many_arguments)]
    pub fn queue_outbox(
        &self,
        account_id: u32,
        from_addr: &str,
        rcpts: &[String],
        recipients: &str,
        subject: &str,
        preview: &str,
        raw: &[u8],
        sent_path: Option<&str>,
        error: &str,
    ) -> Option<u32> {
        let queued_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        self.conn
            .execute(
                "INSERT INTO outbox(account_id, from_addr, rcpts, recipients, subject,                  preview, raw, sent_path, queued_at, attempts, last_error)                  VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1, ?10)",
                params![
                    account_id,
                    from_addr,
                    rcpts.join("\n"),
                    recipients,
                    subject,
                    preview,
                    raw,
                    sent_path,
                    queued_at,
                    error,
                ],
            )
            .map_err(|e| tracing::warn!("could not queue the message: {e}"))
            .ok()?;
        Some(self.conn.last_insert_rowid() as u32)
    }

    /// Everything waiting for this account, oldest first (the order it is sent in).
    pub fn outbox_items(&self, account_id: u32) -> Vec<crate::models::OutboxItem> {
        let mut stmt = match self.conn.prepare(
            "SELECT id, account_id, from_addr, rcpts, recipients, subject, preview, raw,              sent_path, queued_at, attempts, last_error FROM outbox              WHERE account_id = ?1 ORDER BY queued_at, id",
        ) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("could not read the outbox: {e}");
                return Vec::new();
            }
        };
        let rows = stmt.query_map(params![account_id], |r| {
            let rcpts: String = r.get(3)?;
            Ok(crate::models::OutboxItem {
                id: r.get::<_, i64>(0)? as u32,
                account_id: r.get::<_, i64>(1)? as u32,
                from_addr: r.get(2)?,
                rcpts: rcpts.lines().map(str::to_string).collect(),
                recipients: r.get(4)?,
                subject: r.get(5)?,
                preview: r.get(6)?,
                raw: r.get(7)?,
                sent_path: r.get(8)?,
                queued_at: r.get(9)?,
                attempts: r.get::<_, i64>(10)? as u32,
                last_error: r.get(11)?,
            })
        });
        match rows {
            Ok(rows) => rows.filter_map(Result::ok).collect(),
            Err(e) => {
                tracing::warn!("could not read the outbox: {e}");
                Vec::new()
            }
        }
    }

    /// Correct a message's attachment flag once its body has proved the answer,
    /// so the paperclip survives a restart rather than being re-guessed.
    pub fn set_has_attachment(&self, account_id: u32, folder_path: &str, uid: u32, has: bool) {
        let _ = self.conn.execute(
            "UPDATE messages SET has_attachment = ?4 \
             WHERE account_id = ?1 AND folder_path = ?2 AND uid = ?3",
            params![account_id, folder_path, uid, has],
        );
    }

    /// Drop a queued message — it went out, or the user discarded it.
    pub fn delete_outbox(&self, id: u32) {
        let _ = self.conn.execute("DELETE FROM outbox WHERE id = ?1", params![id]);
    }

    /// Record another failed attempt, so the list can say what went wrong.
    pub fn record_outbox_failure(&self, id: u32, error: &str) {
        let _ = self.conn.execute(
            "UPDATE outbox SET attempts = attempts + 1, last_error = ?2 WHERE id = ?1",
            params![id, error],
        );
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

    /// The cache holds message bodies, attachment bytes and the address book, so
    /// it should be no more readable than `accounts.toml` is.
    ///
    /// Ignored by default: it sets `XDG_DATA_HOME`, which is process-global and
    /// would leak into tests running beside it. Run it on its own:
    ///
    /// ```text
    /// cargo test -- --ignored --test-threads=1 the_cache_is_not_world_readable
    /// ```
    #[test]
    #[ignore = "mutates process-global environment"]
    #[cfg(unix)]
    fn the_cache_is_not_world_readable() {
        use std::os::unix::fs::PermissionsExt;

        let base = std::env::temp_dir().join(format!("vireo-cache-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::env::set_var("XDG_DATA_HOME", &base);

        let dir = base.join("vireo");
        // Start from the permissions the old code left behind, to prove an
        // existing cache is tightened rather than only a freshly created one.
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(dir.join("cache.db"), b"").unwrap();
        std::fs::set_permissions(dir.join("cache.db"), std::fs::Permissions::from_mode(0o644))
            .unwrap();

        let cache = Cache::open().expect("cache opens");
        drop(cache);

        let mode = |p: &std::path::Path| {
            std::fs::metadata(p).unwrap().permissions().mode() & 0o777
        };
        assert_eq!(mode(&dir), 0o700, "cache directory");
        assert_eq!(mode(&dir.join("cache.db")), 0o600, "cache.db");
        for side in ["cache.db-wal", "cache.db-shm"] {
            let p = dir.join(side);
            if p.exists() {
                assert_eq!(mode(&p), 0o600, "{side}");
            }
        }

        std::env::remove_var("XDG_DATA_HOME");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn outbox_round_trips_a_queued_message() {
        let c = Cache::in_memory();
        let rcpts = vec!["ada@example.com".to_string(), "bcc@example.com".to_string()];
        let id = c
            .queue_outbox(
                1,
                "me@example.com",
                &rcpts,
                "Ada Lovelace <ada@example.com>",
                "Notes",
                "the body preview",
                b"From: me\r\n\r\nbody",
                Some("Sent"),
                "connection refused",
            )
            .expect("queued");

        let items = c.outbox_items(1);
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item.id, id);
        // The envelope has to survive verbatim — Bcc exists only here, so losing a
        // recipient silently drops someone from the message.
        assert_eq!(item.rcpts, rcpts);
        assert_eq!(item.from_addr, "me@example.com");
        assert_eq!(item.subject, "Notes");
        assert_eq!(item.preview, "the body preview");
        assert_eq!(item.raw, b"From: me\r\n\r\nbody");
        assert_eq!(item.sent_path.as_deref(), Some("Sent"));
        assert_eq!(item.attempts, 1);
        assert_eq!(item.last_error, "connection refused");
        assert!(item.queued_at > 0);

        // Another account's queue is its own.
        assert!(c.outbox_items(2).is_empty());

        c.record_outbox_failure(id, "no route to host");
        let item = c.outbox_items(1).remove(0);
        assert_eq!(item.attempts, 2);
        assert_eq!(item.last_error, "no route to host");

        c.delete_outbox(id);
        assert!(c.outbox_items(1).is_empty());
    }

    #[test]
    fn outbox_keeps_the_order_messages_were_queued_in() {
        let c = Cache::in_memory();
        for (subject, at) in [("second", 200), ("first", 100), ("third", 300)] {
            c.conn
                .execute(
                    "INSERT INTO outbox(account_id, from_addr, rcpts, recipients, subject, \
                     preview, raw, sent_path, queued_at, attempts, last_error) \
                     VALUES(1, 'me@example.com', 'a@b.com', '', ?1, '', x'00', NULL, ?2, 1, '')",
                    params![subject, at],
                )
                .unwrap();
        }
        let subjects: Vec<String> = c.outbox_items(1).into_iter().map(|i| i.subject).collect();
        assert_eq!(subjects, ["first", "second", "third"]);
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

    /// Insert a message carrying threading headers.
    /// Like [`add_threaded`], but with a sender — for the checks that a shared
    /// Message-ID alone is not enough to call two rows the same mail.
    fn add_from(c: &Cache, folder: &str, uid: u32, ts: i64, msgid: &str, from: &str) {
        c.conn
            .execute(
                "INSERT INTO messages (account_id, folder_path, uid, from_name, from_addr, subject, date, ts, unread, starred, has_attachment, message_id, references_) \
                 VALUES (1, ?1, ?2, 'X', ?5, 'S', '', ?3, 0, 0, 0, ?4, '')",
                params![folder, uid, ts, msgid, from],
            )
            .unwrap();
    }

    fn add_threaded(c: &Cache, folder: &str, uid: u32, ts: i64, msgid: &str, refs: &str) {
        c.conn
            .execute(
                "INSERT INTO messages (account_id, folder_path, uid, from_name, from_addr, subject, date, ts, unread, starred, has_attachment, message_id, references_) \
                 VALUES (1, ?1, ?2, 'X', '', 'S', '', ?3, 0, 0, 0, ?4, ?5)",
                params![folder, uid, ts, msgid, refs],
            )
            .unwrap();
    }

    /// A conversation lookup must return the messages that actually reference the
    /// one being opened — nothing else.
    ///
    /// The bind indices for the `references_` comparisons were off by a slot, so
    /// the last one ran against the *account id*. Coerced to text, `instr` then
    /// matched every message whose References merely contained that digit, and
    /// one click dragged thousands of messages into the reader — each of them a
    /// body to fetch and re-render. That is worth a test.
    /// The repair asks the server only about replies. A message that opens a
    /// conversation has no References and never will, so fetching its header
    /// buys nothing — and on a real cache that distinction is the difference
    /// between 68k messages and 4k.
    #[test]
    fn only_replies_are_worth_repairing() {
        let c = Cache::in_memory();
        // A reply whose References is the single id an ENVELOPE gives.
        c.conn.execute(
            "INSERT INTO messages (account_id, folder_path, uid, from_name, from_addr, subject, date, ts, unread, starred, has_attachment, message_id, references_) \
             VALUES (1,'INBOX',10,'X','','Re: hello','',1,0,0,0,'a@x','parent@x')",
            [],
        ).unwrap();
        // A thread opener: no prefix, no references. Nothing to find.
        c.conn.execute(
            "INSERT INTO messages (account_id, folder_path, uid, from_name, from_addr, subject, date, ts, unread, starred, has_attachment, message_id, references_) \
             VALUES (1,'INBOX',11,'X','','hello','',1,0,0,0,'b@x','')",
            [],
        ).unwrap();
        // Already complete: two ids, from a full header parse.
        c.conn.execute(
            "INSERT INTO messages (account_id, folder_path, uid, from_name, from_addr, subject, date, ts, unread, starred, has_attachment, message_id, references_) \
             VALUES (1,'INBOX',12,'X','','Re: hello','',1,0,0,0,'c@x','root@x parent@x')",
            [],
        ).unwrap();

        let want = c.uids_needing_references(1, "INBOX", u32::MAX, 100);
        let uids: Vec<u32> = want.iter().map(|(u, _)| *u).collect();
        assert_eq!(uids, vec![10], "only the thin reply");
    }

    /// The watermark walks strictly down, so a message with nothing to find is
    /// asked about once rather than on every pass forever.
    #[test]
    fn the_repair_walks_downwards_and_finishes() {
        let c = Cache::in_memory();
        for uid in [10u32, 20, 30] {
            c.conn.execute(
                "INSERT INTO messages (account_id, folder_path, uid, from_name, from_addr, subject, date, ts, unread, starred, has_attachment, message_id, references_) \
                 VALUES (1,'INBOX',?1,'X','','Re: t','',1,0,0,0,'m@x','p@x')",
                params![uid],
            ).unwrap();
        }
        assert_eq!(c.refs_repair_state(1, "INBOX"), (u32::MAX, false), "starts at the top");
        let first = c.uids_needing_references(1, "INBOX", u32::MAX, 2);
        assert_eq!(first.iter().map(|(u, _)| *u).collect::<Vec<_>>(), vec![30, 20], "newest first");

        c.set_refs_repair_state(1, "INBOX", 20, false);
        let next = c.uids_needing_references(1, "INBOX", 20, 2);
        assert_eq!(next.iter().map(|(u, _)| *u).collect::<Vec<_>>(), vec![10], "strictly below");

        c.set_refs_repair_state(1, "INBOX", 0, true);
        assert!(c.refs_repair_state(1, "INBOX").1, "and it finishes");
    }

    #[test]
    fn a_conversation_holds_only_messages_that_reference_it() {
        let c = Cache::in_memory();
        add_threaded(&c, "INBOX", 1, 500, "root@x", "");
        add_threaded(&c, "Sent", 2, 600, "reply@x", "root@x");
        // Unrelated, but its References contain the digit "1" — the account id.
        add_threaded(&c, "Archive", 3, 550, "other@x", "31337@elsewhere");

        let found = c.messages_by_thread_ids(1, &["root@x".to_string()]);
        let ids: Vec<&str> = found.iter().map(|(_, m)| m.message_id.as_str()).collect();
        assert_eq!(ids, vec!["reply@x", "root@x"], "only the real conversation");
    }

    /// Gmail stores one message under every label it carries, so INBOX, All Mail
    /// and Important hold three copies with three UIDs. A body read under one
    /// label has to answer for the others, or opening the conversation from the
    /// Inbox re-downloads mail already in hand — and its attachments come back
    /// as a "Load attachments" button rather than the files themselves.
    #[test]
    fn a_body_cached_under_one_gmail_label_answers_for_the_others() {
        let c = Cache::in_memory();
        add_threaded(&c, "INBOX", 42, 500, "same@x", "");
        add_threaded(&c, "[Gmail]/All Mail", 900, 500, "same@x", "");
        c.save_body(1, "[Gmail]/All Mail", 900, "the real body");

        assert_eq!(c.load_body(1, "INBOX", 42).as_deref(), Some("the real body"));
        // The copy that actually holds it still answers directly.
        assert_eq!(
            c.load_body(1, "[Gmail]/All Mail", 900).as_deref(),
            Some("the real body")
        );
    }

    #[test]
    fn attachments_downloaded_under_one_label_answer_for_the_others() {
        let c = Cache::in_memory();
        add_threaded(&c, "INBOX", 42, 500, "same@x", "");
        add_threaded(&c, "Paratype", 7, 500, "same@x", "");
        c.save_attachments(
            1,
            "Paratype",
            7,
            &[Attachment { name: "spec.pdf".into(), data: vec![1, 2, 3] }],
        );

        let found = c.load_attachments(1, "INBOX", 42);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "spec.pdf");
    }

    /// Two different messages must never answer for each other — the fallback
    /// keys on Message-ID, and a message that has none has nothing to match.
    #[test]
    fn a_different_message_never_answers_for_this_one() {
        let c = Cache::in_memory();
        add_threaded(&c, "INBOX", 42, 500, "mine@x", "");
        add_threaded(&c, "[Gmail]/All Mail", 900, 500, "someone-else@x", "");
        c.save_body(1, "[Gmail]/All Mail", 900, "not yours");
        assert_eq!(c.load_body(1, "INBOX", 42), None);

        add_threaded(&c, "INBOX", 43, 500, "", "");
        add_threaded(&c, "Archive", 44, 500, "", "");
        c.save_body(1, "Archive", 44, "unrelated");
        assert_eq!(
            c.load_body(1, "INBOX", 43),
            None,
            "an empty Message-ID matches nothing, not everything"
        );
    }

    /// Message-ID is written by the sender, and spam reuses one across unrelated
    /// mail. Serving one message's body for another would be worse than the
    /// cache miss the fallback exists to avoid, so sender and timestamp have to
    /// agree too.
    #[test]
    fn a_reused_message_id_from_a_different_sender_answers_for_nothing() {
        let c = Cache::in_memory();
        add_from(&c, "INBOX", 42, 500, "dup@x", "me@real.example");
        add_from(&c, "Archive", 900, 500, "dup@x", "spammer@fake.example");
        c.save_body(1, "Archive", 900, "not the same mail");
        assert_eq!(c.load_body(1, "INBOX", 42), None);

        // Same sender, but a different message that happens to reuse the id.
        add_from(&c, "INBOX", 43, 500, "dup2@x", "me@real.example");
        add_from(&c, "Archive", 901, 900, "dup2@x", "me@real.example");
        c.save_body(1, "Archive", 901, "a different day's mail");
        assert_eq!(c.load_body(1, "INBOX", 43), None);
    }

    /// Re-adding an account re-downloads mail that is all older than the moment
    /// it was added. Threading reads what a message answers, not when it was
    /// sent, so every member belongs to the conversation regardless of age.
    #[test]
    fn a_conversation_holds_its_members_however_old_they_are() {
        let c = Cache::in_memory();
        add_threaded(&c, "INBOX", 1, 500, "root@x", "");
        add_threaded(&c, "Sent", 2, 400, "old-reply@x", "root@x");
        add_threaded(&c, "Sent", 3, 600, "new-reply@x", "root@x");

        let found = c.messages_by_thread_ids(1, &["root@x".to_string()]);
        let mut ids: Vec<&str> = found.iter().map(|(_, m)| m.message_id.as_str()).collect();
        ids.sort_unstable();
        assert_eq!(ids, vec!["new-reply@x", "old-reply@x", "root@x"], "all three, any age");
    }
}
