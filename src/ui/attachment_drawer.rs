//! In-message attachment drawer: a resizable footer beneath the reader body that
//! shows every attachment on the open message as a wrapping grid of thumbnails
//! (images and PDFs) or colour-coded type icons (everything else), each with the
//! filename beneath it.
//!
//! It reuses the gallery's thumbnail/icon/open helpers ([`texture_from`],
//! [`thumbnail_texture`], [`icon_for`], [`icon_color_class`], [`open_bytes`]).
//! Hovering a cell reveals
//! the same Download/Open quick actions used in the gallery; right-clicking opens
//! a matching context menu; single-clicking an image opens a modal lightbox
//! (prev/next through the message's images), and clicking a non-image opens it in
//! its default app.
//!
//! Sizing: the drawer *owns* a vertical `GtkPaned` whose top pane is the reader
//! body (passed in via [`DrawerInit`]) and whose bottom pane is the drawer. The
//! divider is the drag grip — native and smooth, and because the reader pane is
//! allowed to shrink, resizing the drawer never grows the window. The size slider
//! in the header scales the thumbnails only; the chevron collapses the grid to
//! just the header. Height, collapsed state, and thumbnail size are persisted.

use std::cell::Cell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use relm4::prelude::*;

use crate::config::{self, DrawerState};
use crate::models::{is_image_name, Attachment};
use crate::ui::attachments_gallery::{
    icon_color_class, icon_for, open_bytes, spawn_pdf_render, texture_from, thumbnail_texture,
    Thumbnail,
};

/// The drawer's cover-cropped picture for a ready thumbnail texture.
fn drawer_picture(tex: &gtk::gdk::Texture) -> gtk::Widget {
    let pic = gtk::Picture::for_paintable(tex);
    pic.set_content_fit(gtk::ContentFit::Cover);
    pic.set_hexpand(true);
    pic.set_vexpand(true);
    pic.upcast()
}

/// The drawer's centred type icon, sized to the current thumbnail scale.
fn drawer_icon(name: &str, thumb: i32) -> gtk::Widget {
    let img = gtk::Image::from_icon_name(icon_for(name));
    img.set_pixel_size((thumb / 2).max(24));
    img.set_halign(gtk::Align::Center);
    img.set_valign(gtk::Align::Center);
    img.set_hexpand(true);
    img.set_vexpand(true);
    img.add_css_class("gallery-file-icon");
    img.add_css_class(icon_color_class(name));
    img.upcast()
}

const MIN_THUMB: f64 = 56.0;
const MAX_THUMB: f64 = 220.0;
/// Minimum expanded drawer height in px (the drag can't go below this).
const MIN_HEIGHT: i32 = 96;

/// What the drawer needs at launch: its remembered state plus the reader widget
/// it docks beneath (which becomes the top pane of its internal `GtkPaned`).
pub struct DrawerInit {
    pub state: DrawerState,
    pub reader: gtk::Widget,
}

pub struct AttachmentDrawer {
    items: Vec<Attachment>,
    /// Display position → index into `items`. Identity for the grid; the list
    /// view is sorted alphabetically (flipped by `sort_desc`).
    display_order: Vec<usize>,
    /// Show an alphabetical list instead of the thumbnail grid (persisted).
    list_view: bool,
    /// Sort the list Z→A instead of A→Z (persisted).
    sort_desc: bool,
    /// Thumbnail edge in px (size slider); does not affect drawer height.
    thumb: i32,
    /// Expanded drawer height in px (the Paned split); persisted.
    height: i32,
    /// Whether the thumbnail area is hidden, leaving only the header.
    collapsed: bool,
    /// The wrapping thumbnail grid (rebuilt on item/size changes).
    flow: gtk::FlowBox,
    /// Reusable right-click context menu, parented to the grid.
    menu: gtk::Popover,
    /// Vertical split: reader body on top, drawer body below. The divider is the
    /// resize grip — native drag, and the reader shrinks rather than the window
    /// growing.
    paned: gtk::Paned,
    /// True while we set the Paned position programmatically, so the resulting
    /// position-notify isn't mistaken for a user drag.
    adjusting: Rc<Cell<bool>>,
    /// Set once the initial split has been applied (needs a realized Paned).
    positioned: Rc<Cell<bool>>,
}

