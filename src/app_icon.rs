//! The app icon the user chose, and how it reaches the desktop.
//!
//! Vireo ships one icon (the yellow squircle since 1.21; the round envelope
//! before it) and carries a gallery of alternatives inside the binary. A
//! choice is applied by writing that artwork over the app's icon name in the
//! user's own icon directory (`~/.local/share/icons/hicolor`), which every
//! desktop searches before the install's — the Flatpak export included — so
//! the dock, app grid and switcher pick it up under the same name. "Default"
//! removes the override and whatever the build installed shows again.
//!
//! Existing installs keep the envelope: the first start of a build that
//! ships the new default records "legacy" for any install that already has
//! settings on disk, so nobody's dock changes without them asking (the
//! gallery offers the switch). Fresh installs pick in the welcome wizard.
//!
//! The tray icon and the in-app uses draw the same choice, so it needs no
//! restart; the window's own icon (X11 fallback, some panels) does, and the
//! restart is offered — see [`restart`].

use std::path::PathBuf;

/// One gallery entry: the id stored in settings, its label, and the art.
pub struct IconChoice {
    pub id: &'static str,
    pub label: &'static str,
    pub png: &'static [u8],
}

/// The id meaning "whatever this build ships".
pub const DEFAULT_ID: &str = "default";
/// The pre-1.21 round envelope, kept for installs that had it.
pub const LEGACY_ID: &str = "legacy";

/// The icon this build installs under its app ID.
#[cfg(not(feature = "beta"))]
const DEFAULT_PNG: &[u8] = include_bytes!("../data/icons/hicolor/512x512/apps/co.hyprlab.Vireo.png");
#[cfg(feature = "beta")]
const DEFAULT_PNG: &[u8] =
    include_bytes!("../data/icons/hicolor/512x512/apps/co.hyprlab.Vireo.Beta.png");

macro_rules! alt {
    ($id:literal, $label:literal) => {
        IconChoice {
            id: $id,
            label: $label,
            png: include_bytes!(concat!("../data/icons/alt/", $id, ".png")),
        }
    };
}

/// The gallery, in display order: the build's own icon first, the colours,
/// and the envelope last. The beta's ribboned icon is its "Default" and is
/// never offered as a colour of its own.
const CATALOG: &[IconChoice] = &[
    IconChoice { id: DEFAULT_ID, label: "Default", png: DEFAULT_PNG },
    alt!("yellow-blue", "Yellow & blue"),
    alt!("blue", "Blue"),
    alt!("blue-dark", "Dark blue"),
    alt!("blue-yellow", "Blue & yellow"),
    alt!("teal", "Teal"),
    alt!("green", "Green"),
    alt!("orange", "Orange"),
    alt!("red", "Red"),
    alt!("peach", "Peach"),
    alt!("pink", "Pink"),
    alt!("purple", "Purple"),
    alt!("grey", "Grey"),
    alt!("pattern-blue", "Pattern, blue"),
    alt!("pattern-pink", "Pattern, pink"),
    alt!("pattern-teal", "Pattern, teal"),
    alt!("bird", "Bird"),
    alt!("legacy", "Classic"),
];

/// Every choice the gallery offers. The classic envelope belongs to the
/// stable app: on the beta it would hide the beta ribbon, so it is left out
/// and treated as the default there.
pub fn catalog() -> impl Iterator<Item = &'static IconChoice> {
    CATALOG.iter().filter(|c| !(cfg!(feature = "beta") && c.id == LEGACY_ID))
}

/// Normalise a stored id to one this build offers.
fn effective(id: &str) -> &'static str {
    catalog().find(|c| c.id == id).map(|c| c.id).unwrap_or(DEFAULT_ID)
}

/// The artwork for an id (the default's for an unknown one).
pub fn png_for(id: &str) -> &'static [u8] {
    let id = effective(id);
    CATALOG.iter().find(|c| c.id == id).map(|c| c.png).unwrap_or(DEFAULT_PNG)
}

