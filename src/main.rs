//! Veem — a clean, fast, GNOME-native email client built with Rust + relm4.

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

const APP_ID: &str = "com.getveem.Veem";

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "veem=info".into()),
        )
        .init();

    register_resources();

    let adw_app = adw::Application::builder()
        .application_id(APP_ID)
        .build();

    let app = RelmApp::from_app(adw_app);
    app.run::<AppModel>(());
}

/// Register the embedded GResource holding Veem's bundled symbolic icons.
///
/// The blob is compiled from `resources/veem.gresource.xml` by `build.rs` and
/// baked into the binary. Registering it makes the icons available under the
/// resource path `/com/getveem/Veem/icons`. Because the app's resource base
/// path is derived from `APP_ID`, GTK automatically appends that `icons`
/// subdirectory to the default icon theme's search path — so every
/// `com.getveem.Veem-*-symbolic` name resolves from the bundle on any distro,
/// no filesystem install required.
fn register_resources() {
    use gtk::{gio, glib};
    let bytes = glib::Bytes::from_static(include_bytes!(concat!(
        env!("OUT_DIR"),
        "/veem.gresource"
    )));
    match gio::Resource::from_data(&bytes) {
        Ok(resource) => gio::resources_register(&resource),
        Err(e) => tracing::error!("failed to register bundled icon resources: {e}"),
    }
}