#[derive(Debug)]
pub enum AttachmentDrawerInput {
    /// Replace the shown attachments (empty hides the drawer).
    SetItems(Vec<Attachment>),
    /// New thumbnail edge from the size slider (thumbnails only, not height).
    SetThumbSize(i32),
    /// The Paned divider moved (user drag) — capture the new height.
    PositionChanged,
    /// Toggle the collapsed/expanded state (chevron).
    ToggleCollapsed,
    /// Switch between the thumbnail grid and the alphabetical list.
    ToggleListView,
    /// Flip the list's sort direction (A→Z / Z→A).
    ToggleSortOrder,
    /// Open an attachment in its default application.
    Open(usize),
    /// Save an attachment to disk (file chooser).
    Download(usize),
    /// Single-click: lightbox for images, open for anything else.
    Activate(usize),
    /// Right-click at (x, y) within the cell.
    ContextMenu { index: usize, x: f64, y: f64 },
}

#[relm4::component(pub)]
impl SimpleComponent for AttachmentDrawer {
    type Init = DrawerInit;
    type Input = AttachmentDrawerInput;
    type Output = ();

    view! {
        #[root]
        gtk::Paned {
            set_orientation: gtk::Orientation::Vertical,
            set_wide_handle: true,
            // The reader (start child) shrinks to make room; the drawer keeps its
            // set size. This is what prevents the window from growing.
            set_resize_start_child: true,
            set_shrink_start_child: true,
            set_resize_end_child: false,
            set_shrink_end_child: false,
            connect_position_notify[sender] => move |_| {
                sender.input(AttachmentDrawerInput::PositionChanged);
            },

            #[wrap(Some)]
            set_end_child = &gtk::Box {
                set_orientation: gtk::Orientation::Vertical,
                add_css_class: "attachment-drawer",
                // Hidden (and the divider disappears) when there's nothing to show.
                #[watch]
                set_visible: !model.items.is_empty(),

                gtk::Box {
                    set_orientation: gtk::Orientation::Horizontal,
                    set_spacing: 8,
                    add_css_class: "attachment-drawer-header",
                    gtk::Button {
                        add_css_class: "flat",
                        add_css_class: "circular",
                        set_valign: gtk::Align::Center,
                        #[watch]
                        set_icon_name: if model.collapsed {
                            "co.hyprlab.Vireo-pan-up-symbolic"
                        } else {
                            "co.hyprlab.Vireo-pan-down-symbolic"
                        },
                        #[watch]
                        set_tooltip_text: Some(if model.collapsed {
                            "Show attachments"
                        } else {
                            "Hide attachments"
                        }),
                        connect_clicked[sender] => move |_| {
                            sender.input(AttachmentDrawerInput::ToggleCollapsed);
                        },
                    },
                    gtk::Image {
                        set_icon_name: Some("co.hyprlab.Vireo-mail-attachment-symbolic"),
                        add_css_class: "dim-label",
                    },
                    gtk::Label {
                        #[watch]
                        set_label: &format!(
                            "{} attachment{}",
                            model.items.len(),
                            if model.items.len() == 1 { "" } else { "s" },
                        ),
                        add_css_class: "heading",
                    },
                    gtk::Box { set_hexpand: true },
                    gtk::Button {
                        add_css_class: "flat",
                        set_valign: gtk::Align::Center,
                        // Only the list has an order to flip.
                        #[watch]
                        set_visible: !model.collapsed && model.list_view,
                        #[watch]
                        set_icon_name: if model.sort_desc {
                            "co.hyprlab.Vireo-view-sort-descending-symbolic"
                        } else {
                            "co.hyprlab.Vireo-view-sort-ascending-symbolic"
                        },
                        #[watch]
                        set_tooltip_text: Some(if model.sort_desc {
                            "Sorted Z to A — switch to A to Z"
                        } else {
                            "Sorted A to Z — switch to Z to A"
                        }),
                        connect_clicked[sender] => move |_| {
                            sender.input(AttachmentDrawerInput::ToggleSortOrder);
                        },
                    },
                    gtk::Button {
                        add_css_class: "flat",
                        set_valign: gtk::Align::Center,
                        #[watch]
                        set_visible: !model.collapsed,
                        #[watch]
                        set_icon_name: if model.list_view {
                            "co.hyprlab.Vireo-view-grid-symbolic"
                        } else {
                            "co.hyprlab.Vireo-view-list-bullet-symbolic"
                        },
                        #[watch]
                        set_tooltip_text: Some(if model.list_view {
                            "Show as thumbnails"
                        } else {
                            "Show as a list"
                        }),
                        connect_clicked[sender] => move |_| {
                            sender.input(AttachmentDrawerInput::ToggleListView);
                        },
                    },
                    gtk::Image {
                        set_icon_name: Some("co.hyprlab.Vireo-image-x-generic-symbolic"),
                        add_css_class: "dim-label",
                        set_pixel_size: 12,
                        // The size slider is meaningless with the grid hidden
                        // (and sizes thumbnails only, so also in the list view).
                        #[watch]
                        set_visible: !model.collapsed && !model.list_view,
                    },
                    gtk::Scale {
                        set_orientation: gtk::Orientation::Horizontal,
                        set_width_request: 130,
                        set_range: (MIN_THUMB, MAX_THUMB),
                        set_value: model.thumb as f64,
                        set_round_digits: 0,
                        set_draw_value: false,
                        set_tooltip_text: Some("Thumbnail size"),
                        set_valign: gtk::Align::Center,
                        #[watch]
                        set_visible: !model.collapsed && !model.list_view,
                        connect_value_changed[sender] => move |s| {
                            sender.input(AttachmentDrawerInput::SetThumbSize(s.value() as i32));
                        },
                    },
                    gtk::Image {
                        set_icon_name: Some("co.hyprlab.Vireo-image-x-generic-symbolic"),
                        add_css_class: "dim-label",
                        set_pixel_size: 22,
                        #[watch]
                        set_visible: !model.collapsed && !model.list_view,
                    },
                },

                #[local_ref]
                scroller -> gtk::ScrolledWindow {
                    set_vexpand: true,
                    set_hscrollbar_policy: gtk::PolicyType::Never,
                    set_vscrollbar_policy: gtk::PolicyType::Automatic,
                    #[watch]
                    set_visible: !model.collapsed,

                    #[local_ref]
                    flow -> gtk::FlowBox {
                        set_orientation: gtk::Orientation::Horizontal,
                        set_selection_mode: gtk::SelectionMode::None,
                        set_activate_on_single_click: true,
                        set_homogeneous: false,
                        set_valign: gtk::Align::Start,
                        set_row_spacing: 6,
                        set_column_spacing: 6,
                        set_margin_top: 8,
                        set_margin_bottom: 8,
                        set_margin_start: 8,
                        set_margin_end: 8,
                        set_min_children_per_line: 1,
                        #[watch]
                        set_max_children_per_line: if model.list_view { 1 } else { 40 },
                        add_css_class: "attachment-drawer-flow",
                        connect_child_activated[sender] => move |_, child| {
                            sender.input(AttachmentDrawerInput::Activate(child.index() as usize));
                        },
                    },
                },
            },
        }
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let flow = gtk::FlowBox::default();
        let scroller = gtk::ScrolledWindow::default();
        let menu = gtk::Popover::new();
        menu.set_has_arrow(false);
        menu.set_position(gtk::PositionType::Bottom);

        let model = AttachmentDrawer {
            items: Vec::new(),
            display_order: Vec::new(),
            list_view: init.state.list_view,
            sort_desc: init.state.sort_desc,
            thumb: init.state.thumb.clamp(MIN_THUMB as i32, MAX_THUMB as i32),
            height: init.state.height.max(MIN_HEIGHT),
            collapsed: init.state.collapsed,
            flow: flow.clone(),
            menu: menu.clone(),
            // The root of this component IS the Paned.
            paned: root.clone(),
            adjusting: Rc::new(Cell::new(false)),
            positioned: Rc::new(Cell::new(false)),
        };

        let widgets = view_output!();
        // Dock the reader as the top pane (the drawer body is the bottom pane).
        root.set_start_child(Some(&init.reader));
        model.menu.set_parent(&model.flow);
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            AttachmentDrawerInput::SetItems(items) => {
                let became_visible = self.items.is_empty() && !items.is_empty();
                self.items = items;
                self.rebuild(&sender);
                // Apply the remembered split the first time the drawer appears
                // (the Paned is allocated only once both children are visible).
                if became_visible && !self.positioned.get() {
                    self.schedule_initial_position();
                }
            }
            AttachmentDrawerInput::SetThumbSize(size) => {
                // Thumbnail size is a live setting only — never persisted.
                let size = size.clamp(MIN_THUMB as i32, MAX_THUMB as i32);
                if size != self.thumb {
                    self.thumb = size;
                    self.rebuild(&sender);
                }
            }
            AttachmentDrawerInput::PositionChanged => {
                // Track the dragged height for this session (so expanding after a
                // collapse restores it), but don't persist it across launches.
                if self.adjusting.get() || !self.positioned.get() || self.collapsed {
                    return;
                }
                let drawer_h = self.paned.height() - self.paned.position();
                if drawer_h >= MIN_HEIGHT {
                    self.height = drawer_h;
                }
            }
            AttachmentDrawerInput::ToggleCollapsed => {
                self.collapsed = !self.collapsed;
                self.apply_position();
                config::save_drawer_collapsed(self.collapsed);
            }
            AttachmentDrawerInput::ToggleListView => {
                self.list_view = !self.list_view;
                self.rebuild(&sender);
                config::save_drawer_list_view(self.list_view);
            }
            AttachmentDrawerInput::ToggleSortOrder => {
                self.sort_desc = !self.sort_desc;
                self.rebuild(&sender);
                config::save_drawer_sort_desc(self.sort_desc);
            }
            AttachmentDrawerInput::Open(i) => {
                if let Some(att) = self.item_at(i) {
                    open_bytes(&att.name, &att.data, self.window().as_ref());
                }
            }
            AttachmentDrawerInput::Download(i) => {
                if let Some(att) = self.item_at(i).cloned() {
                    self.save_attachment(&att);
                }
            }
            AttachmentDrawerInput::Activate(i) => {
                let Some(orig) = self.display_order.get(i).copied() else { return };
                let Some(att) = self.items.get(orig) else { return };
                if is_image_name(&att.name) && texture_from(&att.data).is_some() {
                    self.show_lightbox(orig);
                } else {
                    open_bytes(&att.name, &att.data, self.window().as_ref());
                }
            }
            AttachmentDrawerInput::ContextMenu { index, x, y } => {
                self.show_context_menu(index, x, y, &sender);
            }
        }
    }
}

