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
    let pill = pill_widget();
    let split = paned.clone();
    attach_drag(&pill, paned, move |_| {}, move |want| {
        split.set_position(clamp(&split, want));
    });
    pill
}

/// Just the bar itself, for a host that wires its own drag semantics
/// (the attachment drawer's collapsed snap-out) via [`attach_drag`].
pub fn pill_widget() -> gtk::Box {
    let pill = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    pill.add_css_class("split-grab-pill");
    pill.set_size_request(100, 5);
    pill.set_halign(gtk::Align::Center);
    pill.set_cursor_from_name(Some("ns-resize"));
    pill
}

/// Wire the pill's drag: `on_begin` fires as a drag starts, then `on_want`
/// gets each update's candidate divider position (the position at drag start
/// plus the pointer's travel) — the host decides what to do with it.
pub fn attach_drag(
    pill: &gtk::Box,
    paned: &gtk::Paned,
    on_begin: impl Fn(i32) + 'static,
    on_want: impl Fn(i32) + 'static,
) {
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
        let start = start.clone();
        let to_root_y = to_root_y.clone();
        drag.connect_drag_begin(move |_, x, y| {
            if let Some(ry) = to_root_y(x, y) {
                let pos = split.position();
                start.set((pos, ry));
                on_begin(pos);
            }
        });
    }
    drag.connect_drag_update(move |g, ox, oy| {
        let Some((sx, sy)) = g.start_point() else { return };
        let Some(now) = to_root_y(sx + ox, sy + oy) else { return };
        let (start_pos, start_y) = start.get();
        on_want(start_pos + (now - start_y) as i32);
    });
    pill.add_controller(drag);
}
