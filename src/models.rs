//! Core domain types shared across the UI and backend layers.

/// A configured mail account (one IMAP/SMTP identity).
#[derive(Debug, Clone)]
#[allow(dead_code)] // `id` and `accent` are used once multi-account lands.
pub struct Account {
    pub id: u32,
    pub name: String,
    pub email: String,
    /// How the account is labelled in the UI (All Inboxes, reader chip). Defaults
    /// to the email address.
    pub label: String,
    /// Accent colour used for the account dot, as a CSS colour string.
    pub accent: String,
}

/// The well-known role of a folder, used to pick an icon and ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderKind {
    Inbox,
    Starred,
    Sent,
    Drafts,
    Archive,
    Junk,
    Trash,
    Custom,
}

impl FolderKind {
    pub fn icon(self) -> &'static str {
        match self {
            FolderKind::Inbox => "com.getveem.Veem-mail-inbox-symbolic",
            FolderKind::Starred => "com.getveem.Veem-starred-symbolic",
            FolderKind::Sent => "com.getveem.Veem-mail-send-symbolic",
            FolderKind::Drafts => "com.getveem.Veem-document-edit-symbolic",
            FolderKind::Archive => "com.getveem.Veem-mail-archive-symbolic",
            FolderKind::Junk => "com.getveem.Veem-mail-mark-junk-symbolic",
            FolderKind::Trash => "com.getveem.Veem-user-trash-symbolic",
            FolderKind::Custom => "com.getveem.Veem-folder-symbolic",
        }
    }
}

/// A mail folder within an account.
#[derive(Debug, Clone)]
pub struct Folder {
    pub id: u32,
    pub account_id: u32,
    pub name: String,
    /// IMAP mailbox path (e.g. "INBOX", "INBOX.Sent"). For the mock backend
    /// this mirrors `name`.
    pub path: String,
    pub kind: FolderKind,
    pub unread: u32,
}

/// A single message (summary + body). In a real backend the body is loaded
/// lazily; here it is always present.
#[derive(Debug, Clone)]
pub struct Message {
    pub id: u32,
    /// Owning account; needed to route actions and to merge the unified inbox.
    pub account_id: u32,
    pub folder_id: u32,
    /// IMAP UID, used to fetch the full body lazily. Mock data reuses `id`.
    pub uid: u32,
    pub from_name: String,
    pub from_addr: String,
    /// Original recipients (comma-separated emails), used for Reply All.
    pub to: String,
    pub cc: String,
    pub subject: String,
    pub preview: String,
    pub body: String,
    /// Human-readable timestamp for display.
    pub date: String,
    /// Unix timestamp (seconds) for sorting, e.g. the unified inbox. 0 if unknown.
    pub timestamp: i64,
    pub unread: bool,
    pub starred: bool,
    pub has_attachment: bool,
    /// This message's own Message-ID (normalized, no angle brackets). Empty if
    /// unknown. Used to thread replies accurately (instead of by subject alone).
    pub message_id: String,
    /// Referenced Message-IDs (In-Reply-To + References), space-separated and
    /// normalized. Links a reply to the messages it descends from.
    pub references: String,
}

/// Identifies an existing draft being edited, so saving/sending replaces it
/// (removing the previous version from the Drafts folder).
#[derive(Debug, Clone)]
pub struct DraftOrigin {
    pub account_id: u32,
    pub folder_id: u32,
    pub path: String,
    pub uid: u32,
}

/// A decoded message attachment (fetched on demand for the reader).
#[derive(Debug, Clone)]
pub struct Attachment {
    pub name: String,
    pub data: Vec<u8>,
}

impl Attachment {
    /// Human-readable size, e.g. "12.3 KB".
    pub fn human_size(&self) -> String {
        human_size(self.data.len() as u64)
    }
}

/// Human-readable byte size, e.g. "12.3 KB".
pub fn human_size(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1_048_576.0 {
        format!("{:.1} MB", b / 1_048_576.0)
    } else if b >= 1024.0 {
        format!("{:.1} KB", b / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

/// Whether a filename looks like a raster image we can thumbnail/preview inline.
pub fn is_image_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".ico", ".heic", ".heif", ".avif"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

/// One attachment for the gallery: metadata plus the source message context.
/// `data` is loaded eagerly for small files (so the preview/open is instant) and
/// `None` for large ones (fetched on demand when opened).
#[derive(Debug, Clone)]
pub struct GalleryItem {
    pub account_id: u32,
    pub folder_path: String,
    pub uid: u32,
    pub name: String,
    pub size: u64,
    /// Sender display name of the source message.
    pub from_name: String,
    pub subject: String,
    /// Source message timestamp (for sorting, newest first).
    pub timestamp: i64,
    pub data: Option<Vec<u8>>,
}

impl GalleryItem {
    pub fn is_image(&self) -> bool {
        is_image_name(&self.name)
    }
    pub fn human_size(&self) -> String {
        human_size(self.size)
    }

    /// Compact date of the source message for the gallery meta, e.g. "Jul 12"
    /// (or "Jul 12, 2025" if not this year). Empty when the date is unknown.
    pub fn date_label(&self) -> String {
        use chrono::{Datelike, TimeZone};
        if self.timestamp <= 0 {
            return String::new();
        }
        let Some(local) = chrono::Local.timestamp_opt(self.timestamp, 0).single() else {
            return String::new();
        };
        if local.year() == chrono::Local::now().year() {
            local.format("%b %-d").to_string()
        } else {
            local.format("%b %-d, %Y").to_string()
        }
    }
}

impl Message {
    /// Full receipt date and time for the reader header, e.g.
    /// "Jun 27, 2026 at 3:15 PM". Falls back to the short label if unknown.
    pub fn datetime_full(&self) -> String {
        use chrono::TimeZone;
        if self.timestamp <= 0 {
            return self.date.clone();
        }
        chrono::Local
            .timestamp_opt(self.timestamp, 0)
            .single()
            .map(|dt| dt.format("%b %-d, %Y at %-I:%M %p").to_string())
            .unwrap_or_else(|| self.date.clone())
    }

    /// Compact date + time for list rows: always shows the time, with the day
    /// (and year, if not this year).
    pub fn datetime_list(&self) -> String {
        use chrono::{Datelike, TimeZone};
        if self.timestamp <= 0 {
            return self.date.clone();
        }
        let Some(dt) = chrono::Local.timestamp_opt(self.timestamp, 0).single() else {
            return self.date.clone();
        };
        let now = chrono::Local::now();
        if dt.date_naive() == now.date_naive() {
            dt.format("Today, %-I:%M %p").to_string()
        } else if dt.year() == now.year() {
            dt.format("%b %-d, %-I:%M %p").to_string()
        } else {
            dt.format("%b %-d %Y, %-I:%M %p").to_string()
        }
    }

}
