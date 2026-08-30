//! Vireo — a clean, fast, GNOME-native email client built with Rust + relm4.

mod app;
mod background;
mod avatar;
mod backend;
mod cache;
mod color;
mod config;
mod contacts;
mod datefmt;
mod goa;
mod logo;
mod models;
mod mutf7;
mod notify;
mod oauth;
mod platform;
mod power;
mod rng;
mod ui;
mod verify;
mod worker;

use relm4::RelmApp;

use crate::app::AppModel;

const APP_ID: &str =
    if cfg!(feature = "beta") { "co.hyprlab.Vireo.Beta" } else { "co.hyprlab.Vireo" };

/// The user-visible application name.
pub const APP_NAME: &str = if cfg!(feature = "beta") { "Vireo (beta)" } else { "Vireo" };

/// The user-visible version — the crate version verbatim. Beta builds carry a
/// semver prerelease in Cargo.toml itself (e.g. "1.18.2-beta.2" on the beta
/// branch), so no suffix is bolted on here.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Command-line flag for starting without a window (used by the autostart entry
/// the background portal writes).
pub const HIDDEN_FLAG: &str = "--hidden";

/// Whether this run started hidden. Read once the UI is built, to keep the first
/// activation from presenting the window that was deliberately not shown.
pub static HIDDEN_START: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "vireo=info".into()),
        )
        .init();

    migrate_legacy_dirs();
    // Attachments the user opened in a previous session were decrypted to a temp
    // directory and left there. Clear it before anything else runs.
    ui::attachments_gallery::purge_attachment_dir();
    register_resources();

    // `--hidden` starts without showing the window: the autostart entry written
    // by the background portal uses it, so logging in leaves Vireo checking mail
    // from the Background Apps menu rather than opening a window at you. The flag
    // is stripped before GTK sees the arguments, which would otherwise reject it
    // as unknown.
    let mut args: Vec<String> = std::env::args().collect();
    let hidden = args.iter().any(|a| a == HIDDEN_FLAG);
    args.retain(|a| a != HIDDEN_FLAG);
    HIDDEN_START.store(hidden, std::sync::atomic::Ordering::Relaxed);

    let adw_app = adw::Application::builder()
        .application_id(APP_ID)
        // mailto: links land here (the desktop file registers the scheme) —
        // both on a fresh launch and relayed from a second invocation.
        .flags(gtk::gio::ApplicationFlags::HANDLES_OPEN)
        .build();
    {
        use gtk::gio::prelude::*;
        adw_app.connect_open(|app, files, _hint| {
            for f in files {
                let uri = f.uri().to_string();
                if uri.starts_with("mailto:") {
                    app::queue_mailto(uri);
                }
            }
            // `open` replaces `activate` when a URI is passed: activate
            // explicitly so the window (and on first launch, the whole UI)
            // still comes up, with the composer opening over it.
            app.activate();
        });
    }
    // The embedded icon gresource lives at /co/hyprlab/Vireo regardless of the
    // channel; pin the base path so the beta's app ID (co.hyprlab.Vireo.Beta)
    // doesn't derive a base the bundled symbolic icons aren't under.
    {
        use gtk::gio::prelude::ApplicationExt;
        adw_app.set_resource_base_path(Some("/co/hyprlab/Vireo"));
    }

    // A second launch (a mailto: link, or just opening the app again) must
    // hand off to the running instance and exit. RelmApp's run loop is built
    // for the primary only (a remote instance never leaves it), and the app
    // must NOT be registered early either — relm4 builds the whole UI in a
    // `startup` handler it connects inside run(), and registration is what
    // emits `startup`. So remoteness is checked bus-side, touching nothing.
    if primary_instance_running() {
        relay_to_primary(&args);
        return;
    }

    let app = RelmApp::from_app(adw_app)
        .with_args(args)
        .visible_on_activate(!hidden);
    app.run::<AppModel>(());
}

/// Whether another instance already owns the app's D-Bus name.
fn primary_instance_running() -> bool {
    use gtk::glib::prelude::ToVariant;
    let Ok(conn) = gtk::gio::bus_get_sync(gtk::gio::BusType::Session, gtk::gio::Cancellable::NONE)
    else {
        return false;
    };
    conn.call_sync(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "NameHasOwner",
        Some(&(APP_ID,).to_variant()),
        None,
        gtk::gio::DBusCallFlags::NONE,
        3000,
        gtk::gio::Cancellable::NONE,
    )
    .ok()
    .and_then(|v| v.get::<(bool,)>())
    .is_some_and(|(owned,)| owned)
}

