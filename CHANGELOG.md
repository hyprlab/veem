# Changelog

## 1.15.6 — 2026-08-26

### The Flatpak really opens attachments now

Three sandboxed-open fixes, found by testing against a real installed
Flatpak (`flatpak-builder --run` turned out to have no session bus at
all — nothing portal-shaped can be tested there):

- **Staging moved to `$XDG_RUNTIME_DIR/app/$FLATPAK_ID`.** The document
  portal validates an exported fd by re-opening its path in the HOST
  namespace; a file in the sandbox's private /tmp fails that silently,
  killing every OpenFile before any UI.
- **The portal protocol is spoken directly** (GIO D-Bus): GTK's
  FileLauncher mis-finishes its own async task in this runtime — its
  callback never fires, so successes and failures alike vanished. We now
  subscribe to the request's Response first (own handle_token, no race),
  pass the fd, and read the verdict: quiet attempt, then one `ask: true`
  chooser retry (cancel respected), dialog only on real failure.
- **Options marshal as `a{sv}`** via `Variant::tuple_from_iter` — a Rust
  tuple's ToVariant boxed the dict as "(shv)" and the portal rejected
  every call.

Verified live: chooser → Papers on a machine whose direct portal
launches are broken.

### The lightbox

- **Fills the Vireo window** instead of opening a second window with its
  own titlebar — same overlay look as the gallery's, driven by the app
  (`DrawerOutput::ShowLightbox`), with Escape/arrow keys handled at the
  window level (capture phase, gated on the lightbox being open).
- **Click to zoom 3x, anchored at the click** — the clicked content
  centres in the viewport (a tick callback waits for the resize to lay
  out before positioning). Click or Escape returns to fitted; Escape
  from fitted closes; **dragging pans** the zoomed document (a shared
  movement threshold keeps a pan from also zooming).

## 1.15.5 — 2026-08-26

### Opening attachments: a chooser fallback, and the truth on failure

- **When the portal's direct launch fails, Vireo retries with the app
  chooser** (`OpenFile` with `ask: true`, one blocking zbus call from a worker
  thread). The chooser is the portal backend's own dialog and launches the
  picked app through different machinery than the failing default-handler
  path — verified working on a machine whose direct launches all fail —
  and "always open with" persists in the permission store, so it's one
  confirmation ever, not one per open.
- **The failure dialog earns its keep**: it shows the portal's own error text
  (reportable verbatim) and the two steps that actually help — Download →
  open from Files, and updating `xdg-desktop-portal` + re-login. No Flatseal
  advice: portal access is not a permission, so no toggle exists for this.
  Also fixed: a codegen step had baked a run of 14 literal spaces into the
  dialog copy, which rendered as a janky gap.
- **Double-clicking a lightbox preview opens the document externally** — in
  the gallery's lightbox and the drawer's alike.

## 1.15.4 — 2026-08-26

### The Flatpak opens attachments again

- **Root cause:** the sandboxed build stages an opened attachment into the
  app's *private* `/tmp`, and then handed the desktop portal a `file://` URI
  *string* — a path that, host-side, does not exist. The launched viewer
  pointed at nothing, on every machine; the click read as dead. (The fix took
  a detour: one test machine's portal is also broken for host callers, which
  masked the real mechanism until a second machine reproduced it.)
- **Fix:** the sandboxed branch now launches through `gtk::FileLauncher`,
  which passes the staged file as a **file descriptor** via the document
  portal — the portal exports it at a host-readable path and hands that to
  the handler. Native builds keep GIO-first launching.
- When a portal genuinely cannot launch anything, both chains now end in a
  dialog saying so — and that Download still works — instead of silence.

## 1.15.3 — 2026-08-26

### Send-as aliases (#34)

