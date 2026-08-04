# Changelog

## 1.6.1 — 2026-08-03
- Flatpak reinstalls now migrate the old Veem app's data automatically. The manifest grants read-only access to the old sandbox (`--filesystem=~/.var/app/com.getveem.Veem:ro`), and on first run `migrate_flatpak_data()` (main.rs) copies `config/veem` and `cache/veem` from it into Vireo's own sandbox dirs — accounts, settings and cached mail all carry over. Copy, not rename: the legacy mount is read-only, and the old install stays untouched. Runs only under Flatpak (`/.flatpak-info` present) and only when Vireo's dirs don't exist yet; combined with the keyring fallback from 1.6.0, a Flatpak user's first launch of Vireo restores everything without re-adding accounts.

## 1.6.0 — 2026-08-03
- **Veem is now Vireo.** The app has been renamed to avoid confusion with similarly named products (Veeam Software, Veem payments). Same app, same code, new name and a new icon.
- App ID renamed `com.getveem.Veem` → `co.hyprlab.Vireo`; the binary is now `vireo`, and the GitHub repository moved to `hyprlab/vireo` (old URLs redirect). Distribution moved from getveem.com to https://vireo.hyprlab.co.
- Existing data migrates automatically on native installs: `~/.config/veem` and `~/.cache/veem` are moved to their `vireo` counterparts on first launch, and keyring entries stored under the old service name are read via a fallback and moved to the new service on first use — accounts stay signed in.
- **Flatpak installs do not carry over** (a Flatpak app's identity is its app ID): install Vireo fresh from https://vireo.hyprlab.co and remove the old Veem app. Accounts need to be re-added there (sandboxed data can't cross app IDs).
- The `VEEM_GOOGLE_CLIENT_ID`/`VEEM_GOOGLE_CLIENT_SECRET`/`VEEM_MICROSOFT_CLIENT_ID`/`VEEM_MICROSOFT_CLIENT_SECRET` build/env overrides are now `VIREO_*`.
- Embedded symbolic icons re-prefixed `co.hyprlab.Vireo-*` (gresource path `/co/hyprlab/Vireo`); `resources/veem.gresource.xml` → `resources/vireo.gresource.xml`.
- Fedora RPM and Arch package (added after 1.5.1) are named `vireo` and published on the GitHub release alongside the Flatpak.

## 1.5.1 — 2026-07-28
- A collapsed conversation now stays marked unread until every message in it is read. The thread head row carries a new aggregate `thread_unread` flag (any member unread), which keeps the unread dot and bold sender/subject visible and adds a heavier `thread-unread` accent highlight (28% vs the normal 12% for a single unread message) so unread replies hidden under a collapsed head can't be missed. Previously the head reflected only its own read state, so once the newest message was read the thread looked fully read while unread replies sat hidden beneath it.
- The flag updates in place: the message list now records rendered thread membership (message → conversation key → members) during each rebuild, and any read-state change (`MarkRead`/`SetRead`) recomputes the conversation's aggregate unread state and pushes it to the head row — so the heavy highlight clears the moment the last unread reply is read, with no list rebuild. Opening a thread still marks only the message you opened as read; hidden replies keep their unread state until individually read (expand + select, palette, context menu, or bulk actions).
- New setting: Settings → Message List → "Expand conversations by default". Off (default) keeps the existing behavior — threads start collapsed to their newest message; on renders every conversation expanded. Persisted as `threads_expanded` in `privacy.toml`, applied live. The per-thread chevron still toggles individual conversations away from whichever default is chosen (`expanded_threads` now stores exceptions to the default rather than "expanded" keys), and flipping the setting resets those per-thread toggles.

