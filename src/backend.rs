//! Mail backend abstraction.
//!
//! The UI talks to a [`MailBackend`] and never to IMAP/SMTP directly. This keeps
//! the reactive UI decoupled from the (async, fallible) network layer. The live
//! path is the IMAP worker; [`MockBackend`] serves realistic sample data so the
//! app is fully navigable offline — used for the demo mode (launch with no
//! accounts configured, e.g. an empty `XDG_CONFIG_HOME`).

use crate::models::{Account, Folder, FolderKind, Message};

/// Read access to mail data.
pub trait MailBackend {
    fn accounts(&self) -> Vec<Account>;
    fn folders(&self, account_id: u32) -> Vec<Folder>;
    fn messages(&self, folder_id: u32) -> Vec<Message>;
}

/// In-memory sample data provider for the offline demo.
pub struct MockBackend {
    accounts: Vec<Account>,
    folders: Vec<Folder>,
    messages: Vec<Message>,
}

impl Default for MockBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl MockBackend {
    pub fn new() -> Self {
        let accounts = vec![
            Account {
                id: 1,
                name: "Jason Martin".into(),
                email: "jason@getveem.com".into(),
                label: "jason@getveem.com".into(),
                accent: "#3584e4".into(),
            },
            Account {
                id: 2,
                name: "Hyprlab".into(),
                email: "hello@hyprlab.dev".into(),
                label: "hello@hyprlab.dev".into(),
                accent: "#2ec27e".into(),
            },
        ];

        // Folder ids: 1–7 for account 1, 11–17 for account 2. Inbox ids 1 and 11.
        let folders = vec![
            folder(1, 1, "Inbox", FolderKind::Inbox, 5),
            folder(2, 1, "Starred", FolderKind::Starred, 0),
            folder(3, 1, "Sent", FolderKind::Sent, 0),
            folder(4, 1, "Drafts", FolderKind::Drafts, 1),
            folder(5, 1, "Archive", FolderKind::Archive, 0),
            folder(6, 1, "Junk", FolderKind::Junk, 2),
            folder(7, 1, "Trash", FolderKind::Trash, 0),
            folder(11, 2, "Inbox", FolderKind::Inbox, 3),
            folder(12, 2, "Starred", FolderKind::Starred, 0),
            folder(13, 2, "Sent", FolderKind::Sent, 0),
            folder(14, 2, "Drafts", FolderKind::Drafts, 0),
            folder(15, 2, "Archive", FolderKind::Archive, 0),
            folder(16, 2, "Junk", FolderKind::Junk, 0),
            folder(17, 2, "Trash", FolderKind::Trash, 0),
        ];

        Self {
            accounts,
            folders,
            messages: sample_messages(),
        }
    }

    /// Look up a message by id across all folders (for body/source requests).
    pub fn message(&self, id: u32) -> Option<Message> {
        self.messages.iter().find(|m| m.id == id).cloned()
    }
}

impl MailBackend for MockBackend {
    fn accounts(&self) -> Vec<Account> {
        self.accounts.clone()
    }

    fn folders(&self, account_id: u32) -> Vec<Folder> {
        self.folders
            .iter()
            .filter(|f| f.account_id == account_id)
            .cloned()
            .collect()
    }

    fn messages(&self, folder_id: u32) -> Vec<Message> {
        self.messages
            .iter()
            .filter(|m| m.folder_id == folder_id)
            .cloned()
            .collect()
    }
}

fn folder(id: u32, account_id: u32, name: &str, kind: FolderKind, unread: u32) -> Folder {
    Folder {
        id,
        account_id,
        name: name.into(),
        path: name.into(),
        kind,
        unread,
    }
}

/// Compact spec for a sample message; expanded into a [`Message`] by [`build`].
struct Spec {
    id: u32,
    account_id: u32,
    folder_id: u32,
    from_name: &'static str,
    from_addr: &'static str,
    to: &'static str,
    subject: &'static str,
    preview: &'static str,
    body: &'static str,
    date: &'static str,
    unread: bool,
    starred: bool,
    has_attachment: bool,
    /// A parent message id when this is a reply (drives conversation threading).
    in_reply_to: Option<u32>,
}

fn build(s: &Spec) -> Message {
    Message {
        id: s.id,
        account_id: s.account_id,
        folder_id: s.folder_id,
        uid: s.id,
        // Newer id → more recent. Spaced an hour apart from a fixed base so the
        // demo is deterministic (no wall-clock dependency).
        timestamp: 1_760_000_000 - (s.id as i64) * 3600,
        from_name: s.from_name.into(),
        from_addr: s.from_addr.into(),
        to: s.to.into(),
        cc: String::new(),
        subject: s.subject.into(),
        preview: s.preview.into(),
        body: s.body.into(),
        date: s.date.into(),
        unread: s.unread,
        starred: s.starred,
        has_attachment: s.has_attachment,
        message_id: format!("<demo-{}@veem.local>", s.id),
        references: s
            .in_reply_to
            .map(|p| format!("<demo-{p}@veem.local>"))
            .unwrap_or_default(),
    }
}

