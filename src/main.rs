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

const APP_ID: &str = "co.hyprlab.Vireo";

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
        .build();

    let app = RelmApp::from_app(adw_app)
        .with_args(args)
        .visible_on_activate(!hidden);
    app.run::<AppModel>(());
}

/// One-time migration for the 1.6.0 rename (Veem → Vireo): if an old config or
/// cache directory exists and the new one doesn't, move it over so accounts,
/// settings and cached mail carry across. Keyring entries migrate lazily in
/// `config::load_key`.
fn migrate_legacy_dirs() {
    // Native installs: the old dirs live under the same XDG prefix — rename.
    for base in [dirs::config_dir(), dirs::cache_dir()] {
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
    for (sub, base) in [("config", dirs::config_dir()), ("cache", dirs::cache_dir())] {
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
