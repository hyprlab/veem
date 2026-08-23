# Vireo

Vireo is a clean, fast, GNOME-native email client built with Rust and libadwaita for Wayland desktops. Privacy-first: no telemetry, remote content blocked by default, and credentials kept in the system keyring.

## What's new in 1.13.3

- **Conversations actually group now.** Vireo was reading only half of what
  decides whether two messages belong together, so replies kept landing as
  separate messages instead of one conversation. It now reads the whole thread
  history each message carries. Mail that arrived before this update is repaired
  quietly in the background, a folder at a time, so old conversations knit
  themselves back together without you re-syncing anything.
- **Your own replies appear in the conversation.** Reading a message in the
  Inbox used to show only their side of it, because your replies live in Sent.
  They are now woven in by date, each labelled with the folder it came from.
  Nothing is fetched over the network for this — it comes from mail Vireo has
  already indexed — and messages you have moved to Trash or Junk stay out of it.

Reported in [#21](https://github.com/hyprlab/vireo/issues/21). Thank you.

## In 1.13.2

- **You can now delete messages from Trash.** Deleting anything already in Trash
  used to do nothing at all — the message stayed put, on every kind of account.
  It now erases the message from the server for good. Because there is no undo,
  Vireo asks first; and if you have selected a mix, whatever isn't in Trash yet
  simply moves there as before, so only the messages that would be erased get
  the question. Deleting from anywhere else still means "move to Trash", exactly
  as it did.

Reported in [#20](https://github.com/hyprlab/vireo/issues/20). Thank you.

## In 1.13.1

- **You can now choose to always load remote content.** Vireo still blocks
  images and other remote content by default, and you can still load a single
  message or trust a sender to always load it. But if you would rather not be
  asked at all, there is now a switch for that: Preferences → Privacy →
  *Always load remote content*. It is off unless you turn it on, because remote
  content can be used to tell when and where you read a message.

Contributed by [Isaac](https://github.com/thecalamityjoe87)
([#31](https://github.com/hyprlab/vireo/pull/31)). Thank you.

## In 1.13.0 — security release

**Please update.** This release fixes a flaw that let someone who could send you
mail run their own code inside Vireo's reader, simply by your opening the
conversation — no click, nothing visible. From there they could read every
message in that thread and send it elsewhere. Nothing suggests this was ever
used, and it was reported privately rather than found in the wild.

- **Fixed: a sender could run code in the reader.** A sender's *display name* was
  put into the page around the message without being fully neutralised first.
  Message bodies were never the problem — those have always been sealed off and
  unable to run anything — but the name above them was not. It is now, and the
  page itself has been given a second, independent lock so that even if the
  first check were ever wrong again, nothing can run.
- **Fixed: tracking pixels could load while Vireo said nothing was blocked.**
  Whether a message pulled in remote content was worked out by looking for a few
  exact spellings, and several ordinary ways of writing an image address slipped
  past — which switched off the blocking *and* hid the banner at the same time.
  Blocking now follows your setting rather than that guess, so the worst a miss
  can cost you is a missing notice, never a silent request.
- **Links only open if they're ordinary web or mail links.** A message could
  previously hand any address to whichever application had claimed it.
- **Tightened the Flatpak sandbox.** A permission that allowed running commands
  outside it has been dropped.
- **Your mail cache is no longer readable by other accounts on the machine**,
  and files you open from an attachment are now private and cleared away when
  Vireo starts, instead of piling up.
- **Sign-in is more secure**, and Vireo now refuses to sign in at all rather than
  proceeding if it cannot generate a proper secret.
- **New: notifications can leave out the sender and subject**, since GNOME shows
  them on the lock screen. Preferences → Mail → *Show sender and subject*.

Found and reported privately by [Alexander Lubovenko](https://github.com/typedev),
who reviewed the whole codebase and wrote it up carefully. Thank you.

## In 1.12.0

- **You can print a message.** Press **Ctrl+P**, use the printer button in the toolbar, or Main Menu → Print Message… What comes out is the message as you see it, with a header carrying the subject, who it is from and to, and the date — and it always prints on white, even if you read in dark mode.
- **Print preview, inside Vireo.** The toolbar's printer button opens a preview on a page-shaped sheet, so you can see what will come out before spending paper. **Print…** sends it to the printer, and **Save as PDF…** writes it straight to a file without going through the print dialog at all.

## In 1.11.0

- **Vireo can keep running after you close its window**, so new mail still arrives and notifies. GNOME doesn't have a system tray — instead Vireo appears under **Background Apps** in the system menu, with your unread count beside it, and can be quit from there. It's off by default: turn on *Keep running in the background* in Preferences → Mail.
- **Start at login** (a second switch, once the above is on). Vireo starts without a window and waits in the system menu, watching for mail from the moment you sign in.
- **Quit** is now in the main menu and on Ctrl+Q.

## In 1.10.3

- **Fixed: some accounts imported from GNOME Online Accounts couldn't sign in.** Accounts that use a password (rather than a Google-style sign-in) could end up without one, with no way to correct it. Vireo now asks GNOME Online Accounts for the password each time it connects, so those accounts start working without being re-added — and if the system genuinely has no password stored, Vireo says so and points you to Settings → Online Accounts.

## In 1.10.2

- **Accounts from GNOME Online Accounts are now read-only in Vireo.** Their address, servers and password belong to the system, so Vireo greys those fields out and points you to Settings → Online Accounts, where changing them actually works. Your display name, signature, colour and label are still yours to edit here, and the switch at the top of the account still hides an account in Vireo without removing it from your system.

## In 1.10.1

- **Fixed: attachments that didn't show up.** Some messages — Apple Mail ones in particular, where a PDF or photo is attached "inline" — showed no paperclip in the list and no attachment when opened, even though the file was there. Vireo now checks the message itself rather than trusting what the server's summary implies, so those files appear. Recent mail is corrected in the background before you open it.

## In 1.10.0

- **Nothing is lost when a send fails.** Messages that can't go out — usually because you're offline — now wait in an **Outbox** and are sent as soon as the connection is back. You can open one to edit it, send it by hand, or throw it away, and Vireo tells you when a waiting message has gone. The Outbox behaves like any other folder in the sidebar and only appears while something is in it.
- **Message previews.** The list can show the first lines of each message under its subject. Choose **Off, 1, 2 or 3 lines** in Preferences → Message List; Off also stops previews being downloaded at all.
- **Keyboard shortcuts, no modifier needed.** Press `j`/`k` to move through the list, `r` to reply, `a` to archive, `d` to delete, `w`/`b` to step through a conversation — Gmail's keys, where they and this scheme agree. They're **off by default**; switch them on in Preferences → Message List, and press **Ctrl+?** any time for the full list. `Esc` backs out of a reply and returns to the list even with shortcuts switched off.
- **Folders with non-Latin names read properly.** Gmail labels in Chinese (and any other non-ASCII mailbox name) were showing as `&XfJSoGYfaAc-` instead of 已加星标. They now display correctly, and you can create folders with non-ASCII names too.
- The message list's action buttons moved to their own line under the preview, so they no longer cover the text or shift it sideways, and opening them no longer widens the pane.

## In 1.9.2

- **Vireo now runs on ARM.** If you're on a Raspberry Pi, a Snapdragon X Elite laptop, an ARM virtual machine or anything else `aarch64`, Vireo installs and runs the same way it does everywhere else — the install command works out which build you need, so there is nothing to pick. Previously ARM machines were handed the Intel build and it refused to start.
- Direct downloads now come in both flavours: the site's download button follows your machine, and every release carries `Vireo-x86_64.flatpak` and `Vireo-aarch64.flatpak`. The Fedora RPM remains Intel/AMD only.
- **The About window's Changelog and Release Notes read properly.** They were showing raw Markdown — stray asterisks, backticks and bracketed links — with long entries wrapping awkwardly under their own bullets. Now they're formatted, with working links.

## In 1.9.1

- A correction to how 1.9.0 credited its contributors: Chris Pouliot's work on Proton Bridge support was attributed with an email address GitHub couldn't match to his account, so his name never appeared on the project's contributor list. This release records it properly. Contributors are also now shown with their GitHub handle in the About window's "Thanks" list.

## In 1.9.0

This is the first Vireo release built partly from other people's code. Thanks to [Alfonso Lizárraga](https://github.com/alfonsolzrg) and [Chris Pouliot](https://github.com/chrispouliot), who found these problems, fixed them, and sent the fixes upstream.

- **Sending to a named recipient works.** Picking a contact whose name carries an accent, a comma or a full stop — "Alfonso Lizárraga", "Martin, Jason", "Dr. Chen" — used to fail outright with "Invalid param". Every address field is now assembled properly, so any name goes through.
- **Proton Mail works, through Proton Bridge.** Vireo can now talk to Bridge running on your machine (and to other local bridges like hydroxide or DavMail). Point an account at 127.0.0.1 with Bridge's ports and password; the certificate Bridge signs for itself is accepted for this machine only, and everything on the network is still fully verified.
- **Your mail is there the moment the window opens.** A synced account used to show an empty list while it waited for the connection, even though the messages were already cached. Long folders also scroll and search noticeably faster.
- **The message list stops shifting.** The unread dot keeps its space when a message is read, so subjects no longer jump sideways as you work through the list.
- **Compose has moved** to the top of the message list, next to the notification bell — above the list it adds to, rather than above your folders.
- **New setting:** hide the sidebar's Attachments row if you don't use it (Preferences → Mail).

  Contributions are credited in the About window under "Thanks", with links to the people behind them.

## In 1.8.1

- The sender lightbulb now stays put. It used to appear only once Vireo had a verdict for the message you were reading, which nudged the other toolbar buttons sideways as you moved between messages. It's now always there, greyed out like the other buttons until there's something to report.

## In 1.8.0

- **Know who really sent a message.** Anyone can put anything in an email's "From" line — that's how phishing works. Vireo now shows a lightbulb in the toolbar telling you whether your mail provider could actually confirm the sender: green for verified, amber if something's off, red if the address looks forged. Click it for the reasoning, including things like "replies would go to a different company than the one that sent this".
- **See where a link goes before you click it.** Hover any link in a message and its real destination appears at the bottom of the message. If the link text claims one website while pointing at another — the oldest trick in phishing — Vireo says so directly.

  A verified sender means the address wasn't forged. It doesn't mean the message is safe: a scammer who registers their own convincing-looking domain can pass the check. Use it to catch impersonation, not as a licence to trust.

## In 1.7.2

- Photos sent inside a message can now be saved. Right-click one and choose "Save Image As…" — previously that did nothing at all.
- Those photos also show up as attachments now, with their original filename, so they get a paperclip in the message list and appear in the attachment strip and the gallery like any other attachment. Small embedded images — newsletter logos, spacers, social icons — are still treated as decoration and won't clutter your list with false paperclips.

## In 1.7.1

- Fixed photos sent from Gmail (and images in many newsletters) showing as a filename instead of the picture. These messages point at their images from inside the email itself, and Vireo wasn't following the reference — the images now appear in place, with no network access needed since they arrived with the message.
- **The Arch, Debian/Ubuntu and Snap packages have been discontinued.** Vireo is now published as a Flatpak (for every distribution) and a Fedora RPM. If you installed one of the discontinued packages, remove it and install the Flatpak instead — it works everywhere and updates automatically:

  ```sh
  flatpak install --from https://vireo.hyprlab.co/flatpak/co.hyprlab.Vireo.flatpakref
  ```

## In 1.7.0

- Two new install options, published with every release alongside the Flatpak, RPM and Arch packages:
  - **Debian / Ubuntu** — a native `.deb` for Ubuntu 24.04+ and Debian 13+: download and `sudo apt install ./vireo_*_amd64.deb`.
  - **Snap** — `snap install --dangerous ./vireo_*_amd64.snap` (the flag just means "installed from a file rather than the store").

## In 1.6.1

- Switching from Veem to Vireo on Flatpak is now fully automatic: install Vireo, launch it, and your accounts, settings and cached mail from the old Veem app are picked up on first run — nothing to re-add. (You can remove the old Veem app afterwards; its data is left untouched.)

## In 1.6.0

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
