//! A shared builder for right-click context menus, styled to match GNOME HIG:
//! a flat list of label rows in a borderless `GtkPopover`, grouped into
//! sections by hairline separators. Built from plain widgets (not a
//! `Gio.Menu`/`GtkPopoverMenu`, which on GTK4 wraps everything in an internal
//! `GtkScrolledWindow`), so the popover and its box always size to exactly
//! what the sections need and never grow a scrollbar.

use gtk::prelude::*;

/// One entry in a context menu: a label and the callback to run on
/// activation. Use `.enabled(false)` to grey an item out rather than hiding
/// it — HIG prefers a disabled item users can still find over one that
/// vanishes and makes the menu shift under them.
pub struct MenuEntry {
    label: String,
    enabled: bool,
    activate: Box<dyn Fn()>,
}

impl MenuEntry {
    pub fn new(label: impl Into<String>, activate: impl Fn() + 'static) -> Self {
        Self { label: label.into(), enabled: true, activate: Box::new(activate) }
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
    let popover = gtk::Popover::new();
    popover.set_has_arrow(false);
    popover.set_position(gtk::PositionType::Bottom);
    popover.add_css_class("menu");

    let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
    list.add_css_class("context-menu-list");

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
            let MenuEntry { label, enabled, activate } = entry;

            let lbl = gtk::Label::new(Some(&label));
            lbl.set_xalign(0.0);
            lbl.set_hexpand(true);

            let btn = gtk::Button::new();
            btn.set_child(Some(&lbl));
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
