# Changelog

## 1.10.0 — 2026-08-21

- **New: an Outbox** (#15). A send that fails no longer disappears. The composer has already closed by the time SMTP reports a failure, so anything not kept at that moment was lost; failures are now stored in the cache as the built MIME plus the SMTP envelope, and retried automatically as soon as a connection is back — plus by hand, per message or all at once. A queued message can be opened, edited and sent again; the edited version replaces the queued original rather than joining it. The Outbox appears as an ordinary folder, using the same message list and reader as any other, with a sidebar row that exists only while something is waiting. The envelope is stored separately and verbatim because `Bcc` exists nowhere else — lettre strips it from the bytes that go on the wire, so rebuilding recipients from the headers would silently drop those people. A message sent by a background retry now posts a notification: the last thing the user was told is that it had *failed*, and it going out in silence is not an improvement.
- The **"Send failed: Invalid input"** in that report was the address-parsing bug already fixed in 1.9.0. Sending with attachments now has regression tests (including a filename with a comma and a display name needing quotes), and a failed attachment read names the file — "No such file or directory" alone doesn't say which one, which matters under Flatpak where the portal's paths expire.
- **New: message previews** (#6). `Message.preview` was set to the empty string in every code path that built a summary — the preview line had never existed. The sync fetch now asks for `BODY.PEEK[1]<0.2048>` alongside the summary: section 1 is the first body part of a multipart message (text/plain in the usual layouts) and the whole body of a single-part one, so a single query covers both with no extra round trip per message. Preferences → Message List chooses **Off, 1, 2 or 3 lines**; Off also stops the fetch, since not downloading a slice of every message is half the point of turning it off.
- Those bytes arrive still transfer-encoded, and the header that declares the encoding isn't part of the response, so it is inferred from the bytes: base64 (which would otherwise show as gibberish), quoted-printable, or plain. A 2KB fetch almost never lands on a base64 group boundary, so the incomplete tail is dropped rather than decoded into noise, and a `=` escape cut in half stays literal. HTML parts become text through the same path replies already use, and `>` quoted lines are skipped so the snippet describes *this* message.
- **New: single-key shortcuts** (#5), Gmail-compatible where Gmail and the request agree, **off by default** as they are in Gmail and Geary — a stray keystroke shouldn't archive mail for someone who never asked for it. Enable them in Preferences → Message List; press **Ctrl+?** (or F1, or Main Menu → Keyboard Shortcuts) for the list, which closes on the same key or Escape.

  | Move | | Act on a message | | Everything else | |
  | --- | --- | --- | --- | --- | --- |
  | `j` `↓` | Next message | `r` | Reply | `c` | Compose |
  | `k` `↑` | Previous message | `R` | Reply to all | `Esc` | Back out of a reply |
  | `l` `→` | Open the selected message | `f` | Forward | `?` | Shortcut reference |
  | `h` `←` `u` | Back to the message list | `a` | Archive | | |
  | `w` | Next message in the conversation | `d` | Delete | | |
  | `b` | Previous message in the conversation | `!` | Mark as spam | | |
  | `/` | Search | `s` | Star or unstar | | |
  | | | `m` | Mark read or unread | | |
  | | | `x` | Select this row | | |

- The shortcut handler sits on the window in the *bubble* phase, so whatever has focus always gets first refusal — typing "archive" into the search field types it, and the composer keeps every letter. A guard covers widgets that handle keys without consuming them, chiefly the reader's web view. **Escape** backs out of a reply, forward or compose and returns to the list whether or not single-key shortcuts are enabled; in a search field it still belongs to the field.
- **Fixed: Gmail's non-ASCII folders showed as `&XfJSoGYfaAc-`** (#1). IMAP names mailboxes in modified UTF-7 (RFC 3501 §5.1.3) and nothing decoded them. New `src/mutf7.rs` implements the codec with no new dependency — a modified BASE64 of UTF-16BE where `,` replaces `/` so the hierarchy delimiter stays usable. Only display is decoded: the encoded string is the mailbox's real name, the one SELECT and APPEND must be given, so paths are still stored and sent exactly as the server states them. Encoding was missing too, so creating a folder named 测试 sent raw UTF-8; that now works, surrogate pairs included. Anything that isn't valid modified UTF-7 — a server ignoring the rule, or one speaking `UTF8=ACCEPT` — passes through untouched.
- The message row was rebuilt around the preview. The Actions Palette moved to a line of its own below the text, with the ⋯ button on the left, so nothing overlaps or reflows; rows size to their content rather than a fixed height; and the list reserves the palette's width up front, so opening it for the first time no longer shoves the whole pane wider under the pointer.
- Cached folder names from before this release correct themselves on the next folder list, a second after connecting. No cache re-sync: the Outbox arrives as a new table, and the preview column is added in place.

## 1.9.2 — 2026-08-19
- **ARM64 builds.** The Flatpak repo now carries `aarch64` alongside `x86_64`, so Raspberry Pi 4/5, Snapdragon X Elite laptops and ARM virtual machines install the same way everyone else does — `flatpak install --from …co.hyprlab.Vireo.flatpakref` resolves the architecture itself. Until now an ARM machine got the x86_64 build and failed at startup with `bwrap: execvp ldconfig: Exec format error` (#4). Both architectures are signed with the same key, so `flatpak update` verifies identically on either.
- Releases carry a standalone bundle per architecture: `Vireo-x86_64.flatpak` and `Vireo-aarch64.flatpak`. A bundle holds one architecture by design; the repo holds both, which is why the install command is the recommended route. The Fedora RPM stays x86_64-only.
- Vireo is developed and released from an x86_64 machine, so the ARM64 build is made natively on GitHub's `ubuntu-24.04-arm` runners (`.github/workflows/build-arm64.yml`) — emulating it locally through qemu-user takes hours per release rather than minutes. CI signs nothing: it uploads a plain OSTree repo, which `tools/import-arm64.sh` re-commits into the signed distribution repo under the project's key. The signing key never leaves the maintainer's machine.
- New `tools/local-manifest.sh`, which rewrites the Flatpak manifest's pinned GitHub source to build the working tree instead. The ARM job needs it because the manifest's pin necessarily lands in a commit *after* the release tag, so building the manifest as it exists at the tag would ship the previous version. Local test builds use it too.
- The About window's **Changelog** and **Release Notes** pages render their Markdown properly. Both were fed through a converter that knew only `#`, `##` and `- `, so every `**bold**`, backtick and `[link](url)` was displayed verbatim, and a long entry wrapped back underneath its own bullet because the whole document was a single label. Each block is now its own widget — bullets keep the marker in a separate column so wrapped lines align with the text, headings carry their own spacing, and inline emphasis, code spans and links become Pango markup, with links opening through the app's URI handler so they work inside the sandbox.

## 1.9.1 — 2026-08-19
- Records **[Chris Pouliot](https://github.com/chrispouliot)**'s authorship of the Proton Bridge work in a form GitHub can resolve. 1.9.0 credited him with a `Co-Authored-By:` trailer carrying the address from his own commit on #13, `chrispouliot@icloud.com` — which isn't verified on any GitHub account, so neither his commit nor the trailer could be matched to his profile and he never appeared as a contributor. This release's commit repeats that co-authorship using his `users.noreply.github.com` address, which always resolves. The 1.9.0 commit itself is left alone: `v1.9.0` is tagged, built and published, and rewriting it to fix a display detail would invalidate a shipped release.
- The About window's "Thanks" rows now show each contributor's GitHub handle alongside what they contributed, rather than hiding it in the row's link.

## 1.9.0 — 2026-08-19

First release with code from outside Hyprlab. Thanks to **[Alfonso Lizárraga](https://github.com/alfonsolzrg)** (#14) and **[Chris Pouliot](https://github.com/chrispouliot)** (#13), whose pull requests are the basis of most of what follows.

- **Fixed sending to a named recipient** (from #14). Every mailbox is now built from its parts instead of formatting `Name <addr>` and parsing that string back. An RFC 5322 display name only survives that round trip when it is a bare atom, so a name carrying an accent, a comma or a full stop — or one that is simply the address again, which is what an import with no separate display name produces — failed to parse and the send was rejected with "Invalid param". The pull request fixed `From:`; `To:`, `Cc:` and `Bcc:` went through the identical path and are now built the same way, through the existing `parse_recipients`.
- **Proton Bridge and other local mail bridges now connect** (from #13). Bridge broke two assumptions at once: it speaks STARTTLS rather than TLS from the first byte (`wrong version number`), and it presents a certificate generated on the machine at install time, signed by no CA and issued for an address rather than a name (`self-signed certificate`). IMAP now opens in plaintext and upgrades with STARTTLS when the port is 143 or when the host is this machine on any port but 993, so a bridge moved off its default port still works. Certificate and hostname verification is relaxed for loopback addresses only — where anyone able to intercept the connection is already running code as the user — covering `localhost`, `::1` and all of 127/8, and applied to SMTP and POP3 as well so a bridge is configured the same way throughout. TLS is still required; only the checks against a CA are dropped. The submitted patch keyed on literal `127.0.0.1` with Bridge's two default ports and routed *every* non-993 port through STARTTLS, which would have broken implicit TLS on custom remote ports.
- **A synced account no longer opens to an empty message list** (from #14). The worker awaited the background backfill's IMAP handshake before it would look at its request queue, but the first thing the UI asks for at startup is the visible folder — which the cache answers with no network at all. An incoming request now preempts that connect, and if the connection comes back offline the worker waits for a request instead of spinning on reconnects.
- **The message list rebuilds without cloning the folder** (from #14). Filtering and sorting now work on references and only the page actually rendered is cloned. Every rebuild — each keystroke in search, the cache-backed load at startup — previously copied the entire folder index to then discard all but `render_limit` of it.
- **The unread dot keeps its place.** Hiding it surrendered its slot in the row, so a read message's sender and preview shifted 18px left and the column jittered as mail was read. The dot is now always allocated and only its ink fades. Row spacing was rebalanced with it: 16px from the list's edge to the avatar, 8px between the avatar, the dot and the text.
- **Compose moved to the message-list header**, immediately left of the notification bell, so it sits above the list it adds to rather than above the folder tree. With the sidebar collapsed to its icon rail, the main-menu button is centred and back at full size — it was shrunk to 20px only so it could share that header with Compose.
- **New Preferences switch for the sidebar's Attachments row** (from #14), on by default.
- The sender-authentication lightbulb, the About window's new "Thanks" list, and `accounts.toml.example`'s Proton Bridge stanza round out the release. GOA's `GetAccessToken` failures are now logged with their D-Bus reason instead of being discarded (from #14).
- No cache bump: nothing about body rendering or the stored verdicts changed.

## 1.8.1 — 2026-08-13
- The sender-authentication lightbulb no longer appears and disappears with the verdict. It is always in the reader toolbar and, until a verdict for the open message has arrived, sits insensitive and greyed out like Reply, Archive and the rest — so the icon row never shifts position under the pointer. `set_visible` became `set_sensitive` on the badge's `MenuButton` (`src/app.rs`), which also means the details popover can't be opened while there's nothing to show.
- The verdict tint (`trust-pass`/`trust-suspicious`/`trust-fail`/`trust-unverified`) is now applied only once a verdict exists, via a new `App::sender_badge_classes`. `trust-unverified` carries `opacity: 0.55`, which would otherwise have stacked on top of GTK's insensitive dimming and left the lightbulb visibly fainter than its neighbours. With no verdict the tooltip reads "Sender authentication".
- No cache bump: nothing about body rendering or the stored verdicts changed.

## 1.8.0 — 2026-08-12
- **New: sender authentication.** A lightbulb badge in the reader toolbar (right of View Source) reports whether a message's `From:` address was actually forged, in four states — verified, not verified, check this sender, possible forgery. Colour carries the verdict, the tooltip names it, and clicking opens the evidence: DMARC/DKIM/SPF results, the signing domain, which authority reported each verdict, and any reply-to, bounce or display-name domain that doesn't match. A suspicious or failed verdict also raises a banner across the top of the message.
- The check (`src/verify.rs`) reads back the SPF/DKIM/DMARC results the receiving provider recorded in `Authentication-Results`. It costs no extra network traffic: `load_body` already fetches the whole message, so the verdict is computed from bytes we had.
- Providers lay those headers out differently, and getting this wrong made the feature useless before it was right. Gmail packs every method into one header; **iCloud emits one header per method** (`dmarc.icloud.com`, `dkim-verifier.icloud.com`, `spf.icloud.com`) and leads with BIMI, which carries a `header.d` but no authentication result. Reading only the topmost header — the obvious reading of "trace headers are prepended, so the first is ours" — reported every iCloud message as unverified and credited BIMI's domain as the DKIM signer. All `Authentication-Results` headers are now scanned in order, keeping the first verdict per method (so the provider's still beats anything a sender ships further down), and `header.d` counts only from a header that reported DKIM.
- Only a **DMARC** failure claims forgery. DMARC is the one check that verifies alignment with the `From:` domain; DKIM breaking while SPF passes is routine for mail relayed through bulk senders, and an earlier rule that failed on it flagged legitimate billing mail from Toyota and T-Mobile as forgeries. Crying wolf teaches users to ignore the badge, so that shape now reads "not verified" with the failure visible in the details. Both real-world cases are regression tests.
- A verdict is delivered on **every** path that delivers a body — cache hit, network fetch, body prefetch and attachment prefetch — and held in an app-side `sender_cache`. Opening a message usually renders from the in-memory body cache without asking the worker for anything, so a verdict computed minutes earlier had to be remembered or the badge stayed blank. `Show` clears the outgoing message's verdict, so the stored one is re-asserted after it, not before.
- **New: link destinations.** Hovering a link in a message shows its full target on a plaque in the bottom-left of the body, browser-style. The reader has carried a GTK tooltip for this since 1.0.0, but WebKit handles motion events itself, so GTK's hover timer often never starts and the tooltip never appeared. Since WebKit also reports the link's visible text, text claiming a different site than it points at is called out inline: `https://evil.example/login ⚠ looks like "paypal.com" but goes to evil.example`. Subdomains count as the same site, `mailto:` is ignored, and `https://paypal.com@evil.example/` resolves to `evil.example` — the userinfo trick doesn't fool it.
- Cache `SCHEMA_VERSION` → 11, adding a `sender_checks` table. Dropping `bodies` alongside it means every cached message gains a verdict on next open; the message index is kept, so no re-sync.
- New embedded icon `co.hyprlab.Vireo-lightbulb-symbolic` (Adwaita has none), drawn for this and bundled via `tools/gen-icon-gresource.sh`.

## 1.7.2 — 2026-08-10
- Right-clicking an image in a message and choosing **Save Image As…** now opens a save dialog. WebKit's stock item routes the image through a `WebKitDownload`, which needs a network-session `decide-destination` handler to choose a file — there wasn't one, so the item silently did nothing. Every image the reader draws inline is a `data:` URI whose bytes are already in the document, so `MessageView` now replaces that item (`connect_context_menu`) with one that decodes the URI in-process and opens a `gtk::FileDialog`, reusing the attachment drawer's save path. The item keeps its position in the menu. Remote (http) images still get WebKit's original item, which remains a no-op — resolving that needs the download handler.
- The save dialog pre-fills `image.<ext>` derived from the MIME type: a `data:` URI carries no filename. The correctly named copy is in the attachment list (below).
- Inline `cid:` images of **64 KiB or more now count as attachments** — they show the paperclip, appear in the attachment drawer under their real filename, and feed the gallery. `extract_attachments` and `structure_has_attachment` previously skipped every part carrying a Content-ID, which is what keeps newsletters from showing a paperclip for their logo, spacers and social icons (the false-attachment noise fixed in 1.4.1) — but a photo someone emails you arrives in exactly the same `multipart/related` shape, so that rule threw out real content with the decoration. Size is the only honest discriminator; 64 KiB sits well above logos and icons and well below any photo worth keeping. Both paths share the threshold so the paperclip and the drawer agree, and the BODYSTRUCTURE path scales it by 4/3 since IMAP reports the base64-encoded size.
- No cache bump: body HTML is unaffected. The `has_attachment` flag on already-synced messages corrects itself on the next folder sync, since freshly fetched summaries overwrite cached ones in `merge_index`.

## 1.7.1 — 2026-08-10
- Fixed inline images referenced by `cid:` not rendering — Gmail's `multipart/related` photo mail showed the image's filename (its `alt` text) where the picture should be. `extract_body` passed such HTML through untouched and nothing resolved the URL: `cid:` names another MIME part of the same message (RFC 2392) and WebKit has no handler for the scheme, so the `<img>` simply failed to load. Because `extract_attachments` deliberately skips parts carrying a Content-ID (they're meant to be rendered in place), the picture wasn't reachable from the attachment list either.
- `inline_cid_images` (worker.rs) now rewrites each `cid:` reference to a `data:` URI built from the part it names, in both the lone-HTML-part fast path and the composed multi-part path. The bytes arrived with the message, so there's no network fetch and no CSP change — `data:` was already permitted while remote content is blocked. Matching is case-insensitive, tolerates angle brackets, and percent-decodes the reference (Gmail Content-IDs often contain an `@`, sometimes written `%40`).
- Only image parts are resolved (reusing `image_mime`'s subtype validation so nothing can break out of the `data:` URI), each is charged once against the existing 16 MB inline-image budget however many times it's referenced, and unresolvable references are left verbatim rather than guessed at. Rewrites happen only in resource position (`src=`, `url(`) — never in prose, and never in an `href`, where a click hands the URL to the external browser. Parts rendered in place are no longer also appended as standalone images by the multi-body path.
- Cache `SCHEMA_VERSION` → 10: bodies are cached as rendered HTML and served without re-fetching, so already-broken copies are dropped and re-rendered on first open. Only the `bodies` table is dropped; the message index survives, so no re-sync.
- **Discontinued the Arch, Debian/Ubuntu and Snap packages** (added in 1.5.1 and 1.7.0). Releases now carry the Flatpak — repo, plus a standalone `.flatpak` bundle — and the Fedora RPM only. Each dropped package needed its own container image and a full from-source compile per release for a distribution the Flatpak already covers; `packaging/{arch,debian,snap}/` are removed and `tools/build-packages.sh` builds just the RPM. Existing installs keep working but won't see new versions — switch to the Flatpak (`flatpak install --from https://vireo.hyprlab.co/flatpak/co.hyprlab.Vireo.flatpakref`).

## 1.7.0 — 2026-08-05
- New **Debian/Ubuntu package** (`vireo_<ver>-1_amd64.deb`), published on each GitHub release. Built from source with `dpkg-buildpackage` in an Ubuntu 24.04 container (`packaging/debian/`), so `Depends:` are computed from the real linked libraries (dpkg-shlibdeps); targets Ubuntu 24.04+/Debian 13+. Uses a rustup toolchain because noble's rustc is older than the GTK4 crate stack's MSRV.
- New **Snap package** (`vireo_<ver>_amd64.snap`), also on each release. Strict confinement, `base: core24`, GNOME extension, with `network` and `password-manager-service` plugs; built by snapcraft in destructive mode inside the `ghcr.io/canonical/snapcraft:8_core24` container (`packaging/snap/`, SDK snaps unpacked by `prepare-sdk.sh` since the container has no snapd). Install with `snap install --dangerous ./vireo_<ver>_amd64.snap`.
- `tools/build-packages.sh` grew `deb` and `snap` subcommands (`all` now builds rpm + arch + deb + snap).

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