impl AttachmentDrawer {
    /// The top-level window this drawer lives in (for modal children / dialogs).
    fn window(&self) -> Option<gtk::Window> {
        self.flow.root().and_downcast::<gtk::Window>()
    }

    /// Natural height of the drawer's header row (the first child of the end
    /// pane) — the drawer's height when collapsed.
    fn header_height(paned: &gtk::Paned) -> i32 {
        paned
            .end_child()
            .and_then(|body| body.first_child())
            .map(|header| header.measure(gtk::Orientation::Vertical, -1).1)
            .unwrap_or(44)
            .max(1)
    }

    /// Move the Paned divider so the drawer body is the desired height: the
    /// header alone when collapsed, else `self.height`. Guarded so the resulting
    /// position-notify isn't taken for a user drag.
    fn apply_position(&self) {
        let total = self.paned.height();
        if total <= 1 {
            return; // not allocated yet
        }
        let drawer_h = if self.collapsed {
            Self::header_height(&self.paned)
        } else {
            self.height
        };
        self.adjusting.set(true);
        self.paned.set_position((total - drawer_h).max(0));
        self.adjusting.set(false);
    }

    /// Apply the remembered split once the Paned has been allocated. The drawer
    /// has just become visible, so defer one loop iteration for layout to settle.
    fn schedule_initial_position(&self) {
        let paned = self.paned.clone();
        let adjusting = self.adjusting.clone();
        let positioned = self.positioned.clone();
        let height = self.height;
        let collapsed = self.collapsed;
        glib::idle_add_local_once(move || {
            if positioned.get() {
                return;
            }
            let total = paned.height();
            if total <= 1 {
                return; // still unallocated; a later SetItems will retry
            }
            let drawer_h = if collapsed { Self::header_height(&paned) } else { height };
            adjusting.set(true);
            paned.set_position((total - drawer_h).max(0));
            adjusting.set(false);
            positioned.set(true);
        });
    }

