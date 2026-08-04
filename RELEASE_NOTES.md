# Vireo

Vireo is a clean, fast, GNOME-native email client built with Rust and libadwaita for Wayland desktops. Privacy-first: no telemetry, remote content blocked by default, and credentials kept in the system keyring.

## What's new in 1.6.0

**Veem is now Vireo.** We've renamed the app — the old name was too easily confused with other products. It's the same app underneath, with a new name and a fresh icon.

- **If you installed the Flatpak:** install Vireo fresh from [vireo.hyprlab.co](https://vireo.hyprlab.co), then remove the old Veem app. Because a Flatpak's identity is its ID, the old install can't be updated in place and your accounts need to be added again there — passwords stay safe in your system keyring, and your mail is on the server.
- **If you installed the RPM, Arch package, or built from source:** just install the new `vireo` package — your accounts, settings and cached mail move over automatically the first time Vireo starts.
- Fedora and Arch Linux packages are now published with every release, alongside the Flatpak.
- The website has moved to [vireo.hyprlab.co](https://vireo.hyprlab.co); getveem.com now redirects there.

## In 1.5.1
- Conversations with unread replies no longer look "read" when collapsed. The thread's top row now keeps its unread dot — with an extra-strong highlight — until you've read every message in the conversation, so unread replies tucked under a collapsed thread can't slip past you. Read them by expanding the thread, or mark them read from the row's actions; the highlight clears the instant the last one is read.
- New setting under Settings → Message List: "Expand conversations by default". Choose whether conversations start collapsed to their newest message (as before) or fully expanded in the list. The per-conversation arrow still works either way.

## In 1.5.0
- Reply, Reply All and Forward now open right in the reader: a compose panel drops down over the message showing just your reply — no separate window to manage. Type and send without leaving the message.
- Want the full editor with recipient and subject fields? Click the expand button to pop the reply out into a compose window — and collapse it back inline when you're done. Your draft carries across intact, cursor position and all.
- Switch to another message mid-reply and Veem saves your unsent draft to Drafts automatically (a reply you never touched is simply discarded). "New message" still opens its own window.

## In 1.4.3
- Fixed contact names showing in all lowercase in the contacts browser — they now display with their proper capitalisation.

## In 1.4.2
- Fixed GNOME Contacts on the Flatpak build: the contacts browser was showing an empty list and the "Open GNOME Contacts" button wasn't launching the app. Your contacts now appear, and the button opens GNOME Contacts. (After updating, this needs the new permissions that ship with this version — a normal `flatpak update` applies them.)

## In 1.4.1
- Fixed the paperclip showing on some iCloud messages that don't actually have attachments (typically newsletters and other HTML mail). Veem now clears the false indicator as soon as it has looked at the message, and remembers the correction.

## In 1.4.0
- New in-message attachment drawer. Open a message with attachments and a footer appears beneath it, showing every attachment as a thumbnail — images as picture previews, other files as colour-coded type icons — with the filename under each. Drag the divider to resize it, or collapse it to a slim header with the chevron; a slider adjusts how big the thumbnails are.
- Click an image thumbnail for a full lightbox (step through images with the arrow keys, Esc to close), or use the hover/right-click actions to Download or Open any attachment.
- The attachments dropdown in the reader header now shows image thumbnails too, with Preview, Open, and Download for each file — so you can see what a picture is before opening it.

## In 1.3.9
- Fixed Veem not fetching new mail after your computer wakes from sleep. Putting the machine to sleep silently drops the connection to your mail servers, and previously nothing noticed on wake — so no new mail arrived (and even the Refresh button couldn't recover) until you restarted the app. Veem now detects resume from sleep and reconnects immediately, so new mail shows up right away.

## In 1.3.8
- On Linux Mint, the About window now has a "Keyring Setup Help" entry, so you can reopen the keyring setup guide any time — handy if you closed it the first time it appeared.

## In 1.3.7
- Veem now tells you when your account password can't be saved to the system keyring, instead of silently failing and forgetting it after a restart. On Linux Mint (Cinnamon), it also shows a one-time setup tip explaining how to get the keyring working — and how to stop it asking for an unlock password every time you log in.

## In 1.3.6
- Veem's icons now look the same on every Linux distribution. They're built into the app itself, so they no longer change appearance or go missing depending on your system's icon theme (something that happened on distributions like Zorin).

## In 1.3.5
- Sidebar folder lists stay tidy: your essential folders (Inbox, Sent, Archive, Trash, and the like) are always shown, while custom folders collapse under a "Folders" section that you can reveal when you want it. Each account remembers whether it's expanded, between restarts.
- Attachment thumbnails now offer Download and Go to Message on hover, next to Open.

## In 1.3.4
- The attachments gallery can now be searched and sorted. Search by sender, subject, filename, folder, or file type (like "pdf" or "image"), and sort by date, name, sender, size, or type — each ascending or descending. Every attachment also shows its message's date alongside the folder and size.

## In 1.3.3
- Attachment icons in the gallery are now colour-coded by type — PDFs red, Word docs blue, spreadsheets green, presentations orange, archives amber, and more — so you can spot the file you want at a glance.

## In 1.3.2
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
- The Actions Palette: a per-row ⋯ button slides in quick actions (reply, star, mark read, archive, spam, delete) so you can act on a message without opening it.
- No telemetry and no analytics, ever.

## Desktop integration
- Native GNOME / libadwaita interface with an adaptive three-pane layout, designed for Wayland.
- Per-account colours and emoji avatars, with optional Gravatar (off by default).
- Light and dark themes that follow the system.
- Remembers its window size between launches and supports GNOME edge-tiling.
- Optional GNOME Contacts integration via Evolution Data Server.

## Compatibility
- Handles servers that emit non-standard IMAP responses (such as iCloud) by falling back to raw-header parsing.
