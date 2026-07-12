# Veem

Veem is a clean, fast, GNOME-native email client built with Rust and libadwaita for Wayland desktops. Privacy-first: no telemetry, remote content blocked by default, and credentials kept in the system keyring.

## What's new in 1.3.2
- Fixed newsletter subjects (like The Marginalian's) that showed up as a string of `=?utf-8?Q?…?=` code and stretched the window so wide the close button disappeared. Those subjects now decode correctly, and no subject — however long — can push the window controls off-screen. Already-affected messages fix themselves on upgrade.

## In 1.3.1
- The Attachments gallery now covers every folder — archived and filed mail too, not just inboxes (everything except Trash, Spam and Drafts) — with those attachments fetched in the background so they just show up.
- Right-click an attachment for Download, Open or Go to Message (the menu opens right at your pointer), double-click a thumbnail to open it, or use the hover "Open" button in the corner.
- A responsive grid that starts at 3 across and adds columns as you widen the window, with every thumbnail kept at a tidy 4:3.

## In 1.3.0
- New Attachments gallery: a sidebar entry that shows every attachment across your inboxes in a grid — image thumbnails and file icons. Click one for a full preview with prev/next, Open, and a jump to the source message. Instant and offline (built from the local cache).

## In 1.2.3
- Adding an account is now a one-step choice: pick your provider from a dropdown and Veem fills in the sign-in method and all the server settings. Includes iCloud, Yahoo, Proton (Bridge), Fastmail, AOL, Zoho, GMX, Yandex, Mail.com, plus Google and Microsoft sign-in — and "Other" for anything else.

## In 1.2.2
- Desktop notifications for new mail and error alerts, shown when Veem isn't focused. Click a new-mail notification to jump straight to the message; it clears once you've read it. Toggle it in Preferences → Mail.

## In 1.2.1
- Links in plain-text emails are now clickable and open in your browser.
- Press Delete or Backspace in the message list to delete the selected message(s).
- Right-click an account under "All Inboxes" to jump to its account settings.

## In 1.2.0
- Search now covers every folder of every account, not just the folder you're in. A selector beside the search box switches between "All folders" (default) and "This folder", results are tinted by account, and opening a hit works from whichever folder it lives in.

## In 1.1.5
- Reordered the message-action buttons in the reader header to Archive, Delete, Spam, View Source.
- The message list opens at its snug minimum width instead of a touch wider than needed.

## In 1.1.4
- Fixes 1.1.3 for mail you'd already opened: messages keep a cached copy of how they were rendered, so previously-read mail — including the iPhone photos 1.1.3 was meant to fix — still displayed blank. Those cached renderings are now refreshed on upgrade.

## In 1.1.3
- Fixed photos emailed from an iPhone showing up as a blank message with no attachment. The text now appears, the photos are shown inline in the message, and they're listed as attachments you can save.
- Plain-text messages now follow the light/dark theme instead of always rendering on white.

## In 1.1.2
- Mass delete and archive of large selections is now fast and reliable, with a progress spinner — and on Gmail it correctly moves mail to Trash instead of leaving it in All Mail.
- Accounts removed in GNOME Online Accounts are now dropped from Veem automatically, on startup and while running.

## In 1.1.1
- Fixed the sidebar's unread count badges: they no longer show a stale number after collapsing and expanding the sidebar, and empty inboxes now report the correct count as soon as an account connects.

## In 1.1.0
- The collapsed, icon-only sidebar shows unread count chips directly on the inbox icons.
- The per-account inboxes under "All Inboxes" stay available when the sidebar is collapsed, with a button to expand or collapse them.
- The collapsed sidebar rail is narrower, reclaiming horizontal space.

## Accounts & sign-in
- Multiple IMAP and POP3 accounts, each running on its own background worker.
- Google and Microsoft OAuth 2.0 sign-in (XOAUTH2) via the system browser (authorization-code + PKCE) — one-click in official builds, or through GNOME Online Accounts and your own OAuth client.
- Custom OAuth for any other provider; supply or override client credentials via ~/.config/veem/oauth.toml or the VEEM_* environment variables.
- Import accounts from GNOME Online Accounts.
- Enable or disable each account without deleting it.
- Passwords and OAuth refresh tokens are stored in the system keyring (secret-service), never written to disk.

## Mail
- Unified "All Inboxes" view that merges every account by recency.
- Whole-mailbox sync and search with no message-count cap: a fast first page loads instantly, then a background backfill indexes the rest. Search spans every folder of every account (with a scope selector to narrow to the current folder).
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