/// Forward this invocation to the running primary instance over D-Bus and
/// return once it has been accepted: `Open` with any mailto: URIs, plain
/// `Activate` (present the window) otherwise.
fn relay_to_primary(args: &[String]) {
    use gtk::glib::prelude::ToVariant;
    let uris: Vec<String> =
        args.iter().skip(1).filter(|a| a.starts_with("mailto:")).cloned().collect();
    let conn = match gtk::gio::bus_get_sync(gtk::gio::BusType::Session, gtk::gio::Cancellable::NONE)
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("could not reach the session bus to hand off: {e}");
            return;
        }
    };
    let path = format!("/{}", APP_ID.replace('.', "/"));
    let platform: std::collections::HashMap<String, gtk::glib::Variant> = Default::default();
    let (method, params) = if uris.is_empty() {
        ("Activate", (platform,).to_variant())
    } else {
        ("Open", (uris, String::new(), platform).to_variant())
    };
    if let Err(e) = conn.call_sync(
        Some(APP_ID),
        &path,
        "org.gtk.Application",
        method,
        Some(&params),
        None,
        gtk::gio::DBusCallFlags::NONE,
        5000,
        gtk::gio::Cancellable::NONE,
    ) {
        tracing::warn!("hand-off to the running instance failed: {e}");
    }
}

/// One-time migration for the 1.6.0 rename (Veem → Vireo): if an old config or
/// cache directory exists and the new one doesn't, move it over so accounts,
/// settings and cached mail carry across. Keyring entries migrate lazily in
/// `config::load_key`.
fn migrate_legacy_dirs() {
    // Native installs: the old dirs live under the same XDG prefix — rename.
    for base in [config::config_base(), config::cache_base()] {
        let Some(base) = base else { continue };
        let old = base.join("veem");
        let new = base.join("vireo");
        if old.is_dir() && !new.exists() {
            match std::fs::rename(&old, &new) {
                Ok(()) => tracing::info!("migrated {} to {}", old.display(), new.display()),
                Err(e) => tracing::warn!("could not migrate {}: {e}", old.display()),
            }
        }
    }
    migrate_flatpak_data();
}

/// Flatpak: the old Veem app's data is a *different* sandbox tree
/// (`~/.var/app/com.getveem.Veem`), mounted read-only via finish-args — so it
/// is copied, not renamed, into this app's own dirs on first run.
fn migrate_flatpak_data() {
    if !std::path::Path::new("/.flatpak-info").exists() {
        return;
    }
    let Some(home) = dirs::home_dir() else { return };
    let legacy = home.join(".var/app/com.getveem.Veem");
    for (sub, base) in [("config", config::config_base()), ("cache", config::cache_base())] {
        let Some(base) = base else { continue };
        let old = legacy.join(sub).join("veem");
        let new = base.join("vireo");
        if old.is_dir() && !new.exists() {
            match copy_dir(&old, &new) {
                Ok(()) => tracing::info!("migrated {} to {}", old.display(), new.display()),
                Err(e) => tracing::warn!("could not migrate {}: {e}", old.display()),
            }
        }
    }
}

fn copy_dir(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

/// Register the embedded GResource holding Vireo's bundled symbolic icons.
///
/// The blob is compiled from `resources/vireo.gresource.xml` by `build.rs` and
/// baked into the binary. Registering it makes the icons available under the
/// resource path `/co/hyprlab/Vireo/icons`. Because the app's resource base
/// path is derived from `APP_ID`, GTK automatically appends that `icons`
/// subdirectory to the default icon theme's search path — so every
/// `co.hyprlab.Vireo-*-symbolic` name resolves from the bundle on any distro,
/// no filesystem install required.
fn register_resources() {
    use gtk::{gio, glib};
    let bytes = glib::Bytes::from_static(include_bytes!(concat!(
        env!("OUT_DIR"),
        "/vireo.gresource"
    )));
    match gio::Resource::from_data(&bytes) {
        Ok(resource) => gio::resources_register(&resource),
        Err(e) => tracing::error!("failed to register bundled icon resources: {e}"),
    }
}