    /// Rebuild the thumbnail grid from the current items and thumbnail size.
    fn rebuild(&mut self, sender: &ComponentSender<Self>) {
        // Remove existing cells only. The context-menu popover is also parented
        // to the flow, so skip anything that isn't a FlowBoxChild — and walk
        // siblings captured before removal so this can never loop (removing a
        // non-child is a no-op, which would spin `first_child()` forever).
        let mut child = self.flow.first_child();
        while let Some(c) = child {
            let next = c.next_sibling();
            if c.downcast_ref::<gtk::FlowBoxChild>().is_some() {
                self.flow.remove(&c);
            }
            child = next;
        }
        // The grid keeps the message's own attachment order; the list is
        // alphabetical (case-insensitive), flipped by the sort switch. Cells are
        // handed their *display* index — `item_at` maps it back to `items`.
        let mut order: Vec<usize> = (0..self.items.len()).collect();
        if self.list_view {
            order.sort_by(|&a, &b| {
                self.items[a].name.to_lowercase().cmp(&self.items[b].name.to_lowercase())
            });
            if self.sort_desc {
                order.reverse();
            }
        }
        self.display_order = order;
        for (i, &orig) in self.display_order.iter().enumerate() {
            let att = &self.items[orig];
            let cell = if self.list_view {
                build_list_row(i, att, sender)
            } else {
                build_cell(i, att, self.thumb, sender)
            };
            self.flow.insert(&cell, -1);
        }
    }

