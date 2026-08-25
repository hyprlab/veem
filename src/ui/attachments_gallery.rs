//! Attachments gallery: a grid of every cached attachment across the connected
//! inboxes, with a lightbox preview (prev/next, open, go to message).
//!
//! Data comes from the SQLite cache (what the background prefetch has already
//! downloaded), fed in via [`GalleryInput::SetItems`]. Image attachments show as
//! thumbnails; other files show a type icon. Clicking a cell opens a large
//! overlay preview.

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use relm4::prelude::*;

use crate::models::GalleryItem;

pub struct AttachmentsGallery {
    /// Full set of attachments, unfiltered — the source for search and sort.
    all_items: Vec<GalleryItem>,
    /// Indices into `all_items`, filtered by `query` and ordered by `sort`: the
    /// list actually shown in the grid and stepped through in the lightbox.
    items: Vec<usize>,
    query: String,
    sort: SortBy,
    /// Index into `items` currently shown in the lightbox, if any.
    preview: Option<usize>,
    loading: bool,
    flow: gtk::FlowBox,
    /// Reusable right-click context menu, parented to the grid.
    menu: gtk::Popover,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum SortBy {
    #[default]
    Newest,
    Oldest,
    Name,
    NameDesc,
    Sender,
    SenderDesc,
    Largest,
    Smallest,
    Type,
    TypeDesc,
}

impl SortBy {
    /// Map the sort dropdown's selected row to a criterion. The order must match
    /// the `StringList` built in the view.
    fn from_index(i: u32) -> SortBy {
        match i {
            1 => SortBy::Oldest,
            2 => SortBy::Name,
            3 => SortBy::NameDesc,
            4 => SortBy::Sender,
            5 => SortBy::SenderDesc,
            6 => SortBy::Largest,
            7 => SortBy::Smallest,
            8 => SortBy::Type,
            9 => SortBy::TypeDesc,
            _ => SortBy::Newest,
        }
    }
}

#[derive(Debug)]
pub enum GalleryInput {
    /// Replace the gallery contents (already merged across accounts).
    SetItems(Vec<GalleryItem>),
    SetLoading(bool),
    /// Filter the grid to items matching this search text (sender, subject,
    /// filename, folder, and type keywords like "pdf" or "spreadsheet").
    SetQuery(String),
    /// Re-sort the grid; the value is the sort dropdown's selected row index.
    SetSort(u32),
    /// A grid cell was activated (single click) — open the lightbox on that item.
    Activate(u32),
    Prev,
    Next,
    ClosePreview,
    /// Open the current (previewed) item's file in its default application.
    OpenCurrent,
    /// Jump to the current (previewed) item's source message.
    GoToCurrent,
    /// Open item `index` externally (double-click / context menu / lightbox).
    OpenItem(usize),
    /// Save item `index` to a file the user picks.
    DownloadItem(usize),
    /// Jump to item `index`'s source message.
    GoToItem(usize),
    /// A cell was double-clicked: open it externally, closing any preview.
    OpenExternal(usize),
    /// Right-click on cell `index` at `(x, y)` (cell-relative) — show its menu there.
    ContextMenu { index: usize, x: f64, y: f64 },
}

#[derive(Debug)]
pub enum GalleryOutput {
    /// Open the source message of an attachment in the reader.
    OpenMessage { account_id: u32, folder_path: String, uid: u32 },
}

#[relm4::component(pub)]
impl Component for AttachmentsGallery {
    type Init = ();
    type Input = GalleryInput;
    type Output = GalleryOutput;
    type CommandOutput = ();

