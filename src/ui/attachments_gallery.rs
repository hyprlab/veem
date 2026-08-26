//! Attachments gallery: a grid of every cached attachment across the connected
//! inboxes, with a lightbox preview (prev/next, open, go to message).
//!
//! Data comes from the SQLite cache (what the background prefetch has already
//! downloaded), fed in via [`GalleryInput::SetItems`]. Image attachments and PDFs
//! (rendered from their first page) show as thumbnails; other files show a type
//! icon. Clicking a cell opens a large overlay preview.

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use relm4::prelude::*;

use crate::models::{is_image_name, GalleryItem};

/// Width of the table's trailing quick-actions column (three icon buttons);
/// the header carries a spacer of the same width so the columns line up.
const TABLE_ACTIONS_WIDTH: i32 = 100;

pub struct AttachmentsGallery {
    /// Full set of attachments, unfiltered — the source for search and sort.
    all_items: Vec<GalleryItem>,
    /// Indices into `all_items`, filtered by `query`/`type_filter` and ordered
    /// by `sort`: the list actually shown and stepped through in the lightbox.
    items: Vec<usize>,
    query: String,
    sort: SortBy,
    /// Index into `items` currently shown in the lightbox, if any.
    preview: Option<usize>,
    /// What the lightbox shows for the current item: a decoded image, or a
    /// PDF's first page once its full-size render lands (None while a PDF
    /// render is in flight, and for types with nothing to show).
    preview_texture: Option<gdk::Texture>,
    loading: bool,
    /// Show the sortable table instead of the thumbnail grid (persisted).
    view_table: bool,
    /// Grid thumbnail cell width in px, driven by the footer slider (persisted).
    thumb_width: i32,
    /// The footer type dropdown's row: 0 = all, then one bucket per row.
    type_filter: u32,
    /// Debounce for the size slider — one rebuild after the drag settles.
    resize_timer: Option<glib::SourceId>,
    flow: gtk::FlowBox,
    /// The table view's rows (the grid's sibling stack page).
    table: gtk::ListBox,
    /// Reusable right-click context menu, parented to the gallery root.
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