/// The choice in force, settling it on the first start that finds none:
/// an install with settings already on disk keeps the envelope it had, a
/// fresh one gets the default (the wizard lets it pick). The override on
/// disk is brought in line either way, so a reinstall (or a changed
/// default) never silently swaps the icon someone chose.
pub fn init_on_startup() -> String {
    if std::env::var("VIREO_DEMO").is_ok() {
        return DEFAULT_ID.to_string();
    }
    let id = match crate::config::load_app_icon() {
        Some(id) => id,
        None => {
            let id = if crate::config::settings_on_disk() { LEGACY_ID } else { DEFAULT_ID };
            crate::config::save_app_icon(id);
            id.to_string()
        }
    };
    let id = effective(&id).to_string();
    apply(&id);
    id
}

/// Persist a choice and put it on the desktop.
pub fn set(id: &str) -> String {
    let id = effective(id).to_string();
    crate::config::save_app_icon(&id);
    if std::env::var("VIREO_DEMO").is_err() {
        apply(&id);
    }
    id
}

/// The user's icon directory — the host's, from inside the sandbox too (the
/// manifest mounts `xdg-data/icons/hicolor` there, and this is what every
/// desktop's icon lookup searches first).
fn hicolor_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from).or_else(dirs::home_dir)?;
    Some(home.join(".local/share/icons/hicolor"))
}

/// The override files, largest first: every size the install ships must be
/// shadowed, or a desktop asking for a small icon still finds the shipped
/// 256 the closer match.
fn override_paths(hicolor: &std::path::Path) -> [(PathBuf, i32); 2] {
    let name = format!("{}.png", crate::APP_ID);
    [
        (hicolor.join("512x512/apps").join(&name), 512),
        (hicolor.join("256x256/apps").join(&name), 256),
    ]
}

/// Write (or, for the default, remove) the override, and nudge the icon
/// directory's timestamp so the desktop notices: icon lookups re-scan a
/// theme when one of its directories changed, not when a file inside did.
fn apply(id: &str) {
    let Some(hicolor) = hicolor_dir() else { return };
    let changed = if id == DEFAULT_ID {
        remove_override(&hicolor)
    } else {
        write_override(&hicolor, png_for(id))
    };
    if changed {
        touch(&hicolor);
        if let Some(parent) = hicolor.parent() {
            touch(parent);
        }
    }
}

fn remove_override(hicolor: &std::path::Path) -> bool {
    let mut changed = false;
    for (path, _) in override_paths(hicolor) {
        if path.exists() {
            match std::fs::remove_file(&path) {
                Ok(()) => changed = true,
                Err(e) => tracing::warn!("could not remove {}: {e}", path.display()),
            }
        }
    }
    changed
}

fn write_override(hicolor: &std::path::Path, png: &[u8]) -> bool {
    let mut changed = false;
    for (path, size) in override_paths(hicolor) {
        let bytes = if size == 512 {
            std::borrow::Cow::Borrowed(png)
        } else {
            match scaled_png(png, size) {
                Some(b) => std::borrow::Cow::Owned(b),
                None => continue,
            }
        };
        // Only a real change touches the disk: this runs at every start.
        if std::fs::read(&path).map(|cur| cur == *bytes).unwrap_or(false) {
            continue;
        }
        if let Some(dir) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(dir) {
                tracing::warn!("could not create {}: {e}", dir.display());
                continue;
            }
        }
        match std::fs::write(&path, &*bytes) {
            Ok(()) => changed = true,
            Err(e) => tracing::warn!("could not write {}: {e}", path.display()),
        }
    }
    changed
}

/// The artwork re-encoded at `size` (the 256 the install also ships).
fn scaled_png(png: &[u8], size: i32) -> Option<Vec<u8>> {
    use gtk::gdk_pixbuf::{InterpType, PixbufLoader};
    use gtk::prelude::PixbufLoaderExt;
    let loader = PixbufLoader::with_type("png").ok()?;
    loader.write(png).ok()?;
    loader.close().ok()?;
    let pb = loader.pixbuf()?.scale_simple(size, size, InterpType::Hyper)?;
    pb.save_to_bufferv("png", &[]).ok()
}

fn touch(dir: &std::path::Path) {
    if let Ok(f) = std::fs::File::open(dir) {
        let _ = f.set_modified(std::time::SystemTime::now());
    }
}