    view! {
        gtk::Overlay {
            add_css_class: "attachments-gallery",

            // Base layer: a search/sort toolbar above the scrolling grid.
            #[wrap(Some)]
            set_child = &gtk::Box {
                set_orientation: gtk::Orientation::Vertical,

                gtk::Box {
                    add_css_class: "gallery-toolbar",
                    set_spacing: 8,
                    #[watch]
                    set_visible: !model.all_items.is_empty(),

                    gtk::SearchEntry {
                        set_hexpand: true,
                        set_placeholder_text: Some("Search by sender, subject, type, filename…"),
                        connect_search_changed[sender] => move |e| {
                            sender.input(GalleryInput::SetQuery(e.text().to_string()));
                        },
                    },
                    gtk::DropDown {
                        set_tooltip_text: Some("Sort"),
                        #[wrap(Some)]
                        set_model = &gtk::StringList::new(&[
                            "Newest first",
                            "Oldest first",
                            "Name (A–Z)",
                            "Name (Z–A)",
                            "Sender (A–Z)",
                            "Sender (Z–A)",
                            "Largest first",
                            "Smallest first",
                            "Type (A–Z)",
                            "Type (Z–A)",
                        ]),
                        connect_selected_notify[sender] => move |d| {
                            sender.input(GalleryInput::SetSort(d.selected()));
                        },
                    },
                },

                gtk::Stack {
                    set_vexpand: true,
                    set_transition_type: gtk::StackTransitionType::Crossfade,
                    #[watch]
                    set_visible_child_name: model.page(),

                    add_named[Some("loading")] = &gtk::Box {
                        set_halign: gtk::Align::Center,
                        set_valign: gtk::Align::Center,
                        set_orientation: gtk::Orientation::Vertical,
                        set_spacing: 14,
                        gtk::Spinner { set_spinning: true, set_width_request: 36, set_height_request: 36 },
                        gtk::Label { set_label: "Loading attachments…", add_css_class: "dim-label" },
                    },

                    add_named[Some("empty")] = &adw::StatusPage {
                        set_icon_name: Some("co.hyprlab.Vireo-mail-attachment-symbolic"),
                        set_title: "No attachments",
                        set_description: Some("Attachments from your inboxes will appear here."),
                    },

                    add_named[Some("noresults")] = &adw::StatusPage {
                        set_icon_name: Some("co.hyprlab.Vireo-system-search-symbolic"),
                        set_title: "No matching attachments",
                        set_description: Some("Try a different search or clear the filter."),
                    },

                    add_named[Some("grid")] = &gtk::ScrolledWindow {
                    set_hscrollbar_policy: gtk::PolicyType::Never,
                    set_vexpand: true,

                    #[local_ref]
                    flow -> gtk::FlowBox {
                        set_valign: gtk::Align::Start,
                        set_max_children_per_line: 8,
                        set_min_children_per_line: 3,
                        set_row_spacing: 14,
                        set_column_spacing: 14,
                        set_homogeneous: true,
                        set_selection_mode: gtk::SelectionMode::None,
                        set_activate_on_single_click: true,
                        add_css_class: "gallery-flow",
                        connect_child_activated[sender] => move |_, child| {
                            sender.input(GalleryInput::Activate(child.index() as u32));
                        },
                    },
                    },
                },
            },

            // Lightbox overlay, shown while previewing an item.
            add_overlay = &gtk::Box {
                add_css_class: "gallery-lightbox",
                set_orientation: gtk::Orientation::Vertical,
                #[watch]
                set_visible: model.preview.is_some(),

                // Top bar: title + close.
                gtk::CenterBox {
                    add_css_class: "gallery-lightbox-bar",
                    #[wrap(Some)]
                    set_start_widget = &gtk::Label {
                        #[watch]
                        set_label: &model.current().map(|i| i.name.clone()).unwrap_or_default(),
                        set_ellipsize: gtk::pango::EllipsizeMode::Middle,
                        set_halign: gtk::Align::Start,
                        add_css_class: "gallery-lightbox-title",
                    },
                    #[wrap(Some)]
                    set_end_widget = &gtk::Button {
                        set_icon_name: "co.hyprlab.Vireo-window-close-symbolic",
                        set_tooltip_text: Some("Close"),
                        add_css_class: "circular",
                        add_css_class: "flat",
                        connect_clicked => GalleryInput::ClosePreview,
                    },
                },

                // Middle: prev  |  image/icon  |  next.
                gtk::Box {
                    set_orientation: gtk::Orientation::Horizontal,
                    set_vexpand: true,
                    set_spacing: 8,

                    gtk::Button {
                        set_icon_name: "co.hyprlab.Vireo-go-previous-symbolic",
                        set_tooltip_text: Some("Previous"),
                        set_valign: gtk::Align::Center,
                        add_css_class: "circular",
                        add_css_class: "osd",
                        #[watch]
                        set_sensitive: model.items.len() > 1,
                        connect_clicked => GalleryInput::Prev,
                    },

                    #[name = "preview_stack"]
                    gtk::Stack {
                        set_hexpand: true,
                        set_vexpand: true,
                        #[watch]
                        set_visible_child_name: if model.current().is_some_and(|i| i.is_image() && i.data.is_some()) { "image" } else { "file" },

                        #[name = "preview_picture"]
                        add_named[Some("image")] = &gtk::Picture {
                            set_can_shrink: true,
                            set_content_fit: gtk::ContentFit::Contain,
                        },

                        add_named[Some("file")] = &gtk::Box {
                            set_orientation: gtk::Orientation::Vertical,
                            set_halign: gtk::Align::Center,
                            set_valign: gtk::Align::Center,
                            set_spacing: 12,
                            gtk::Image {
                                #[watch]
                                set_icon_name: model.current().map(|i| icon_for(&i.name)),
                                set_pixel_size: 96,
                                #[watch]
                                set_css_classes: &[
                                    "gallery-file-icon",
                                    model.current().map(|i| icon_color_class(&i.name)).unwrap_or("ftype-generic"),
                                ],
                            },
                            gtk::Label {
                                #[watch]
                                set_label: &model.current().map(|i| i.name.clone()).unwrap_or_default(),
                                set_ellipsize: gtk::pango::EllipsizeMode::Middle,
                                add_css_class: "title-3",
                            },
                        },
                    },

                    gtk::Button {
                        set_icon_name: "co.hyprlab.Vireo-go-next-symbolic",
                        set_tooltip_text: Some("Next"),
                        set_valign: gtk::Align::Center,
                        add_css_class: "circular",
                        add_css_class: "osd",
                        #[watch]
                        set_sensitive: model.items.len() > 1,
                        connect_clicked => GalleryInput::Next,
                    },
                },

                // Bottom bar: caption + actions.
                gtk::CenterBox {
                    add_css_class: "gallery-lightbox-bar",
                    #[wrap(Some)]
                    set_start_widget = &gtk::Label {
                        #[watch]
                        set_label: &model.current().map(caption).unwrap_or_default(),
                        set_halign: gtk::Align::Start,
                        set_ellipsize: gtk::pango::EllipsizeMode::End,
                        add_css_class: "dim-label",
                    },
                    #[wrap(Some)]
                    set_end_widget = &gtk::Box {
                        set_spacing: 8,
                        gtk::Button {
                            set_label: "Open",
                            set_tooltip_text: Some("Open in the default app"),
                            #[watch]
                            set_sensitive: model.current().is_some_and(|i| i.data.is_some()),
                            connect_clicked => GalleryInput::OpenCurrent,
                        },
                        gtk::Button {
                            set_label: "Go to Message",
                            add_css_class: "suggested-action",
                            connect_clicked => GalleryInput::GoToCurrent,
                        },
                    },
                },
            },
        }
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let menu = gtk::Popover::new();
        menu.set_has_arrow(false);
        menu.set_position(gtk::PositionType::Bottom);
        menu.add_css_class("menu");
        let model = AttachmentsGallery {
            all_items: Vec::new(),
            items: Vec::new(),
            query: String::new(),
            sort: SortBy::default(),
            preview: None,
            loading: false,
            flow: gtk::FlowBox::new(),
            menu,
        };
        let flow = &model.flow;
        let widgets = view_output!();
        // Parent the context menu to the gallery root (not the FlowBox, whose
        // children must be FlowBoxChild and which we clear on every rebuild).
        model.menu.set_parent(&root);

        // Arrow keys navigate the lightbox; Escape closes it.
        let key = gtk::EventControllerKey::new();
        let ks = sender.clone();
        key.connect_key_pressed(move |_, keyval, _, _| {
            match keyval {
                gdk::Key::Left => ks.input(GalleryInput::Prev),
                gdk::Key::Right => ks.input(GalleryInput::Next),
                gdk::Key::Escape => ks.input(GalleryInput::ClosePreview),
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
        root.add_controller(key);

        ComponentParts { model, widgets }
    }

    fn update_with_view(
        &mut self,
        widgets: &mut Self::Widgets,
        msg: Self::Input,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match msg {
            GalleryInput::SetItems(items) => {
                self.all_items = items;
                self.loading = false;
                self.preview = None;
                self.apply();
                self.rebuild_grid(&sender);
            }
            GalleryInput::SetQuery(q) => {
                self.query = q;
                self.preview = None;
                self.apply();
                self.rebuild_grid(&sender);
            }
            GalleryInput::SetSort(i) => {
                self.sort = SortBy::from_index(i);
                self.preview = None;
                self.apply();
                self.rebuild_grid(&sender);
            }
            GalleryInput::SetLoading(on) => self.loading = on,
            GalleryInput::Activate(i) => {
                if (i as usize) < self.items.len() {
                    self.preview = Some(i as usize);
                    self.load_preview_image(widgets);
                }
            }
            GalleryInput::Prev => self.step(-1, widgets),
            GalleryInput::Next => self.step(1, widgets),
            GalleryInput::ClosePreview => self.preview = None,
            GalleryInput::OpenCurrent => {
                if let Some(i) = self.preview {
                    self.open_item(i);
                }
            }
            GalleryInput::GoToCurrent => {
                if let Some(i) = self.preview {
                    self.goto_item(i, &sender);
                }
            }
            GalleryInput::OpenItem(i) => self.open_item(i),
            GalleryInput::DownloadItem(i) => self.download_item(i),
            GalleryInput::GoToItem(i) => self.goto_item(i, &sender),
            GalleryInput::OpenExternal(i) => {
                // Double-click: skip/close the preview and open the file directly.
                self.preview = None;
                self.open_item(i);
            }
            GalleryInput::ContextMenu { index, x, y } => {
                self.show_context_menu(index, x, y, &sender)
            }
        }
        self.update_view(widgets, sender);
    }
}

impl AttachmentsGallery {
    fn page(&self) -> &'static str {
        if self.loading && self.all_items.is_empty() {
            "loading"
        } else if self.all_items.is_empty() {
            "empty"
        } else if self.items.is_empty() {
            "noresults"
        } else {
            "grid"
        }
    }

    /// Recompute the displayed `items` (indices into `all_items`) from the
    /// current search `query` and `sort`.
    fn apply(&mut self) {
        let query = self.query.to_ascii_lowercase();
        let tokens: Vec<&str> = query.split_whitespace().collect();
        let mut items: Vec<usize> = self
            .all_items
            .iter()
            .enumerate()
            .filter(|(_, it)| {
                if tokens.is_empty() {
                    return true;
                }
                let hay = item_haystack(it);
                tokens.iter().all(|t| hay.contains(t))
            })
            .map(|(i, _)| i)
            .collect();
        sort_indices(&mut items, &self.all_items, self.sort);
        self.items = items;
    }

    /// The `GalleryItem` at display position `display` (mapping through the
    /// filtered/sorted `items` into `all_items`).
    fn item_at(&self, display: usize) -> Option<&GalleryItem> {
        self.items.get(display).and_then(|&i| self.all_items.get(i))
    }

    fn current(&self) -> Option<&GalleryItem> {
        self.preview.and_then(|i| self.item_at(i))
    }

    fn step(&mut self, delta: i32, widgets: &mut AttachmentsGalleryWidgets) {
        if self.items.is_empty() {
            return;
        }
        if let Some(i) = self.preview {
            let n = self.items.len() as i32;
            self.preview = Some((((i as i32 + delta) % n + n) % n) as usize);
            self.load_preview_image(widgets);
        }
    }

    /// Point the lightbox Picture at the current image's bytes (if it is one).
    fn load_preview_image(&self, widgets: &AttachmentsGalleryWidgets) {
        let texture = self
            .current()
            .filter(|i| i.is_image())
            .and_then(|i| i.data.as_ref())
            .and_then(|d| texture_from(d));
        widgets.preview_picture.set_paintable(texture.as_ref());
    }

    fn rebuild_grid(&mut self, sender: &ComponentSender<Self>) {
        // Remove existing cells; only FlowBoxChild children (not, say, a popover
        // that happens to be parented nearby).
        let mut child = self.flow.first_child();
        while let Some(c) = child {
            let next = c.next_sibling();
            if c.downcast_ref::<gtk::FlowBoxChild>().is_some() {
                self.flow.remove(&c);
            }
            child = next;
        }
        for display in 0..self.items.len() {
            if let Some(item) = self.item_at(display) {
                self.flow.append(&build_cell(display, item, sender));
            }
        }
    }

    /// Open item `index` in its default application (if its bytes are cached).
    fn open_item(&self, index: usize) {
        if let Some(item) = self.item_at(index) {
            if let Some(data) = &item.data {
                let parent = self.flow.root().and_downcast::<gtk::Window>();
                open_bytes(&item.name, data, parent.as_ref());
            }
        }
    }

    /// Save item `index` to a file the user chooses.
    fn download_item(&self, index: usize) {
        let Some(item) = self.item_at(index) else { return };
        let Some(data) = item.data.clone() else { return };
        let dialog = gtk::FileDialog::builder()
            .title("Save Attachment")
            .initial_name(&item.name)
            .modal(true)
            .build();
        let parent = self.flow.root().and_downcast::<gtk::Window>();
        dialog.save(parent.as_ref(), gtk::gio::Cancellable::NONE, move |res| {
            if let Ok(file) = res {
                if let Some(path) = file.path() {
                    if let Err(e) = std::fs::write(&path, &data) {
                        tracing::warn!("could not save attachment: {e}");
                    }
                }
            }
        });
    }

    fn goto_item(&mut self, index: usize, sender: &ComponentSender<Self>) {
        if let Some(item) = self.item_at(index) {
            let _ = sender.output(GalleryOutput::OpenMessage {
                account_id: item.account_id,
                folder_path: item.folder_path.clone(),
                uid: item.uid,
            });
            self.preview = None;
        }
    }

    /// Pop up the right-click menu (Download / Open / Go to Message) at the click
    /// point `(x, y)` (relative to cell `index`). Download/Open are only enabled
    /// when the file's bytes are cached.
    fn show_context_menu(&self, index: usize, x: f64, y: f64, sender: &ComponentSender<Self>) {
        let Some(item) = self.item_at(index) else { return };
        let has_data = item.data.is_some();

        let menu = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let item_btn = |label: &str, enabled: bool| {
            let b = gtk::Button::with_label(label);
            b.add_css_class("flat");
            b.set_sensitive(enabled);
            let child = b.child().and_downcast::<gtk::Label>();
            if let Some(l) = child {
                l.set_xalign(0.0);
                l.set_halign(gtk::Align::Start);
            }
            b
        };

        let download = item_btn("Download…", has_data);
        let open = item_btn("Open", has_data);
        let goto = item_btn("Go to Message", true);
        for b in [&download, &open, &goto] {
            menu.append(b);
        }
        let s = sender.clone();
        download.connect_clicked(move |_| s.input(GalleryInput::DownloadItem(index)));
        let s = sender.clone();
        open.connect_clicked(move |_| s.input(GalleryInput::OpenItem(index)));
        let s = sender.clone();
        goto.connect_clicked(move |_| s.input(GalleryInput::GoToItem(index)));
        // Close the popover on any choice.
        let pop = self.menu.clone();
        for b in [&download, &open, &goto] {
            let p = pop.clone();
            b.connect_clicked(move |_| p.popdown());
        }

        self.menu.set_child(Some(&menu));
        // Point at the click position, translated from the cell into the menu
        // parent's coordinate space, so the menu opens under the pointer.
        if let (Some(child), Some(parent)) =
            (self.flow.child_at_index(index as i32), self.menu.parent())
        {
            let point = gtk::graphene::Point::new(x as f32, y as f32);
            if let Some(p) = child.compute_point(&parent, &point) {
                self.menu.set_pointing_to(Some(&gtk::gdk::Rectangle::new(
                    p.x() as i32,
                    p.y() as i32,
                    1,
                    1,
                )));
            }
        }
        self.menu.popup();
    }
}

/// One grid cell: a thumbnail (image) or type icon, plus name + size.
fn build_cell(
    index: usize,
    item: &GalleryItem,
    sender: &ComponentSender<AttachmentsGallery>,
) -> gtk::Widget {
    let cell = gtk::Box::new(gtk::Orientation::Vertical, 6);
    cell.add_css_class("gallery-cell");
    cell.set_hexpand(true);
    cell.set_halign(gtk::Align::Fill);
    cell.set_tooltip_text(Some(&format!("{} — {}", item.name, item.human_size())));

    let thumb_holder = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    thumb_holder.add_css_class("gallery-thumb");
    thumb_holder.set_halign(gtk::Align::Fill);
    thumb_holder.set_valign(gtk::Align::Fill);

    let thumb = item
        .is_image()
        .then_some(item.data.as_ref())
        .flatten()
        .and_then(|d| texture_from(d));
    match thumb {
        Some(tex) => {
            let pic = gtk::Picture::for_paintable(&tex);
            pic.set_content_fit(gtk::ContentFit::Cover);
            pic.set_hexpand(true);
            pic.set_vexpand(true);
            pic.add_css_class("gallery-thumb-image");
            thumb_holder.append(&pic);
        }
        None => {
            let img = gtk::Image::from_icon_name(icon_for(&item.name));
            img.set_pixel_size(56);
            img.set_hexpand(true);
            img.add_css_class("gallery-file-icon");
            img.add_css_class(icon_color_class(&item.name));
            thumb_holder.append(&img);
        }
    }

    // Lock the thumbnail section to a 4:3 aspect ratio; its width tracks the
    // (responsive) column width and the height follows, filling the cell.
    let aspect = RatioBox::new(&thumb_holder);
    // Preferred column width — the FlowBox packs at least 3 of these per row and
    // adds more as the window widens (up to max-children-per-line).
    aspect.set_width_request(230);
    aspect.set_hexpand(true);

    // Overlay quick-action buttons at the thumbnail's bottom-right corner; they
    // fade in on hover (via CSS). Download and Open need the file's bytes cached;
    // "Go to Message" always works, so it shows even for uncached attachments.
    let thumb_overlay = gtk::Overlay::new();
    thumb_overlay.set_child(Some(&aspect));

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    actions.set_halign(gtk::Align::End);
    actions.set_valign(gtk::Align::End);
    actions.set_margin_end(6);
    actions.set_margin_bottom(6);
    let action_btn = |icon: &str, tip: &str| {
        let b = gtk::Button::from_icon_name(icon);
        b.add_css_class("gallery-open");
        b.add_css_class("circular");
        b.add_css_class("osd");
        b.set_tooltip_text(Some(tip));
        b
    };
    if item.data.is_some() {
        let download = action_btn("co.hyprlab.Vireo-folder-download-symbolic", "Download");
        let s = sender.clone();
        download.connect_clicked(move |_| s.input(GalleryInput::DownloadItem(index)));
        actions.append(&download);

        let open = action_btn("co.hyprlab.Vireo-document-open-symbolic", "Open");
        let s = sender.clone();
        open.connect_clicked(move |_| s.input(GalleryInput::OpenItem(index)));
        actions.append(&open);
    }
    let goto = action_btn("co.hyprlab.Vireo-mail-unread-symbolic", "Go to Message");
    let s = sender.clone();
    goto.connect_clicked(move |_| s.input(GalleryInput::GoToItem(index)));
    actions.append(&goto);
    thumb_overlay.add_overlay(&actions);

    cell.append(&thumb_overlay);

    let name = gtk::Label::new(Some(&item.name));
    name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    name.set_max_width_chars(18);
    name.add_css_class("gallery-name");
    cell.append(&name);

    let from = item.from_name.trim();
    if !from.is_empty() {
        let sender = gtk::Label::new(Some(from));
        sender.set_ellipsize(gtk::pango::EllipsizeMode::End);
        sender.set_max_width_chars(18);
        sender.add_css_class("gallery-from");
        cell.append(&sender);
    }

    let subject = item.subject.trim();
    if !subject.is_empty() {
        let subj = gtk::Label::new(Some(subject));
        subj.set_ellipsize(gtk::pango::EllipsizeMode::End);
        subj.set_max_width_chars(18);
        subj.add_css_class("gallery-subject");
        subj.add_css_class("dim-label");
        cell.append(&subj);
    }

    let mut meta = vec![folder_label(&item.folder_path)];
    let date = item.date_label();
    if !date.is_empty() {
        meta.push(date);
    }
    meta.push(item.human_size());
    let sub = gtk::Label::new(Some(&meta.join(" · ")));
    sub.set_ellipsize(gtk::pango::EllipsizeMode::End);
    sub.set_max_width_chars(18);
    sub.add_css_class("gallery-size");
    sub.add_css_class("dim-label");
    cell.append(&sub);

    let child = gtk::FlowBoxChild::new();
    child.set_child(Some(&cell));

    // Right-click → context menu at the click point.
    let right = gtk::GestureClick::new();
    right.set_button(gtk::gdk::BUTTON_SECONDARY);
    let s = sender.clone();
    right.connect_pressed(move |_, _, x, y| {
        s.input(GalleryInput::ContextMenu { index, x, y });
    });
    child.add_controller(right);

    // Double-click (primary) → open externally. Single click keeps the FlowBox's
    // built-in activation (which opens the preview).
    let dbl = gtk::GestureClick::new();
    dbl.set_button(gtk::gdk::BUTTON_PRIMARY);
    let s = sender.clone();
    dbl.connect_pressed(move |_, n, _, _| {
        if n == 2 {
            s.input(GalleryInput::OpenExternal(index));
        }
    });
    child.add_controller(dbl);

    child.upcast()
}

fn caption(item: &GalleryItem) -> String {
    let who = if item.from_name.trim().is_empty() { "Unknown" } else { item.from_name.trim() };
    let subject = item.subject.trim();
    let mut parts = vec![who.to_string()];
    if !subject.is_empty() {
        parts.push(subject.to_string());
    }
    parts.push(folder_label(&item.folder_path));
    let date = item.date_label();
    if !date.is_empty() {
        parts.push(date);
    }
    parts.push(item.human_size());
    parts.join(" · ")
}

/// A friendly folder name from a mailbox path (the last path segment).
fn folder_label(path: &str) -> String {
    let name = path.rsplit(['/', '.']).next().unwrap_or(path);
    if name.eq_ignore_ascii_case("inbox") {
        "Inbox".to_string()
    } else {
        // Paths are stored as the server names them, in modified UTF-7.
        crate::mutf7::decode(name)
    }
}

/// The lowercase extension of a filename (empty when there is none).
fn ext_of(name: &str) -> String {
    name.rsplit('.').next().unwrap_or("").to_ascii_lowercase()
}

/// Searchable category words for a file, keyed off its extension, so a query
/// like "image" or "spreadsheet" matches even when the word isn't in the name.
fn type_keywords(name: &str) -> &'static str {
    match ext_of(name).as_str() {
        "pdf" => "pdf document",
        "doc" | "docx" | "odt" | "rtf" => "word document",
        "xls" | "xlsx" | "ods" | "csv" => "excel spreadsheet",
        "ppt" | "pptx" | "odp" => "powerpoint presentation slides",
        "zip" | "gz" | "tar" | "7z" | "rar" | "xz" | "bz2" => "archive compressed",
        "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" => "audio music sound",
        "mp4" | "mov" | "mkv" | "webm" | "avi" | "m4v" => "video movie",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "heic" | "heif" | "avif" | "ico" => {
            "image photo picture"
        }
        "ics" => "calendar event",
        "txt" | "md" => "text document",
        _ => "file",
    }
}

/// Lowercase text blob a search query is matched against: filename, sender,
/// subject, folder, and type keywords.
fn item_haystack(item: &GalleryItem) -> String {
    format!(
        "{} {} {} {} {}",
        item.name,
        item.from_name,
        item.subject,
        folder_label(&item.folder_path),
        type_keywords(&item.name),
    )
    .to_ascii_lowercase()
}

/// Order the display indices (into `all`) by the chosen criterion.
fn sort_indices(idx: &mut [usize], all: &[GalleryItem], sort: SortBy) {
    use std::cmp::Reverse;
    match sort {
        SortBy::Newest => idx.sort_by_key(|&a| Reverse(all[a].timestamp)),
        SortBy::Oldest => idx.sort_by_key(|&a| all[a].timestamp),
        SortBy::Largest => idx.sort_by_key(|&a| Reverse(all[a].size)),
        SortBy::Smallest => idx.sort_by_key(|&a| all[a].size),
        SortBy::Name => idx.sort_by_key(|&a| all[a].name.to_ascii_lowercase()),
        SortBy::NameDesc => idx.sort_by_key(|&a| Reverse(all[a].name.to_ascii_lowercase())),
        SortBy::Sender => idx.sort_by_key(|&a| all[a].from_name.to_ascii_lowercase()),
        SortBy::SenderDesc => idx.sort_by_key(|&a| Reverse(all[a].from_name.to_ascii_lowercase())),
        SortBy::Type => {
            idx.sort_by_key(|&a| (ext_of(&all[a].name), all[a].name.to_ascii_lowercase()))
        }
        SortBy::TypeDesc => {
            idx.sort_by_key(|&a| Reverse((ext_of(&all[a].name), all[a].name.to_ascii_lowercase())))
        }
    }
}

/// A `gdk::Texture` from raw image bytes, or `None` if the format isn't loadable.
pub(crate) fn texture_from(data: &[u8]) -> Option<gdk::Texture> {
    gdk::Texture::from_bytes(&glib::Bytes::from(data)).ok()
}

/// A symbolic icon name for a filename by extension.
pub(crate) fn icon_for(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    match ext {
        "pdf" => "co.hyprlab.Vireo-x-office-document-symbolic",
        "doc" | "docx" | "odt" | "rtf" | "txt" | "md" => "co.hyprlab.Vireo-x-office-document-symbolic",
        "xls" | "xlsx" | "ods" | "csv" => "co.hyprlab.Vireo-x-office-spreadsheet-symbolic",
        "ppt" | "pptx" | "odp" => "co.hyprlab.Vireo-x-office-presentation-symbolic",
        "zip" | "gz" | "tar" | "7z" | "rar" | "xz" | "bz2" => "co.hyprlab.Vireo-package-x-generic-symbolic",
        "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" => "co.hyprlab.Vireo-audio-x-generic-symbolic",
        "mp4" | "mov" | "mkv" | "webm" | "avi" | "m4v" => "co.hyprlab.Vireo-video-x-generic-symbolic",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "heic" | "heif" | "avif" | "ico" => {
            "co.hyprlab.Vireo-image-x-generic-symbolic"
        }
        "ics" => "co.hyprlab.Vireo-x-office-calendar-symbolic",
        _ => "co.hyprlab.Vireo-text-x-generic-symbolic",
    }
}

/// CSS class that tints a type icon by file kind: PDFs red, Word docs blue,
/// spreadsheets green, and so on (see `styles.css`). Symbolic icons pick up the
/// class's `color`, so an unthumbnailed attachment reads at a glance.
pub(crate) fn icon_color_class(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    match ext {
        "pdf" => "ftype-pdf",
        "doc" | "docx" | "odt" | "rtf" => "ftype-doc",
        "xls" | "xlsx" | "ods" | "csv" => "ftype-sheet",
        "ppt" | "pptx" | "odp" => "ftype-slides",
        "zip" | "gz" | "tar" | "7z" | "rar" | "xz" | "bz2" => "ftype-archive",
        "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" => "ftype-audio",
        "mp4" | "mov" | "mkv" | "webm" | "avi" | "m4v" => "ftype-video",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "heic" | "heif" | "avif" | "ico" => {
            "ftype-image"
        }
        "ics" => "ftype-calendar",
        _ => "ftype-generic",
    }
}

/// Write bytes to a temp file and open it in the default application.
///
/// The filename comes from an email, so it comes from whoever sent it. The
/// sanitizer below removes every path separator, which is what stops traversal;
/// the rest of the care here is about `/tmp` being shared on a native install
/// (under Flatpak it is per-app): the directory is created private and its
/// ownership checked, and each file is created fresh rather than written
/// through whatever already sits at a guessable path.
///
/// Launched through the portal's `UriLauncher` rather than
/// `AppInfo::launch_default_for_uri`: for a type with no registered default
/// (common for attachments — nothing may ever have been "set as default" for
/// a `.pdf`) the portal falls back to GNOME's own app-chooser dialog instead
/// of silently doing nothing.
pub(crate) fn open_bytes(name: &str, data: &[u8], parent: Option<&gtk::Window>) {
    let safe: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' })
        .collect();
    let Some(dir) = attachment_dir() else { return };
    let base = if safe.is_empty() { "attachment".to_string() } else { safe };
    let Some((mut file, path)) = create_private(&dir, &base) else {
        tracing::warn!("could not open a temporary file for the attachment");
        return;
    };
    // Write through the handle rather than reopening by path: the checks below
    // are about what is at that name, and going back through it would hand the
    // result to whatever is there by the time we look again.
    if let Err(e) = std::io::Write::write_all(&mut file, data) {
        tracing::warn!("could not write the attachment: {e}");
        return;
    }
    drop(file);
    let uri = format!("file://{}", path.to_string_lossy());
    gtk::UriLauncher::new(&uri).launch(parent, gtk::gio::Cancellable::NONE, move |res| {
        // Falls back to the non-portal launch path if the portal itself is
        // unreachable or misbehaving (some sandboxes/containers, or a desktop
        // with no working `xdg-desktop-portal` backend) rather than leaving
        // the click looking like it did nothing.
        if let Err(e) = res {
            tracing::warn!("portal launch failed, falling back: {e}");
            let _ =
                gtk::gio::AppInfo::launch_default_for_uri(&uri, gtk::gio::AppLaunchContext::NONE);
        }
    });
}

/// The private directory opened attachments are staged in, created if needed.
///
/// Returns `None` if the path exists but is not a directory we own with nobody
/// else's access — an attacker who pre-creates it on a shared host would
/// otherwise get every attachment the user opens.
fn attachment_dir() -> Option<std::path::PathBuf> {
    let dir = std::env::temp_dir().join("vireo-attachments");
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
        if !dir.exists() {
            let _ = std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&dir);
        }
        // `symlink_metadata`, so a symlink pointing at somewhere friendlier is
        // seen for what it is rather than followed.
        let md = std::fs::symlink_metadata(&dir).ok()?;
        if !md.is_dir() || md.uid() != our_uid() {
            tracing::warn!("{} is not ours; not staging attachments", dir.display());
            return None;
        }
        if md.permissions().mode() & 0o077 != 0 {
            let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
        }
    }
    #[cfg(not(unix))]
    {
        let _ = std::fs::create_dir_all(&dir);
    }
    Some(dir)
}

