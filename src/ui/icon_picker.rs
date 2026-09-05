//! The app-icon gallery: a grid of every icon in `app_icon::catalog`, with
//! the current choice ringed. Shared by Settings (System & Appearance) and
//! the welcome wizard's personalize page; the caller hears each pick.

use std::rc::Rc;

use gtk::prelude::*;

/// Build the gallery as a fixed six-wide grid (three even rows) with
/// `selected` ringed; `on_pick` runs for every change the user makes (not
/// for the initial selection).
pub fn gallery(selected: &str, tile: i32, on_pick: Rc<dyn Fn(&str)>) -> gtk::FlowBox {
    build(selected, tile, 6, on_pick)
}

/// The gallery as one row that scrolls sideways, for a settings row.
pub fn strip(selected: &str, tile: i32, on_pick: Rc<dyn Fn(&str)>) -> gtk::ScrolledWindow {
    let n = crate::app_icon::catalog().count() as u32;
    let row = build(selected, tile, n.max(1), on_pick);
    row.set_halign(gtk::Align::Start);
    row.set_hexpand(false);
    let sw = gtk::ScrolledWindow::new();
    sw.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Never);
    sw.set_propagate_natural_height(true);
    sw.set_overlay_scrolling(true);
    sw.set_child(Some(&row));
    // Open on the current choice rather than the start of the row.
    if let Some(child) = row.selected_children().into_iter().next() {
        let sw2 = sw.clone();
        gtk::glib::idle_add_local_once(move || {
            let x = child.allocation().x() as f64;
            let w = child.allocation().width() as f64;
            let page = sw2.hadjustment().page_size();
            sw2.hadjustment().set_value((x + w / 2.0 - page / 2.0).max(0.0));
        });
    }
    sw
}

fn build(selected: &str, tile: i32, per_line: u32, on_pick: Rc<dyn Fn(&str)>) -> gtk::FlowBox {
    let grid = gtk::FlowBox::new();
    grid.add_css_class("icon-gallery");
    grid.set_selection_mode(gtk::SelectionMode::Single);
    grid.set_homogeneous(true);
    grid.set_activate_on_single_click(true);
    grid.set_min_children_per_line(per_line);
    grid.set_max_children_per_line(per_line);
    grid.set_column_spacing(2);
    grid.set_row_spacing(2);
    grid.set_halign(gtk::Align::Fill);
    grid.set_hexpand(true);

    let mut to_select = None;
    for choice in crate::app_icon::catalog() {
        let cell = gtk::Box::new(gtk::Orientation::Vertical, 4);
        cell.set_halign(gtk::Align::Center);
        let pic = gtk::Picture::new();
        // 2x for HiDPI; still far cheaper than decoding the full 512.
        if let Some(tex) = crate::app_icon::texture(choice.id, tile * 2) {
            pic.set_paintable(Some(&tex));
        }
        pic.set_size_request(tile, tile);
        pic.set_can_shrink(true);
        pic.set_content_fit(gtk::ContentFit::Contain);
        cell.append(&pic);
        let label = gtk::Label::new(Some(choice.label));
        label.add_css_class("icon-gallery-label");
        // Two-word names ("Yellow & blue") wrap onto a second line where
        // the cell is narrow (the wizard's card) rather than ellipsizing.
        label.set_wrap(true);
        label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        label.set_lines(2);
        label.set_max_width_chars(11);
        label.set_justify(gtk::Justification::Center);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        cell.append(&label);
        cell.set_tooltip_text(Some(choice.label));

        let child = gtk::FlowBoxChild::new();
        child.set_child(Some(&cell));
        child.set_widget_name(choice.id);
        grid.append(&child);
        if choice.id == selected {
            to_select = Some(child);
        }
    }
    if let Some(child) = to_select.or_else(|| grid.child_at_index(0)) {
        grid.select_child(&child);
    }

    // Connected after the initial selection, so only the user's picks report.
    let current = std::cell::RefCell::new(selected.to_string());
    grid.connect_selected_children_changed(move |g| {
        let Some(child) = g.selected_children().into_iter().next() else { return };
        let id = child.widget_name().to_string();
        if *current.borrow() == id {
            return;
        }
        *current.borrow_mut() = id.clone();
        on_pick(&id);
    });
    grid
}
