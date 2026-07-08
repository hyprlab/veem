# Changelog

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