fn sample_messages() -> Vec<Message> {
    const ME: &str = "jason@getveem.com";
    const LAB: &str = "hello@hyprlab.dev";
    let specs = [
        // ---- Account 1 · Inbox ----
        Spec { id: 1, account_id: 1, folder_id: 1, from_name: "GNOME Foundation", from_addr: "news@gnome.org", to: ME,
            subject: "GNOME 49 release candidate is here",
            preview: "The release candidate for GNOME 49 is now available for testing. This cycle brings major performance work…",
            body: "Hi Jason,\n\nThe release candidate for GNOME 49 is now available for testing. This cycle brings major performance work across the shell and a refreshed libadwaita with new adaptive widgets.\n\nHighlights:\n  • Faster startup and lower memory use\n  • New AdwMultiLayoutView for responsive layouts\n  • Improved Wayland fractional scaling\n\nPlease help us test and file issues before the final release.\n\n— The GNOME Release Team",
            date: "9:42 AM", unread: true, starred: true, has_attachment: false, in_reply_to: None },
        Spec { id: 2, account_id: 1, folder_id: 1, from_name: "Sophie Turner", from_addr: "sophie@studio.dev", to: ME,
            subject: "Q3 roadmap review",
            preview: "Sharing the draft roadmap ahead of Thursday. The migration phase is the big open question — see the timeline…",
            body: "Hi Jason,\n\nSharing the draft roadmap ahead of Thursday. The migration phase is the big open question — see the timeline in the doc and let me know if the sequencing works.\n\nThanks,\nSophie",
            date: "9:05 AM", unread: true, starred: false, has_attachment: true, in_reply_to: None },
        Spec { id: 3, account_id: 1, folder_id: 1, from_name: "Jason Martin", from_addr: ME, to: "sophie@studio.dev",
            subject: "Re: Q3 roadmap review",
            preview: "Looks great overall. I left comments on the migration phase — I think we can parallelize the first two steps…",
            body: "Looks great overall. I left comments on the migration phase — I think we can parallelize the first two steps and pull the whole thing in by a week. Happy to walk through it tomorrow at 10.\n\nJason",
            date: "9:28 AM", unread: false, starred: false, has_attachment: false, in_reply_to: Some(2) },
        Spec { id: 4, account_id: 1, folder_id: 1, from_name: "Rust Weekly", from_addr: "digest@this-week-in-rust.org", to: ME,
            subject: "This Week in Rust #612",
            preview: "Crate of the week, RFCs, and community updates. This issue: async closures stabilize, and a deep dive into…",
            body: "Welcome to another issue of This Week in Rust!\n\nThis week: async closures stabilize on stable, a deep dive into zero-copy parsing, and 14 new crates worth your attention.\n\nRead online for the full digest.",
            date: "Yesterday", unread: true, starred: false, has_attachment: false, in_reply_to: None },
        Spec { id: 5, account_id: 1, folder_id: 1, from_name: "Apple", from_addr: "no-reply@apple.com", to: ME,
            subject: "Your receipt from Apple",
            preview: "Thank you for your purchase. Your receipt is attached. Order ID W1234567890…",
            body: "Thank you for your purchase.\n\nOrder ID: W1234567890\niCloud+ 2TB — $9.99\n\nYour receipt is attached.",
            date: "Yesterday", unread: false, starred: false, has_attachment: true, in_reply_to: None },
        Spec { id: 6, account_id: 1, folder_id: 1, from_name: "Marcus Chen", from_addr: "marcus@studio.dev", to: ME,
            subject: "Design tokens are merged 🎨",
            preview: "The design token pipeline is finally merged into main. Dark mode now derives entirely from the token set…",
            body: "Hey,\n\nThe design token pipeline is finally merged into main. Dark mode now derives entirely from the token set, so we no longer maintain two stylesheets. Pull main when you get a chance.\n\nMarcus",
            date: "Wed", unread: true, starred: false, has_attachment: false, in_reply_to: None },
        Spec { id: 7, account_id: 1, folder_id: 1, from_name: "Calendar", from_addr: "calendar@getveem.com", to: ME,
            subject: "Invitation: Architecture sync @ Thu 2:00 PM",
            preview: "You have been invited to Architecture sync. Thursday 2:00 PM – 3:00 PM. Conference Room B / video link…",
            body: "You have been invited to: Architecture sync\n\nWhen: Thursday 2:00 PM – 3:00 PM\nWhere: Conference Room B / video link\n\nAccept · Decline · Maybe",
            date: "Wed", unread: false, starred: false, has_attachment: false, in_reply_to: None },
        Spec { id: 8, account_id: 1, folder_id: 1, from_name: "Emma Wright", from_addr: "emma@example.com", to: ME,
            subject: "Lunch this weekend?",
            preview: "It's been ages! Are you free Saturday for lunch at that new place downtown? Let me know what works…",
            body: "Hey stranger!\n\nIt's been ages! Are you free Saturday for lunch at that new place downtown? Let me know what works.\n\nxo Emma",
            date: "Tue", unread: false, starred: true, has_attachment: false, in_reply_to: None },
        Spec { id: 9, account_id: 1, folder_id: 1, from_name: "Linear", from_addr: "notifications@linear.app", to: ME,
            subject: "3 issues assigned to you this sprint",
            preview: "VEEM-142 Reader dark mode, VEEM-148 Infinite scroll spinner, VEEM-151 OAuth for Microsoft…",
            body: "You have 3 issues in the current sprint:\n\n  • VEEM-142  Reader dark mode\n  • VEEM-148  Infinite scroll spinner\n  • VEEM-151  OAuth for Microsoft\n\nOpen in Linear to update status.",
            date: "Tue", unread: true, starred: false, has_attachment: false, in_reply_to: None },
        Spec { id: 10, account_id: 1, folder_id: 1, from_name: "Framer", from_addr: "team@framer.com", to: ME,
            subject: "Your weekly site analytics",
            preview: "getveem.com had 4,218 visitors this week, up 32%. Top page: /download. See the full breakdown…",
            body: "getveem.com — weekly summary\n\nVisitors: 4,218 (+32%)\nTop page: /download\nAvg. time on page: 1m 47s\n\nView the full report online.",
            date: "Mon", unread: false, starred: false, has_attachment: false, in_reply_to: None },
        // ---- Account 1 · Drafts ----
        Spec { id: 20, account_id: 1, folder_id: 4, from_name: "Jason Martin", from_addr: ME, to: "team@getveem.com",
            subject: "Release notes for 0.2",
            preview: "Draft — Highlights for the next build: Actions Palette, message-content theme, infinite scroll…",
            body: "Draft.\n\nHighlights for 0.2:\n  • Actions Palette with slide-in animation\n  • Per-message light/dark content theme\n  • Infinite scroll for large folders\n\nTODO: add screenshots.",
            date: "Mon", unread: false, starred: false, has_attachment: false, in_reply_to: None },
        // ---- Account 2 · Inbox ----
        Spec { id: 30, account_id: 2, folder_id: 11, from_name: "Proton", from_addr: "security@proton.me", to: LAB,
            subject: "New sign-in to your account",
            preview: "We noticed a new sign-in from Fedora Linux · Firefox. If this was you, no action is needed…",
            body: "We noticed a new sign-in to your Proton account.\n\nDevice: Fedora Linux · Firefox\nLocation: —\n\nIf this was you, no action is needed.",
            date: "10:11 AM", unread: true, starred: false, has_attachment: false, in_reply_to: None },
        Spec { id: 31, account_id: 2, folder_id: 11, from_name: "Buy Me a Coffee", from_addr: "no-reply@buymeacoffee.com", to: LAB,
            subject: "You have a new supporter ☕",
            preview: "Alex bought you a coffee and left a note: “Love Veem — the GNOME-native mail client I've wanted for years!”",
            body: "Good news!\n\nAlex bought you a coffee and left a note:\n\n  “Love Veem — the GNOME-native mail client I've wanted for years!”\n\nSay thanks from your dashboard.",
            date: "Yesterday", unread: true, starred: true, has_attachment: false, in_reply_to: None },
        Spec { id: 32, account_id: 2, folder_id: 11, from_name: "GitHub", from_addr: "notifications@github.com", to: LAB,
            subject: "[hyprlab/veem] Star milestone: 1,000 ⭐",
            preview: "Your repository hyprlab/veem just reached 1,000 stars. Nice work! See who starred recently…",
            body: "hyprlab/veem just reached 1,000 stars 🎉\n\nRecent stargazers: @ada, @torvalds-fan, @rustacean…\n\nView on GitHub.",
            date: "Yesterday", unread: true, starred: false, has_attachment: false, in_reply_to: None },
        Spec { id: 33, account_id: 2, folder_id: 11, from_name: "Flathub", from_addr: "noreply@flathub.org", to: LAB,
            subject: "Your submission is under review",
            preview: "Thanks for submitting com.getveem.Veem to Flathub. A reviewer has been assigned and will follow up shortly…",
            body: "Thanks for submitting com.getveem.Veem to Flathub.\n\nA reviewer has been assigned and will follow up on the pull request shortly.\n\n— The Flathub Team",
            date: "Fri", unread: false, starred: false, has_attachment: false, in_reply_to: None },
    ];
    specs.iter().map(build).collect()
}