- An account can declare extra From identities — a "Send-as aliases" field in
  the account editor (`Ann <ann@work.org>, ann@shop.org`). The composer's
  From menu offers every identity; an alias changes only the From header on
  the wire (and the Message-ID's domain, so replies thread back) — SMTP
  server, credentials and the Sent copy stay the account's. Replying to mail
  addressed to an alias answers from that alias automatically. Aliases
  persist in accounts.toml; two new tests pin the wire format.

### Attachments

- **The drawer activates on double-click only.** Its single-click lightbox
  stole the first click of every double-click (the modal opened, the second
  click landed in it), which made "open externally" unreachable and read as
  the Open path being broken. Single clicks now do nothing; double-click
  previews images/PDFs in the lightbox (whose Open button launches the
  default app) and opens other types externally at once. The toolbar
  attachment menu matches: double-click a row to open, and its Preview
  button drives the lightbox directly — which also fixes a latent index
  mismatch when the drawer's list view was sorted.

### Fit and finish

- The conversation chevron pill's border-radius drops from 9999px to a
  concrete 10px: GTK paints an oversized radius on a 1px-bordered pill with
  a faint dot at the centre of each rounded end.

## 1.15.2 — 2026-08-26

A community-feedback release: most of this answers p-mitana's issue
series (#22, #25, #27, #28 — #23 was already fixed in 1.13.4) and the
inline-forward half of #52.

### The reader (#22)

- **Conversation cards lead with an initials avatar**, tinted per sender
  address (pure escaped markup — no image bytes cross into the sandboxed
  document), so who wrote each card and which are your own Sent replies reads
  at a glance.
- **Card headers stick** to the top while their message scrolls, over an
  opaque ground; hovering anywhere on a card tints its header.
- **Reading width caps at 1000px**, centred, for cards and single messages.
- **View Source** shows angle brackets (new `code-symbolic`) instead of the
  ghost; the **sender-authentication badge** is a checkmark seal (new
  `verified-checkmark-symbolic`) instead of the lightbulb.

### The composer (#25, #52)

- **The inline reply/forward pane shows its address fields.** From (with more
  than one account) and To in every host — an inline forward was literally
  unaddressable before. Subject shows for new messages and drafts, and waits
  behind the To row's "More" button for replies/forwards, with Cc and Bcc
  (Cc surfaces on its own when a reply-all carries it). Focus lands in To
  when it's empty.

### The message list (#27, #28)

- **Sent-folder rows name their recipients** — "To: <names>", with the circle
  showing (and face-looking-up) the first recipient instead of the sender.
- **The pane width survives a restart**: divider drags persist (debounced,
  one write per drag; clamped 350–4000px) and the window opens with it. The
  fixed opening width is gone; `state.toml` gains `list_pane_width`.

### Attachments and chrome

- **The drawer previews in-app**: single click shows images *and PDFs* in its
  lightbox (PDF first pages render on workers into the shared full-size
  cache); other types ignore single clicks, and double click opens anything
  externally — grid cells and list rows alike.
- **Opening externally works outside Flatpak** even where the XDG portal
  accepts an OpenURI and silently launches nothing: native builds lead with
  GIO and fall back to the portal; the Flatpak keeps portal-first.
- **"Go to Message" moves the sidebar highlight** to the message's folder
  (same for notification opens), so Attachments can be clicked to return.
- The status bar leads with the bell (matching its toolbar button), the
  thread chevron is an accent-outlined pill sized to its count chip, and an
  expanded conversation widens the list floor by the indent so child rows
  never clip.

## 1.15.1 — 2026-08-26

### Attachments

- **Attachments download the moment a message or conversation opens** — disk
  cache first, then the server. The reader's "load attachments" button (a
  download icon that turned into the paperclip) is gone: the toolbar shows a
  spinner while fetching and the paperclip when they land. Applies to single
  messages, whole threads, popped-out windows and attachments discovered late
  in a body (inline PDFs, #9); `AttachmentsPending` remains handled as a
  safety net that fetches instead of asking.
- **The gallery lightbox renders PDFs in-app**: a full-size first-page render
  (1600px, off the main thread behind a spinner page, cached per content
  hash — failures too, so a broken PDF isn't re-rendered on every arrow
  press). Stepping through mixed images and PDFs stays in the lightbox.
- **Table view: headers line up with their columns** — the header buttons'
  own padding was offsetting every label — and rows gain the grid's quick
  actions as a trailing column (Download, Open, Go to Message), with a
  matching header spacer.

## 1.15.0 — 2026-08-26

### Attachments

- **Opening an attachment works again** (adapted from PR #44 by Isaac,
  @thecalamityjoe87). The staging code passed `0o200000` as O_NOFOLLOW, but on
  Linux that value is O_DIRECTORY — creating a regular file with it fails, so
  no attachment could be staged, and the symlink refusal the flag was meant to
  provide was silently absent. The constant is now the real O_NOFOLLOW
  (`0o400000`), with a comment naming the old bug. Opening also goes through
  the XDG portal (`gtk::UriLauncher`, properly parented) instead of
  `AppInfo::launch_default_for_uri`, which under Flatpak — or for a type with
  no registered default app — silently does nothing; the portal shows GNOME's
  app chooser instead, and the old call remains as a fallback when the portal
  itself is unreachable.
- **PDF attachments show their first page as the thumbnail** in the gallery
  and the in-message drawer (adapted from PR #49 / issue #48, also Isaac):
  rendered via poppler-glib at 360px on a white ground, falling back to the
  type icon when a PDF won't parse. The Flatpak manifest gains a poppler
  module (config after Evince's, JPEG2000 off); native builds need
  poppler-glib-devel / libpoppler-glib-dev.
- **All thumbnails decode off the GTK thread, behind per-cell spinners.**
  Rendering PDF pages (and decoding images) while cells were being built froze
  the window — long enough for GNOME's Force Quit dialog on a real mailbox.
  Cells now show a spinner immediately, workers produce the texture, and
  finished renders are cached by content hash so rebuilds, searches and
  revisits this session never decode the same attachment twice. The gallery's
  "Loading attachments…" page also actually shows now (its flag was being
  reset by the clear that followed it).
- **The attachment drawer covers whole conversations.** Opening a thread never
  loaded attachments at all — the request lived only in the single-message
  path, so the drawer stayed empty until a member was clicked and re-clicked.
  A conversation now asks the disk cache for every member's attachments as it
  opens and shows the deduplicated union, re-merging as members' attachments
  or late-arriving related messages land. Selecting one message out of the
  thread still shows just that message's attachments.

### The attachments gallery

- **A control footer**: grid ↔ table view toggle, a type filter (images,
  PDFs, documents, archives, audio & video, other), the sort dropdown (moved
  down from the toolbar, which keeps just the search), a thumbnail-size
  slider (140–380px, one debounced rebuild per drag), and a shown-of-total
  count. View choice, thumbnail size and sort order persist across sessions.
- **A table view**: name, sender, type, date and size under clickable column
  headers — click to sort, click again to flip, with the dropdown following.
  Rows keep the grid's behaviours (click to preview, double-click to open,
  right-click for the menu) and reuse already-rendered mini thumbnails
  without ever spawning renders of their own.

### The reader

- **A "To:" line above the Cc line** for single messages (adapted from PR #43,
  Isaac) — at a glance, who a Sent/replied message went to, or which of
  several addresses an auto-forward landed on. Conversation cards already
  carry this in their recipients chip (1.14.2), so nothing changes there.

### Project

- **GitHub release pages carry only their own release's notes** (issue #46,
  suggested by @yioannides). `tools/release-notes.sh` extracts one version's
  section (RELEASE_NOTES.md, falling back to CHANGELOG.md) with a footer
  linking the full history; all 72 existing releases were rewritten through
  it.

## 1.14.4 — 2026-08-26

### Sender avatars

Adapted from PR #8 by Anton Palgunov (@Toxblh), unified with the sender-logo
pipeline that had grown on main in the meantime.

- **GNOME Contacts photos as avatars.** A background thread indexes photo
  locations (not bytes) from Evolution Data Server's local and CardDAV
  address-book caches, with a stat-fingerprint stability check against
  half-written SQLite files. Sender circles resolve personal-first: contact
  photo → Gravatar → domain icon → coloured initials, each tier consulted
  only while its switch is on — so switching a tier off hides its cached
  images immediately (this also fixed cached sender logos surviving the
  "Show sender logos" switch being turned off).
- **vCard PHOTO parsing** handles Nextcloud `data:` URIs, standard folded
  `ENCODING=b`, grouped (`item1.PHOTO`) properties, and EDS-materialized
  `file://` photos. Remote PHOTO URLs are never fetched.
- **Confinement and decode hardening.** `file://` photos are canonicalized
  and confined to EDS's own data/cache directories (`confine_to_roots`, with
  tests for `..` traversal, symlink escape, lookalike sibling directories,
  unresolvable roots and host-bearing URIs). Only known raster formats reach
  the decoder — never SVG — and images are size-capped, pixel-capped and
  downscaled to 160px during decode, off the GTK thread.
- **Gravatar improvements.** Requests are coalesced per sender with a
  concurrency cap, timeouts and a 30-second retry backoff; only a definitive
  404 is cached as a miss. No hash leaves the machine before the local
  contact index has had a chance to answer, and the hash sent is now SHA-256
  rather than dictionary-reversible MD5 (the `md5` dependency is gone).
- **Fresh without flicker.** An EDS/CardDAV sync bumps the index generation
  and refreshes visible circles without moving the list's scroll position;
  generation-stamped caching discards stale results rather than masking a
  newly synchronized photo. The reader correlates avatar results by sender
  address instead of mailbox-scoped IMAP uid.

## 1.14.3 — 2026-08-26

### GNOME Online Accounts

Adapted from PR #7 by Anton Palgunov (@Toxblh).

- **Custom server ports are honored.** GOA stores a non-standard port inside
  the host string itself (`mail.example.com:1143`, `[2001:db8::1]:1993`);
  discovery now splits host and port apart — IPv6 brackets included — instead
  of discarding the port and assuming 993/143 and 465/587.
- **Pausing instead of losing.** Switching an account's Mail service off in
  GNOME Settings pauses the account in Vireo: workers stop, it leaves the
  sidebar, and every local setting (label, colour, emoji, signature, sidebar
  state) survives. Switching Mail back on restores it, including whether it
  was enabled in Vireo before the pause. New `AccountConfig` fields
  `goa_mail_disabled` and `goa_enabled_before_mail_disabled`; the accounts
  window locks the enable toggle and says why while GNOME Settings owns it.
- **GOA changes are handled off the main thread, debounced.** One Settings
  edit emits a burst of D-Bus signals; the watcher now coalesces them for
  300 ms and takes a single `GetManagedObjects` snapshot on its own thread,
  delivered inside the `GoaChanged` message — the GTK main thread no longer
  performs blocking D-Bus I/O. The watcher also subscribes to
  `PropertiesChanged` on account objects, so property edits (the Mail toggle
  among them) are seen at all, not just removals.

### Connections

- **The connection test authenticates SMTP the way sending does.** OAuth
  accounts (Gmail, Microsoft) are tested with XOAUTH2 and a fresh token
  instead of PLAIN/LOGIN with a password they don't have — so their test no
  longer reports a false failure. The server address is passed as a
  `(host, port)` pair, so a bare IPv6 SMTP host no longer mis-parses.
- **IMAP connection attempts time out after 30 seconds.** TCP connect, the
  TLS handshake and authentication together now run under one deadline, so a
  server that accepts the socket and stalls surfaces an error naming the host
  instead of hanging the worker forever.

## 1.14.2 — 2026-08-26

### The window

- **Tiles to half a screen.** With a populated mailbox the window's minimum
  width was 1070px — wider than half of a 1920px display — so GNOME refused
  Super+←/→ and edge-drag tiling and offered only the top-edge maximize. The
  message list and sidebar scrollers no longer impose their content's width on
  the window's minimum: names ellipsize and rows clip gracefully instead of
  vetoing the tile.
- **The sidebar gives way when the window narrows.** Below 1120px — a
  half-screen tile on a 1920px display — the sidebar drops to its icon rail
  automatically, which is what leaves the message list its full Actions Palette
  width in the tile. The user's own collapse preference is restored the moment
  there is room again, and the automatic switch never overwrites it.
- **Expanding while narrow floats.** From the rail in a tiled window, the
  sidebar's expand button opens it as an overlay above the panes rather than
  pushing them aside. Picking a folder closes it, as does clicking the dimmed
  content or pressing Escape.

### The sidebar

- **Every folder shows its unread count**, not only the Inbox. The counts were
  already fetched per folder (IMAP `STATUS (UNSEEN)`); now each folder row
  carries the badge, updating in place — in the icon rail it rides the icon's
  corner.
- **Sub-folders in the server's structure.** Custom folders sort by their full
  hierarchical path, case-insensitively, and nested folders are indented under
  their parents — rather than appearing in whatever order the server's LIST
  returned them. Depth follows listed ancestors, so a Dovecot-style `INBOX.`
  namespace prefix doesn't produce a phantom level.

### The message list

- **The toolbar's Delete deletes the whole selection.** With several messages
  selected, the trash button (and the `d` shortcut) removes them all through
  the same path as the selection bar — including the "delete permanently"
  confirmation when the selection is already in Trash. Its tooltip says how
  many it is about to take.

### Conversations

- **Each card names everyone the message went to.** An "N recipients" chip in
  the card's header expands into the full To/Cc list — selectable for copying —
  and collapses again so headers stay one line tall. Recipient headers are
  attacker-controlled text and land escaped in the trusted wrapper document,
  under regression tests.

### Attachments

- **The drawer grew a list view.** A header toggle switches the thumbnail grid
  to an alphabetical list — type icon, filename, size, Download/Open — with an
  A→Z / Z→A switch beside it. Both choices persist. Hovering anywhere on a
  thumbnail now shows the full filename.

### Status bar

- **The notification dropdown is now called the status bar** — in its tooltip
  and its panel — which is what it is.

## 1.14.1 — 2026-08-25

### Conversations

- **Mail that was already in the mailbox threads.** 1.14.0 stamped a
  `threading_since` instant the first time it ran and grouped only messages at
  or after it. An account added after that — or the same account re-added on
  another machine — therefore had an inbox where nothing threaded at all, however
  complete its `References` headers were, because `compute_thread_keys` filtered
  on the timestamp *before* the union-find ran and those messages never reached
  it. The stamp, the `thread_old_mail` preference that gated it, and the
  `ts >= ?` predicate in `messages_by_thread_ids` are gone: a message threads
  because its headers say what it answers, whenever it was sent. Covered by a
  test built from the headers iCloud actually delivers.
- **The References repair runs for every folder**, rather than only when the
  retired setting was switched on. A message indexed by an older build carries
  only its In-Reply-To, and threading reads References — so this is what makes
  old mail threadable in the first place. Unchanged otherwise: replies only,
  chunked at 500, resumable by watermark, once per folder.
- **Conversations join across folders further back.** The reply-header pool grew
  from 4,000 messages to 20,000. It bounds memory and rebuild cost, never
  correctness — a conversation whose members are all on screen never consults it
  — and what one conversation costs to *open* is still capped at 50 members, the
  bound the date was only ever a proxy for.

## 1.14.0 — 2026-08-25

### Conversations

- **Threading is back, and no longer exhausts memory.** 1.13.3 could consume
  every byte on the machine when a message was opened. The cause was one wrong
  index: `messages_by_thread_ids` binds ids, then padded ids, then the account,
  but read the `References` comparisons from slot `n+2` instead of `n+1`, so
  every one shifted and the last ran against the *account id* — which SQLite
  coerced to text, matching every message whose References merely contained that
  digit. On a real cache one click returned 6,278 "related" messages instead of
  1, each a body to fetch and a re-render of a document holding all the others.
  Two tests cover the query; the first fails on the old indices.
- **A conversation reads oldest first**, with the message that started it as the
  row on screen and its replies descending beneath. Opening the head shows the
  whole thread in that order; opening a reply shows that message alone.
- **Each message is a card**, with its own Reply, Reply all and Forward — the
  toolbar's are disabled while a conversation is shown, because they could not
  say which message they meant. Clicking a card selects that message, Ctrl and
  Shift extend the selection as they do in the list, and the two stay in step in
  both directions. A quoted reply chain is folded behind a ••• that expands and
  collapses, the card growing and shrinking with it.
- **Unread messages are marked** and clear as they are scrolled through, rather
  than the whole thread being marked read on opening it.
- **Threading applies from this release forward by default**, with "Thread older
  messages too" for the whole mailbox. An archive's conversations run years and
  hundreds of messages deep; the two things the date bound was quietly holding
  down — the cross-folder links and the size of one conversation — are bounded
  by count instead, so either setting is safe.
- **A conversation is joined through the folders it spans.** Every reply in an
  Inbox answers something in Sent, so where both sides of an exchange arrived in
  one mailbox it grouped and where they did not it fell into pieces. Reply
  headers from an account's other folders now join what is shown without
  appearing themselves.
- **Replies carry `In-Reply-To` and `References`, and our own mail carries a
  `Message-ID`.** Vireo threaded incoming mail by headers it never wrote: a
  reply began a new conversation for everyone who received it, and with no id of
  our own the SMTP server assigned one on the way out, leaving the copy filed in
  Sent with no identity at all.
- **Gmail's labels no longer show a message three times** ([#45](https://github.com/hyprlab/vireo/pull/45)).
  One message lives in INBOX, All Mail and Important at once; the reader keyed
  on folder and UID, so a six-message thread rendered as eighteen. Bodies,
  sender checks and attachments were keyed the same way, so data cached under
  one label was invisible to the others. Both now match on Message-ID, with
  sender and timestamp required to agree.
- **A conversation's bodies are fetched in one request** rather than one apiece,
  in batches of ten ([#45](https://github.com/hyprlab/vireo/pull/45)).

### The reader

- **Opening a conversation paints once.** It re-rendered per body that arrived,
  and each render is a full page load — so the reader flashed its way through
  loading. It now holds the spinner until the conversation has settled, bounded
  so an unanswered lookup cannot strand it, and returning to a thread already
  assembled paints from it with no spinner at all.
- **A message frame opens at the height it had last time**, so a reopened
  conversation lays out on its first frame instead of every card jumping once
  measured. Frames are sized to their content rather than only ever grown, so a
  short message no longer sits in a tall card.
- **The spinner and the cover behind it follow the message theme**, not the
  app's: a light message in a dark app used to hand a dark spinner to a white
  page.
- **A body can no longer be applied to the wrong message.** A UID is unique only
  within its folder, and the background prefetch pushes bodies from every folder
  it syncs.

### Elsewhere

- **Dragging several messages moves all of them** ([#23](https://github.com/hyprlab/vireo/issues/23)).
  The payload was built from the dragged row alone, and the drop was discarded
  when that row's account differed from the target folder's — which is why
  dragging from the unified inbox often did nothing at all.
- **"All Inboxes" no longer omits an account** that was still syncing, offline,
  or busy backfilling. It cleared every account's slice on entry and rebuilt
  from whatever came back; it now keeps each account's last known inbox and
  tops it up from the caches.
- **Preferences: the sender fields sit with their lists.** "Always allow sender"
  is the first row of Allowed Senders, and it and the Blacklist row add with a
  + button.
- **The blocked-remote-content banner can be quieter, or silent.** A grey style
  with outlined buttons, and a switch to hide it — which gates the notice only;
  remote content is blocked exactly as before.

## 1.13.5 — 2026-08-24

- **Fixed: replies sent from Vireo were not part of any conversation.** Vireo
  threaded incoming mail by its reply headers but never wrote them on the mail
  it sent: `build_email` set From, To, Cc, Bcc and Subject and nothing else. A
  reply composed in Vireo therefore began a new conversation for every client
  that received it — Vireo included — so replying back and forth between two
  accounts produced a pile of unrelated messages. The composer now carries the
  parent's Message-ID and the thread's id chain from the reply prefill through
  to the outgoing message, and `build_email` writes `In-Reply-To` and
  `References`, re-wrapped in the angle brackets the wire format wants (they are
  stored bare). References carries the whole chain rather than just the parent,
  so a client can place the reply even if it never saw the immediate parent.
  Tests cover both directions: a reply carries both headers, and a message that
  starts a conversation carries neither. Mail already sent without them cannot
  be repaired — there is nothing on the wire to reconstruct from. Forwards
  deliberately stay out of the parent's thread.

## 1.13.4 — 2026-08-24

- **Conversation threading is back, and no longer exhausts memory.** 1.13.3
  grouped messages into conversations and pulled in the rest of a thread from
  other folders. Opening one message could then consume every byte of memory on
  the machine. The cause was a single wrong index: `messages_by_thread_ids`
  binds its parameters ids-then-padded-then-account, but read the `References`
  comparisons from slot `n+2` instead of `n+1`. Every comparison shifted by one,
  and the last ran against the *account id* — which SQLite coerced to text, so
  `instr` matched every message whose References merely contained that digit. On
  a real cache one click returned 6,278 "related" messages instead of 1, each
  becoming a body to fetch and a re-render of a document holding all the others.
  Two tests cover the query now; the first fails on the old index.
- **Threading applies to mail from this release forward.** The cutoff is stamped
  once into `state.toml` on first run. Older mail never groups and is never
  pulled into a conversation: an archive's conversations run years and hundreds
  of messages deep, and every member is a body the reader loads and renders.
  Recent mail threads normally — in a real mailbox the largest conversation in
  90 days is five messages. This also retires 1.13.3's References repair pass
  entirely, along with its watermark table: old mail never threads, so it never
  needs its headers backfilled. A conversation is additionally capped at 50
  members.
- **Conversation re-renders are coalesced.** A conversation renders as one
  document holding every member's body, and each arriving body triggered one.
  Bodies arrive in bursts — the background prefetch alone pushes fifty per
  folder synced — so an N-message conversation meant N loads of an N-body
  document, faster than WebKit could retire them. Arrivals now mark the reader
  dirty and one render runs on a short timer.
- **A body can no longer be applied to the wrong message.** `WorkerEvent::Body`
  now carries the folder it was read from. A UID is unique only within its
  folder, so matching on the number alone let a prefetched body from one folder
  overwrite a different message that happened to share it.
- **Fixed: dragging several messages moved only one** ([#23](https://github.com/hyprlab/vireo/issues/23)).
  The drag payload was built from the dragged row alone, and the drop was
  discarded outright when that row's account differed from the target folder's —
  which is why dragging from the unified inbox often did nothing at all. The
  list now publishes its rows' (account, folder, uid, id) keys, a drag maps the
  selected indices through them and carries every selected message, and the app
  groups them by source folder into one server-side `MoveMessages` per group.
  Messages from another account stay put and say so.
- **Fixed: "All Inboxes" could omit an account entirely.** It cleared every
  account's slice on entry and rebuilt purely from what each worker sent back,
  so an account busy backfilling, reconnecting, or offline was simply missing —
  while its own Inbox, seeded from cache, still listed its mail. It now keeps
  each account's last known inbox, tops it up from the folder caches the way
  opening a single folder does, and replaces a slice only when that account's
  load lands.
- **Preferences: the sender fields sit with their lists.** "Always allow sender"
  was in the Privacy group, nowhere near the list it fed; it is now the first
  row of Allowed Senders. It and the Blacklist row both add with a + button
  rather than a checkmark, so the two lists read and behave alike.
- **A quieter blocked-content banner, and a switch to silence it.** The warning
  can now be drawn in grey with outlined amber buttons instead of a full amber
  bar, and can be hidden altogether. Hiding it gates the notice only — remote
  content is still blocked exactly as before.

## 1.13.2 — 2026-08-23

- **Fixed: messages could not be deleted from Trash.** Every delete route ended
  at `move_to(m, FolderKind::Trash)`, and the move is a no-op when the source
  and the destination are the same folder — so in Trash the request was dropped
  before it reached the worker and nothing happened at all, on IMAP and Gmail
  alike ([#20](https://github.com/hyprlab/vireo/issues/20)). Deleting something
  already in Trash now erases it: `UID STORE +FLAGS (\Deleted)` followed by
  `UID EXPUNGE` (RFC 4315), so only the messages you picked go and anything
  another client flagged in the meantime is left alone; servers without UIDPLUS
  answer `BAD` while the response stream is drained, which is caught and
  retried as a plain `EXPUNGE`. Because there is no undo it always asks first.
  A mixed selection splits — whatever is still outside Trash moves there as
  before, and only the messages already in Trash prompt. The purge is grouped
  by folder into one request each, drops the messages from the local cache so
  they don't reappear on the next load, and reuses the bulk spinner for large
  selections. All three entry points go through it: the reader and popped-out
  windows, the row palette, and multi-select.

## 1.13.1 — 2026-08-23

- **New: an option to always load remote content.** Preferences → Privacy →
  *Always load remote content*, off unless you turn it on. Blocking by default
  stays, as do the two existing ways out — load once, or trust this sender —
  but people who would rather not be asked no longer have to be. It hooks
  `remote_allowed`, the single point every render path already funnels through,
  so the reader, conversations, popped-out windows and printing all follow the
  setting without a second gate to keep in sync; since 1.13.0 that same flag
  decides what is stripped and what the content policy permits, so turning it on
  relaxes both together. Toggling it re-renders whatever is open — a
  conversation as a conversation, not collapsed to its first message.
  Contributed by [Isaac](https://github.com/thecalamityjoe87) in
  [#31](https://github.com/hyprlab/vireo/pull/31).

## 1.13.0 — 2026-08-23 — security

Reported privately by [Alexander Lubovenko](https://github.com/typedev), who
reviewed the whole tree and wrote it up properly. Thank you.

- **Fixed: a sender could run JavaScript in the reader.** The `From:` display
  name was escaped for an *attribute* value — `&` and `"` — but it is rendered
  as element text, where `<` and `>` are structural. The message bodies sit in
  sandboxed frames that cannot run scripts, but the headers are drawn in the
  wrapper document around them, which does run one script of its own and can
  read every frame in the thread. So a display name of `<script>…</script>`,
  delivered as an RFC 2047 encoded-word, executed as soon as a conversation was
  opened: no click, nothing visible, and from there every message in that thread
  could be read and sent anywhere. Header text is now escaped as text.
- **The wrapper document also has a Content-Security-Policy now**, so the script
  that sizes the message frames is the only script that can run in it — it
  carries a per-render nonce, and nothing else in that document does. Escaping is
  the fix; this is what stands behind it if the escaping is ever wrong again.
  Confirmed against the engine rather than assumed: with the escaping bug put
  back deliberately, the injected `<script>` is parsed into the page and WebKit
  still refuses to run it.
- **Fixed: remote content could load while the UI said nothing was blocked.**
  Whether a message referenced remote resources was decided by searching for
  fixed strings like `src="http`, so `src="//host/p.gif"` (a protocol-relative
  URL, which resolves perfectly well), `src = "http://…"` with spaces around the
  `=`, `<video poster>`, SVG `<image href>` and `@import "//…"` all went unseen.
  Worse, that same guess also chose the content policy — so a miss switched off
  the stripping, relaxed the policy *and* hid the banner, all at once, and a
  tracking pixel loaded silently.
- **Blocking now follows your setting, not that guess.** The detector decides
  only whether the "Remote content was blocked" banner appears; what is stripped
  and what the policy permits come from your own choice. A detector miss now
  costs a banner rather than the blocking. The detector itself was rewritten to
  walk the markup rather than search it, so the cases above are caught — and so
  detection and stripping can no longer disagree, which they previously did.
- **Links only open if they are `http`, `https` or `mailto`.** An HTML message
  keeps its own `href` values, so it could name `file://`, `smb://`, or any
  scheme some installed application had registered, and one click handed it over.
- **Dropped `--talk-name=org.freedesktop.Flatpak` from the Flatpak manifest.** It
  permits `flatpak-spawn --host` — arbitrary commands outside the sandbox — which
  made the rest of the sandboxing advisory. It was only a fallback for the "Open
  GNOME Contacts" button, which reaches the host app by D-Bus activation anyway.
- **The mail cache is no longer world-readable.** `cache.db` holds message
  bodies, attachment bytes and the harvested address book, and it was `0644`
  inside a `0755` directory while `accounts.toml` — which by design holds no
  passwords at all — was correctly `0600`. The directory is now `0700` and the
  database and its sidecars `0600`, existing caches included. The fallback that
  put the cache in a shared temp directory when there was no data directory is
  gone; there is no acceptable location in that case.
- **Attachments you open are cleaned up, and are no longer readable by other
  users.** They were written `0644` into a predictable `/tmp/vireo-attachments`
  and left there indefinitely. The directory is now created `0700` and checked to
  be ours, each file is created fresh with `O_EXCL`/`O_NOFOLLOW` at `0600` rather
  than written through whatever sits at a guessable name, and the whole directory
  is cleared at startup. Under Flatpak `/tmp` is per-app, so only the
  accumulation applied there; the RPM and Arch packages had all of it.
- **OAuth now uses PKCE `S256` instead of `plain`.** With `plain` the challenge
  *is* the verifier, so anyone able to read the authorization URL — browser
  history, an extension, another local process — could redeem a stolen code,
  which is the one thing PKCE exists to prevent.
- **A failure to read randomness is now an error instead of a silent constant.**
  If `/dev/urandom` could not be read the buffer stayed all zeroes and the
  function returned the same string every time — as both the anti-CSRF `state`
  and the PKCE verifier, with nothing logged. Sign-in now fails and says so.
- **`accounts.toml`, `privacy.toml` and `sidebar.toml` are created `0600`**
  rather than written with the umask's permissions and tightened a moment later.
- **New: notifications can leave out the sender and subject.** GNOME draws
  notifications on the lock screen, and the only control was on/off.
  **Preferences → Mail → Show sender and subject.** On by default.
- Added a `SECURITY.md` with a documented private reporting channel.

## 1.12.7 — 2026-08-23
- **New: sender logos** (#30, asked for by [@doodoobug-dot](https://github.com/doodoobug-dot)). The sender circle can carry the brand's own icon instead of coloured initials, so mail from Capital One, US Bank, GitHub or Amazon is recognisable at a glance. **Preferences → Privacy → Show sender logos.**
- No third-party service and no bundled logo database: the icon comes from the sender's own domain, best first — `apple-touch-icon.png` (180px, what a site publishes when it cares how it looks as an icon), then `favicon.ico`, each tried bare and at `www.`. The domain is the registrable one, so `usbank@notifications.usbank.com` asks `usbank.com`, with three labels kept for country-code pairs like `bbc.co.uk`.
- **Off by default**, because the request tells that domain your IP address — which is what blocking remote content otherwise avoids. The switch says so rather than burying it.
- A Gravatar still wins where one exists: it belongs to the person, not their employer. Each domain is asked at most once per session and misses are remembered, so a sender with no icon costs one request rather than one per row. `.ico` files decode through GdkPixbuf, since `GdkTexture` reads only PNG and JPEG.
- Responses are checked by content type before being read: some sites answer *every* icon path with their home page under a 200 — `pm.me` sends 347KB of HTML for `/favicon.ico` — and downloading that to discover it isn't an icon is a third of a megabyte wasted per attempt. Senders with no icon anywhere keep their initials.

## 1.12.6 — 2026-08-23
- **Fixed: dates and times ignored the system format** (#32, reported by [@edisso999](https://github.com/edisso999)). Every date went through chrono with a hard-coded pattern, and chrono has no notion of a locale, so a machine set to German still showed `Aug 23, 2026 at 5:40 AM`. Formatting now goes through GLib, which reads `LC_TIME`, and learns three things from the locale itself: its **field order** (by asking it to write 25 December 2026 and seeing which number comes first — day-first, month-first, or year-first as Japanese and Swedish are), its **clock** (by asking it to write one in the afternoon: a result containing "13" means 24-hour), and the **separator** a day-first locale uses, since German writes "23. Aug" where British and French write "23 Aug". German now reads `23. Aug 2026 at 05:40`; American English is unchanged.
- Vireo keeps its own arrangement rather than adopting the locale's written date (`%x`, which is all digits): the complaint was that the order and the clock were American, not that the month should stop being spelled, and a spelled month is quicker to place when scanning a list.
- **New: the format can be set independently of the system**, in **Preferences → Date and Time**. *Date format* offers Follow system, `Aug 23, 2026`, `23 Aug 2026` and `2026 Aug 23`; *Clock* offers Follow system, 12-hour and 24-hour. Both follow the system by default. Changing either rebuilds the list and re-renders the open message, so it takes effect at once — and it reaches everywhere a date appears, including the printed page.

## 1.12.5 — 2026-08-23
- **Fixed: some message previews showed MIME machinery instead of the message.** Previews fetch `BODY.PEEK[1]` — MIME section 1 — on the assumption that it holds the text. When the first part is itself a multipart, as in the `mixed(alternative(text, html), attachment)` layout ProtonMail sends, section 1 *is* the nested container, and the preview read `--b2=_cipkIEq1…`: its boundary. The preview builder now descends into a multipart, prefers `text/plain` over `text/html` as the reader does, and decodes each part by the `Content-Transfer-Encoding` it declares rather than guessing base64 from the bytes. A message that merely opens with `--`, as a signature does, is still read as text.
- **Fixed: previews of marketing mail showed a tracking URL instead of the greeting.** A plain-text alternative generated from HTML renders links as `text ( url )`, so a message whose first element is a linked logo begins with a bare URL — which was the whole preview. Rendered links are now dropped, keeping the words around them, along with any URL-only lines left at the top. Brackets that are not links are untouched, and a message that is only a link still shows it rather than nothing.
- **Fixed: the background backfill erased previews.** It re-fetches summaries without asking for a body slice, and the cache wrote rows with `INSERT OR REPLACE`, so every message a backfill pass touched lost its preview. Writes are now an upsert that will not let an empty preview overwrite a stored one.
- Previews already cached from the two bugs above are cleared when the cache opens; those rows show nothing until their folder syncs again, which beats showing a boundary or a tracking link.

## 1.12.4 — 2026-08-23
- **The message list can be dragged much narrower.** With the sender circles off (1.12.3) the list still refused to go below 340px, and the messages were never the reason. Three things above them were holding the pane open: the bulk-action bar, whose `SlideDown` revealer reserves its child's full **width** even while collapsed, so seven buttons set the pane's minimum whether or not anything was selected; the search scope drop-down, sized to its widest entry ("All folders"); and the folder title and "N selected" label, neither of which could ellipsize. The bar now sits in a scroller with no minimum of its own, the drop-down's button label ellipsizes (the list still spells both choices out), and both labels give way. The pane's floor drops from **340px to 171px**.
- With the circles off the rows themselves now reach **90px**: the Actions Palette line stops reserving its 260px — turning the circles off is a request for a narrow list, and that reservation was the only thing in the way — and the date, the one item on the sender line with no give, ellipsizes as well. Below the palette's own width it is clipped by the row rather than pushing the pane wider, and the row's content is clipped so nothing paints across the divider into the reader. Circles on is unchanged: floor still 350px, palette space still reserved.
- The list still **opens** at 350px rather than at its minimum, which is a fine width to be able to drag down to and a poor one to be handed on startup.
- **Fixed: squeezing the window pushed the reader's toolbar icons off the right-hand edge.** The pane was allowed to be allocated below its own minimum, so its header kept being given less room than its buttons needed. The reader now has a floor of 535px. The window's own minimum becomes 848px as a result — still half of a 1920px display, but wider than before.

## 1.12.3 — 2026-08-22
- **New: Sender circles can be turned off** (#29, reported by [@taprobane99](https://github.com/taprobane99)). The coloured circle of initials beside every message costs horizontal room that a small screen would rather give to the sender and subject. **Preferences → Message List → Sender circles** hides it in the message list, above the open message, and in popped-out message windows — the reader too, since a setting that applied to one and not the other would just look like a bug. On by default; the rows are rebuilt as the switch moves, so it takes effect at once without losing your place in the list.
- The circle is hidden rather than faded, so the row gives up its slot and the width is actually reclaimed. With the circle gone the unread dot leads the row, so it gets an equal gap on each side instead of keeping the wider inset that had been holding the circle clear of the list's edge.
- **Gravatar fetching stops while the circles are hidden.** It is wasted work, and it would send a hash of each sender's address to a third party to fill in a circle that is never drawn.

## 1.12.2 — 2026-08-22
- **Fixed: deleting a message could throw the selection to the top of the folder** (#19). Two things decide what is selected after a delete: the list picks the row that slides into the deleted one's place, and then the folder sync that follows checks whether the reader's message is still there. That second check advanced to whatever now occupies the message's old slot, found by looking it up in the cached copy of the folder — but deleting prunes the cache first, so the lookup failed on exactly that path, and the fallback was `messages.first()`: the top of the folder. The selection went there and the list scrolled with it. There is no sensible message to advance to when the old slot is unknown, so the reader now clears instead.
- **Focus follows the row the list advances to.** Removing the focused row left GTK to choose its own replacement, and moving focus drags the viewport with it. Taking focus deliberately also means the single-key shortcuts carry on from the row that is now selected rather than from wherever focus landed.
- The message selected after a delete is still the one **below** — the direction Thunderbird and Apple Mail take, so deleting a run of mail keeps moving one way.

## 1.12.1 — 2026-08-22
- **Fixed: printed pages and saved PDFs carried scrollbars, and long messages were cut off** (#16). The reader wraps each message in a sandboxed iframe — right on screen, since an email's CSS cannot escape it — but a print engine does not paginate what is inside a frame: it draws the frame as a box at its on-screen size, scrollbars and all, and clips the rest. So a message came out with a grey bar down the right edge, another under the text, and everything past the first screenful missing.
- **Printing now builds a document of its own, with no frames**: the header, then every message inlined into the page, so it flows across as many pages as it needs. Long URLs wrap instead of running off the sheet, and wide content — images, tables, `pre` blocks — is scaled to the page rather than clipped. **Ctrl+P** renders that same document offscreen and prints it, so the print dialog and the preview can no longer disagree.
- A printed **conversation** now names each message: inlining removed the per-message frame headers, so every message in a thread prints its sender and date above it.
- Inlining also gives up the iframe's CSS isolation, so each message's own `html`/`body` rules are redirected to the block that message prints in — otherwise one sender's `body{font-family:monospace}` would restyle the header and the rest of the thread. Bare type selectors (`p`, `a`) can still reach across, which is the remaining price of printing a thread as one page.

## 1.12.0 — 2026-08-22
- **New: printing** (#16). There was no way to print a message at all. The reader is a WebKit view, so it prints what it is already showing — the message as rendered, quoting, inline images and current theme — through the system print dialog, which inside Flatpak is the portal's, so printers configured for the desktop work with no extra permissions. **Ctrl+P**, **Main Menu → Print Message…**, or the printer button in the reader toolbar; **Ctrl+P** also works in a popped-out message window.
- **The printed page carries the message's identity, not just its body.** WebKit prints the document it is showing, and everything naming the mail — sender, recipients, date — lives in the GTK pane above it, which cannot be printed. Those facts now go into the document and are hidden with `@media`, so the screen is unchanged and paper gains a header: subject, From, To, Cc and the full date. Empty fields are omitted, and the subject is escaped — it is text, not markup. Printing is forced light in the wrapper and inside each message frame; reading in dark mode would otherwise put white text on a black page.
- **New: a print preview inside Vireo** — the toolbar's printer button, **Ctrl+Shift+P**, or **Main Menu → Print Preview…**. It shows the message with its print styling on a page-shaped sheet, with **Print…** and **Save as PDF…** in its header bar. The preview renders the same `document_html` the reader builds and prints the very view on screen, so it cannot drift from what comes out.
- The preview is Vireo's own window rather than an exported PDF opened elsewhere. That route — a temporary file, a URI, the document portal, and whatever the desktop registers for `application/pdf` — has four links that can each fail without saying anything, and two of them duly did: the file printer's name is translated (so asking for "Print to File" fails), and a URI built with `format!` breaks on the spaces and brackets that mail subjects put in filenames. **Save as PDF…** keeps the two lessons: ask GTK for a virtual printer that accepts PDF, and take the URI from GIO.
- Printing uses **GtkPrintDialog**, not WebKit's `run_dialog`. The latter spins a nested main loop, and polling a glib future inside one aborts the process outright — which it did, the first time this shipped as a test build. This raises the gtk4 requirement to **4.14** for building from source; the GNOME 50 runtime the Flatpak builds against has 4.20.
- The ARM64 build was again compiled with `rust-nightly`: Flathub is still serving a 404 for an object of `org.freedesktop.Sdk.Extension.rust-stable/aarch64/25.08`.

## 1.11.0 — 2026-08-22
- **New: Vireo can keep running when its window is closed** (#3). The request was for a system tray; GNOME has no tray, and its equivalent is the **Background Apps** section of Quick Settings, which xdg-desktop-portal populates from sandboxed apps running without a window. Staying alive after the last window closes is therefore all it takes to appear there — no icon to draw, no shell extension. **Off by default**, since closing a window is expected to quit: turn on *Keep running in the background* in Preferences → Mail, after which closing hides the window and mail keeps arriving and notifying.
- The portal permission is requested at the moment the switch is moved, so GNOME's "Allow Vireo to run in the background?" dialog appears while the user is looking at the setting rather than at some later unexplained point. `SetStatus` puts the unread count beside the entry ("3 unread messages", or "Checking for new mail"), so a process with no window says what it is there for.
- **New: Start at login**, a second switch enabled only when background running is. The portal's autostart entry runs `vireo --hidden`, a new flag that starts the app without presenting its window — it builds as usual, syncs, notifies, and waits in the system menu. The flag is stripped before the arguments reach GTK, which would otherwise reject it as unknown, and the activation handler that re-presents a hidden window skips exactly one activation on such a run: that first activation *is* the launch, and presenting there would undo the point of it.
- **New: a `quit` action on the application** (main menu, and Ctrl+Q). GNOME's Background Apps menu quits an app by activating this over D-Bus and only resorts to `flatpak kill` if nothing answers within five seconds, so without it the ✕ there would be a hard kill. Activating Vireo — its icon, a notification, the autostart entry — brings a hidden window back rather than doing nothing.
- Hiding rather than closing also sidesteps the reason that handler exits outright: nothing is torn down, so there is nothing to abort in GTK, WebKit or the per-account worker threads.
- The ARM64 build was again compiled with `rust-nightly`; Flathub is still 404ing an object of `org.freedesktop.Sdk.Extension.rust-stable/aarch64/25.08`.

## 1.10.3 — 2026-08-22
- **Fixed: an IMAP/SMTP account imported from GNOME Online Accounts could fail to authenticate** (#17), while a Gmail one — which uses a token rather than a password — was fine. The password was read exactly once, during import, and never again, so an import that came back empty left an account permanently unable to log in. Vireo also never asked GOA to `EnsureCredentials` first, which is how GOA is told to unlock the keyring or refresh a credential before handing it over; Geary does, which fits the report that Geary worked on the same account.
- `EnsureCredentials` now runs before the read, and a GOA account with no stored password asks GOA again when its worker connects, storing what comes back so a later run works even if GOA is slow to start. That repairs accounts already imported in the broken state — which matters more since 1.10.2, where the password field is greyed out for GOA accounts and typing it in by hand is no longer possible.
- The credential id is no longer assumed. GOA's mail provider files these under `imap-password` and `smtp-password`, other providers use a plain `password`, and some builds return the account's secret whatever id they are given; all three are tried, and a separate SMTP password falls back to the incoming one rather than sending none. If GOA still has nothing, the error says so and points at Settings → Online Accounts instead of surfacing a bare authentication failure.
- The ARM64 build was again compiled with `rust-nightly` — Flathub is still serving a 404 for an object of `org.freedesktop.Sdk.Extension.rust-stable/aarch64/25.08`.

## 1.10.2 — 2026-08-22
- **Accounts imported from GNOME Online Accounts can no longer be edited in Vireo.** Their address, servers, protocol and credentials come from the system, but the editor let them be typed over — and anything changed that way was either overwritten the next time GOA was read or left quietly disagreeing with what the rest of the desktop uses. Those fields are now insensitive, with an explanation at the top of the editor and a button that opens Settings → Online Accounts, which is where they are actually changed. What Vireo owns stays editable: the sender's display name, signature, colour, emoji and label.
- Saving such an account now restores its connection settings from the stored account rather than reading them back out of the form, so a greyed-out field can't be written back through some other route (an insensitive widget still holds and returns its value). **Test Connection stays available** — it is read-only, and confirming that the imported settings really connect is exactly what someone would want to do on that screen.
- The GOA explanation used to be split in two, with a near-identical paragraph and a second "Open Online Accounts" button in a group at the bottom of the editor. It is said once now, at the top, with the **Enabled in Vireo** switch — which hides an account locally without touching the system account — as the first thing on the page.
- The ARM64 build of this release was again compiled with `rust-nightly`: Flathub is still serving a 404 for an object of `org.freedesktop.Sdk.Extension.rust-stable/aarch64/25.08`. CI picks stable as soon as Flathub can serve it.

## 1.10.1 — 2026-08-22
- **Fixed: a message with an inline attachment showed no paperclip and no attachment** (#9). An Apple Mail PDF marked `Content-Disposition: inline` with a filename and no Content-ID was extracted correctly — that part has worked since 1.7.2 — but nothing ever asked. The paperclip is guessed before any body is fetched, from BODYSTRUCTURE or (on servers whose structure Vireo's IMAP parser rejects, notably iCloud) from the top-level `Content-Type`, and attachments are only downloaded for messages the guess flagged. A message the guess missed could never correct itself: no flag, no fetch, no attachments, indefinitely.
- `load_body` already holds the whole message for the body and the sender check, so it now also reports whether attachments are genuinely present, and the worker emits `HasAttachments` — the mirror of the existing `NoAttachments`, which has been clearing *false* paperclips from the same evidence all along. Nothing extra is fetched. The corrected flag is written to the cache so it survives a restart instead of being re-guessed, and if the message is the one on screen its files are fetched too, since the reader only requests attachments when the flag was already set. Background body prefetch runs the same check, so for recent mail the paperclip is right before the message is ever opened.
- The guess itself is deliberately unchanged. Treating a top-level `multipart/alternative` as attachment-bearing would have caught the nested Apple Mail shape at the cost of a false paperclip on nearly every HTML newsletter — the noise removed in 1.4.1. Evidence from the body is the honest answer, and small inline `cid:` decoration still earns no paperclip.
- The ARM64 build of this release was compiled with `rust-nightly`, for the reason given under 1.10.0: Flathub is still serving a 404 for an object of `org.freedesktop.Sdk.Extension.rust-stable/aarch64/25.08`. The CI job picks stable whenever Flathub can serve it.

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
- **The ARM64 build of this release was compiled with `rust-nightly` rather than `rust-stable`.** Flathub's copy of `org.freedesktop.Sdk.Extension.rust-stable/aarch64/25.08` was serving 404s for one of its objects for hours — the ref was listed but not downloadable, from CI and from a workstation alike — while the nightly extension was intact. Rather than hold the ARM release behind someone else's outage, the build fell back to nightly; the x86_64 build is stable-compiled as always. The CI job now picks stable when it can and nightly only when it must, so this reverts by itself.

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
