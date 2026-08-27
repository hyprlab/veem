//! A shared builder for right-click context menus, styled to match GNOME HIG:
//! a flat list of icon + label rows in a borderless `GtkPopover`, grouped into
//! sections by hairline separators. Built from plain widgets (not a
//! `Gio.Menu`/`GtkPopoverMenu`, which on GTK4 wraps everything in an internal
//! `GtkScrolledWindow`), so the popover and its box always size to exactly
//! what the sections need and never grow a scrollbar.
//!
//! Icons deliberately mirror the reader-toolbar buttons for the same actions,
//! tying the menu entries to the buttons users already know. In a menu where
//! any entry carries an icon, iconless entries get a blank slot of the same
//! width so every label stays aligned.

use gtk::prelude::*;

/// One entry in a context menu: a label, an optional leading symbolic icon,
/// and the callback to run on activation. Use `.enabled(false)` to grey an
/// item out rather than hiding it — HIG prefers a disabled item users can
/// still find over one that vanishes and makes the menu shift under them.
pub struct MenuEntry {
    label: String,
    icon: Option<String>,
    enabled: bool,
    activate: Box<dyn Fn()>,
}

impl MenuEntry {
    pub fn new(label: impl Into<String>, activate: impl Fn() + 'static) -> Self {
        Self { label: label.into(), icon: None, enabled: true, activate: Box::new(activate) }
    }

    /// Leading symbolic icon — use the same icon as the toolbar button that
    /// performs this action, so the menu teaches the toolbar.
    pub fn icon(mut self, name: impl Into<String>) -> Self {
        self.icon = Some(name.into());
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

/// Build and pop up a HIG-style context menu anchored at `(x, y)` in
/// `parent`'s own coordinate space (typically the exact widget the
/// right-click landed on, so no coordinate translation is needed). `sections`
/// groups entries into visually separated clusters, e.g. `[[Reply, Reply All,
/// Forward], [Star, Mark Read], [Spam, Archive, Delete]]`.
pub fn show_context_menu(parent: &impl IsA<gtk::Widget>, x: f64, y: f64, sections: Vec<Vec<MenuEntry>>) {
    show_context_menu_with_header(parent, x, y, None, sections);
}

/// [`show_context_menu`] with an optional dim caption header above the first
/// section (e.g. the bulk menu's "5 selected").
pub fn show_context_menu_with_header(
    parent: &impl IsA<gtk::Widget>,
    x: f64,
    y: f64,
    header: Option<&str>,
    sections: Vec<Vec<MenuEntry>>,
) {
    let popover = gtk::Popover::new();
    popover.set_has_arrow(false);
    popover.set_position(gtk::PositionType::Bottom);
    popover.add_css_class("menu");

    let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
    list.add_css_class("context-menu-list");

    if let Some(text) = header {
        let caption = gtk::Label::new(Some(text));
        caption.set_xalign(0.0);
        caption.add_css_class("dim-label");
        caption.add_css_class("caption");
        caption.set_margin_start(10);
        caption.set_margin_top(4);
        caption.set_margin_bottom(2);
        list.append(&caption);
    }

    // Any icon in the menu means every row reserves the icon slot, keeping
    // the labels of iconless entries aligned with the rest.
    let has_icons = sections.iter().flatten().any(|e| e.icon.is_some());

    let mut first = true;
    for entries in sections {
        if entries.is_empty() {
            continue;
        }
        if !first {
            list.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        }
        first = false;

        for entry in entries {
            let MenuEntry { label, icon, enabled, activate } = entry;

            let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            if has_icons {
                let img = match &icon {
                    Some(name) => gtk::Image::from_icon_name(name),
                    None => gtk::Image::new(),
                };
                img.set_pixel_size(16);
                row.append(&img);
            }
            let lbl = gtk::Label::new(Some(&label));
            lbl.set_xalign(0.0);
            lbl.set_hexpand(true);
            row.append(&lbl);

            let btn = gtk::Button::new();
            btn.set_child(Some(&row));
            btn.add_css_class("flat");
            btn.add_css_class("context-menu-item");
            btn.set_halign(gtk::Align::Fill);
            btn.set_sensitive(enabled);

            let weak = popover.downgrade();
            btn.connect_clicked(move |_| {
                activate();
                if let Some(p) = weak.upgrade() {
                    p.popdown();
                }
            });
            list.append(&btn);
        }
    }

    popover.set_child(Some(&list));
    popover.set_parent(parent);
    popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    popover.connect_closed(|p| p.unparent());
    popover.popup();
}