/// Create a new 0600 file for `base` in `dir`, returning it open with its path.
///
/// `create_new` means an existing file — or a symlink planted at a guessable
/// name — makes the call fail rather than being written through, so the next
/// candidate name is tried instead. Overwriting the user's previous copy of the
/// same attachment silently would also be wrong.
fn create_private(
    dir: &std::path::Path,
    base: &str,
) -> Option<(std::fs::File, std::path::PathBuf)> {
    for n in 0..64 {
        let name = if n == 0 {
            base.to_string()
        } else {
            match base.rsplit_once('.') {
                Some((stem, ext)) if !stem.is_empty() => format!("{stem}-{n}.{ext}"),
                _ => format!("{base}-{n}"),
            }
        };
        let path = dir.join(&name);
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
            // Refuse to follow a symlink instead of racing to check for one.
            opts.custom_flags(0o400000 /* O_NOFOLLOW */);
        }
        if let Ok(file) = opts.open(&path) {
            return Some((file, path));
        }
    }
    None
}

/// This process's real user ID.
///
/// Vireo has no libc dependency; `getuid` is always available and cannot fail,
/// so declaring it directly is cheaper than taking one on.
#[cfg(unix)]
fn our_uid() -> u32 {
    extern "C" {
        fn getuid() -> u32;
    }
    unsafe { getuid() }
}

