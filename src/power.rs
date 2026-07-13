//! System suspend/resume detection via systemd-logind.
//!
//! When the machine sleeps, open IMAP sockets are silently killed by the OS or
//! dropped by NAT/keepalive timeouts. On wake our long-lived worker sessions
//! (including a parked IMAP IDLE) are dead but nothing notices, so the app stops
//! pulling new mail until it is restarted. We watch logind's `PrepareForSleep`
//! signal and fire `on_wake` on the resume edge so the app can reconnect.
//!
//! logind exposes `PrepareForSleep(start: b)` on the **system** bus: `true` is
//! emitted just before sleeping, `false` once the system has resumed. We only
//! care about the resume edge. Works inside the Flatpak sandbox too — the system
//! bus is available there.

const LOGIN1_DEST: &str = "org.freedesktop.login1";
const LOGIN1_PATH: &str = "/org/freedesktop/login1";
const LOGIN1_MANAGER: &str = "org.freedesktop.login1.Manager";

/// Watch systemd-logind for resume-from-sleep, invoking `on_wake` each time the
/// system wakes. Runs on a dedicated thread; silently no-ops if logind or the
/// system bus is unavailable.
pub fn watch_resume<F: Fn() + Send + 'static>(on_wake: F) {
    let _ = std::thread::Builder::new()
        .name("logind-watch".into())
        .spawn(move || {
            if let Err(e) = watch_loop(&on_wake) {
                tracing::debug!("logind sleep watch stopped: {e}");
            }
        });
}

fn watch_loop<F: Fn()>(on_wake: &F) -> Result<(), Box<dyn std::error::Error>> {
    let conn = zbus::blocking::Connection::system()?;
    let proxy = zbus::blocking::Proxy::new(&conn, LOGIN1_DEST, LOGIN1_PATH, LOGIN1_MANAGER)?;
    // Blocks until logind emits PrepareForSleep; ends if the bus closes.
    let mut signals = proxy.receive_signal("PrepareForSleep")?;
    for msg in signals.by_ref() {
        // start == true just before sleeping, false once resumed. Only the
        // resume edge needs a reconnect.
        if let Ok(false) = msg.body().deserialize::<bool>() {
            on_wake();
        }
    }
    Ok(())
}
