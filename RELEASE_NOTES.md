# Veem

The first stable release of Veem — a clean, fast, GNOME-native email client built with Rust and libadwaita for Wayland desktops. Privacy-first: no telemetry, remote content blocked by default, and credentials kept in the system keyring.

## Accounts & sign-in
- Multiple IMAP and POP3 accounts, each running on its own background worker.
- Google and Microsoft OAuth 2.0 sign-in (XOAUTH2) via the system browser (authorization-code + PKCE) — one-click in official builds, or through GNOME Online Accounts and your own OAuth client.
- Custom OAuth for any other provider; supply or override client credentials via ~/.config/veem/oauth.toml or the VEEM_* environment variables.
- Import accounts from GNOME Online Accounts.
- Enable or disable each account without deleting it.
- Passwords and OAuth refresh tokens are stored in the system keyring (secret-service), never written to disk.

## Mail
- Unified "All Inboxes" view that merges every account by recency.
- Whole-mailbox sync and search with no message-count cap: a fast first page loads instantly, then a background backfill indexes the rest.
- Infinite scroll through the entire folder, with a loading spinner at the bottom while more messages stream in.
- Deletions and moves made on your phone or another client sync back automatically (server reconciliation plus IMAP IDLE).
- Conversation threading, grouped by Message-ID and References (toggleable).
- Compose, reply, reply-all and forward over SMTP, with HTML signatures and editable drafts.
- Create and delete folders, drag messages onto a folder to move them, and archive, mark spam or trash — target folders are created on the server when missing.
- New-mail push over IMAP IDLE, plus a configurable automatic check interval.
- Sort by date, sender, subject, unread or flagged.

## Reading & privacy
- Remote content (images, trackers) is blocked by default; allow it per message or trust a sender to always load it.
- Per-message content theme: follow the system, or force light or dark for email content only — independent of the app's own theme.
- Attachments are pre-fetched for instant opening, with a download-on-demand fallback and save-to-disk.
- Pop any message out into its own window.
- Per-sender allow and block lists, with automatic deletion of blocklisted senders.
- The Actions Palette: a per-row chevron slides in quick actions (reply, star, mark read, archive, spam, delete) so you can act on a message without opening it.
- No telemetry and no analytics, ever.

## Desktop integration
- Native GNOME / libadwaita interface with an adaptive three-pane layout, designed for Wayland.
- Per-account colours and emoji avatars, with optional Gravatar (off by default).
- Light and dark themes that follow the system.
- Remembers its window size between launches and supports GNOME edge-tiling.
- Optional GNOME Contacts integration via Evolution Data Server.

## Compatibility
- Handles servers that emit non-standard IMAP responses (such as iCloud) by falling back to raw-header parsing.
