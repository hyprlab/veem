# Vireo

Vireo is a clean, fast, GNOME-native email client built with Rust and libadwaita for Wayland desktops. Privacy-first: no telemetry, remote content blocked by default, and credentials kept in the system keyring.

## What's new in 1.18.2

Built with the community: Isaac (@thecalamityjoe87) drove the sidebar work in PRs #89 and #95, and @taprobane99 requested the header seal (#88).

**Trust at a glance.** The sender-authentication seal now sits right beside the sender's name in the message header — blue when DKIM, SPF and DMARC check out, amber when something's off, red when authentication fails. Click it for the full verdict.

**Your sidebar, your chevrons.** A new Chevron placement setting offers the classic trailing chevrons or a leading layout where every disclosure chevron, icon and unread chip lines up in clean columns — designed with Isaac across two PRs. Avatars slim down a touch, icons stop drifting off their column, and double-clicking All Inboxes folds its account list.

**Conversations tell you what's new.** A collapsed thread now shows the newest reply's sender and preview instead of repeating the message that started it — and its right-click menu can mark the whole conversation read or unread in one go.

## What's new in 1.18.1

The polish release 1.18.0's feedback asked for — thank you @thecalamityjoe87, @yioannides, @frenchy82, @p-mitana, @taprobane99 and @tbaumann.

**Vireo is your email client now.** GNOME Settings lists Vireo under Default Applications → Mail, and clicking a mailto: link anywhere opens a composer prefilled with the address, subject and body. Opening the app a second time presents the existing window instantly.

**Special folders, your way.** When a provider's Sent, Trash, Junk, Drafts or Archive isn't detected (or lands wrong), each account's editor now has a Special Folders section to pin any role to any real folder — deletes, archives and sent mail follow your mapping.

