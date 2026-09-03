//! A tray icon, for the desktops that have a tray (issue #116).
//!
//! GNOME has none — its answer is Background Apps, see [`crate::background`] —
//! but Cinnamon, KDE, MATE, XFCE and GNOME with the AppIndicator extension all
//! speak the freedesktop **StatusNotifierItem** protocol: the app publishes an
//! item over D-Bus and registers it with `org.kde.StatusNotifierWatcher`, and
//! the panel draws it. That is what "AppIndicator" means today.
//!
//! The item is an icon that wears a red dot while any inbox has unread mail,
//! a tooltip saying how many, a menu (open, accounts, settings, quit), and a click that brings the
//! window back. Off by default: on a desktop with no watcher nothing is drawn
//! and nothing else changes — the Background Apps listing comes from the
//! portal, which this never touches. The item keeps waiting, so enabling a
//! tray extension later picks it up without a restart.
//!
//! Icons are sent as pixel data rather than by name: the panel lives outside
//! the sandbox and may not resolve our icon theme, and the dot has to be drawn
//! on anyway. The Vireo icon is the app icon itself; the envelope variants are
//! the reader's `mail-unread-symbolic` in plain white or black, for panels
//! that don't recolour symbolic icons.

use gtk::cairo;
use gtk::gdk_pixbuf::{InterpType, Pixbuf, PixbufLoader};
use gtk::prelude::*;
use ksni::blocking::{Handle, TrayMethods};
use ksni::menu::{MenuItem, StandardItem};
use ksni::{Category, Icon, Status, ToolTip, Tray};

use crate::app::AppMsg;
use crate::config::TrayIcon;

/// The app icon, as shipped.
const APP_ICON_PNG: &[u8] = include_bytes!("../data/icons/hicolor/256x256/apps/co.hyprlab.Vireo.png");
/// The envelope, as the reader draws it; its fill is swapped for the chosen colour.
const ENVELOPE_SVG: &str =
    include_str!("../resources/icons/scalable/actions/co.hyprlab.Vireo-mail-unread-symbolic.svg");
/// Panels ask for different sizes; a set covers them without upscaling blur.
const SIZES: [i32; 6] = [16, 22, 24, 32, 48, 64];
/// GNOME's red (`@error_color`).
const DOT_RGB: (f64, f64, f64) = (0xe0 as f64 / 255.0, 0x1b as f64 / 255.0, 0x24 as f64 / 255.0);

/// A running tray item. Dropping it does not remove the icon; call
/// [`TrayHandle::stop`].
pub struct TrayHandle {
    handle: Handle<VireoTray>,
    last_unread: std::cell::Cell<u32>,
}

impl TrayHandle {
    /// Publish the item. `None` when the session bus can't be reached at
    /// all; a missing watcher is not that — the item then waits for one.
    pub fn start(sender: relm4::Sender<AppMsg>, icon: TrayIcon, unread: u32) -> Option<Self> {
        let tray = VireoTray {
            plain: render_set(icon, false),
            dotted: render_set(icon, true),
            unread,
            sender,
        };
        match tray.disable_dbus_name(true).assume_sni_available(true).spawn() {
            Ok(handle) => Some(Self { handle, last_unread: std::cell::Cell::new(unread) }),
            Err(e) => {
                tracing::info!("tray icon not shown: {e}");
                None
            }
        }
    }

    /// The unread total across inboxes: the dot is on while it is non-zero.
    pub fn set_unread(&self, unread: u32) {
        if self.last_unread.replace(unread) == unread {
            return;
        }
        self.handle.update(|t| t.unread = unread);
    }

    /// Swap the icon set.
    pub fn set_icon(&self, icon: TrayIcon) {
        let plain = render_set(icon, false);
        let dotted = render_set(icon, true);
        self.handle.update(move |t| {
            t.plain = plain;
            t.dotted = dotted;
        });
    }

    /// Take the item down.
    pub fn stop(self) {
        let _ = self.handle.shutdown();
    }
}

struct VireoTray {
    plain: Vec<Icon>,
    dotted: Vec<Icon>,
    unread: u32,
    sender: relm4::Sender<AppMsg>,
}

impl Tray for VireoTray {
    fn id(&self) -> String {
        crate::APP_ID.to_string()
    }

    fn title(&self) -> String {
        "Vireo".to_string()
    }

    fn category(&self) -> Category {
        Category::Communications
    }