    /// The dropdown row for a criterion — [`SortBy::from_index`]'s inverse, so
    /// a table-header click can move the dropdown's selection with it.
    fn index(self) -> u32 {
        match self {
            SortBy::Newest => 0,
            SortBy::Oldest => 1,
            SortBy::Name => 2,
            SortBy::NameDesc => 3,
            SortBy::Sender => 4,
            SortBy::SenderDesc => 5,
            SortBy::Largest => 6,
            SortBy::Smallest => 7,
            SortBy::Type => 8,
            SortBy::TypeDesc => 9,
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
    /// A table column header was clicked: sort by that column, or flip its
    /// direction when it is already the active column (0 name, 1 sender,
    /// 2 date, 3 size, 4 type).
    SortColumn(u8),
    /// Switch between the thumbnail grid and the table.
    SetViewTable(bool),
    /// The footer size slider moved (grid thumbnail width, px).
    SetThumbWidth(f64),
    /// The size slider settled — rebuild the grid at the new width.
    ApplyThumbWidth,
    /// Show only one type bucket (the footer type dropdown's row; 0 = all).
    SetTypeFilter(u32),
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
    /// A lightbox-size PDF render finished (keyed by content hash) — show it
    /// if that PDF is still the one being previewed.
    PreviewRendered(u64),
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

                    // Table view: fixed sortable column headers over the rows.
                    add_named[Some("table")] = &gtk::Box {
                        set_orientation: gtk::Orientation::Vertical,

                        gtk::Box {
                            add_css_class: "gallery-table-header",
                            set_spacing: 10,

                            // Aligns the headers with the rows' leading thumbnail.
                            gtk::Box { set_width_request: 28 },
                            gtk::Button {
                                add_css_class: "flat",
                                set_hexpand: true,
                                connect_clicked => GalleryInput::SortColumn(0),
                                gtk::Label {
                                    #[watch]
                                    set_label: &column_header("Name", model.sort, SortBy::Name, SortBy::NameDesc),
                                    set_xalign: 0.0,
                                },
                            },
                            gtk::Button {
                                add_css_class: "flat",
                                set_width_request: 170,
                                connect_clicked => GalleryInput::SortColumn(1),
                                gtk::Label {
                                    #[watch]
                                    set_label: &column_header("Sender", model.sort, SortBy::Sender, SortBy::SenderDesc),
                                    set_xalign: 0.0,
                                },
                            },
                            gtk::Button {
                                add_css_class: "flat",
                                set_width_request: 90,
                                connect_clicked => GalleryInput::SortColumn(4),
                                gtk::Label {
                                    #[watch]
                                    set_label: &column_header("Type", model.sort, SortBy::Type, SortBy::TypeDesc),
                                    set_xalign: 0.0,
                                },
                            },
                            gtk::Button {
                                add_css_class: "flat",
                                set_width_request: 110,
                                connect_clicked => GalleryInput::SortColumn(2),
                                gtk::Label {
                                    #[watch]
                                    set_label: &column_header("Date", model.sort, SortBy::Oldest, SortBy::Newest),
                                    set_xalign: 0.0,
                                },
                            },
                            gtk::Button {
                                add_css_class: "flat",
                                set_width_request: 90,
                                connect_clicked => GalleryInput::SortColumn(3),
                                gtk::Label {
                                    #[watch]
                                    set_label: &column_header("Size", model.sort, SortBy::Smallest, SortBy::Largest),
                                    set_xalign: 1.0,
                                    set_hexpand: true,
                                },
                            },
                            // Aligns with the rows' trailing actions column.
                            gtk::Box { set_width_request: TABLE_ACTIONS_WIDTH },
                        },

                        gtk::ScrolledWindow {
                            set_hscrollbar_policy: gtk::PolicyType::Never,
                            set_vexpand: true,

                            #[local_ref]
                            table -> gtk::ListBox {
                                set_selection_mode: gtk::SelectionMode::None,
                                set_activate_on_single_click: true,
                                add_css_class: "gallery-table",
                                connect_row_activated[sender] => move |_, row| {
                                    sender.input(GalleryInput::Activate(row.index() as u32));
                                },
                            },
                        },
                    },
                },

                // Footer: view toggle, filtering/ordering, size, count — the
                // gallery's controls in one place, out of the content's way.
                gtk::ActionBar {
                    add_css_class: "gallery-footer",
                    #[watch]
                    set_revealed: !model.all_items.is_empty(),

                    pack_start = &gtk::Box {
                        add_css_class: "linked",

                        gtk::ToggleButton {
                            set_icon_name: "co.hyprlab.Vireo-view-grid-symbolic",
                            set_tooltip_text: Some("Thumbnail grid"),
                            #[watch]
                            #[block_signal(grid_toggle)]
                            set_active: !model.view_table,
                            connect_clicked[sender] => move |_| {
                                sender.input(GalleryInput::SetViewTable(false));
                            } @grid_toggle,
                        },
                        gtk::ToggleButton {
                            set_icon_name: "co.hyprlab.Vireo-view-list-bullet-symbolic",
                            set_tooltip_text: Some("Table"),
                            #[watch]
                            #[block_signal(table_toggle)]
                            set_active: model.view_table,
                            connect_clicked[sender] => move |_| {
                                sender.input(GalleryInput::SetViewTable(true));
                            } @table_toggle,
                        },
                    },

                    pack_start = &gtk::DropDown {
                        set_tooltip_text: Some("Show only this type"),
                        #[wrap(Some)]
                        set_model = &gtk::StringList::new(&[
                            "All types",
                            "Images",
                            "PDFs",
                            "Documents",
                            "Archives",
                            "Audio & Video",
                            "Other",
                        ]),
                        connect_selected_notify[sender] => move |d| {
                            sender.input(GalleryInput::SetTypeFilter(d.selected()));
                        },
                    },

                    #[name = "sort_dropdown"]
                    pack_start = &gtk::DropDown {
                        set_tooltip_text: Some("Sort"),
                        set_selected: model.sort.index(),
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

                    pack_end = &gtk::Label {
                        add_css_class: "dim-label",
                        #[watch]
                        set_label: &count_text(model.items.len(), model.all_items.len()),
                    },

                    pack_end = &gtk::Scale {
                        set_range: (140.0, 380.0),
                        set_value: model.thumb_width as f64,
                        set_increments: (10.0, 40.0),
                        set_width_request: 140,
                        set_draw_value: false,
                        set_tooltip_text: Some("Thumbnail size"),
                        #[watch]
                        set_visible: !model.view_table,
                        connect_value_changed[sender] => move |s| {
                            sender.input(GalleryInput::SetThumbWidth(s.value()));
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
                        set_visible_child_name: if model.preview_texture.is_some() {
                            "image"
                        } else if model.current().is_some_and(|i| is_pdf_name(&i.name) && i.data.is_some()) {
                            // A PDF whose full-size render is still on its way.
                            "rendering"
                        } else {
                            "file"
                        },

                        #[name = "preview_picture"]
                        add_named[Some("image")] = &gtk::Picture {
                            set_can_shrink: true,
                            set_content_fit: gtk::ContentFit::Contain,
                            #[watch]
                            set_paintable: model.preview_texture.as_ref(),
                        },

                        add_named[Some("rendering")] = &gtk::Box {
                            set_halign: gtk::Align::Center,
                            set_valign: gtk::Align::Center,
                            gtk::Spinner {
                                set_spinning: true,
                                set_width_request: 36,
                                set_height_request: 36,
                            },
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
        let (view_table, thumb_width, sort_index) = crate::config::load_gallery_view();
        let model = AttachmentsGallery {
            all_items: Vec::new(),
            items: Vec::new(),
            query: String::new(),
            sort: SortBy::from_index(sort_index),
            preview: None,
            preview_texture: None,
            loading: false,
            view_table,
            thumb_width,
            type_filter: 0,
            resize_timer: None,
            flow: gtk::FlowBox::new(),
            table: gtk::ListBox::new(),
            menu,
        };
        let flow = &model.flow;
        let table = &model.table;
        let widgets = view_output!();
        // Parent the context menu to the gallery root (not the FlowBox, whose
        // children must be FlowBoxChild and which we clear on every rebuild).
        model.menu.set_parent(&root);

        // Double-clicking the preview opens the document in its external app.
        let dbl = gtk::GestureClick::new();
        dbl.set_button(gtk::gdk::BUTTON_PRIMARY);
        let ds = sender.clone();
        dbl.connect_pressed(move |_, n, _, _| {
            if n == 2 {
                ds.input(GalleryInput::OpenCurrent);
            }
        });
        widgets.preview_picture.add_controller(dbl);

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
                self.preview_texture = None;
                self.apply();
                self.rebuild_view(&sender);
            }
            GalleryInput::SetQuery(q) => {
                self.query = q;
                self.preview = None;
                self.apply();
                self.rebuild_view(&sender);
            }
            GalleryInput::SetSort(i) => {
                let sort = SortBy::from_index(i);
                if self.sort != sort {
                    self.sort = sort;
                    self.preview = None;
                    self.apply();
                    self.rebuild_view(&sender);
                    crate::config::save_gallery_sort(i);
                }
            }
            GalleryInput::SortColumn(col) => {
                // First click sorts a column its natural way; a second flips it.
                let sort = match (col, self.sort) {
                    (0, SortBy::Name) => SortBy::NameDesc,
                    (0, _) => SortBy::Name,
                    (1, SortBy::Sender) => SortBy::SenderDesc,
                    (1, _) => SortBy::Sender,
                    (2, SortBy::Newest) => SortBy::Oldest,
                    (2, _) => SortBy::Newest,
                    (3, SortBy::Largest) => SortBy::Smallest,
                    (3, _) => SortBy::Largest,
                    (4, SortBy::Type) => SortBy::TypeDesc,
                    (4, _) | (_, _) => SortBy::Type,
                };
                // The dropdown follows; its notify handler sees the same value
                // and does nothing further.
                widgets.sort_dropdown.set_selected(sort.index());
                if self.sort != sort {
                    self.sort = sort;
                    self.preview = None;
                    self.apply();
                    self.rebuild_view(&sender);
                    crate::config::save_gallery_sort(sort.index());
                }
            }
            GalleryInput::SetViewTable(table) => {
                if self.view_table != table {
                    self.view_table = table;
                    self.rebuild_view(&sender);
                    crate::config::save_gallery_table_view(table);
                }
            }
            GalleryInput::SetThumbWidth(v) => {
                let width = (v.round() as i32).clamp(140, 380);
                if width != self.thumb_width {
                    self.thumb_width = width;
                    // One rebuild once the drag settles, not one per pixel.
                    if let Some(id) = self.resize_timer.take() {
                        id.remove();
                    }
                    let s = sender.clone();
                    self.resize_timer = Some(glib::timeout_add_local_once(
                        std::time::Duration::from_millis(150),
                        move || s.input(GalleryInput::ApplyThumbWidth),
                    ));
                }
            }
            GalleryInput::ApplyThumbWidth => {
                self.resize_timer = None;
                if !self.view_table {
                    self.rebuild_view(&sender);
                }
                crate::config::save_gallery_thumb_width(self.thumb_width);
            }
            GalleryInput::SetTypeFilter(bucket) => {
                if self.type_filter != bucket {
                    self.type_filter = bucket;
                    self.preview = None;
                    self.apply();
                    self.rebuild_view(&sender);
                }
            }
            GalleryInput::SetLoading(on) => self.loading = on,
            GalleryInput::Activate(i) => {
                if (i as usize) < self.items.len() {
                    self.preview = Some(i as usize);
                    self.refresh_preview(&sender);
                }
            }
            GalleryInput::Prev => self.step(-1, &sender),
            GalleryInput::Next => self.step(1, &sender),
            GalleryInput::ClosePreview => {
                self.preview = None;
                self.preview_texture = None;
            }
            GalleryInput::PreviewRendered(key) => {
                // Only meaningful if the rendered PDF is still on show.
                let still_current = self
                    .current()
                    .and_then(|i| i.data.as_ref())
                    .is_some_and(|d| thumb_cache_key(d) == key);
                if still_current {
                    self.refresh_preview(&sender);
                }
            }
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
        } else if self.view_table {
            "table"
        } else {
            "grid"
        }
    }

    /// Recompute the displayed `items` (indices into `all_items`) from the
    /// current search `query`, `type_filter` and `sort`.
    fn apply(&mut self) {
        let query = self.query.to_ascii_lowercase();
        let tokens: Vec<&str> = query.split_whitespace().collect();
        let mut items: Vec<usize> = self
            .all_items
            .iter()
            .enumerate()
            .filter(|(_, it)| self.type_filter == 0 || type_bucket(&it.name) == self.type_filter)
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

    fn step(&mut self, delta: i32, sender: &ComponentSender<Self>) {
        if self.items.is_empty() {
            return;
        }
        if let Some(i) = self.preview {
            let n = self.items.len() as i32;
            self.preview = Some((((i as i32 + delta) % n + n) % n) as usize);
            self.refresh_preview(sender);
        }
    }

    /// Work out what the lightbox shows for the current item: an image decodes
    /// on the spot; a PDF's first page comes from the full-size render cache,
    /// or a worker renders it now and [`GalleryInput::PreviewRendered`] circles
    /// back here. Anything else has no texture — the file-icon page shows.
    fn refresh_preview(&mut self, sender: &ComponentSender<Self>) {
        self.preview_texture = None;
        let Some(item) = self.current() else { return };
        let Some(data) = item.data.as_ref() else { return };
        if item.is_image() {
            self.preview_texture = texture_from(data);
            return;
        }
        if !is_pdf_name(&item.name) {
            return;
        }
        let key = thumb_cache_key(data);
        match PDF_PREVIEWS.with(|c| c.borrow().get(&key).cloned()) {
            Some(texture) => self.preview_texture = texture,
            None => {
                let s = sender.clone();
                lightbox_pdf_texture(data, move |_| {
                    s.input(GalleryInput::PreviewRendered(key));
                });
            }
        }
    }

    /// Repopulate whichever view is showing. The other keeps stale children;
    /// switching to it rebuilds it, so only one view's widgets are ever built
    /// for a given change.
    fn rebuild_view(&mut self, sender: &ComponentSender<Self>) {
        if self.view_table {
            self.rebuild_table(sender);
        } else {
            self.rebuild_grid(sender);
        }
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
                self.flow
                    .append(&build_cell(display, item, self.thumb_width, sender));
            }
        }
    }

    fn rebuild_table(&mut self, sender: &ComponentSender<Self>) {
        while let Some(row) = self.table.first_child() {
            self.table.remove(&row);
        }
        for display in 0..self.items.len() {
            if let Some(item) = self.item_at(display) {
                self.table.append(&build_row(display, item, sender));
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
        // Point at the click position, translated from the clicked cell/row
        // into the menu parent's coordinate space, so it opens under the
        // pointer whichever view is showing.
        let source: Option<gtk::Widget> = if self.view_table {
            self.table.row_at_index(index as i32).map(|r| r.upcast())
        } else {
            self.flow.child_at_index(index as i32).map(|c| c.upcast())
        };
        if let (Some(child), Some(parent)) = (source, self.menu.parent()) {
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

/// One grid cell: a thumbnail (image or PDF first page) or type icon, plus name
/// + size.
fn build_cell(
    index: usize,
    item: &GalleryItem,
    width: i32,
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

    let thumb = match item.data.as_ref() {
        Some(d) => thumbnail_texture(&item.name, d),
        None => Thumbnail::Fallback,
    };
    match thumb {
        Thumbnail::Ready(tex) => thumb_holder.append(&gallery_picture(&tex)),
        Thumbnail::Fallback => thumb_holder.append(&gallery_icon(&item.name)),
        Thumbnail::Pending => {
            thumb_holder.append(&thumbnail_spinner());
            let holder = thumb_holder.downgrade();
            let name = item.name.clone();
            let data = item.data.clone().unwrap_or_default();
            spawn_thumbnail_render(&item.name, data, move |tex| {
                // The cell may be gone by now (search narrowed, list rebuilt);
                // the render still landed in the cache for the next build.
                let Some(holder) = holder.upgrade() else { return };
                while let Some(child) = holder.first_child() {
                    holder.remove(&child);
                }
                match tex {
                    Some(tex) => holder.append(&gallery_picture(&tex)),
                    None => holder.append(&gallery_icon(&name)),
                }
            });
        }
    }

    // Lock the thumbnail section to a 4:3 aspect ratio; its width tracks the
    // (responsive) column width and the height follows, filling the cell.
    let aspect = RatioBox::new(&thumb_holder);
    // Preferred column width (the footer slider's value) — the FlowBox packs at
    // least 3 per row and adds more as the window widens.
    aspect.set_width_request(width);
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

/// One table row: mini thumbnail/type icon, name, sender, type, date, size —
/// the same widths as the header buttons, so the columns line up.
fn build_row(
    index: usize,
    item: &GalleryItem,
    sender: &ComponentSender<AttachmentsGallery>,
) -> gtk::ListBoxRow {
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    line.add_css_class("gallery-table-row");

    // Cache-only mini thumbnail: a decode already paid for is shown, but a
    // 24px row never spawns a render of its own.
    let lead: gtk::Widget = match item.data.as_ref().map(|d| thumbnail_texture(&item.name, d)) {
        Some(Thumbnail::Ready(tex)) => {
            let pic = gtk::Picture::for_paintable(&tex);
            pic.set_content_fit(gtk::ContentFit::Cover);
            pic.set_size_request(28, 28);
            pic.add_css_class("gallery-table-thumb");
            pic.upcast()
        }
        _ => {
            let img = gtk::Image::from_icon_name(icon_for(&item.name));
            img.set_pixel_size(20);
            img.set_size_request(28, 28);
            img.add_css_class("gallery-file-icon");
            img.add_css_class(icon_color_class(&item.name));
            img.upcast()
        }
    };
    line.append(&lead);

    let name = gtk::Label::new(Some(&item.name));
    name.set_xalign(0.0);
    name.set_hexpand(true);
    name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    name.set_tooltip_text(Some(&format!("{} — {}", item.subject, item.human_size())));
    line.append(&name);

    let sender_label = gtk::Label::new(Some(&item.from_name));
    sender_label.set_xalign(0.0);
    sender_label.set_width_request(170);
    sender_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    sender_label.set_max_width_chars(1);
    line.append(&sender_label);

    let kind = gtk::Label::new(Some(&ext_of(&item.name).to_ascii_uppercase()));
    kind.set_xalign(0.0);
    kind.set_width_request(90);
    kind.add_css_class("dim-label");
    line.append(&kind);

    let date = gtk::Label::new(Some(&date_text(item.timestamp)));
    date.set_xalign(0.0);
    date.set_width_request(110);
    date.add_css_class("dim-label");
    line.append(&date);

    let size = gtk::Label::new(Some(&item.human_size()));
    size.set_xalign(1.0);
    size.set_width_request(90);
    size.add_css_class("dim-label");
    size.add_css_class("numeric");
    line.append(&size);

    // The same quick actions the grid cells carry, as a trailing column.
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    actions.set_width_request(TABLE_ACTIONS_WIDTH);
    actions.set_halign(gtk::Align::End);
    let act = |icon: &str, tip: &str| {
        let b = gtk::Button::from_icon_name(icon);
        b.add_css_class("flat");
        b.set_valign(gtk::Align::Center);
        b.set_tooltip_text(Some(tip));
        b
    };
    if item.data.is_some() {
        let download = act("co.hyprlab.Vireo-folder-download-symbolic", "Download");
        let s = sender.clone();
        download.connect_clicked(move |_| s.input(GalleryInput::DownloadItem(index)));
        actions.append(&download);
        let open = act("co.hyprlab.Vireo-document-open-symbolic", "Open");
        let s = sender.clone();
        open.connect_clicked(move |_| s.input(GalleryInput::OpenItem(index)));
        actions.append(&open);
    }
    let goto = act("co.hyprlab.Vireo-mail-unread-symbolic", "Go to Message");
    let s = sender.clone();
    goto.connect_clicked(move |_| s.input(GalleryInput::GoToItem(index)));
    actions.append(&goto);
    line.append(&actions);

    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&line));

    // The same gestures the grid cells carry: right-click for the context
    // menu, double-click to open externally (single click previews).
    let right = gtk::GestureClick::new();
    right.set_button(gtk::gdk::BUTTON_SECONDARY);
    let s = sender.clone();
    right.connect_pressed(move |_, _, x, y| {
        s.input(GalleryInput::ContextMenu { index, x, y });
    });
    row.add_controller(right);
    let dbl = gtk::GestureClick::new();
    dbl.set_button(gtk::gdk::BUTTON_PRIMARY);
    let s = sender.clone();
    dbl.connect_pressed(move |_, n, _, _| {
        if n == 2 {
            s.input(GalleryInput::OpenExternal(index));
        }
    });
    row.add_controller(dbl);
    row
}

/// A column header's label, carrying the sort arrow when it is the active
/// column: `asc`/`desc` are the two criteria that column maps to.
fn column_header(label: &str, current: SortBy, asc: SortBy, desc: SortBy) -> String {
    if current == asc {
        format!("{label} \u{2191}")
    } else if current == desc {
        format!("{label} \u{2193}")
    } else {
        label.to_string()
    }
}

/// The footer's item count: what's shown of what's there.
fn count_text(shown: usize, total: usize) -> String {
    if shown == total {
        format!("{total} attachment{}", if total == 1 { "" } else { "s" })
    } else {
        format!("{shown} of {total}")
    }
}

/// "Aug 26, 2026" for a table row, or nothing when the timestamp is unknown.
fn date_text(timestamp: i64) -> String {
    glib::DateTime::from_unix_local(timestamp)
        .ok()
        .and_then(|d| d.format("%b %e, %Y").ok())
        .map(|s| s.to_string())
        .unwrap_or_default()
}

/// The footer type dropdown's bucket for a filename. Row 0 is "All types";
/// the rest must match the `StringList` built in the view.
fn type_bucket(name: &str) -> u32 {
    match ext_of(name).as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "heic" | "heif" | "avif" | "ico" => 1,
        "pdf" => 2,
        "doc" | "docx" | "odt" | "rtf" | "txt" | "md" | "xls" | "xlsx" | "ods" | "csv" | "ppt"
        | "pptx" | "odp" | "ics" => 3,
        "zip" | "gz" | "tar" | "7z" | "rar" | "xz" | "bz2" => 4,
        "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" | "mp4" | "mov" | "mkv" | "webm" | "avi"
        | "m4v" => 5,
        _ => 6,
    }
}

/// The gallery's cover-cropped thumbnail picture for a ready texture.
fn gallery_picture(tex: &gdk::Texture) -> gtk::Picture {
    let pic = gtk::Picture::for_paintable(tex);
    pic.set_content_fit(gtk::ContentFit::Cover);
    pic.set_hexpand(true);
    pic.set_vexpand(true);
    pic.add_css_class("gallery-thumb-image");
    pic
}

/// The gallery's centred type icon for anything without a thumbnail.
fn gallery_icon(name: &str) -> gtk::Image {
    let img = gtk::Image::from_icon_name(icon_for(name));
    img.set_pixel_size(56);
    img.set_hexpand(true);
    img.add_css_class("gallery-file-icon");
    img.add_css_class(icon_color_class(name));
    img
}

/// A `gdk::Texture` from raw image bytes, or `None` if the format isn't loadable.
pub(crate) fn texture_from(data: &[u8]) -> Option<gdk::Texture> {
    gdk::Texture::from_bytes(&glib::Bytes::from(data)).ok()
}

/// What a grid cell can show for an attachment right now.
pub(crate) enum Thumbnail {
    /// A texture is available immediately: a decoded image, or a PDF page
    /// already in the render cache.
    Ready(gdk::Texture),
    /// An image or PDF not decoded/rendered yet. Show a spinner and call
    /// [`spawn_thumbnail_render`] — decoding on the main thread while cells
    /// were being built froze the whole window (the "Force Quit" dialog).
    Pending,
    /// No thumbnail for this type (or it wouldn't decode): type icon.
    Fallback,
}

thread_local! {
    /// Finished thumbnail renders — decoded images and PDF pages alike,
    /// successes and failures, keyed by content hash — so a gallery rebuild or
    /// a revisit this session never decodes the same attachment twice.
    /// Main-thread only; results land here from `spawn_thumbnail_render`.
    static THUMB_CACHE: std::cell::RefCell<std::collections::HashMap<u64, Option<gdk::Texture>>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
    /// Lightbox-size PDF page renders, cached separately from the thumbnails
    /// (same key, much bigger pixels). Failures cache too.
    static PDF_PREVIEWS: std::cell::RefCell<std::collections::HashMap<u64, Option<gdk::Texture>>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Whether a filename names a PDF (by extension).
pub(crate) fn is_pdf_name(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".pdf")
}

/// A lightbox-size render of a PDF's first page: handed to `on_done` on the
/// main thread — at once from the cache, or after a worker renders it.
/// Failures cache too, so a broken PDF is rendered at most once.
pub(crate) fn lightbox_pdf_texture(
    data: &[u8],
    on_done: impl FnOnce(Option<gdk::Texture>) + 'static,
) {
    let key = thumb_cache_key(data);
    if let Some(cached) = PDF_PREVIEWS.with(|c| c.borrow().get(&key).cloned()) {
        on_done(cached);
        return;
    }
    let data = data.to_vec();
    glib::spawn_future_local(async move {
        let tex =
            gtk::gio::spawn_blocking(move || pdf_page_texture(&data, PREVIEW_RENDER_WIDTH))
                .await
                .ok()
                .flatten();
        PDF_PREVIEWS.with(|c| {
            c.borrow_mut().insert(key, tex.clone());
        });
        on_done(tex);
    });
}

fn thumb_cache_key(data: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    data.len().hash(&mut h);
    data.hash(&mut h);
    h.finish()
}

/// A grid-cell thumbnail for any attachment type that has one — a decoded
/// image or a PDF's first page — answered from the render cache, or `Pending`
/// until a worker produces it.
pub(crate) fn thumbnail_texture(name: &str, data: &[u8]) -> Thumbnail {
    if !is_image_name(name) && !is_pdf_name(name) {
        return Thumbnail::Fallback;
    }
    match THUMB_CACHE.with(|c| c.borrow().get(&thumb_cache_key(data)).cloned()) {
        Some(Some(tex)) => Thumbnail::Ready(tex),
        Some(None) => Thumbnail::Fallback,
        None => Thumbnail::Pending,
    }
}

/// Decode an image, or render a PDF's first page, off the main thread; hand
/// the result to `on_done` back on it and record it in the cache either way.
/// The GTK loop keeps running — cells show their spinner instead of the
/// window freezing.
pub(crate) fn spawn_thumbnail_render(
    name: &str,
    data: Vec<u8>,
    on_done: impl FnOnce(Option<gdk::Texture>) + 'static,
) {
    let key = thumb_cache_key(&data);
    let image = is_image_name(name);
    glib::spawn_future_local(async move {
        let tex = gtk::gio::spawn_blocking(move || {
            if image {
                texture_from(&data)
            } else {
                pdf_page_texture(&data, THUMB_RENDER_WIDTH)
            }
        })
        .await
        .ok()
        .flatten();
        THUMB_CACHE.with(|c| {
            c.borrow_mut().insert(key, tex.clone());
        });
        on_done(tex);
    });
}

/// The centred spinner a cell shows while its PDF page renders.
pub(crate) fn thumbnail_spinner() -> gtk::Spinner {
    let spinner = gtk::Spinner::new();
    spinner.set_spinning(true);
    spinner.set_width_request(28);
    spinner.set_height_request(28);
    spinner.set_halign(gtk::Align::Center);
    spinner.set_valign(gtk::Align::Center);
    spinner.set_hexpand(true);
    spinner.set_vexpand(true);
    spinner
}

/// Width thumbnails render at — soft would do, but sharp is cheap at 360px.
const THUMB_RENDER_WIDTH: f64 = 360.0;
/// Width the lightbox renders a PDF page at: sharp on a large window without
/// tripping the decoder's pixel ceiling (A4 portrait at 1600 ≈ 3.6M pixels).
const PREVIEW_RENDER_WIDTH: f64 = 1600.0;

/// Render a PDF's first page to a texture at `target_width`, by way of an
/// in-memory PNG — the same route every other thumbnail here already goes
/// through, so cropping, caching, and format all stay uniform.
fn pdf_page_texture(data: &[u8], target_width: f64) -> Option<gdk::Texture> {
    let doc = poppler::Document::from_bytes(&glib::Bytes::from(data), None).ok()?;
    let page = doc.page(0)?;
    let (w, h) = page.size();
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    // Poppler's default is one pixel per point (72 dpi) — fine for text but
    // soft on screen, so render at the caller's width instead.
    let scale = target_width / w;
    let surface = gtk::cairo::ImageSurface::create(
        gtk::cairo::Format::ARgb32,
        target_width.round() as i32,
        (h * scale).round() as i32,
    )
    .ok()?;
    let cr = gtk::cairo::Context::new(&surface).ok()?;
    // A PDF page's own background is transparent; without painting white first,
    // the thumbnail would show through to whatever is behind it (a hole in
    // dark mode rather than a page).
    cr.set_source_rgb(1.0, 1.0, 1.0);
    cr.paint().ok()?;
    cr.scale(scale, scale);
    page.render(&cr);
    drop(cr);
    let mut png = Vec::new();
    surface.write_to_png(&mut png).ok()?;
    texture_from(&png)
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
    // Outside Flatpak, GIO launches the default handler directly; the portal
    // route has been seen accepting an OpenURI request and then launching
    // nothing — reported success, no app, click looked dead. Inside the
    // sandbox the portal is the only road out and GIO's launcher is the one
    // that can't work. So each side leads with the road that works for it
    // and keeps the other as the fallback.
    if std::path::Path::new("/.flatpak-info").exists() {
        // Inside the sandbox the file was staged into the app's PRIVATE /tmp —
        // a path the host cannot read. Handing the portal a file:// URI string
        // therefore launches a handler pointed at a file that, host-side, does
        // not exist. FileLauncher instead passes the file as a descriptor
        // through the document portal, which exports it where the handler can
        // read it. When even that fails (a broken portal), say so instead of
        // doing nothing — a host-side AppInfo fallback can't work here.
        // Not GTK's FileLauncher: in this runtime it mis-finishes its own
        // async task in the sandboxed path (task-tag assertion), so its
        // callback never fires and every failure vanishes. Speaking the
        // portal protocol directly restores the contract.
        portal_open_file(path, false, parent.cloned());
    } else if let Err(e) =
        gtk::gio::AppInfo::launch_default_for_uri(&uri, gtk::gio::AppLaunchContext::NONE)
    {
        tracing::warn!("gio launch failed ({e}), trying the portal");
        let owned = parent.cloned();
        gtk::UriLauncher::new(&uri).launch(parent, gtk::gio::Cancellable::NONE, move |res| {
            if let Err(e) = res {
                tracing::warn!("portal launch also failed: {e}");
                launch_failed_dialog(owned.as_ref(), &e.to_string());
            }
        });
    }
}

/// Both launch roads failed — tell the user what happened and what still
/// works, rather than leaving a click that does nothing.
fn launch_failed_dialog(parent: Option<&gtk::Window>, error: &str) {
    let dialog = gtk::AlertDialog::builder()
        .message("The file could not be opened")
        // No Flatseal toggle can help here: portal access is not a
        // permission, and the failure is inside the portal's own launcher.
        // Offer the steps that actually work.
        .detail(format!(
            "The desktop portal reported: {error}\n\n\
             \u{2022} Download the file, then open it from Files\n\
             \u{2022} Updating \u{201c}xdg-desktop-portal\u{201d} and logging back in may fix direct opening"
        ))
        .modal(true)
        .build();
    dialog.show(parent);
}

/// Open a staged file through the OpenURI portal, speaking the protocol
/// directly over GIO's D-Bus. The subscription to the request's `Response`
/// signal is set up FIRST, on a path derived from our own `handle_token`, so
/// a fast reply can't race it. Response 0 is success. On the quiet attempt
/// (`ask == false`) any failure retries once with the app chooser — the
/// backend's own dialog, whose launch machinery works even where the direct
/// default-handler launch is broken (seen on Fedora 44), and whose "always
/// open with" sticks in the permission store. A cancelled chooser (1) is not
/// an error; any other chooser failure gets the dialog.
fn portal_open_file(path: std::path::PathBuf, ask: bool, parent: Option<gtk::Window>) {
    use gtk::gio;
    use gtk::glib;
    use gtk::glib::prelude::*;
    use std::os::fd::AsFd;

    let conn = match gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) {
        Ok(c) => c,
        Err(e) => {
            launch_failed_dialog(parent.as_ref(), &e.to_string());
            return;
        }
    };
    let file = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!("staged attachment vanished: {e}");
            return;
        }
    };
    let fd_list = gio::UnixFDList::new();
    let handle = match fd_list.append(file.as_fd()) {
        Ok(h) => h,
        Err(e) => {
            launch_failed_dialog(parent.as_ref(), &e.to_string());
            return;
        }
    };

    let token = crate::rng::nonce(16)
        .map(|t| t.replace('-', "_"))
        .unwrap_or_else(|_| format!("vireo{}", std::process::id()));
    let sender_token = conn
        .unique_name()
        .map(|n| n.trim_start_matches(':').replace('.', "_"))
        .unwrap_or_default();
    let request_path =
        format!("/org/freedesktop/portal/desktop/request/{sender_token}/{token}");

    let sub_id: std::rc::Rc<std::cell::RefCell<Option<gtk::gio::SignalSubscriptionId>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let sub = sub_id.clone();
    let sig_conn = conn.clone();
    let retry_path = path.clone();
    let retry_parent = parent.clone();
    let id = conn.signal_subscribe(
        Some("org.freedesktop.portal.Desktop"),
        Some("org.freedesktop.portal.Request"),
        Some("Response"),
        Some(&request_path),
        None,
        gio::DBusSignalFlags::NONE,
        move |_, _, _, _, _, params| {
            if let Some(id) = sub.borrow_mut().take() {
                sig_conn.signal_unsubscribe(id);
            }
            let code = params.child_value(0).get::<u32>().unwrap_or(2);
            match (code, ask) {
                (0, _) => {}
                (_, false) => {
                    tracing::warn!("portal open answered {code}; retrying with the chooser");
                    portal_open_file(retry_path.clone(), true, retry_parent.clone());
                }
                (1, true) => {} // the user closed the chooser — their call
                (_, true) => launch_failed_dialog(
                    retry_parent.as_ref(),
                    &format!("the portal answered response code {code}"),
                ),
            }
        },
    );
    *sub_id.borrow_mut() = Some(id);

    let options = glib::VariantDict::new(None);
    options.insert_value("handle_token", &token.to_variant());
    if ask {
        options.insert_value("ask", &true.to_variant());
    }
    // Not the tuple's ToVariant: that boxes the dict as a nested "v" and the
    // portal rejects "(shv)". tuple_from_iter splices each child at its own
    // type, producing the "(sha{sv})" the interface declares.
    let params = glib::Variant::tuple_from_iter([
        "".to_variant(),
        glib::variant::Handle(handle).to_variant(),
        options.end(),
    ]);
    let call_conn = conn.clone();
    conn.call_with_unix_fd_list(
        Some("org.freedesktop.portal.Desktop"),
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.OpenURI",
        "OpenFile",
        Some(&params),
        None,
        gio::DBusCallFlags::NONE,
        10_000,
        Some(&fd_list),
        gio::Cancellable::NONE,
        move |res| {
            if let Err(e) = res {
                if let Some(id) = sub_id.borrow_mut().take() {
                    call_conn.signal_unsubscribe(id);
                }
                tracing::warn!("portal OpenFile call failed: {e}");
                launch_failed_dialog(parent.as_ref(), &e.to_string());
            }
        },
    );
}

/// The private directory opened attachments are staged in, created if needed.
///
/// Returns `None` if the path exists but is not a directory we own with nobody
/// else's access — an attacker who pre-creates it on a shared host would
/// otherwise get every attachment the user opens.
fn attachment_dir() -> Option<std::path::PathBuf> {
    // Inside Flatpak /tmp is PRIVATE to the sandbox: the document portal
    // validates an exported fd by re-opening its path in the HOST namespace,
    // so a /tmp-staged file fails validation and every portal open dies
    // silently before any UI. $XDG_RUNTIME_DIR/app/$FLATPAK_ID is
    // bind-mounted from the host at the SAME path — the one temp location
    // both sides of the sandbox agree on (and it's session-scoped tmpfs,
    // like /tmp). Native builds keep /tmp.
    let dir = match std::env::var("FLATPAK_ID") {
        Ok(app_id) => {
            let runtime = std::env::var("XDG_RUNTIME_DIR").ok()?;
            std::path::Path::new(&runtime)
                .join("app")
                .join(app_id)
                .join("vireo-attachments")
        }
        Err(_) => std::env::temp_dir().join("vireo-attachments"),
    };
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
            // 0o400000 is O_NOFOLLOW on every Linux arch Vireo ships for; the
            // wrong constant here once passed O_DIRECTORY (0o200000) instead,
            // which made this open() fail and every attachment click a no-op.
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

    /// A minimal one-page PDF, built by hand — just enough structure for
    /// poppler to load and render, with no dependency on an external file.
    fn minimal_pdf() -> Vec<u8> {
        let content = b"1 0 0 rg 100 100 300 300 re f";
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Contents 4 0 R >>".to_string(),
            format!(
                "<< /Length {} >>\nstream\n{}\nendstream",
                content.len(),
                std::str::from_utf8(content).unwrap()
            ),
        ];
        let mut out = Vec::new();
        out.extend_from_slice(b"%PDF-1.4\n");
        let mut offsets = vec![0usize];
        for (i, obj) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
            out.extend_from_slice(obj.as_bytes());
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref_offset = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        out.extend_from_slice(b"0000000000 65535 f \n");
        for off in &offsets[1..] {
            out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF",
                objects.len() + 1,
                xref_offset
            )
            .as_bytes(),
        );
        out
    }

