//! The grab pill: a thin rounded bar floated near a pane boundary, the iOS
//! home-indicator look, that drags a vertical `GtkPaned` divider. The Paned's
//! own separator is styled invisible where this is used — the pill is the
//! visible affordance, and dragging it moves the real divider.

use gtk::prelude::*;

/// Build a pill that drags `paned`'s divider. Dragging down always increases
/// the position (the top pane grows), which reads correctly from either side
/// of the divider. `clamp` bounds each candidate position; hand the Paned's
/// own limits back unchanged with `|_, p| p`.
///
/// The caller aligns the pill against its boundary (`set_valign` +
/// margin) and adds it to an Overlay over the pane it rides.
///
/// The pointer is tracked in ROOT coordinates: the pill rides a pane the drag
/// itself resizes, so pill-local offsets would feed back into the resize and
/// jitter (the console grip's trick).
pub fn paned_grab_pill(
    paned: &gtk::Paned,
    clamp: impl Fn(&gtk::Paned, i32) -> i32 + 'static,
) -> gtk::Box {
    let pill = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    pill.add_css_class("split-grab-pill");
    pill.set_size_request(100, 5);
    pill.set_halign(gtk::Align::Center);
    pill.set_cursor_from_name(Some("ns-resize"));

    let drag = gtk::GestureDrag::new();
    let split = paned.clone();
    let start = std::rc::Rc::new(std::cell::Cell::new((0i32, 0f32)));
    let to_root_y = {
        let pill = pill.clone();
        move |x: f64, y: f64| -> Option<f32> {
            let root = pill.root()?;
            pill.compute_point(
                root.upcast_ref::<gtk::Widget>(),
                &gtk::graphene::Point::new(x as f32, y as f32),
            )
            .map(|p| p.y())
        }
    };
    {
        let split = split.clone();
        let start = start.clone();
        let to_root_y = to_root_y.clone();
        drag.connect_drag_begin(move |_, x, y| {
            if let Some(ry) = to_root_y(x, y) {
                start.set((split.position(), ry));
            }
        });
    }
    drag.connect_drag_update(move |g, ox, oy| {
        let Some((sx, sy)) = g.start_point() else { return };
        let Some(now) = to_root_y(sx + ox, sy + oy) else { return };
        let (start_pos, start_y) = start.get();
        let want = start_pos + (now - start_y) as i32;
        split.set_position(clamp(&split, want));
    });
    pill.add_controller(drag);
    pill
}
