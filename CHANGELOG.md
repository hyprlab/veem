# Changelog

## 1.3.3 — 2026-07-12
- Attachment type icons in the gallery are now colour-coded by file kind: PDFs red, Word/documents blue, spreadsheets green, presentations orange, archives amber, audio purple, video pink, calendars teal, images cyan, and everything else grey. Applied to both the grid cells and the lightbox preview icon; colours read on light and dark themes.

## 1.3.2 — 2026-07-12
- Fixed subjects that arrived as raw `=?utf-8?Q?…?=` code. Some senders (notably Mailchimp newsletters like The Marginalian) pack the whole subject into one RFC 2047 encoded-word far longer than the 75-character limit; the decoder aborted on those and left the raw text. It now decodes them, as Apple Mail and Thunderbird do. Subjects already cached this way are re-decoded in place on upgrade — no re-sync needed.
- An extreme subject can no longer push the toolbar and window controls off-screen: the reader subject now breaks mid-word for unbreakable tokens, so its minimum width stays small regardless of content.

## 1.3.1 — 2026-07-12
- The Attachments gallery now spans every folder — archived mail and mail filed in folders, not just inboxes — excluding only Trash, Spam and Drafts. Attachments in those folders are prefetched in the background so they appear without opening each message.
- Right-click an attachment for Download…, Open, and Go to Message; the menu now opens at the pointer instead of below the thumbnail. Double-click a thumbnail to open the file in its default app.
- Each thumbnail has a hover "Open" button in its bottom-right corner (for files already cached).
- The grid is now responsive: a minimum of 3 thumbnails per row that adds columns as the window widens, with each thumbnail locked to a 4:3 ratio that fills its cell (implemented via a custom height-for-width `RatioBox` widget).
- Fixed the window controls (close/minimize) disappearing while the gallery was open — the gallery page now carries its own header bar.

## 1.3.0 — 2026-07-11
- New Attachments gallery: a sidebar entry (under All Inboxes, above the accounts) that shows every attachment across your connected inboxes in a grid — image thumbnails and type icons for other files, with the sender and size.
- Clicking an attachment opens a large lightbox preview with previous/next navigation (arrow keys and Escape too), an "Open" button (opens the file in its default app), and "Go to Message" to jump to the source email.
- The gallery is built entirely from the local attachment cache, so it's instant and works offline. Files under 6 MB are preview-ready immediately; larger files are opened on demand. Capped at 300 items per inbox, newest first.

## 1.2.3 — 2026-07-11
- Add Account now starts with a single Provider dropdown that sets everything up for you. Pick your provider and the sign-in method and IMAP/SMTP servers + ports are chosen automatically.
- Password providers with auto-filled servers: iCloud, Yahoo, Proton Mail (Bridge), Fastmail, AOL, Zoho, GMX, Yandex, and Mail.com — each with a hint (e.g. app-specific password, or Proton Bridge).
- OAuth providers (Google, Microsoft/Outlook, and Custom OAuth) are in the same dropdown; selecting one shows the browser sign-in and hides the manual server fields. "Other (IMAP/POP3)…" remains for manual setup.
- Removed the separate Authentication dropdown (its options moved into the Provider list) and the non-working password-based Gmail and Outlook/Hotmail entries (those providers require OAuth now).
- Editing an account auto-selects its matching provider in the dropdown.

## 1.2.2 — 2026-07-11
- Desktop (system) notifications: Veem now posts a notification for new inbox mail and for genuine error alerts (send/auth failures) via GNotification. Notifications appear only when Veem isn't the focused window; transient connection blips that auto-recover are excluded.
- Clicking a new-mail notification raises Veem, navigates to the message's folder, and opens it (the summary notification opens the newest of a batch). Error notifications raise the window.
- A new-mail notification is withdrawn once its mail is read — by clicking the notification or opening any unread message from that account.
- Added a "Desktop notifications" toggle in Preferences → Mail (on by default), persisted to privacy.toml.

## 1.2.1 — 2026-07-11
- Bare URLs (http/https and `www.`) in plain-text message bodies are now clickable links that open in the browser. Links are only ever http(s) — a bare `www.` host is forced to `https://` — so no `javascript:`-style link can be forged; trailing sentence punctuation is trimmed while balanced parentheses in a URL are kept. Cached bodies are re-rendered on upgrade (cache `user_version` 7 → 8) so previously-read mail picks up the links.
- In the message list, Delete or Backspace now deletes the selected message(s), moving them to Trash and advancing the reader. It's scoped to the list, so Backspace still edits text in the search box.
- Right-clicking an account under "All Inboxes" now offers "Account Settings…" in its context menu.

## 1.2.0 — 2026-07-11
- Search now spans every folder of every account, not just the folder you're viewing. A scope selector beside the search box switches between "All folders" (the default) and "This folder".
- Cross-folder search runs entirely over the locally indexed messages (subject, sender, preview), so it's instant and works offline. The whole mailbox is covered once background indexing finishes; the pool is snapshotted when a search begins.
- Multi-account search results are tinted by account (as in the unified inbox) so each hit's origin is legible, and opening a result works from whichever folder it lives in.
- A search now survives a background re-sync of the folder you're viewing instead of being cleared; switching folders still clears it.

