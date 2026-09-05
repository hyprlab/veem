//! Core domain types shared across the UI and backend layers.
use crate::i18n::{i18n, i18n_f};

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
            FolderKind::Inbox => "co.hyprlab.Vireo-mail-inbox-symbolic",
            FolderKind::Starred => "co.hyprlab.Vireo-starred-symbolic",
            FolderKind::Sent => "co.hyprlab.Vireo-mail-send-symbolic",
            FolderKind::Drafts => "co.hyprlab.Vireo-document-edit-symbolic",
            FolderKind::Archive => "co.hyprlab.Vireo-mail-archive-symbolic",
            FolderKind::Junk => "co.hyprlab.Vireo-mail-mark-junk-symbolic",
            FolderKind::Trash => "co.hyprlab.Vireo-user-trash-symbolic",
            FolderKind::Custom => "co.hyprlab.Vireo-folder-symbolic",
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
    /// Where the sender asked replies to go (comma-separated emails), from the
    /// Reply-To header. Empty when the header is absent — reply to `from_addr`.
    pub reply_to: String,
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

/// How much a message's claimed sender can be trusted, worst finding wins.
///
/// This answers "did this really come from the domain in the From: line?" — not
/// "is this message safe". A phisher who registers their own domain and
/// authenticates it properly earns [`SenderTrust::Pass`]; the check proves the
/// From: address wasn't forged, nothing more.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SenderTrust {
    /// The receiving server authenticated the From: domain (DMARC, or aligned
    /// SPF/DKIM). Forging this address would have been rejected.
    Pass,
    /// No usable authentication result — an old server, a POP3 account, or mail
    /// that predates the check. Says nothing either way.
    Unverified,
    /// Authenticated, but something about the addressing is off: a reply-to on
    /// another domain, or a display name impersonating one.
    Suspicious,
    /// Authentication failed. The From: address is very likely forged.
    Fail,
}

impl SenderTrust {
    /// Round-trip tag for the cache.
    pub fn as_tag(self) -> &'static str {
        match self {
            SenderTrust::Pass => "pass",
            SenderTrust::Unverified => "unverified",
            SenderTrust::Suspicious => "suspicious",
            SenderTrust::Fail => "fail",
        }
    }

    pub fn from_tag(tag: &str) -> SenderTrust {
        match tag {
            "pass" => SenderTrust::Pass,
            "suspicious" => SenderTrust::Suspicious,
            "fail" => SenderTrust::Fail,
            _ => SenderTrust::Unverified,
        }
    }

    /// Heading for the details popover, and the badge's tooltip. Not drawn on
    /// screen: the toolbar badge is the icon alone, coloured by verdict.
    pub fn label(self) -> String {
        i18n(match self {
            SenderTrust::Pass => "Verified sender",
            SenderTrust::Unverified => "Sender not verified",
            SenderTrust::Suspicious => "Check this sender",
            SenderTrust::Fail => "Possible forgery",
        })
    }

    /// CSS class for the badge's colour.
    pub fn css_class(self) -> &'static str {
        match self {
            SenderTrust::Pass => "trust-pass",
            SenderTrust::Unverified => "trust-unverified",
            SenderTrust::Suspicious => "trust-suspicious",
            SenderTrust::Fail => "trust-fail",
        }
    }

    /// Whether this verdict deserves a banner across the top of the message
    /// rather than just a badge beside the sender.
    pub fn is_alarming(self) -> bool {
        matches!(self, SenderTrust::Suspicious | SenderTrust::Fail)
    }
}

/// The result of checking whether a message's From: address was forged.
#[derive(Debug, Clone)]
pub struct SenderCheck {
    pub trust: SenderTrust,
    /// One-line plain-English verdict, shown in the banner and popover heading.
    pub summary: String,
    /// Supporting detail, one line each, for the "Details" popover.
    pub findings: Vec<String>,
}

impl Default for SenderCheck {
    fn default() -> Self {
        SenderCheck {
            trust: SenderTrust::Unverified,
            summary: i18n("This message hasn't been checked."),
            findings: Vec::new(),
        }
    }
}