/// Delete anything left in the attachment staging directory.
///
/// Opened attachments used to accumulate in `/tmp` for the life of the machine —
/// decrypted, readable, and long past the point the user considers them gone.
/// Called once at startup, because the helper application the user opened a file
/// with may still have it open while Vireo is running.
pub fn purge_attachment_dir() {
    let dir = std::env::temp_dir().join("vireo-attachments");
    if !dir.exists() {
        return;
    }
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => tracing::debug!("cleared staged attachments in {}", dir.display()),
        Err(e) => tracing::warn!("could not clear {}: {e}", dir.display()),
    }
}

glib::wrapper! {
    /// A single-child container that forces a fixed 4:3 (width:height) aspect
    /// ratio via true height-for-width sizing, so the thumbnail fills the
    /// (responsive) column width and its height follows — no centred gaps and
    /// no continuous frame-clock ticking.
    pub struct RatioBox(ObjectSubclass<imp::RatioBox>) @extends gtk::Widget;
}

impl RatioBox {
    fn new(child: &impl IsA<gtk::Widget>) -> Self {
        let obj: Self = glib::Object::new();
        child.set_parent(&obj);
        obj
    }
}

mod imp {
    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;

    /// Height as a fraction of width — 3/4 gives a 4:3 landscape thumbnail.
    const HEIGHT_OVER_WIDTH_NUM: i32 = 3;
    const HEIGHT_OVER_WIDTH_DEN: i32 = 4;