## 1.5.0 — 2026-07-19
- Reply, Reply All and Forward from the reader toolbar now open an inline compose panel that drops down over the message body (a SlideDown `gtk::Revealer` prepended into the reader pane's content box) instead of spawning a separate window. The inline panel shows only the reply body (quoted message + signature); From/To/Cc/Bcc/Subject stay hidden until the panel is expanded.
- Added an expand/collapse toggle (`view-fullscreen` / `view-restore`) to the compose header bar: inline → promote to the full compose window, windowed → collapse back into the reader. The **same** live component (and its WebKit editor) is reparented between the reader's revealer and an app-owned `adw::Window`, so the draft body, caret, selection and undo history survive the move with no reload — a `WebKitWebView` keeps its web process across a cross-toplevel reparent (verified with a spike before building). "New message", compose-to and edit-draft still open standalone windows.
- Navigating to another message while an inline reply has unsent edits auto-saves it to Drafts and closes the panel; a pristine, quote-only reply is discarded without creating a Drafts entry. Dirtiness is tracked from recipient/subject edits and an editor `input` flag (`RichEditor::is_dirty`, read via JS which also works while the pane is unrealized).
- Refactored `Compose` to be host-agnostic: its root is now an `adw::ToolbarView` (was `adw::Window`); window-coupled calls (`root.close()`, file/contacts dialog parents) resolve the live toplevel dynamically; and the app tracks composers by id (`ComposeHost` / `ReaderCompose`, `ComposeOutput::{ToggleWindow,Close}`) to host, reparent and tear them down. The inline pane uses a fixed editor height (300px, scrolls internally) with `vexpand` disabled so the panel height is deterministic regardless of reply length.

## 1.4.6 — 2026-07-19
- Changed the message-list Actions Palette toggle from a chevron to a horizontal ellipsis (⋯), the more conventional "more actions" affordance. Because the ellipsis reads the same open or closed, the icon is now static — the palette sliding open (the revealer) is the state cue — replacing the previous open/closed chevron-direction switch (`pan-start`/`pan-end`) in `src/ui/message_list.rs`. Added the `view-more-horizontal-symbolic` icon to the embedded, app-ID-prefixed icon set (sourced from Adwaita; regenerated via `tools/gen-icon-gresource.sh`).

## 1.4.5 — 2026-07-19
- Fixed sidebar unread chips not updating in real time while viewing a single account. IMAP IDLE watches one folder per account (each account idles on its own inbox after the default "All Inboxes" load), and on new mail it re-synced the message list but never re-emitted the unread *count* — so a background inbox's chip stayed stale until an explicit reload (e.g. clicking "All Inboxes"). The IDLE `NewData` handler in `worker.rs` now also runs `selected_unseen` and emits `WorkerEvent::FolderUnread`, which flows through `AppMsg::FolderUnread` → `push_unread_counts` → in-place chip update (including the "All Inboxes" total), so chips tick up on their own without a manual refresh.
- Collapsing an account's section now shows its inbox unread count on the account's avatar circle instead of hiding it. Previously the Inbox row's chip slid into the (hidden) folder-list revealer on collapse, so the count disappeared until the account was expanded again. The avatar is now wrapped in the same mini unread-badge overlay used by the collapsed "All Inboxes" rail (new `account_circle_badges` map in `sidebar.rs`), visible only while the section is collapsed and kept in sync on build, on live count updates (`SetUnread`), and on live collapse/expand toggles (`ToggleCollapseLocal`).

## 1.4.4 — 2026-07-16
- Fixed messages showing a blank date in the message list and the reader header when the sender omits (or sends an unparseable) `Date:` header. Some bulk mailers — e.g. the "Trusted Servants Pro" notifications delivered to `public@dccma.com` — emit no `Date:` line at all, so Veem derived an empty label and a `0` sort timestamp, leaving those rows dateless and sinking them to the bottom of the list.
- Veem now falls back to the IMAP `INTERNALDATE` (the server's delivery date — i.e. the date of receipt) whenever the `Date:` header is missing or fails to parse. `INTERNALDATE` was added to the four summary FETCH item lists (the structured-`ENVELOPE` and raw-header paths in both `fetch_window` and `fetch_summaries_by_uid`), and a new `internal_date_summary()` helper feeds the fallback in `build_summary` and `summary_from_headers`. Both the list row and the opened-message header key off the same `timestamp`, so populating it fixes both places and restores correct sort order. Existing cached rows self-correct on the next folder sync (summaries are written with `INSERT OR REPLACE`).

## 1.4.3 — 2026-07-15
- Fixed contact names in the contacts browser displaying in all lowercase. EDS stores each book's `full_name` column case-folded for search (e.g. `aaron arnwine`); the properly-cased name lives only in the vCard's `FN` property. The reader now selects the vCard column (`ECacheOBJ` for CardDAV caches, `vcard` for the local book) and parses `FN` — preserving the original capitalisation — via a new `vcard_display_name()` that handles line folding, property parameters, and text escapes, falling back to the email when `FN` is empty. Covered by unit tests.

## 1.4.2 — 2026-07-15
- Fixed GNOME Contacts integration under Flatpak: the contacts browser showed an empty list and the "Open GNOME Contacts" button did nothing. Both were sandbox-only (native builds were unaffected).
- Empty list: inside the sandbox `dirs::{data,cache,config}_dir()` are redirected into `~/.var/app/com.getveem.Veem/`, so the Evolution Data Server SQLite books were never found. Under Flatpak the reader now resolves the EDS address-book dirs from the real home (`~/.local/share`, `~/.cache`, `~/.config`) and opens the books with `immutable=1` (a read-only host mount can't service a WAL database otherwise). Added `--filesystem=xdg-{data,cache,config}/evolution:ro` so the caches are visible, and `read_book_db` now logs failures instead of silently returning empty. This also fixes book enumeration for "Add to Contacts".
- "Open GNOME Contacts" did nothing because it exec'd `gnome-contacts` inside the sandbox, where it doesn't exist. Under Flatpak it now D-Bus-activates the host app (`org.gnome.Contacts` → `org.freedesktop.Application.Activate`), with a `flatpak-spawn --host` fallback. Added `--talk-name=org.gnome.Contacts` and `--talk-name=org.freedesktop.Flatpak`.
- Verified in the actual sandbox: 191 contacts read (matching native), and GNOME Contacts launches on the host.

## 1.4.1 — 2026-07-14
- Fixed false attachment indicators (the paperclip) on iCloud messages that have no real attachments — typically marketing/HTML mail (e.g. TheraPlatform, Atlas Arts). iCloud sends non-compliant BODYSTRUCTURE, so Veem falls back to a header-only path that guessed "has attachment" from a top-level `Content-Type: multipart/mixed` — which is also how newsletters wrap their HTML plus inline `cid:` images, so they were wrongly flagged even though `extract_attachments` (which the drawer uses) correctly skips inline parts.
- Added `Cache::attachmentless_uids` (messages in `attachments_checked` with zero stored attachments) and reconcile the `has_attachment` flag against it in `cache::load_messages` and after a fresh server fetch (`worker::reconcile_attachment_flags`). This is robust against re-sync re-deriving the flag and can never hide a real attachment (a message is only reconciled once its full body has been extracted).
- Added a live correction: background prefetch now emits `WorkerEvent::NoAttachments` when a flagged message turns out to hold no real attachments, and the message row drops its paperclip immediately (the indicator is now `#[watch]`ed) — no refresh needed.

## 1.4.0 — 2026-07-14
- Added an in-message attachment drawer: a resizable footer beneath the reader body that shows every attachment on the open message as a wrapping grid of square (1:1) thumbnails — images as cover-cropped previews, everything else as colour-coded type icons — each with its filename beneath it. New module `src/ui/attachment_drawer.rs`.
- The drawer owns a vertical `GtkPaned` whose top pane is the reader body and bottom pane is the drawer, so the divider is a smooth native resize grip and the reader shrinks rather than the window growing. A collapse/expand chevron in the drawer header hides the grid to just the header; a size slider scales the thumbnails (thumbnail size and height do not affect each other). Only the collapsed/expanded state is remembered across launches (via `state.toml`); height defaults to 160px and thumbnails to the slider minimum each session.
- Thumbnails use a fixed-size `SquareBox` widget so images can't blow the cell out to their native pixel width; the grid flows left-to-right and wraps. Hovering a cell reveals Download/Open quick actions (matching the gallery, ~25% smaller); right-click gives an Open/Download menu; single-clicking an image opens a modal lightbox (prev/next, ←/→, Esc), and clicking a non-image opens it in its default app.
- The reader header's attachments dropdown now shows an image thumbnail (or type icon) per row and Preview / Open / Download actions. Preview reuses the drawer's lightbox and Download reuses its file chooser.
- Reused the attachments gallery's thumbnail/icon/open helpers (`texture_from`, `icon_for`, `icon_color_class`, `open_bytes`) — now `pub(crate)` — across the drawer and popover.

## 1.3.9 — 2026-07-13
- Fixed the app not pulling new mail after the system resumes from sleep. IMAP worker sessions are long-lived (one persistent connection per account, plus a parked ~29-minute IMAP IDLE when push is enabled); suspending the machine silently kills those sockets, and previously nothing detected the resume — so no new mail arrived, and even the Refresh button couldn't help because a `LoadMessages` request could sit behind a worker parked in an IDLE wait, until the app was restarted.
- Added a systemd-logind watcher (`src/power.rs`, new `power` module) that subscribes to `PrepareForSleep` on the **system** D-Bus (Flatpak-safe) and fires `AppMsg::SystemResumed` on the resume edge (`start == false`), modeled on `goa::watch_removals`. It no-ops silently if logind/the system bus is unavailable.
- On resume, Veem sends `MailRequest::Reconnect` to every worker — this drops the stale session, logs in fresh and re-arms IMAP IDLE, and also unsticks any worker parked in an IDLE wait (the request breaks its `select!` loop) — then triggers a `Refresh` to reload the visible folder and re-arms the auto-fetch timer whose monotonic countdown was frozen during sleep.

## 1.3.8 — 2026-07-12
- Added a "Keyring Setup Help" row to the About window for Linux Mint / Cinnamon users. It reopens the one-time keyring setup tip (added in 1.3.7), so anyone who dismissed it can bring it back. The row only appears on Mint/Cinnamon (gated on the same `platform::is_mint_cinnamon()` check as the tip), and activating it emits `AppMsg::ShowKeyringHelp { problem: false }`.

## 1.3.7 — 2026-07-12
- Passwords that fail to save to the system keyring no longer fail silently. Veem stores account passwords in the Secret Service (never on disk); if the keyring doesn't actually persist the password (e.g. no keyring is set up, or it's locked), the account would previously look saved but couldn't sign in after a restart. Veem now verifies the password round-trips after saving and, if not, shows a dialog explaining how to set up the keyring.
- Added Linux Mint / Cinnamon detection (`src/platform.rs`, Flatpak-aware via `/run/host/os-release`) and a one-time, dismissible setup tip shown there. It covers installing gnome-keyring + seahorse, creating a default "Login" keyring, and — crucially — how to stop the keyring prompting for an unlock password at every login (match the Login-keyring password to your user login password and avoid automatic login, or blank the keyring password to remove the prompt entirely, at the cost of at-rest encryption). The "don't show again" choice is saved in `~/.config/veem/state.toml`.

## 1.3.6 — 2026-07-12
- The symbolic icons used throughout the UI are now embedded in the binary instead of pulled from the host icon theme, so they look identical on every distribution. On some systems (e.g. Zorin) icons previously rendered differently or went missing because the local icon theme drew them its own way or lacked them entirely.
- Every icon Veem draws (59 of them) is bundled as a GResource compiled into the binary by `build.rs` and registered at startup, each renamed with a `com.getveem.Veem-` prefix so no host theme can override it. Sources: `resources/icons/` + `resources/veem.gresource.xml`; regenerate with `tools/gen-icon-gresource.sh`. GTK's own window chrome (close button, back arrows) still follows the host theme.
- No filesystem icon install is needed anymore (works the same under Flatpak); the dev-only theme search path is retained just for the app icon when running uninstalled.

## 1.3.5 — 2026-07-12
- Sidebar folders are now split per account: the essential folders (Inbox, Sent, Drafts, Archive, Junk, Trash, Starred) stay visible, while user-created folders are tucked under a collapsible "Folders (N)" section that's hidden by default. Its expanded/collapsed state is saved per account and persists between restarts. Drag-and-drop, right-click actions, and selection all work through the collapsed section.
- Also in the attachments gallery: the thumbnail hover actions now include Download and Go to Message alongside Open (Go to Message shows even for attachments that aren't cached yet).

## 1.3.4 — 2026-07-12
- The attachments gallery gained a search bar: filter by sender, subject, filename, folder, or file-type keywords (e.g. "pdf", "image", "spreadsheet"). Multiple words are matched together, and a "No matching attachments" page shows when a search comes up empty.
- Added a sort control with Newest/Oldest, Name (A–Z / Z–A), Sender (A–Z / Z–A), Largest/Smallest first, and Type (A–Z / Z–A).
- Each attachment now shows the source message's date in its meta line (and in the lightbox caption), alongside the folder and size.

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