**The list holds its ground.** Deleting mail or closing a compose while scrolled elsewhere no longer yanks the list back to the selection (thanks Isaac for PR #85). Deletion advances in the direction you're triaging, like Apple Mail. Undo puts you on the restored message — reliably, even on iCloud — and spins the refresh indicator while it works.

**A calmer, denser message list.** Row text sits centred in its pill, rows tighten up, and the ⋯ Actions Palette slides out over the row on one seamless card — with Add to Contacts and View Source aboard, one palette open at a time, and the thread rail ending neatly at the last reply's dot. Right-clicking any email address now offers Add to Contacts, and the menu closes like it should.

**Small things that add up.** The reader toolbar folds exactly when your window-button layout needs it to; All Inboxes only shows its total count when folded (and both are now optional); sender logos survive restarts; Settings dropdowns never truncate; previews refill the moment you turn them back on.

## What's new in 1.18.0

The 1.18 feature release, beta-tested by the community (thank you @p-mitana, @thecalamityjoe87, @frenchy82 and @yioannides for the feedback that shaped it).

**Contacts, without leaving your mail.** The sidebar's Contacts row now opens a full view right in the app: search, sort and browse everyone from GNOME Contacts, with a full card for each person — their photo (click to expand; iCloud photos finally render), every email with compose and copy at hand, phone numbers, addresses, websites, birthday, notes, and which account the entry belongs to. Edit contacts, add new ones, and delete them without opening GNOME Contacts — changes sync back through Evolution Data Server, so CardDAV accounts pick them up. Composing from a contact slides the composer down right over the card.

**One Settings window.** Accounts and Settings live together behind a standard GNOME view switcher, with every option regrouped into focused sections and a preference for which view opens first.

**Mail that doesn't make you wait.** Bulk actions apply instantly and finish quietly in the background; the refresh spinner shows something's working and the status bar (burger menu, long-press Refresh, or Ctrl+Shift+S) tells you exactly what. All Inboxes paints from the local cache the instant the app opens. Emptied folders show a proper "No Messages" page.

**Conversations, polished.** Threads slide open and shut in the message list with an animated caret (thanks to Isaac's PR #79); conversations open on their first unread; thread-wide delete, Ctrl+A, and threaded popout windows; optional newest-message-first reading order; and an optional card style for single messages.

**Composer & reader.** A Reply-To field joins Cc/Bcc behind "More"; forwards finally keep their formatting, safely — bodies pass through a proper HTML sanitizer so a forwarded invoice still looks like the invoice, with nothing dangerous along for the ride. An "Always show recipients" option keeps the To line visible under every sender. Desktop notifications clear when you read the mail in the app.

**Sidebar.** Contacts and Attachments pin to the sidebar's bottom edge as one clean section (from Isaac's PR #80), chevrons align, account labels are honoured, and the menu gains sections.

## What's new in 1.17.1

**Microsoft 365 accounts from GNOME Online Accounts work now (issue
#36).** GNOME's Microsoft 365 sign-in only carries permission for
Microsoft's Graph API — it cannot log in to IMAP at all, which is why
imports produced an account that failed with "No address associated
with hostname". Vireo now speaks the Graph API directly for these
accounts, using GNOME's own sign-in: folders, reading, attachments,
moves, undo, drafts, search, and sending all work, and new mail is
polled every couple of minutes (or at your auto-fetch interval).
Existing broken imports repair themselves on upgrade.

Along with that, Google and Microsoft sign-in now belong to GNOME
Online Accounts outright — the old built-in Microsoft sign-in is
retired — and GOA accounts became first-class citizens in the Accounts
window: switching one off returns it to the import list (it stays in
GNOME), the Remove button works on them (removing from Vireo only), the
irrelevant greyed-out server fields are gone from their editor, and
editing the things Vireo does own — label, signature, colour, aliases —
saves properly.

The chrome got calmer too:

- **The sidebar carries the actions now.** Refresh sits beside a new
  **+ New Message** pill; **Contacts** gets its own row below
  Attachments (with a Preferences toggle); the status-bar button is
  retired — errors reveal the bar themselves, and "Reveal Status Bar"
  in the menu covers the rest.
- **The message list gained a row of space**: the folder-name line is
  gone, and the message count and sort menu moved up into the pane's
  header bar.

## What's new in 1.17.0

A big release: composing without leaving the window, undo for every move,
and send-as aliases with their own SMTP servers.

- **Compose right where you read.** New message now slides down over the
  reading pane — toolbar and all — as a full-height editor, with a
  pop-out button when you'd rather have the old separate window (and a
  preference to keep composing in a window permanently). The sidebar
  gains a proper accent-coloured **New message** row to start from.
- **Ctrl+Z undoes it.** Deleted a message by accident? Moved a thread to
  the wrong folder? Marked something spam too fast? Every move — single,
  bulk, drag-and-drop, or spam — can be undone, without limit, straight
  from the keyboard. Messages return to the folder they came from even
  though the server renumbers them in transit. **Ctrl+W** now closes the
  window (mail keeps syncing in the background) and both shortcuts are in
  the keyboard shortcuts window. (Issue #64.)
- **Aliases with their own SMTP server (issue #34).** A send-as alias can
  now carry its own outgoing server — host, port, username, and a
  password in the system keyring — with a Test button in the alias
  editor. Mail sent (or retried from the Outbox) as that alias goes
  through its server; everything else uses the account's, as before.
- **Reading feels like one conversation.** Every message — threaded or
  not — uses the conversation layout: single messages fill the pane
  edge-to-edge as one continuous surface (plain-text mail included),
  threads keep their inset cards. Opening a thread with unread mail
  scrolls straight to the last unread message — riding out image loads
  and quote collapses on the way — and messages are only marked read when
  you click them, not just for scrolling past.
- **Message actions live on the message.** Each card has its own action
  row — reply, forward, move, spam, read/unread, view source and more —
  tucked behind a ⋯ that appears on hover, always shown, or shown
  automatically while hovering: your choice in Preferences, sharing one
  auto-collapse timeout with the message list's palette (which can now
  also open on hover). Mark as Read/Unread joins the reader toolbar too.
- **A bolder, calmer message list.** Selection is now a full accent-colour
  pill with white text in both schemes, the unread dot alone marks unread
  mail (white on a selected row), thread count and caret merge into one
  quiet grey chip that inverts when selected, threads date themselves by
  their newest message, and the separators are gone. The list can also
  shrink further for narrow screens.
- **Details throughout.** The remote-content banner follows light/dark
  properly, reads better, and sits at a steadier 48px; the attachment
  drawer loses its ugly grab handle — resize it after expanding, and the
  height sticks across restarts — with Save All always visible;
  Preferences, Accounts and About remember their window heights; startup
  highlights All Inboxes in the sidebar; and opening several PDF
  attachments at once can no longer crash the app.

## What's new in 1.16.2

- Cancelling the "Open With" dialog for an attachment no longer makes it
  instantly reappear. (Reported by p-mitana, issue #65.)
- The message list's highlights are now rounded inset pills — matching the
  conversation cards — and the row separators float inset from the edges
  instead of running wall to wall.

## What's new in 1.16.1

Folder management, rounded out:

- **Rename folders** from the right-click menu — sub-folders keep their
  place under the new name.
- **Click a folder to expand it**: single-clicking any folder with
  sub-folders toggles them open or closed, no need to aim for the caret.
- **New folders appear instantly**, and moving folders no longer clears
  the unread count chips (seen on Gmail).

## What's new in 1.16.0

A big one: the folder tree, dark mode that always reads, and a sidebar
that floats.

- **Your folders are a real tree now.** Nested IMAP folders show as a
  collapsible hierarchy with smoothly spinning carets and sliding rows,
  full-path tooltips to tell nine "Archive"s apart — and you can **move
  folders by drag-and-drop**: drop one on another to nest it, or on the
  "Folders" header to bring it to the top level. Moves apply instantly and
  sync in the background. (Requested by JeremiahCornelius, issue #51.)
- **Dark mode emails are always readable.** Vireo now adapts every colour a
  message declares — dark text lightens, light backgrounds deepen, mail
  designed dark passes through untouched — so black-on-black text can't
  happen. The reader's own backgrounds follow your GTK theme instead of
  fixed colours, and the verified-sender check uses the theme's proper
  green in both schemes. (Reported by isorropisths, issue #35.)
- **Pick your look.** New Appearance preference: follow the system, or
  force the app light or dark — independent of the message-content theme.
- **The sidebar floats.** In a narrow window (or whenever you keep it
  collapsed), the icon rail can expand over your mail as a floating panel —
  on click, or just by hovering with the new "Expand the sidebar on hover"
  preference — and folds back a moment after the pointer leaves. At full
  width, the arrow pins it open like before.
- **Conversations look like conversations.** Expanded thread messages are
  now inset cards on a dotted rail, the selected message keeps a (dimmed)
  highlight while you read, and right-click menus across the app share one
  GNOME-styled design with icons matching the toolbar.
- **Quieter warnings, tidier chrome.** The blocked-remote-content banner is
  now always the compact grey style with small pill buttons; the reader
  toolbar collapses into a ⋯ menu when space runs short instead of pushing
  the close button off screen; attachments live in the drawer (with Save
  All) instead of a toolbar menu.
- Plus hardening: a flaky server response can no longer blank an account's
  folder list, and a malformed message can no longer take the message list
  down with it.

With thanks to **yioannides** and **p-mitana**, whose detailed GNOME design
feedback in issue #62 shaped the thread cards, the banner, the menus, and
the theme-colour work — and to **Isaac** (thecalamityjoe87) for the shared
context-menu builder (PR #63), **JeremiahCornelius** for the folder-tree
request, and **isorropisths** for the dark-mode report.

## What's new in 1.15.7

- Panning a zoomed attachment preview is smooth now — no more jitter while
  dragging.
- The toolbar attachment menu's Open button opens files properly, matching
  the drawer and the gallery.

## In 1.15.6

- **Opening attachments from the Flatpak truly works now.** Three deep
  sandbox bugs — where the file was staged, how the system portal was
  spoken to, and how the request was encoded — each silently broke the
  chain. All three are fixed and verified live: your PDF viewer opens
  directly, or via one "Open With" confirmation that's remembered.
- **The attachment preview is part of the window now.** No more separate
  preview window with double chrome: the lightbox fills Vireo itself,
  just like the attachments gallery's.
- **Click a previewed document to zoom 3× right where you clicked**, drag
  to look around, click or press Escape to fit again. Arrow keys still
  step between attachments.

## In 1.15.5

- **Opening attachments now works even where the desktop's portal is
  misbehaving.** If the system fails to launch your default app directly,
  Vireo brings up GNOME's "Open With" chooser instead — pick your viewer,
  tick "always", and future opens go straight through. Only when nothing at
  all can launch does Vireo show a dialog, and it now tells you exactly what
  the system reported and what to do about it.
- **Double-click a lightbox preview** to open the document in its app —
  from the attachments gallery and the message drawer alike.

## In 1.15.4

- **Opening attachments works in the Flatpak.** The sandboxed build was
  handing the system's app portal a file path only Vireo itself could see, so
  "open in the default app" launched a viewer pointed at nothing. The file
  now travels to the portal as a proper document handle, and your PDF viewer,
  image viewer or editor receives something it can actually read.
- If your desktop's portal genuinely can't launch applications, Vireo now
  says so in a dialog — and Download always works — instead of a click that
  does nothing.

## In 1.15.3

- **Send-as aliases.** If other mailboxes forward into this one, you can now
  answer as them: give an account extra From addresses in the account editor,
  pick them from the composer's From menu — and replies to mail sent to an
  alias choose that alias by themselves. Mail still travels through the
  account's own server. (Suggested by
  [somepaulo](https://github.com/somepaulo) — thank you!)
- **Attachments open the way you'd expect.** In the message drawer, a double
  click is now the gesture: photos and PDFs preview in the lightbox (with an
  Open button for the full app), everything else opens in its app directly.
  Single clicks no longer steal the double click, which could leave opening
  seemingly broken. The toolbar's attachment menu works the same way.
- The conversation chevron pill loses the two faint dots at its rounded ends.

## In 1.15.2

A community-feedback release — most of it answers a thorough issue series from
[p-mitana](https://github.com/p-mitana) (thank you!):

- **Conversations read at a glance.** Every card in a thread now leads with a
  tinted initials avatar, so senders — and your own replies — stand apart.
  Card headers stay pinned while you scroll a long message, hovering a card
  highlights its header, and text keeps to a comfortable reading width.
- **Inline forwarding actually works.** The inline composer now shows its
  address fields — To is right there, with Cc, Bcc and Subject a click away —
  no more popping out to a window just to type a recipient.
- **Sent folders say who you wrote to**, not your own name on every row, and
  the little circle shows the recipient too.
- **The message list remembers its width** across restarts.
- **Attachments polish:** PDFs and photos preview in the message drawer's
  lightbox on a single click, a double click opens anything in its app (and
  that works now on systems where the desktop portal quietly did nothing),
  and "Go to Message" lands you properly in the message's folder.
- Clearer icons: View Source shows code brackets, sender verification a
  checkmark seal, and the status bar its bell. The conversation chevron is a
  neat accent pill, and expanded conversations no longer clip their replies.

## In 1.15.1

Attachment polish, following 1.15.0 on the same day:

- **No more clicking to load attachments.** Opening a message or a
  conversation fetches its attachments by itself — the toolbar shows a brief
  spinner, then the paperclip. The separate "load attachments" button is
  gone.
- **PDFs open right in the gallery's lightbox**, as a sharp full-size render
  of their first page — no external app needed for a quick look, and
  arrowing through images and PDFs alike stays in-app.
- **The gallery's table view straightened up**: column headers now line up
  with their columns, and every row carries Download, Open and Go-to-Message
  buttons on the right, just like the thumbnails.

## In 1.15.0

An attachments release, with three contributions by
[Isaac](https://github.com/thecalamityjoe87) (thank you!) adapted from his pull
requests:

- **Opening attachments works again.** A single wrong flag constant meant no
  attachment could even be staged for opening — and quietly disabled a
  symlink protection at the same time. Both fixed; opening now also goes
  through GNOME's portal, so a file type with no default app gets the app
  chooser instead of a click that does nothing.
- **PDFs show their first page.** In the attachments gallery and the drawer
  beneath a message, a PDF now appears as its actual first page instead of a
  generic icon. Every thumbnail renders in the background behind a brief
  spinner — the window never freezes, however many attachments you have —
  and finished renders are reused for the rest of the session.
- **The attachments gallery grew a control footer.** Switch between the
  thumbnail grid and a sortable table (click a column header to sort, again
  to flip), filter by type, resize thumbnails with a slider, and see how many
  attachments match. Your view, size and sort choices are remembered.
- **Conversations show their attachments immediately.** Opening a thread now
  fills the attachment drawer with everything attached anywhere in the
  conversation; before, it stayed empty until you clicked around.
- **The reader says who a message went to.** A "To:" line joins the Cc line
  for single messages — handy for Sent mail and auto-forwarded addresses.
- For release-page readers: each GitHub release now carries only its own
  notes (thanks @yioannides for the nudge, #46).

## In 1.14.4

**Your contacts' photos, beside their mail.** When a sender has a photo in
GNOME Contacts, that photo now fills the circle next to their messages — in the
list and in the reader. Contributed by
[Anton Palgunov](https://github.com/Toxblh) (thank you!) and adapted from his
pull request:

- **Personal first.** The circle shows the contact's own photo when there is
  one; only then Gravatar (if you've turned it on), then the sender's site
  icon (likewise), then the familiar coloured initials.
- **Private by design.** Photos are read from your address book's local cache —
  no network request, and nothing a vCard says can make Vireo read files
  outside it or fetch a remote picture. When Gravatar is consulted at all, it
  now receives a stronger SHA-256 hash, and never before your local contacts
  have had the chance to answer first.
- **Always current.** Add or change a photo in GNOME Contacts and your mail
  picks it up within moments, without the list jumping.
- Turning Gravatar or sender logos off now hides their images immediately —
  including ones already on screen.

## In 1.14.3

Better GNOME Online Accounts integration, contributed by
[Anton Palgunov](https://github.com/Toxblh) (thank you!) and adapted from his
pull request:

- **Custom server ports work.** An Online Account whose mail server lives on a
  non-standard port (IPv6 addresses included) now connects to that port instead
  of the default one.
- **The Mail switch in GNOME Settings pauses instead of forgetting.** Turn an
  account's Mail service off and Vireo sets the account aside — turn it back on
  and it returns exactly as you left it: label, colour, signature and sidebar
  place intact.
- **Vireo reacts to Online Accounts changes instantly and smoothly**, without
  the brief stutters a Settings edit could cause before.
- **Connection tests are honest for Gmail and Microsoft accounts.** The SMTP
  check now authenticates the way sending actually does, so it no longer
  reports a failure for accounts that send fine.
- **No more endless "connecting…".** A mail server that accepts the connection
  and then stalls is given 30 seconds before Vireo reports the problem.

## In 1.14.2

- **The window tiles to half a screen.** Snapping to the left or right edge
  (or Super+←/→) works now: the window's minimum width no longer exceeds half
  a 1920px display. When the window narrows, the sidebar collapses to its icon
  rail on its own so the message list keeps its full Actions Palette — and
  expanding from the rail floats the sidebar over the panes instead of pushing
  them. Your own sidebar preference returns when there's room.
- **Unread counts on every folder**, and sub-folders sorted and indented the
  way the server structures them.
- **Delete acts on your whole selection.** Multi-select messages and the
  toolbar's trash button (or `d`) removes them all.
- **Conversation cards name their recipients.** A compact chip on each card
  expands to the full To/Cc list and folds away again.
- **Attachments as a list.** The attachment drawer can switch from thumbnails
  to an alphabetical list with a sort-direction switch, and hovering any
  thumbnail shows the full filename.
- The notification dropdown is now the **status bar**.

## In 1.14.1

- **Conversations now include the mail you already had.** Threading applied only
  to mail that arrived after you upgraded, so an account you had just added —
  or the same account set up again on another machine — showed no conversations
  at all, even though every reply in it says what it answers. Vireo reads those
  headers now whatever the date, so your existing mail threads on sight. The
  "Thread older messages too" setting is gone with it: there is nothing left for
  it to switch on. Nothing is re-downloaded, and opening a long conversation is
  still bounded.

## In 1.14.0

- **Conversations, properly.** A thread reads oldest first, each message its own
  card you can reply to, forward, select or act on individually. Quoted replies
  fold away behind a ••• so you see what was actually written. Unread messages
  in a thread are marked and clear as you read down. The version that first
  attempted this could exhaust your machine's memory; that is fixed, and covered
  by tests.
- **Threading starts from this release**, with a setting to reach back through
  your whole mailbox if you want it. Nothing is re-downloaded either way.
- **Replies you send stay in their conversation.** Vireo grouped incoming mail
  by headers it never wrote on its own, so replies sent from it arrived as new
  conversations for everyone who received them.
- **Gmail accounts show a message once.** Gmail keeps one message under several
  labels; a six-message thread could appear as eighteen, with attachments that
  looked missing. Thanks to [Alexander Lubovenko](https://github.com/typedev).
- **Opening a thread no longer flashes**, and returning to one is instant.
- **Dragging several messages moves all of them**, and "All Inboxes" no longer
  leaves out an account that is still syncing.
- **A quieter blocked-content warning**, or none at all — your choice; remote
  content stays blocked either way.

## In 1.13.5

- **Replies you send now stay in their conversation.** Vireo grouped incoming
  mail correctly, but the replies it sent carried none of the headers that mark
  them as replies — so they arrived as new conversations for whoever received
  them, and for Vireo itself. Replies sent from here on thread properly, in
  Vireo and in every other mail client. Messages already sent can't be
  retrofitted: the information never left your machine.

## In 1.13.4

- **Conversations are back, and safe.** Vireo groups a message with its replies
  again, and shows the rest of a conversation from your other folders — the
  reply you sent appears inside the thread you're reading. The version that
  first added this could exhaust your machine's memory when you opened a
  message; that is fixed, and covered by tests.
- **Threading starts from this release.** Mail that arrives from now on threads
  normally. Your archive is left alone: grouping years of old mail meant reading
  hundreds of messages to show one conversation, and it is not what threading is
  for. Nothing is re-downloaded and no background repair runs over your mailbox.
- **Dragging several messages now moves all of them** — previously only one
  moved, and dragging from All Inboxes often did nothing at all.
- **All Inboxes no longer drops an account.** An account that was still syncing
  or offline could be missing from the merged view even though its own Inbox
  showed its mail. It now appears immediately from the cache.
- **Preferences: adding senders makes more sense.** The "always allow" field now
  sits at the top of the Allowed Senders list it feeds, and both it and the
  Blacklist add with a + button.
- **The blocked-content warning can be quieter, or silent.** Choose a grey
  banner with outlined buttons instead of the amber bar, or hide it entirely.
  Hiding it changes only the notice — remote content stays blocked.

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