    #[derive(Default)]
    pub struct RatioBox;

    #[glib::object_subclass]
    impl ObjectSubclass for RatioBox {
        const NAME: &'static str = "VireoRatioBox";
        type Type = super::RatioBox;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for RatioBox {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for RatioBox {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            match orientation {
                gtk::Orientation::Vertical => {
                    // Height follows the allocated width. Until the width is
                    // known (for_size < 0) request nothing and let the parent
                    // stretch us horizontally first.
                    let h = if for_size > 0 {
                        for_size * HEIGHT_OVER_WIDTH_NUM / HEIGHT_OVER_WIDTH_DEN
                    } else {
                        0
                    };
                    (h, h, -1, -1)
                }
                // Width is driven by the parent (hexpand + width-request); ask
                // for nothing intrinsic so we fill whatever the column offers.
                _ => (0, 0, -1, -1),
            }
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            if let Some(child) = self.obj().first_child() {
                child.allocate(width, height, baseline, None);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(name: &str, from: &str, subject: &str, folder: &str, ts: i64, size: u64) -> GalleryItem {
        GalleryItem {
            account_id: 1,
            folder_path: folder.into(),
            uid: 1,
            name: name.into(),
            size,
            from_name: from.into(),
            subject: subject.into(),
            timestamp: ts,
            data: None,
        }
    }

    #[test]
    fn haystack_matches_sender_subject_folder_and_type() {
        let it = item("Budget.XLSX", "Bob Jones", "Q3 numbers", "Archive", 1, 10);
        let hay = item_haystack(&it);
        // filename, sender, subject, folder — all lowercased and searchable.
        for needle in ["budget", "xlsx", "bob", "jones", "q3", "numbers", "archive"] {
            assert!(hay.contains(needle), "haystack missing {needle}: {hay}");
        }
        // type keywords derived from the extension.
        assert!(hay.contains("spreadsheet"));
        assert!(hay.contains("excel"));
        assert!(!hay.contains("image"));
    }

    #[test]
    fn type_keywords_cover_common_kinds() {
        assert!(type_keywords("a.pdf").contains("document"));
        assert!(type_keywords("a.png").contains("image"));
        assert!(type_keywords("a.mp3").contains("audio"));
        assert!(type_keywords("a.ics").contains("calendar"));
    }

    #[test]
    fn sort_indices_orders_each_criterion() {
        // c: oldest/smallest, name "a"; a: newest, name "c"; b: middle, name "b", largest.
        let all = vec![
            item("c.txt", "Zed", "s", "INBOX", 300, 5),   // 0
            item("b.txt", "Amy", "s", "INBOX", 200, 90),  // 1
            item("a.txt", "Mel", "s", "INBOX", 100, 5),   // 2
        ];
        let mut idx = vec![0, 1, 2];

        sort_indices(&mut idx, &all, SortBy::Newest);
        assert_eq!(idx, vec![0, 1, 2]); // ts 300, 200, 100
        sort_indices(&mut idx, &all, SortBy::Oldest);
        assert_eq!(idx, vec![2, 1, 0]);
        sort_indices(&mut idx, &all, SortBy::Name);
        assert_eq!(idx, vec![2, 1, 0]); // a, b, c
        sort_indices(&mut idx, &all, SortBy::NameDesc);
        assert_eq!(idx, vec![0, 1, 2]); // c, b, a
        sort_indices(&mut idx, &all, SortBy::Largest);
        assert_eq!(idx[0], 1); // b is 90 bytes
        sort_indices(&mut idx, &all, SortBy::Smallest);
        assert_eq!(idx.last(), Some(&1)); // b (90 bytes) is largest → last
        sort_indices(&mut idx, &all, SortBy::Sender);
        assert_eq!(idx, vec![1, 2, 0]); // Amy, Mel, Zed
        sort_indices(&mut idx, &all, SortBy::SenderDesc);
        assert_eq!(idx, vec![0, 2, 1]); // Zed, Mel, Amy
        // All .txt here, so type ties and falls back to name order.
        sort_indices(&mut idx, &all, SortBy::Type);
        assert_eq!(idx, vec![2, 1, 0]); // a, b, c
        sort_indices(&mut idx, &all, SortBy::TypeDesc);
        assert_eq!(idx, vec![0, 1, 2]); // c, b, a
    }

    #[test]
    fn icon_color_class_maps_types() {
        assert_eq!(icon_color_class("report.pdf"), "ftype-pdf");
        assert_eq!(icon_color_class("Notes.DOCX"), "ftype-doc"); // case-insensitive
        assert_eq!(icon_color_class("budget.xlsx"), "ftype-sheet");
        assert_eq!(icon_color_class("deck.pptx"), "ftype-slides");
        assert_eq!(icon_color_class("bundle.zip"), "ftype-archive");
        assert_eq!(icon_color_class("song.mp3"), "ftype-audio");
        assert_eq!(icon_color_class("clip.mov"), "ftype-video");
        assert_eq!(icon_color_class("invite.ics"), "ftype-calendar");
        assert_eq!(icon_color_class("photo.png"), "ftype-image");
        assert_eq!(icon_color_class("data.bin"), "ftype-generic");
        assert_eq!(icon_color_class("noext"), "ftype-generic");
    }
}
