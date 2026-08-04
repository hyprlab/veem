//! Vireo — a clean, fast, GNOME-native email client built with Rust + relm4.

mod app;
mod avatar;
mod backend;
mod cache;
mod color;
mod config;
mod contacts;
mod goa;
mod models;
mod notify;
mod oauth;
mod platform;
mod power;
mod ui;
mod worker;

use relm4::RelmApp;

use crate::app::AppModel;

const APP_ID: &str = "co.hyprlab.Vireo";

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "vireo=info".into()),
        )
        .init();

    migrate_legacy_dirs();
    register_resources();

    let adw_app = adw::Application::builder()
        .application_id(APP_ID)
        .build();

    let app = RelmApp::from_app(adw_app);
    app.run::<AppModel>(());
}

/// One-time migration for the 1.6.0 rename (Veem → Vireo): if an old config or
/// cache directory exists and the new one doesn't, move it over so accounts,
/// settings and cached mail carry across. Keyring entries migrate lazily in
/// `config::load_key`.
fn migrate_legacy_dirs() {
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
