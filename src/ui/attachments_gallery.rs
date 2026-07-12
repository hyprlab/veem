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
    items: Vec<GalleryItem>,
    /// Index into `items` currently shown in the lightbox, if any.
    preview: Option<usize>,
    loading: bool,
    flow: gtk::FlowBox,
    /// Reusable right-click context menu, parented to the grid.
    menu: gtk::Popover,
}

#[derive(Debug)]
pub enum GalleryInput {
    /// Replace the gallery contents (already merged + sorted newest-first).
    SetItems(Vec<GalleryItem>),
    SetLoading(bool),
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

            // Base layer: the scrolling grid (or empty/loading state).
            #[wrap(Some)]
            set_child = &gtk::Stack {
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
                    set_icon_name: Some("mail-attachment-symbolic"),
                    set_title: "No attachments",
                    set_description: Some("Attachments from your inboxes will appear here."),
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
                        set_icon_name: "window-close-symbolic",
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
                        set_icon_name: "go-previous-symbolic",
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
                                add_css_class: "gallery-file-icon",
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
                        set_icon_name: "go-next-symbolic",
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
            items: Vec::new(),
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
                self.items = items;
                self.loading = false;
                self.preview = None;
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
        if self.loading && self.items.is_empty() {
            "loading"
        } else if self.items.is_empty() {
            "empty"
        } else {
            "grid"
        }
    }

    fn current(&self) -> Option<&GalleryItem> {
        self.preview.and_then(|i| self.items.get(i))
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
        for (i, item) in self.items.iter().enumerate() {
            self.flow.append(&build_cell(i, item, sender));
        }
    }

    /// Open item `index` in its default application (if its bytes are cached).
    fn open_item(&self, index: usize) {
        if let Some(item) = self.items.get(index) {
            if let Some(data) = &item.data {
                open_bytes(&item.name, data);
            }
        }
    }

    /// Save item `index` to a file the user chooses.
    fn download_item(&self, index: usize) {
        let Some(item) = self.items.get(index) else { return };
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
        if let Some(item) = self.items.get(index) {
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
        let Some(item) = self.items.get(index) else { return };
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
            img.add_css_class("dim-label");
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

    // Overlay a quick "Open" button at the thumbnail's bottom-right corner; it
    // fades in on hover (via CSS) and opens the file externally. Only for files
    // whose bytes are cached.
    let thumb_overlay = gtk::Overlay::new();
    thumb_overlay.set_child(Some(&aspect));
    if item.data.is_some() {
        let open_btn = gtk::Button::from_icon_name("document-open-symbolic");
        open_btn.add_css_class("gallery-open");
        open_btn.add_css_class("circular");
        open_btn.add_css_class("osd");
        open_btn.set_halign(gtk::Align::End);
        open_btn.set_valign(gtk::Align::End);
        open_btn.set_margin_end(6);
        open_btn.set_margin_bottom(6);
        open_btn.set_tooltip_text(Some("Open"));
        let s = sender.clone();
        open_btn.connect_clicked(move |_| s.input(GalleryInput::OpenItem(index)));
        thumb_overlay.add_overlay(&open_btn);
    }
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

    let sub = gtk::Label::new(Some(&format!(
        "{} · {}",
        folder_label(&item.folder_path),
        item.human_size()
    )));
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
    let folder = folder_label(&item.folder_path);
    let subject = item.subject.trim();
    if subject.is_empty() {
        format!("{who} · {folder} · {}", item.human_size())
    } else {
        format!("{who} · {subject} · {folder} · {}", item.human_size())
    }
}

/// A friendly folder name from a mailbox path (the last path segment).
fn folder_label(path: &str) -> String {
    let name = path.rsplit(['/', '.']).next().unwrap_or(path);
    if name.eq_ignore_ascii_case("inbox") { "Inbox".to_string() } else { name.to_string() }
}

/// A `gdk::Texture` from raw image bytes, or `None` if the format isn't loadable.
fn texture_from(data: &[u8]) -> Option<gdk::Texture> {
    gdk::Texture::from_bytes(&glib::Bytes::from(data)).ok()
}

/// A symbolic icon name for a filename by extension.
fn icon_for(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    match ext {
        "pdf" => "x-office-document-symbolic",
        "doc" | "docx" | "odt" | "rtf" | "txt" | "md" => "x-office-document-symbolic",
        "xls" | "xlsx" | "ods" | "csv" => "x-office-spreadsheet-symbolic",
        "ppt" | "pptx" | "odp" => "x-office-presentation-symbolic",
        "zip" | "gz" | "tar" | "7z" | "rar" | "xz" | "bz2" => "package-x-generic-symbolic",
        "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" => "audio-x-generic-symbolic",
        "mp4" | "mov" | "mkv" | "webm" | "avi" | "m4v" => "video-x-generic-symbolic",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "heic" | "heif" | "avif" | "ico" => {
            "image-x-generic-symbolic"
        }
        "ics" => "x-office-calendar-symbolic",
        _ => "text-x-generic-symbolic",
    }
}

/// Write bytes to a temp file and open it in the default application.
fn open_bytes(name: &str, data: &[u8]) {
    let safe: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' })
        .collect();
    let dir = std::env::temp_dir().join("veem-attachments");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(if safe.is_empty() { "attachment".into() } else { safe });
    if std::fs::write(&path, data).is_ok() {
        let uri = format!("file://{}", path.to_string_lossy());
        let _ = gtk::gio::AppInfo::launch_default_for_uri(&uri, gtk::gio::AppLaunchContext::NONE);
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
        const NAME: &'static str = "VeemRatioBox";
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