/// A texture of an icon, decoded at `px` pixels (call with 2x the display
/// size for HiDPI) rather than the full 512, to keep a gallery cheap.
pub fn texture(id: &str, px: i32) -> Option<gtk::gdk::Texture> {
    use gtk::gdk_pixbuf::PixbufLoader;
    use gtk::prelude::PixbufLoaderExt;
    let loader = PixbufLoader::with_type("png").ok()?;
    loader.set_size(px, px);
    loader.write(png_for(id)).ok()?;
    loader.close().ok()?;
    Some(gtk::gdk::Texture::for_pixbuf(&loader.pixbuf()?))
}

// ---------------------------------------------------------------------------
// Restarting
// ---------------------------------------------------------------------------

/// Command-line flag for the restart helper: waits for the running instance
/// to let go of its D-Bus name, then becomes a fresh one.
pub const RESTART_FLAG: &str = "--restart-helper";

/// Well-known name of the restart helper's D-Bus service. Inside Flatpak the
/// helper can't just be spawned — the sandbox is torn down with the process
/// that started it — so it is *activated*: the bus starts the exported
/// service (a new sandbox instance) and the running app exits under it.
pub fn restart_service_name() -> String {
    format!("{}.Restart", crate::APP_ID)
}

/// Whether this process runs inside a Flatpak sandbox.
fn in_flatpak() -> bool {
    std::path::Path::new("/.flatpak-info").exists()
}

/// Start the helper that will bring the app back, and report whether it is
/// in place. The caller quits only on `Ok`, so a failed attempt leaves the
/// app running rather than gone.
pub fn launch_restart_helper() -> Result<(), String> {
    if in_flatpak() {
        activate_restart_service()
    } else {
        spawn_restart_helper()
    }
}

/// Outside Flatpak: run our own binary as a detached helper.
fn spawn_restart_helper() -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    std::process::Command::new(exe)
        .arg(RESTART_FLAG)
        .process_group(0)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Inside Flatpak: ask the session bus to start the helper service, which
/// the export turned into `flatpak run … --restart-helper`. Returns once
/// the helper owns its name (or the bus gave up).
fn activate_restart_service() -> Result<(), String> {
    use gtk::glib::prelude::ToVariant;
    let conn = gtk::gio::bus_get_sync(gtk::gio::BusType::Session, gtk::gio::Cancellable::NONE)
        .map_err(|e| e.to_string())?;
    let reply = conn
        .call_sync(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "StartServiceByName",
            Some(&(restart_service_name(), 0u32).to_variant()),
            None,
            gtk::gio::DBusCallFlags::NONE,
            15_000,
            gtk::gio::Cancellable::NONE,
        )
        .map_err(|e| e.to_string())?;
    match reply.get::<(u32,)>() {
        // 1 = started, 2 = was already running: either way a helper is up.
        Some((1,)) | Some((2,)) => Ok(()),
        other => Err(format!("unexpected StartServiceByName reply {other:?}")),
    }
}

/// The helper's whole life: own the service name (so a bus activation
/// completes), wait for the app to release its own name, then exec a normal
/// instance in this process — inside Flatpak that keeps the helper's
/// sandbox alive as the app's.
pub fn run_restart_helper() -> ! {
    use gtk::glib::prelude::ToVariant;
    use std::os::unix::process::CommandExt;
    let conn = gtk::gio::bus_get_sync(gtk::gio::BusType::Session, gtk::gio::Cancellable::NONE).ok();
    if let Some(conn) = &conn {
        let _ = conn.call_sync(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "RequestName",
            Some(&(restart_service_name(), 4u32).to_variant()), // DO_NOT_QUEUE
            None,
            gtk::gio::DBusCallFlags::NONE,
            3000,
            gtk::gio::Cancellable::NONE,
        );
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    // Give the app a moment to start quitting before the first look.
    std::thread::sleep(std::time::Duration::from_millis(300));
    while crate::primary_instance_running() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    drop(conn);
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("vireo"));
    let err = std::process::Command::new(exe).exec();
    eprintln!("vireo: restart failed: {err}");
    std::process::exit(1);
}