    /// The attachment shown at display position `i` (grid or sorted list).
    fn item_at(&self, i: usize) -> Option<&Attachment> {
        self.items.get(*self.display_order.get(i)?)
    }

    /// Save one attachment via a file chooser.
    fn save_attachment(&self, att: &Attachment) {
        let dialog = gtk::FileDialog::builder()
            .initial_name(&att.name)
            .title("Save Attachment")
            .build();
        let data = att.data.clone();
        dialog.save(self.window().as_ref(), gtk::gio::Cancellable::NONE, move |res| {
            if let Ok(file) = res {
                if let Some(path) = file.path() {
                    let _ = std::fs::write(path, &data);
                }
            }
        });
    }

    /// Right-click menu: Open / Download, matching the gallery's actions.
    fn show_context_menu(&self, index: usize, x: f64, y: f64, sender: &ComponentSender<Self>) {
        let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
        list.set_width_request(160);
        let item = |icon: &str, label: &str| {
            let b = gtk::Button::new();
            b.add_css_class("flat");
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            row.append(&gtk::Image::from_icon_name(icon));
            let l = gtk::Label::new(Some(label));
            l.set_halign(gtk::Align::Start);
            l.set_hexpand(true);
            row.append(&l);
            b.set_child(Some(&row));
            b
        };

        let open = item("co.hyprlab.Vireo-document-open-symbolic", "Open");
        let s = sender.clone();
        let menu = self.menu.clone();
        open.connect_clicked(move |_| {
            menu.popdown();
            s.input(AttachmentDrawerInput::Open(index));
        });
        list.append(&open);

        let download = item("co.hyprlab.Vireo-folder-download-symbolic", "Download…");
        let s = sender.clone();
        let menu = self.menu.clone();
        download.connect_clicked(move |_| {
            menu.popdown();
            s.input(AttachmentDrawerInput::Download(index));
        });
        list.append(&download);

        self.menu.set_child(Some(&list));
        self.menu
            .set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        self.menu.popup();
    }

