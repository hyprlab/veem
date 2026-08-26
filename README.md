<p align="center">
  <img src="docs/logo.png" width="120" alt="Vireo logo">
</p>

<h1 align="center">Vireo</h1>

<p align="center">
  A clean, fast, <strong>GNOME-native</strong> email client — built with Rust and libadwaita, privacy-first.
</p>

<p align="center">
  <a href="https://vireo.hyprlab.co">Website</a> ·
  <a href="RELEASE_NOTES.md">Release notes</a> ·
  <a href="https://github.com/hyprlab/vireo/releases">Releases</a>
  <br>
  <img alt="License: AGPL-3.0" src="https://img.shields.io/badge/license-AGPL--3.0-blue">
</p>

<p align="center">
  <img src="docs/screenshot.png" width="900" alt="Vireo showing a unified inbox in light mode">
</p>

---

> [!NOTE]
> **Veem is now Vireo.** As of v1.6.0 the app formerly known as Veem has a new
> name and icon — the old name was too easily confused with similarly named
> products. Same app, same code. Native installs migrate your data
> automatically; Flatpak users should [install Vireo fresh](https://vireo.hyprlab.co)
> and remove the old Veem app. getveem.com now redirects here.

Vireo is a desktop email client for Wayland that feels at home in GNOME. It talks
IMAP/SMTP directly, keeps your mail and credentials on your machine, and blocks
trackers by default — no telemetry, no analytics.

## Features

- **Multiple accounts** — IMAP and POP3, each on its own background worker, with a unified *All Inboxes* view.
- **OAuth 2.0 sign-in** — Google, Microsoft and custom providers over XOAUTH2, plus import from GNOME Online Accounts.
- **Whole-mailbox sync & search** — no message-count cap; a fast first page loads instantly, the rest indexes in the background with infinite scroll.
- **Two-way sync** — deletions and moves from your phone or another client sync back automatically (IMAP IDLE + reconciliation).
- **Conversation threading**, compose/reply/forward with HTML signatures, editable drafts, and full folder management.
- **Outbox** — a send that fails is kept and retried when the connection returns, not lost; queued messages can be edited, sent by hand or discarded.
- **Message previews** — the first one to three lines of each message under its subject in the list (or off).
- **Single-key shortcuts** — Gmail-style `j`/`k`, `r`, `a`, `d` and friends, without a modifier (see below).
- **Printing** — print a message with its sender, recipients and date, with an in-app preview that also saves straight to PDF.
- **Runs in the background** (optional) — closing the window keeps mail arriving; Vireo appears under *Background Apps* in the GNOME system menu, and can start at login without opening a window.
- **Privacy-first reading** — remote content blocked by default, per-sender allow/block lists, and a per-message light/dark content theme.
- **GNOME-native** — adaptive three-pane layout, per-account colours and emoji avatars, light/dark following the system, optional GNOME Contacts.

See **[RELEASE_NOTES.md](RELEASE_NOTES.md)** for the full list.

## Keyboard shortcuts

Vireo can be driven from the keyboard without holding a modifier, in the style of
Gmail and Geary. The scheme is **off by default** — a stray keystroke shouldn't
archive mail — so switch it on first in **Preferences → Message List → Single-key
shortcuts**. Press **Ctrl+?** (or F1, or *Main Menu → Keyboard Shortcuts*) at any
time for this list in the app; the same key closes it again.

| Move around | | Act on a message | |
| --- | --- | --- | --- |
| <kbd>j</kbd> <kbd>↓</kbd> | Next message | <kbd>r</kbd> | Reply |
| <kbd>k</kbd> <kbd>↑</kbd> | Previous message | <kbd>R</kbd> | Reply to all |
| <kbd>l</kbd> <kbd>→</kbd> | Open the selected message | <kbd>f</kbd> | Forward |
| <kbd>h</kbd> <kbd>←</kbd> <kbd>u</kbd> | Back to the message list | <kbd>a</kbd> | Archive |
| <kbd>w</kbd> | Next message in the conversation | <kbd>d</kbd> | Delete |
| <kbd>b</kbd> | Previous message in the conversation | <kbd>!</kbd> | Mark as spam |
| <kbd>/</kbd> | Search | <kbd>s</kbd> | Star or unstar |
| <kbd>c</kbd> | Compose | <kbd>m</kbd> | Mark read or unread |
| <kbd>?</kbd> | This list | <kbd>x</kbd> | Select the row (for a bulk action) |

<kbd>Esc</kbd> backs out of a reply, forward or compose and returns you to the
message list. It works whether or not single-key shortcuts are enabled — as does
everything in the menus — and in a search field it still clears the search.

Keys never fire while you are typing: whatever has focus gets first refusal, so
"archive" typed into the search box searches for it.

## Installing

**Flatpak (recommended)** — works on any distribution, on **x86_64 and
aarch64 (ARM64)**; see [vireo.hyprlab.co](https://vireo.hyprlab.co) for the
signed Flatpak repo. Installing from the repo picks the right architecture on
its own:

```sh
flatpak install --from https://vireo.hyprlab.co/flatpak/co.hyprlab.Vireo.flatpakref
```

Prefer a direct download? Each release carries `Vireo-x86_64.flatpak` and
`Vireo-aarch64.flatpak`; grab the one matching `uname -m` from the
[latest release](https://github.com/hyprlab/vireo/releases/latest) and run
`flatpak install ./Vireo-*.flatpak` — the bundle carries the repo address and
signing key, so it still receives updates from the official repo. (A bundle
holds a single architecture; the repo above holds both.)

**Fedora** — download the `.rpm` from the
[latest release](https://github.com/hyprlab/vireo/releases/latest) and:

```sh
sudo dnf install ./vireo-*.x86_64.rpm
```

Arch, Debian/Ubuntu and Snap packages were discontinued after 1.7.0 — use the
Flatpak (it works on every distribution) or build from source.

The RPM targets current Fedora releases (44+) on x86_64 only — on ARM, or on
anything older, use the Flatpak or build from source. A Secret Service provider (e.g. gnome-keyring,
preinstalled on GNOME) is needed at runtime for password storage.

## Building from source

Vireo needs the Rust toolchain and the GTK 4 / libadwaita / WebKitGTK 6
development libraries, plus a Secret Service provider (e.g. gnome-keyring) at
runtime.

**Fedora**

```sh
sudo dnf install gtk4-devel libadwaita-devel webkitgtk6.0-devel poppler-glib-devel
```

**Debian / Ubuntu**

```sh
sudo apt install libgtk-4-dev libadwaita-1-dev libwebkitgtk-6.0-dev libpoppler-glib-dev
```

**Build & install**

```sh
git clone https://github.com/hyprlab/vireo.git
cd vireo
cargo build --release
./install.sh          # installs the binary, icon and .desktop file into ~/.local
```

## Configuration

Add accounts from **Settings → Accounts** in the app. For a plain IMAP/SMTP
account you only need the server and an app-specific password. See
[`accounts.toml.example`](accounts.toml.example) for the on-disk format —
passwords you enter there are migrated into the system keyring on first run and
removed from the file.

### OAuth (Google / Microsoft)

**Microsoft** works out of the box — pick *Microsoft* in the account editor and
sign in.

**Google** signs in through **GNOME Online Accounts** — add your Google account in
*GNOME Settings → Online Accounts*, then import it in Vireo. Official builds don't
bundle a Google OAuth client (Google's secret can't live in a public repo), so
GNOME Online Accounts is the standard path. You can also use your own OAuth client
(see below).

To use **your own** OAuth client (a fork, a self-hosted build, or to replace the
bundled ones), put it in `~/.config/vireo/oauth.toml`:

```toml
[google]
client_id = "your-client-id.apps.googleusercontent.com"
client_secret = "your-client-secret"

[microsoft]
client_id = "your-azure-application-client-id"  # public client, no secret
```

or via the `VIREO_GOOGLE_CLIENT_ID` / `VIREO_GOOGLE_CLIENT_SECRET` and
`VIREO_MICROSOFT_CLIENT_ID` / `VIREO_MICROSOFT_CLIENT_SECRET` environment variables.

**Bundling a Google client at build time** (for maintainers) — set the env vars
during the build and they're compiled in via `option_env!`:

```sh
VIREO_GOOGLE_CLIENT_ID=... VIREO_GOOGLE_CLIENT_SECRET=... cargo build --release
```

## Privacy

Vireo collects no telemetry and sends no analytics. Remote content in messages is
blocked by default to defeat tracking pixels. Passwords and OAuth refresh tokens
live in the system keyring (secret-service), never in plain files.

## AI notice

Vireo is built by a human maintainer working with generative AI as a
development tool:

- **Code** — the large majority of the Rust code in this repository was written
  with Anthropic's Claude (via Claude Code), working from the maintainer's
  direction. The maintainer decides what gets built, reviews the results, tests
  every release, and signs off on everything that ships.
- **Text** — documentation, release notes, and website copy are largely
  AI-drafted and human-edited.
- **Artwork** — the app icon and other visual assets are human-made, without
  generative AI.
- **The app itself contains no AI.** Vireo has no AI features, makes no
  requests to AI services, and never sends your mail or any other data to one —
  AI was used to *build* the app, not to run it. See [Privacy](#privacy).

Bug reports and pull requests are welcome from humans and their AI tools alike;
everything merged gets the same human review.

## Contributors

Vireo is maintained by Hyprlab. Thanks to the people who have sent patches
upstream — their work ships in the app and is credited in the About window:

- [**Alfonso Lizárraga**](https://github.com/alfonsolzrg) ([#14](https://github.com/hyprlab/vireo/pull/14)) — sending
  to recipients with punctuated or accented names, the startup message list,
  message-list rebuild performance, the unread dot, and the Attachments-row
  setting.
- [**Chris Pouliot**](https://github.com/chrispouliot) ([#13](https://github.com/hyprlab/vireo/pull/13)) — Proton
  Bridge connections: IMAP STARTTLS and locally signed certificates.
- [**Isaac**](https://github.com/thecalamityjoe87) ([#31](https://github.com/hyprlab/vireo/pull/31)) — the
  preference to always load remote content, for people who would rather not be
  asked.
- [**Alexander Lubovenko**](https://github.com/typedev) ([#45](https://github.com/hyprlab/vireo/pull/45)) — Gmail
  conversations: showing a message once rather than once per label, answering it
  from whichever label already holds its body or attachments, and fetching a
  conversation's bodies in one request instead of one apiece.
- [**Anton Palgunov**](https://github.com/Toxblh) ([#7](https://github.com/hyprlab/vireo/pull/7),
  [#8](https://github.com/hyprlab/vireo/pull/8)) — sender avatars from GNOME
  Contacts photos, and GNOME Online Accounts refinements: custom server ports
  (IPv6 included), pausing an account while its Mail service is off in GNOME
  Settings, OAuth-aware connection tests, and a timeout on stalled IMAP
  connections.

Pull requests are welcome. There's no CLA — by opening one you agree your
contribution ships under the [AGPL-3.0-or-later](LICENSE), and it may be adapted
before it lands (with the change explained on the pull request).

## Contact & support

- Website — [vireo.hyprlab.co](https://vireo.hyprlab.co)
- Email — [hyprlab@proton.me](mailto:hyprlab@proton.me)
- [Buy me a coffee](https://buymeacoffee.com/hyprlab) ☕

## License

Vireo is free software licensed under the **GNU Affero General Public License
v3.0 or later** ([AGPL-3.0-or-later](LICENSE)).

© 2026 Hyprlab
