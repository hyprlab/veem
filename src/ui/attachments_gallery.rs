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
}

#[derive(Debug)]
pub enum GalleryInput {
    /// Replace the gallery contents (already merged + sorted newest-first).
    SetItems(Vec<GalleryItem>),
    SetLoading(bool),
    /// A grid cell was activated — open the lightbox on that item.
    Activate(u32),
    Prev,
    Next,
    ClosePreview,
    /// Open the current item's file in its default application.
    OpenCurrent,
    /// Jump to the current item's source message.
    GoToCurrent,
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
                        set_max_children_per_line: 12,
                        set_min_children_per_line: 2,
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
        let model = AttachmentsGallery {
            items: Vec::new(),
            preview: None,
            loading: false,
            flow: gtk::FlowBox::new(),
        };
        let flow = &model.flow;
        let widgets = view_output!();

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
                if let Some(item) = self.current() {
                    if let Some(data) = &item.data {
                        open_bytes(&item.name, data);
                    }
                }
            }
            GalleryInput::GoToCurrent => {
                if let Some(item) = self.current() {
                    let _ = sender.output(GalleryOutput::OpenMessage {
                        account_id: item.account_id,
                        folder_path: item.folder_path.clone(),
                        uid: item.uid,
                    });
                    self.preview = None;
                }
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
        while let Some(child) = self.flow.first_child() {
            self.flow.remove(&child);
        }
        for item in &self.items {
            self.flow.append(&build_cell(item, sender));
        }
    }
}

/// One grid cell: a thumbnail (image) or type icon, plus name + size.
fn build_cell(item: &GalleryItem, _sender: &ComponentSender<AttachmentsGallery>) -> gtk::Widget {
    let cell = gtk::Box::new(gtk::Orientation::Vertical, 6);
    cell.add_css_class("gallery-cell");
    cell.set_tooltip_text(Some(&format!("{} — {}", item.name, item.human_size())));

    let thumb_holder = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    thumb_holder.add_css_class("gallery-thumb");
    thumb_holder.set_size_request(150, 150);
    thumb_holder.set_halign(gtk::Align::Fill);
    thumb_holder.set_valign(gtk::Align::Center);

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
    cell.append(&thumb_holder);

    let name = gtk::Label::new(Some(&item.name));
    name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    name.set_max_width_chars(18);
    name.add_css_class("gallery-name");
    cell.append(&name);

    let sub = gtk::Label::new(Some(&item.human_size()));
    sub.add_css_class("gallery-size");
    sub.add_css_class("dim-label");
    cell.append(&sub);

    let child = gtk::FlowBoxChild::new();
    child.set_child(Some(&cell));
    child.upcast()
}

fn caption(item: &GalleryItem) -> String {
    let who = if item.from_name.trim().is_empty() { "Unknown" } else { item.from_name.trim() };
    let subject = item.subject.trim();
    if subject.is_empty() {
        format!("{who} · {}", item.human_size())
    } else {
        format!("{who} · {subject} · {}", item.human_size())
    }
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