    /// Open a modal lightbox stepping through the message's image attachments.
    fn show_lightbox(&self, start: usize) {
        // Image attachments only, paired with their bytes; prev/next cycle these.
        let images: Vec<(String, Vec<u8>)> = self
            .items
            .iter()
            .filter(|a| is_image_name(&a.name) && texture_from(&a.data).is_some())
            .map(|a| (a.name.clone(), a.data.clone()))
            .collect();
        if images.is_empty() {
            return;
        }
        // Map the activated item index to its slot among the images.
        let start_pos = self
            .items
            .iter()
            .take(start + 1)
            .filter(|a| is_image_name(&a.name) && texture_from(&a.data).is_some())
            .count()
            .saturating_sub(1);

        let win = gtk::Window::builder()
            .modal(true)
            .default_width(920)
            .default_height(720)
            .title(&images[start_pos].0)
            .build();
        if let Some(parent) = self.window() {
            win.set_transient_for(Some(&parent));
        }

        let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        outer.add_css_class("gallery-lightbox");

        // Top bar: filename + close.
        let top = gtk::CenterBox::new();
        top.add_css_class("gallery-lightbox-bar");
        let title = gtk::Label::new(Some(&images[start_pos].0));
        title.add_css_class("gallery-lightbox-title");
        title.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        top.set_center_widget(Some(&title));
        let close = gtk::Button::from_icon_name("co.hyprlab.Vireo-window-close-symbolic");
        close.add_css_class("flat");
        close.add_css_class("circular");
        close.set_tooltip_text(Some("Close"));
        let w = win.clone();
        close.connect_clicked(move |_| w.close());
        top.set_end_widget(Some(&close));
        outer.append(&top);

        // Middle: prev · picture · next.
        let mid = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        mid.set_vexpand(true);
        let prev = nav_button("co.hyprlab.Vireo-go-previous-symbolic", "Previous");
        let picture = gtk::Picture::new();
        picture.set_hexpand(true);
        picture.set_vexpand(true);
        picture.set_content_fit(gtk::ContentFit::Contain);
        let next = nav_button("co.hyprlab.Vireo-go-next-symbolic", "Next");
        let multi = images.len() > 1;
        prev.set_visible(multi);
        next.set_visible(multi);
        mid.append(&prev);
        mid.append(&picture);
        mid.append(&next);
        outer.append(&mid);

        // Bottom bar: caption + Open / Download.
        let bottom = gtk::CenterBox::new();
        bottom.add_css_class("gallery-lightbox-bar");
        let caption = gtk::Label::new(None);
        caption.add_css_class("dim-label");
        bottom.set_center_widget(Some(&caption));
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let open_btn = gtk::Button::from_icon_name("co.hyprlab.Vireo-document-open-symbolic");
        open_btn.add_css_class("flat");
        open_btn.set_tooltip_text(Some("Open"));
        let download_btn = gtk::Button::from_icon_name("co.hyprlab.Vireo-folder-download-symbolic");
        download_btn.add_css_class("flat");
        download_btn.set_tooltip_text(Some("Download…"));
        actions.append(&open_btn);
        actions.append(&download_btn);
        bottom.set_end_widget(Some(&actions));
        outer.append(&bottom);

        win.set_child(Some(&outer));

        // Shared cursor into `images`; render() repaints the picture + labels.
        let pos = Rc::new(Cell::new(start_pos));
        let images = Rc::new(images);
        let render = {
            let images = images.clone();
            let pos = pos.clone();
            let picture = picture.clone();
            let title = title.clone();
            let caption = caption.clone();
            let win = win.clone();
            move || {
                let (name, data) = &images[pos.get()];
                if let Some(tex) = texture_from(data) {
                    picture.set_paintable(Some(&tex));
                }
                title.set_text(name);
                caption.set_text(&format!(
                    "{} · {} of {}",
                    name,
                    pos.get() + 1,
                    images.len()
                ));
                win.set_title(Some(name));
            }
        };

        let step = {
            let images = images.clone();
            let pos = pos.clone();
            let render = render.clone();
            move |delta: i32| {
                let n = images.len() as i32;
                let cur = pos.get() as i32;
                pos.set((((cur + delta) % n + n) % n) as usize);
                render();
            }
        };

        {
            let step = step.clone();
            prev.connect_clicked(move |_| step(-1));
        }
        {
            let step = step.clone();
            next.connect_clicked(move |_| step(1));
        }

        // Escape closes; ←/→ step (when there's more than one image).
        let keys = gtk::EventControllerKey::new();
        let w = win.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            match key {
                gtk::gdk::Key::Escape => {
                    w.close();
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::Left => {
                    step(-1);
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::Right => {
                    step(1);
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        win.add_controller(keys);

        {
            let images = images.clone();
            let pos = pos.clone();
            let win = win.clone();
            open_btn.connect_clicked(move |_| {
                let (name, data) = &images[pos.get()];
                open_bytes(name, data, Some(&win));
            });
        }
        {
            let images = images.clone();
            let pos = pos.clone();
            let win = win.clone();
            download_btn.connect_clicked(move |_| {
                let (name, data) = &images[pos.get()];
                let dialog = gtk::FileDialog::builder()
                    .initial_name(name)
                    .title("Save Attachment")
                    .build();
                let data = data.clone();
                dialog.save(Some(&win), gtk::gio::Cancellable::NONE, move |res| {
                    if let Ok(file) = res {
                        if let Some(path) = file.path() {
                            let _ = std::fs::write(path, &data);
                        }
                    }
                });
            });
        }

        render();
        win.present();
    }
}

/// A circular nav button for the lightbox's prev/next.
fn nav_button(icon: &str, tip: &str) -> gtk::Button {
    let b = gtk::Button::from_icon_name(icon);
    b.add_css_class("circular");
    b.add_css_class("osd");
    b.set_valign(gtk::Align::Center);
    b.set_tooltip_text(Some(tip));
    b
}

/// Build one grid cell: a square thumbnail (image/PDF texture or type icon)
/// with hover Download/Open actions and the filename beneath it.
fn build_cell(
    index: usize,
    att: &Attachment,
    thumb: i32,
    sender: &ComponentSender<AttachmentDrawer>,
) -> gtk::FlowBoxChild {
    let cell = gtk::Box::new(gtk::Orientation::Vertical, 4);
    cell.add_css_class("drawer-cell");
    cell.set_halign(gtk::Align::Center);

    // The thumbnail content: a cover-cropped image/PDF-page render, or a
    // centred type icon. A PDF not yet in the render cache gets a spinner
    // while a worker renders its page — never a frozen window.
    let content: gtk::Widget = match thumbnail_texture(&att.name, &att.data) {
        Thumbnail::Ready(tex) => drawer_picture(&tex),
        Thumbnail::Fallback => drawer_icon(&att.name, thumb),
        Thumbnail::Pending => {
            let holder = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            holder.set_hexpand(true);
            holder.set_vexpand(true);
            holder.append(&crate::ui::attachments_gallery::thumbnail_spinner());
            let weak = holder.downgrade();
            let name = att.name.clone();
            spawn_pdf_render(att.data.clone(), move |tex| {
                let Some(holder) = weak.upgrade() else { return };
                while let Some(child) = holder.first_child() {
                    holder.remove(&child);
                }
                match tex {
                    Some(tex) => holder.append(&drawer_picture(&tex)),
                    None => holder.append(&drawer_icon(&name, thumb)),
                }
            });
            holder.upcast()
        }
    };

    // Force a fixed 1:1 square regardless of the image's intrinsic size, so cells
    // stay uniform and the grid flows left-to-right, wrapping into rows.
    let square = SquareBox::new(&content, thumb);
    square.set_overflow(gtk::Overflow::Hidden);
    square.add_css_class("drawer-thumb");

    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&square));

    // Hover quick actions (Download, Open) — same style as the gallery.
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    actions.set_halign(gtk::Align::End);
    actions.set_valign(gtk::Align::End);
    actions.set_margin_end(6);
    actions.set_margin_bottom(6);
    let action_btn = |icon: &str, tip: &str| {
        let b = gtk::Button::from_icon_name(icon);
        b.add_css_class("gallery-open");
        b.add_css_class("drawer-action");
        b.add_css_class("circular");
        b.add_css_class("osd");
        b.set_tooltip_text(Some(tip));
        b
    };
    let download = action_btn("co.hyprlab.Vireo-folder-download-symbolic", "Download");
    let s = sender.clone();
    download.connect_clicked(move |_| s.input(AttachmentDrawerInput::Download(index)));
    actions.append(&download);
    let open = action_btn("co.hyprlab.Vireo-document-open-symbolic", "Open");
    let s = sender.clone();
    open.connect_clicked(move |_| s.input(AttachmentDrawerInput::Open(index)));
    actions.append(&open);
    overlay.add_overlay(&actions);

    cell.append(&overlay);

    let name = gtk::Label::new(Some(&att.name));
    name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    name.set_max_width_chars(1);
    name.set_width_request(thumb);
    name.set_justify(gtk::Justification::Center);
    name.set_tooltip_text(Some(&att.name));
    name.add_css_class("drawer-filename");
    name.add_css_class("caption");
    cell.append(&name);

    let child = gtk::FlowBoxChild::new();
    child.set_child(Some(&cell));
    // The full filename on hover anywhere in the cell — the label below the
    // thumbnail is ellipsized, so the thumbnail itself must answer too.
    child.set_tooltip_text(Some(&att.name));

    // Right-click → context menu at the click point.
    let right = gtk::GestureClick::new();
    right.set_button(gtk::gdk::BUTTON_SECONDARY);
    let s = sender.clone();
    right.connect_pressed(move |_, _, x, y| {
        s.input(AttachmentDrawerInput::ContextMenu { index, x, y });
    });
    child.add_controller(right);

    child
}

/// One row of the drawer's list view: type icon, filename, size, and the same
/// Download/Open quick actions the grid shows on hover. Activation (click) and
/// the right-click context menu match the grid's behaviour.
fn build_list_row(
    index: usize,
    att: &Attachment,
    sender: &ComponentSender<AttachmentDrawer>,
) -> gtk::FlowBoxChild {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.add_css_class("drawer-list-row");

    let icon = gtk::Image::from_icon_name(icon_for(&att.name));
    icon.set_pixel_size(20);
    icon.add_css_class(icon_color_class(&att.name));
    row.append(&icon);

    let name = gtk::Label::new(Some(&att.name));
    name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    name.set_halign(gtk::Align::Start);
    name.set_hexpand(true);
    name.set_xalign(0.0);
    row.append(&name);

    let size = gtk::Label::new(Some(&att.human_size()));
    size.add_css_class("dim-label");
    size.add_css_class("caption");
    size.set_valign(gtk::Align::Center);
    row.append(&size);

    let action_btn = |icon: &str, tip: &str| {
        let b = gtk::Button::from_icon_name(icon);
        b.add_css_class("flat");
        b.set_valign(gtk::Align::Center);
        b.set_tooltip_text(Some(tip));
        b
    };
    let download = action_btn("co.hyprlab.Vireo-folder-download-symbolic", "Download");
    let s = sender.clone();
    download.connect_clicked(move |_| s.input(AttachmentDrawerInput::Download(index)));
    row.append(&download);
    let open = action_btn("co.hyprlab.Vireo-document-open-symbolic", "Open");
    let s = sender.clone();
    open.connect_clicked(move |_| s.input(AttachmentDrawerInput::Open(index)));
    row.append(&open);

    let child = gtk::FlowBoxChild::new();
    child.set_child(Some(&row));
    child.set_tooltip_text(Some(&att.name));

    let right = gtk::GestureClick::new();
    right.set_button(gtk::gdk::BUTTON_SECONDARY);
    let s = sender.clone();
    right.connect_pressed(move |_, _, x, y| {
        s.input(AttachmentDrawerInput::ContextMenu { index, x, y });
    });
    child.add_controller(right);

    child
}

glib::wrapper! {
    /// A single-child container that always measures to a fixed N×N square,
    /// ignoring the child's intrinsic size. This keeps every thumbnail uniformly
    /// sized (so images don't blow the cell out to their native pixel width) and
    /// lets the grid pack many per row, flowing left-to-right.
    pub struct SquareBox(ObjectSubclass<imp::SquareBox>) @extends gtk::Widget;
}

impl SquareBox {
    fn new(child: &impl IsA<gtk::Widget>, size: i32) -> Self {
        use gtk::subclass::prelude::ObjectSubclassIsExt;
        let obj: Self = glib::Object::new();
        obj.imp().size.set(size);
        child.set_parent(&obj);
        obj
    }
}

mod imp {
    use std::cell::Cell;

    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;

    #[derive(Default)]
    pub struct SquareBox {
        pub size: Cell<i32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SquareBox {
        const NAME: &'static str = "VireoSquareBox";
        type Type = super::SquareBox;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for SquareBox {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for SquareBox {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::ConstantSize
        }

        fn measure(&self, _orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            let s = self.size.get().max(1);
            (s, s, -1, -1)
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            if let Some(child) = self.obj().first_child() {
                child.allocate(width, height, baseline, None);
            }
        }
    }
}