    #[test]
    fn type_buckets_match_the_footer_dropdown_rows() {
        assert_eq!(type_bucket("photo.JPG"), 1);
        assert_eq!(type_bucket("report.pdf"), 2);
        assert_eq!(type_bucket("notes.docx"), 3);
        assert_eq!(type_bucket("backup.tar"), 4);
        assert_eq!(type_bucket("song.flac"), 5);
        assert_eq!(type_bucket("unknown.xyz"), 6);
    }

    #[test]
    fn sort_index_roundtrips_every_criterion() {
        for i in 0..10 {
            assert_eq!(SortBy::from_index(i).index(), i);
        }
    }

    #[test]
    fn column_headers_carry_the_sort_arrow() {
        use super::column_header;
        assert_eq!(column_header("Name", SortBy::Name, SortBy::Name, SortBy::NameDesc), "Name ↑");
        assert_eq!(
            column_header("Name", SortBy::NameDesc, SortBy::Name, SortBy::NameDesc),
            "Name ↓"
        );
        assert_eq!(column_header("Name", SortBy::Newest, SortBy::Name, SortBy::NameDesc), "Name");
    }

    #[test]
    fn counts_read_naturally() {
        use super::count_text;
        assert_eq!(count_text(1, 1), "1 attachment");
        assert_eq!(count_text(42, 42), "42 attachments");
        assert_eq!(count_text(12, 42), "12 of 42");
    }

    #[test]
    fn pdf_thumbnail_renders_the_first_page() {
        let tex = pdf_page_texture(&minimal_pdf(), 360.0).expect("should render a page");
        assert!(tex.width() > 0 && tex.height() > 0);
        assert!(pdf_page_texture(b"not a real pdf", 360.0).is_none());
    }

    #[test]
    fn thumbnail_texture_classifies_attachment_types() {
        // No thumbnail for a type without one; an uncached PDF is rendered
        // asynchronously, so the immediate answer is Pending.
        assert!(matches!(
            thumbnail_texture("archive.zip", b"not a real zip"),
            Thumbnail::Fallback
        ));
        assert!(matches!(
            thumbnail_texture("notes.pdf", &minimal_pdf()),
            Thumbnail::Pending
        ));
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