/// A message that could not be sent and is waiting in the Outbox.
///
/// The built MIME bytes are stored rather than the composed fields: a retry has
/// to send exactly what was composed, and the attachments' files may be gone by
/// then — under Flatpak the portal paths handed to the file chooser expire.
/// `from_addr` and `rcpts` are the SMTP envelope, which is not always what the
/// headers say (Bcc is in the envelope only).
#[derive(Debug, Clone)]
pub struct OutboxItem {
    pub id: u32,
    pub account_id: u32,
    /// Envelope sender.
    pub from_addr: String,
    /// Envelope recipients (To + Cc + Bcc), one per line as stored.
    pub rcpts: Vec<String>,
    /// Header recipients, for display ("Ada Lovelace, bob@example.com").
    pub recipients: String,
    pub subject: String,
    pub preview: String,
    pub raw: Vec<u8>,
    /// The account's Sent folder at queue time, appended to once it goes out.
    pub sent_path: Option<String>,
    /// Unix seconds when it was first queued.
    pub queued_at: i64,
    pub attempts: u32,
    /// Why the last attempt failed, shown in the list.
    pub last_error: String,
}

impl OutboxItem {
    /// "Waiting since 10 minutes ago" — what the reader shows where a received
    /// message shows its date, since an unsent message has no send time yet.
    pub fn waiting_label(queued_at: i64) -> String {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(queued_at);
        let secs = (now - queued_at).max(0);
        let ago = match secs {
            s if s < 90 => "just now".to_string(),
            s if s < 3600 => format!("{} minutes ago", s / 60),
            s if s < 7200 => "an hour ago".to_string(),
            s if s < 86400 => i18n_f("{n} hours ago", &[("n", &(s / 3600).to_string())]),
            s if s < 172_800 => i18n("yesterday"),
            s => i18n_f("{n} days ago", &[("n", &(s / 86400).to_string())]),
        };
        i18n_f("Waiting since {ago}", &[("ago", &ago)])
    }

    /// The queued message as a list row. Everything the list needs is stored with
    /// the message, so the Outbox reads as an ordinary folder: same list, same
    /// reader, same sorting. `folder_id` is `OUTBOX_FOLDER_ID`, which no real
    /// folder uses, so a row can always be traced back here.
    pub fn as_message(&self) -> Message {
        Message {
            id: self.id,
            account_id: self.account_id,
            folder_id: OUTBOX_FOLDER_ID,
            uid: self.id,
            // The list's headline column is the sender everywhere else; for
            // unsent mail the useful name is who it is going to.
            from_name: if self.recipients.trim().is_empty() {
                "(no recipients)".to_string()
            } else {
                self.recipients.clone()
            },
            from_addr: self.from_addr.clone(),
            reply_to: String::new(),
            to: self.recipients.clone(),
            cc: String::new(),
            subject: if self.subject.trim().is_empty() {
                "(no subject)".to_string()
            } else {
                self.subject.clone()
            },
            // The row's one line of context: how long it has been stuck and what
            // is stopping it, falling back to the body when nothing has failed
            // yet (a message queued while offline never got an error).
            preview: {
                let waiting = Self::waiting_label(self.queued_at);
                let attempts = match self.attempts {
                    0 | 1 => String::new(),
                    n => format!(" · {n} attempts"),
                };
                let reason = if self.last_error.trim().is_empty() {
                    self.preview.trim().to_string()
                } else {
                    self.last_error.trim().to_string()
                };
                if reason.is_empty() {
                    format!("{waiting}{attempts}")
                } else {
                    format!("{waiting}{attempts} · {reason}")
                }
            },
            body: String::new(),
            date: String::new(),
            timestamp: self.queued_at,
            // Never dimmed as read: it is still waiting to go out.
            unread: true,
            starred: false,
            has_attachment: false,
            message_id: String::new(),
            references: String::new(),
        }
    }
}

/// Folder id for the Outbox's synthetic rows. Real folders are numbered from 1
/// by the worker, so this can't collide.
pub const OUTBOX_FOLDER_ID: u32 = u32::MAX;

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

/// The file extension matching an image's magic bytes ("jpg" when unsure —
/// for content that is known to be an image but arrived without a name).
pub fn image_ext(data: &[u8]) -> &'static str {
    match data {
        [0x89, b'P', b'N', b'G', ..] => "png",
        [b'G', b'I', b'F', b'8', ..] => "gif",
        [b'B', b'M', ..] => "bmp",
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => "webp",
        _ => "jpg",
    }
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
        if self.timestamp <= 0 {
            return String::new();
        }
        if crate::datefmt::year(self.timestamp) == crate::datefmt::year(crate::datefmt::now()) {
            crate::datefmt::day_month(self.timestamp)
        } else {
            crate::datefmt::day_month_year(self.timestamp)
        }
    }
}