    fn status(&self) -> Status {
        Status::Active
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        if self.unread > 0 { self.dotted.clone() } else { self.plain.clone() }
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            title: "Vireo".to_string(),
            description: crate::background::status_text(self.unread),
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.sender.send(AppMsg::PresentWindow);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: "Open Vireo".to_string(),
                activate: Box::new(|t: &mut Self| {
                    let _ = t.sender.send(AppMsg::PresentWindow);
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            // The settings window sits on the main window, so that comes
            // back first when it was hidden.
            StandardItem {
                label: "Accounts".to_string(),
                activate: Box::new(|t: &mut Self| {
                    let _ = t.sender.send(AppMsg::PresentWindow);
                    let _ = t.sender.send(AppMsg::OpenAccounts);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Settings".to_string(),
                activate: Box::new(|t: &mut Self| {
                    let _ = t.sender.send(AppMsg::PresentWindow);
                    let _ = t.sender.send(AppMsg::OpenPreferences);
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".to_string(),
                activate: Box::new(|t: &mut Self| {
                    let _ = t.sender.send(AppMsg::QuitFromTray);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// Every size of one icon, with or without the dot.
fn render_set(icon: TrayIcon, dotted: bool) -> Vec<Icon> {
    SIZES
        .iter()
        .filter_map(|&size| render(icon, dotted, size))
        .collect()
}

/// Decode the chosen icon at `size` and composite the dot over its top-right.
fn render(icon: TrayIcon, dotted: bool, size: i32) -> Option<Icon> {
    let pixbuf = base_pixbuf(icon, size)?;
    let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, size, size).ok()?;
    {
        let cr = cairo::Context::new(&surface).ok()?;
        // Centre a pixbuf the loader sized under the canvas (an SVG keeps its
        // aspect, so a wide envelope comes back shorter than `size`).
        let x = f64::from(size - pixbuf.width()) / 2.0;
        let y = f64::from(size - pixbuf.height()) / 2.0;
        cr.set_source_pixbuf(&pixbuf, x, y);
        cr.paint().ok()?;
        if dotted {
            let s = f64::from(size);
            let r = (s * 0.19).max(2.0);
            let (cx, cy) = (s - r - s * 0.04, r + s * 0.04);
            // A clear ring first, so the dot reads as sitting on top of the
            // icon rather than merging into its corner.
            cr.set_operator(cairo::Operator::Clear);
            cr.arc(cx, cy, r + (s * 0.06).max(1.0), 0.0, std::f64::consts::TAU);
            cr.fill().ok()?;
            cr.set_operator(cairo::Operator::Over);
            cr.set_source_rgb(DOT_RGB.0, DOT_RGB.1, DOT_RGB.2);
            cr.arc(cx, cy, r, 0.0, std::f64::consts::TAU);
            cr.fill().ok()?;
        }
    }
    surface.flush();
    let stride = surface.stride() as usize;
    let mut surface = surface;
    let data = surface.data().ok()?;
    Some(Icon {
        width: size,
        height: size,
        data: argb_network_order(&data, size as usize, size as usize, stride),
    })
}

/// The icon's pixels before any dot: the app icon scaled down, or the
/// envelope rasterised in the chosen colour.
fn base_pixbuf(icon: TrayIcon, size: i32) -> Option<Pixbuf> {
    match icon {
        TrayIcon::Vireo => {
            let loader = PixbufLoader::with_type("png").ok()?;
            loader.write(APP_ICON_PNG).ok()?;
            loader.close().ok()?;
            loader.pixbuf()?.scale_simple(size, size, InterpType::Bilinear)
        }
        TrayIcon::EnvelopeLight | TrayIcon::EnvelopeDark => {
            let fill = if icon == TrayIcon::EnvelopeLight { "#ffffff" } else { "#000000" };
            let svg = ENVELOPE_SVG.replace("#2e3436", fill);
            let loader = PixbufLoader::with_type("svg").ok()?;
            loader.set_size(size, size);
            loader.write(svg.as_bytes()).ok()?;
            loader.close().ok()?;
            loader.pixbuf()
        }
    }
}

/// Cairo's ARGB32 is premultiplied and native-endian; the protocol wants
/// straight alpha in network (big-endian) byte order.
fn argb_network_order(data: &[u8], width: usize, height: usize, stride: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        let row = &data[y * stride..y * stride + width * 4];
        for px in row.chunks_exact(4) {
            let native = u32::from_ne_bytes([px[0], px[1], px[2], px[3]]);
            let a = (native >> 24) & 0xff;
            let un = |c: u32| -> u8 {
                if a == 0 { 0 } else { ((c * 255 + a / 2) / a).min(255) as u8 }
            };
            let r = un((native >> 16) & 0xff);
            let g = un((native >> 8) & 0xff);
            let b = un(native & 0xff);
            out.extend_from_slice(&[a as u8, r, g, b]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpremultiplies_into_network_order() {
        // One half-transparent pure-red pixel, premultiplied, native-endian.
        let native = (0x80u32 << 24) | (0x80 << 16);
        let px = native.to_ne_bytes();
        let out = argb_network_order(&px, 1, 1, 4);
        assert_eq!(out, vec![0x80, 0xff, 0x00, 0x00]);
        // Fully transparent stays all-zero rather than dividing by zero.
        assert_eq!(argb_network_order(&[0, 0, 0, 0], 1, 1, 4), vec![0, 0, 0, 0]);
    }

    #[test]
    fn every_icon_renders_at_every_size() {
        for icon in [TrayIcon::Vireo, TrayIcon::EnvelopeLight, TrayIcon::EnvelopeDark] {
            for dotted in [false, true] {
                let set = render_set(icon, dotted);
                assert_eq!(set.len(), SIZES.len(), "{icon:?} dotted={dotted}");
                for (i, size) in SIZES.iter().enumerate() {
                    assert_eq!(set[i].width, *size);
                    assert_eq!(set[i].data.len(), (size * size * 4) as usize);
                }
            }
        }
    }

    #[test]
    fn the_dot_is_red_and_only_when_asked() {
        let size = 32usize;
        let plain = render(TrayIcon::EnvelopeLight, false, size as i32).unwrap();
        let dotted = render(TrayIcon::EnvelopeLight, true, size as i32).unwrap();
        // The dot's centre, per `render`.
        let s = size as f64;
        let r = s * 0.19;
        let (cx, cy) = ((s - r - s * 0.04) as usize, (r + s * 0.04) as usize);
        let at = |icon: &Icon| {
            let i = (cy * size + cx) * 4;
            (icon.data[i], icon.data[i + 1], icon.data[i + 2], icon.data[i + 3])
        };
        let (a, red, g, b) = at(&dotted);
        assert_eq!(a, 0xff);
        assert_eq!((red, g, b), (0xe0, 0x1b, 0x24));
        assert_ne!(at(&plain), at(&dotted));
    }
}