## 1.1.5 — 2026-07-11
- Reordered the reader header's message-action buttons to Archive, Delete, Spam, View Source (left to right).
- The message list now launches at its minimum width — just wide enough for a row's Actions Palette to fit — instead of a slightly-too-wide fixed value. The divider position isn't hardcoded: `shrink_start_child` is false, so GtkPaned clamps the launch position up to the pane's natural minimum, which self-adjusts to font/theme changes.

## 1.1.4 — 2026-07-10
- Fixed 1.1.3's message-body fix not applying to mail that had already been read. Bodies are cached as rendered HTML and `LoadBody` serves that cache without ever re-fetching, so any message opened under an earlier build kept its old (blank) rendering forever — including the iPhone photo mail 1.1.3 was meant to fix. The cache is now invalidated on upgrade (`user_version` 6 → 7).
- Cache upgrades that only change how bodies are rendered now drop just the derived `bodies` table instead of the whole cache, so the message index survives and no whole-mailbox re-sync is triggered.

## 1.1.3 — 2026-07-10
- Fixed photo mail from iPhones (Apple Mail) arriving as a blank message with no attachment. Apple sends photos as `Content-Disposition: inline` parts of a `multipart/mixed`, which broke two things: the reader rendered only the *first* body part — an empty text part — so the message looked blank, and attachment detection required a disposition of `attachment`, so no paperclip appeared and the image was never downloaded.
- Message bodies are now composed from every display part in order rather than just the first, and inline images are embedded as `data:` URIs, so photos render in place. Nothing is fetched from the network: the bytes arrive with the message, and remote content stays blocked as before. Embedding is capped at 16 MiB per message; larger images remain available as attachments.
- Attachment detection now counts any non-text part, except one carrying a `Content-ID` — that marks a `cid:` resource referenced from the HTML body (e.g. a newsletter logo), which is rendered in place rather than listed. The attachment list follows the same rule, so it can no longer contradict the paperclip.
- Plain-text messages now follow the app's light/dark theme instead of always rendering on white.

## 1.1.2 — 2026-07-08
- Mass delete/archive of large selections is now fast and reliable: the whole selection is moved in a single server-side operation per folder (previously one slow request per message, which could freeze the UI and silently drop moves on big mailboxes such as Gmail's All Mail), with a spinner shown over the list until it completes.
- Fixed deletes/archives being routed to the wrong folder on Gmail: destinations now prefer the real RFC 6154 SPECIAL-USE folder (e.g. `[Gmail]/Trash`) over a same-named stray label, so mail actually leaves All Mail instead of just gaining a label.
- GNOME Online Accounts are kept in sync: an account removed in GNOME Settings is now dropped from Veem automatically — both on startup and live via a D-Bus watcher — instead of lingering. Reconciliation is skipped when GOA is unreachable, so a momentary outage never wipes accounts.

## 1.1.1 — 2026-07-07
- Fixed the sidebar's unread count chips: they no longer revert to a stale number when the sidebar is collapsed/expanded (in-place unread updates are now persisted, not just applied to the visible label), and empty inboxes report the correct count as soon as an account connects — the inbox count now comes from an accurate SEARCH UNSEEN instead of the STATUS count some servers (e.g. iCloud) report unreliably.

## 1.1.0 — 2026-07-06
- Compact (icon-only) sidebar improvements: inboxes now show unread count chips on their icons, the per-account inboxes under "All Inboxes" stay visible (with a button to expand/collapse them), and the collapsed rail is narrower.

## 1.0.9 — 2026-07-05
- Added a Changelog section to the About window, backed by this file so the version history stays in sync everywhere.

## 1.0.8 — 2026-07-05
- Quit cleanly on window close: save window state, then exit the process directly instead of running the full GTK/WebKit/worker teardown, which could abort with SIGABRT (surfaced as a crash notification under Flatpak).

## 1.0.7 — 2026-07-05
- Removed all user-facing Flathub references (About link, demo email, README/manifest/comment wording); Veem is distributed from its own signed repo, not Flathub.

## 1.0.6 — 2026-07-05
- Use "Jason M." rather than a full name in the bundled demo/sample content.

## 1.0.5 — 2026-07-05
- Renamed the application ID to `com.getveem.Veem` across the desktop file, metainfo, icons, GTK/D-Bus app ID, keyring service, and Flatpak manifest (Flathub requires an ID matching a controlled domain).

## 1.0.4 — 2026-07-05
- Manage GNOME Online Accounts from the account editor: an enable/disable toggle plus an "Open Online Accounts" button instead of Remove.
- Badge each account in the list as an "Online Account" (GOA) or "Veem" account.
- Dropped the "experimental" label from Google OAuth, which now routes through GNOME Online Accounts.

## 1.0.3 — 2026-07-05
- Guide Google sign-in to GNOME Online Accounts from the account editor when no OAuth client is configured.

## 1.0.2 — 2026-07-05
- Open sign-in and About-window links through the XDG OpenURI portal so OAuth works inside the Flatpak sandbox.
- Initial Flatpak packaging.

## 1.0.1 — 2026-07-04
- Built-in Microsoft OAuth client; Google client injected at build time (later replaced by GNOME Online Accounts).

## 1.0.0 — 2026-07-04
- First release: multi-account IMAP/POP with OAuth, a unified inbox, whole-mailbox sync and search, conversation threading, compose/reply/forward, and privacy-first reading (remote content blocked by default).
