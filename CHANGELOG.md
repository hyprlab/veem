# Changelog

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