impl Message {
    /// Strip interior NUL bytes from every text field. GTK's C strings end at
    /// the first NUL and glib panics rather than truncate when handed one
    /// mid-string — a single message with a stray 0x00 in its envelope (they
    /// exist in the wild) took the whole message list down with it. Called at
    /// the ingestion choke points (envelope parse, cache load), so nothing
    /// NUL-bearing ever reaches a label, tooltip, or document.
    pub fn scrub_nuls(&mut self) {
        for s in [
            &mut self.from_name,
            &mut self.from_addr,
            &mut self.reply_to,
            &mut self.to,
            &mut self.cc,
            &mut self.subject,
            &mut self.preview,
            &mut self.body,
            &mut self.date,
        ] {
            if s.contains('\0') {
                *s = s.replace('\0', " ");
            }
        }
    }

    /// Full receipt date and time for the reader header, e.g.
    /// "Jun 27, 2026 at 3:15 PM". Falls back to the short label if unknown.
    pub fn datetime_full(&self) -> String {
        if self.timestamp <= 0 {
            return self.date.clone();
        }
        let full = crate::datefmt::date_time(self.timestamp);
        if full.trim().is_empty() {
            self.date.clone()
        } else {
            full
        }
    }

    /// Compact date + time for list rows: always shows the time, with the day
    /// (and year, if not this year).
    pub fn datetime_list(&self) -> String {
        if self.timestamp <= 0 {
            return self.date.clone();
        }
        let now = crate::datefmt::now();
        let time = crate::datefmt::time(self.timestamp);
        if time.is_empty() {
            return self.date.clone();
        }
        if crate::datefmt::day_key(self.timestamp) == crate::datefmt::day_key(now) {
            format!("Today, {time}")
        } else if crate::datefmt::year(self.timestamp) == crate::datefmt::year(now) {
            format!("{}, {time}", crate::datefmt::day_month(self.timestamp))
        } else {
            format!("{}, {time}", crate::datefmt::day_month_year(self.timestamp))
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    fn item() -> OutboxItem {
        OutboxItem {
            id: 7,
            account_id: 1,
            from_addr: "me@example.com".into(),
            rcpts: vec!["ada@example.com".into()],
            recipients: "Ada Lovelace <ada@example.com>".into(),
            subject: "Quarterly numbers".into(),
            preview: "Here are the figures".into(),
            raw: Vec::new(),
            sent_path: None,
            queued_at: 0,
            attempts: 1,
            last_error: String::new(),
        }
    }

    #[test]
    fn a_queued_message_reads_as_an_ordinary_row() {
        let row = item().as_message();
        // The list's headline column is who it is going to, not who sent it: in
        // an Outbox every message is from the same person.
        assert_eq!(row.from_name, "Ada Lovelace <ada@example.com>");
        assert_eq!(row.subject, "Quarterly numbers");
        assert_eq!(row.folder_id, OUTBOX_FOLDER_ID);
        assert_eq!(row.id, 7);
        // Still waiting, so never shown as read.
        assert!(row.unread);
    }

    #[test]
    fn a_queued_row_says_why_it_is_stuck() {
        let mut i = item();
        i.attempts = 4;
        i.last_error = "Connection refused (os error 111)".into();
        let row = i.as_message();
        assert!(row.preview.contains("4 attempts"), "{}", row.preview);
        assert!(row.preview.contains("Connection refused"), "{}", row.preview);
        // With nothing failed yet, the body stands in for the reason.
        let row = item().as_message();
        assert!(row.preview.contains("Here are the figures"), "{}", row.preview);
        assert!(!row.preview.contains("attempts"), "{}", row.preview);
    }

    #[test]
    fn an_empty_subject_or_recipient_still_reads_sensibly() {
        let mut i = item();
        i.subject = "  ".into();
        i.recipients = String::new();
        let row = i.as_message();
        assert_eq!(row.subject, "(no subject)");
        assert_eq!(row.from_name, "(no recipients)");
    }
}
