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

    let adw_app = adw::Application::builder()
        .application_id(APP_ID)
        .build();

    let app = RelmApp::from_app(adw_app);
    app.run::<AppModel>(());
}
