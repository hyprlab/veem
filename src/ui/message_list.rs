//! Middle pane: the scrollable list of messages in the selected folder,
//! with a search field and live filtering.

use adw::prelude::*;
use relm4::factory::FactoryVecDeque;
use relm4::prelude::*;

use crate::models::Message;
use crate::ui::context_menu::{show_context_menu, show_context_menu_with_header, MenuEntry};

/// Max rows rendered at once. GtkListBox isn't virtualized, so the full folder
/// index is kept in memory for search but only this many rows are built.
const RENDER_CAP: usize = 200;

/// An action chosen from a message's right-click context menu.
#[derive(Debug, Clone, Copy)]
pub enum RowAction {
    Reply,
    ReplyAll,
    Forward,
    ToggleStar,
    ToggleRead,
    Spam,
    Archive,
    Delete,
    ViewSource,
}

/// Init for a row: the message, Gravatar flag, and optional account-ring class
/// (the account colour drawn as a ring around the avatar in the unified view).
pub struct RowInit {
    pub msg: Message,
    pub gravatar: bool,
    /// Whether the sender circle is drawn at all (#29).
    pub avatars: bool,
    /// Whether a sender's site icon may fill it (#30).
    pub sender_logos: bool,
    /// How many lines of the message's text the row shows (1–3).
    pub preview_lines: u32,
    pub ring_class: Option<String>,
    /// Shared Actions Palette collapse delay in seconds — how long it stays open
    /// after the cursor leaves it (read live when scheduling).
    pub palette_collapse_secs: std::rc::Rc<std::cell::Cell<u64>>,
    /// Shared "open the palette on row hover" flag (read live on each hover).
    pub palette_hover: std::rc::Rc<std::cell::Cell<bool>>,
    /// Number of messages in this conversation (only set on a thread head; 1 for
    /// a standalone message).
    pub thread_count: usize,
    /// This row is a (newer/older) reply nested under a thread head — indent it.
    pub is_thread_child: bool,
    /// Whether this thread head is currently expanded.
    pub thread_expanded: bool,
    /// The conversation key, set on a thread head so its chevron can toggle it.
    pub thread_key: Option<(u32, String)>,
    /// The newest member's display time (thread heads only): shown in place of
    /// the head's own — the row says when the conversation last moved.
    pub thread_date: Option<String>,
    /// Any message in this conversation is unread (thread heads only) — keeps
    /// the head marked unread while unread replies are hidden beneath it.
    pub thread_unread: bool,
    /// Every currently shown row as (account, folder, uid, id), in list order.
    /// Shared with the list so a drag can turn the ListBox's selected row
    /// *indices* into message ids and carry the whole selection (#23).
    pub drag_keys: DragKeys,
    /// Show who the message went to instead of who sent it — a Sent folder's
    /// rows all say "me" otherwise (#27).
    pub show_recipient: bool,
    /// Starting state for the row's own Revealer. A newly-inserted thread
    /// reply starts `false` and is flipped to `true` right after mounting, so
    /// it slides open instead of simply appearing; everything else starts
    /// (and stays) `true`.
    pub revealed: bool,
}

/// The shown rows' (account, folder, uid, id) keys, in list order — rebuilt with
/// the list and read live when a drag starts.
pub type DragKeys = std::rc::Rc<std::cell::RefCell<Vec<(u32, u32, u32, u32)>>>;

/// The message-list pane's floor: exactly what a conversation-member card
/// needs to show a row's full Actions Palette — the tightest real constraint
/// in the list. The sum of the card's insets (10px rail margin, 10px + 8px
/// card margins, 12px + 12px card padding), the avatar (38px), the unread
/// dot (8px), three 8px gaps, and the 234px actions-line reservation.
const LIST_MIN_WIDTH: i32 = 348;

/// What an expanded conversation needs beyond [`LIST_MIN_WIDTH`]: the member
/// cards' 10px rail indent plus their card margin/padding beyond a plain
/// pill's. The pane's floor grows by this while any thread is open, so the
/// cards' (and the head pill's) right inset is never clipped off the pane.
const THREAD_EXPANDED_EXTRA: i32 = 12;

/// A background face lookup's answer, correlated by sender address (a recycled
/// row compares before using it). The tiers are personal-first: the contact's
/// own photo, their Gravatar, then the icon their domain publishes (#30), with
/// the UI's coloured initials as the implicit last resort.
#[derive(Debug)]
pub enum FaceCmd {
    /// The avatar tiers (contact photo, Gravatar) answered. `logo` carries the
    /// logo tier's answer when it was consulted in the same trip: found bytes,
    /// or a definitive miss to remember.
    Avatar {
        email: String,
        generation: u64,
        mode: crate::avatar::FetchMode,
        outcome: crate::avatar::FetchOutcome,
        logo: Option<Option<Vec<u8>>>,
    },
    /// A logo-only lookup — the avatar tiers had already answered from cache.
    Logo { email: String, bytes: Option<Vec<u8>> },
}

/// Run the avatar tiers off the main thread, falling through to the domain icon
/// when they come up empty and `want_logo` says the switch is on. `generation`
/// and `mode` come from [`crate::avatar::lookup`] and ride along so the result
/// can be cached against the EDS state that was actually queried.
pub async fn find_face(
    email: String,
    generation: u64,
    mode: crate::avatar::FetchMode,
    want_logo: bool,
) -> FaceCmd {
    let lookup_email = email.clone();
    let result = tokio::task::spawn_blocking(move || {
        let outcome = crate::avatar::fetch(&lookup_email, mode);
        let logo = (want_logo && !matches!(outcome, crate::avatar::FetchOutcome::Found(_)))
            .then(|| crate::logo::fetch(&lookup_email));
        (outcome, logo)
    })
    .await;
    let (outcome, logo) = result.unwrap_or((crate::avatar::FetchOutcome::Retry, None));
    FaceCmd::Avatar { email, generation, mode, outcome, logo }
}

/// Fetch just the domain icon, off the main thread.
pub async fn find_logo(email: String) -> FaceCmd {
    let lookup_email = email.clone();
    let bytes = tokio::task::spawn_blocking(move || crate::logo::fetch(&lookup_email))
        .await
        .ok()
        .flatten();
    FaceCmd::Logo { email, bytes }
}

/// One message summary row.
pub struct MessageRow {
    msg: Message,
    gravatar: bool,
    avatars: bool,
    sender_logos: bool,
    preview_lines: u32,
    avatar_texture: Option<gtk::gdk::Texture>,
    ring_class: Option<String>,
    /// Whether the pointer is over this row (drives the chevron fade).
    row_hovered: bool,
    /// Whether the Actions Palette is slid open on this row.
    palette_open: bool,
    /// Pending auto-collapse timer (armed when the cursor isn't over the palette;
    /// cancelled while it is, so the palette stays open).
    collapse_timer: Option<gtk::glib::SourceId>,
    /// Shared collapse delay (seconds) after the cursor leaves the palette.
    palette_collapse_secs: std::rc::Rc<std::cell::Cell<u64>>,
    /// Shared "open the palette on row hover" flag (read live per hover).
    palette_hover: std::rc::Rc<std::cell::Cell<bool>>,
    /// Conversation size (only meaningful on a thread head).
    thread_count: usize,
    /// Nested reply under a thread head.
    is_thread_child: bool,
    /// Whether this head's conversation is expanded.
    thread_expanded: bool,
    /// Sent-folder rows name the recipient, not the sender (#27).
    show_recipient: bool,
    /// Conversation key for the head's expand/collapse toggle.
    thread_key: Option<(u32, String)>,
    /// Newest member's display time (thread heads only), shown as the row date.
    thread_date: Option<String>,
    /// Any message in this conversation is unread (heads only).
    thread_unread: bool,
    /// Shared row keys, so a drag from this row can carry the whole selection.
    drag_keys: DragKeys,
    /// Drives the row's own Revealer — false only for the brief window a
    /// newly-expanded reply is sliding open, or a collapsing one is sliding
    /// shut before it's removed from the list.
    revealed: bool,
}

#[derive(Debug)]
pub enum MessageRowInput {
    SetRead(bool),
    SetStarred(bool),
    SetHasAttachment(bool),
    /// The pointer entered/left the row — fade the chevron in/out.
    SetRowHover(bool),
    /// The chevron was clicked — slide the Actions Palette open or shut.
    TogglePalette,
    /// The cursor moved onto the palette — keep it open (cancel auto-collapse).
    PaletteEnter,
    /// The cursor left the palette — arm the auto-collapse countdown.
    PaletteLeave,
    /// The auto-collapse countdown elapsed — slide the palette shut.
    CollapsePalette,
    Action(RowAction),
    /// The conversation chevron was clicked — expand/collapse the thread.
    ToggleThreadClicked,
    /// The thread's aggregate unread state changed (a hidden reply was read).
    SetThreadUnread(bool),
    /// Drive the row's own Revealer directly — used to slide a reply open
    /// right after it's inserted, or shut just before it's removed.
    SetRevealed(bool),
    /// The head's conversation was expanded/collapsed — updates in place
    /// (the head row survives a toggle; see `expand_thread`/
    /// `collapse_thread_rows`) so the chevron actually rotates instead of
    /// mounting pre-set to its final angle.
    SetThreadExpanded(bool),
}

#[derive(Debug)]
pub enum MessageRowOutput {
    Action { action: RowAction, message: Box<Message> },
    ToggleThread((u32, String)),
}

/// The keys of every selected row in the ListBox this drag started from, in list
/// order. Empty when the row has no list parent yet or nothing is selected — the
/// caller then falls back to the dragged row alone.
/// Display names from a raw To header: "Ann <a@x>, b@y" -> "Ann, b@y".
fn recipient_names(to: &str) -> String {
    let mut names: Vec<String> = Vec::new();
    for part in to.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let name = match part.split_once('<') {
            Some((n, _)) if !n.trim().trim_matches('"').is_empty() => {
                n.trim().trim_matches('"').to_string()
            }
            Some((_, rest)) => rest.trim_end_matches('>').trim().to_string(),
            None => part.to_string(),
        };
        if !name.is_empty() {
            names.push(name);
        }
    }
    names.join(", ")
}

/// The first recipient's bare address from a raw To header, if any.
fn first_recipient_addr(to: &str) -> Option<String> {
    let first = to.split(',').map(str::trim).find(|p| !p.is_empty())?;
    let addr = match first.split_once('<') {
        Some((_, rest)) => rest.trim_end_matches('>').trim(),
        None => first,
    };
    (!addr.is_empty()).then(|| addr.to_string())
}

fn drag_selection(src: &gtk::DragSource, keys: &DragKeys) -> Vec<(u32, u32, u32, u32)> {
    let Some(list) = src.widget().and_then(|w| w.parent()).and_downcast::<gtk::ListBox>() else {
        return Vec::new();
    };
    let keys = keys.borrow();
    list.selected_rows()
        .iter()
        .filter_map(|r| keys.get(r.index() as usize).copied())
        .collect()
}

#[relm4::factory(pub)]
impl FactoryComponent for MessageRow {
    type Init = RowInit;
    type Input = MessageRowInput;
    type Output = MessageRowOutput;
    type CommandOutput = FaceCmd;
    type ParentWidget = gtk::ListBox;

    view! {
        gtk::ListBoxRow {
            // Unread rows get a pale accent background (cleared once read);
            // thread replies are indented.
            #[watch]
            set_css_classes: &self.row_css(),

            // Track hover so the Actions Palette chevron can fade in/out.
            add_controller = gtk::EventControllerMotion {
                connect_enter[sender] => move |_, _, _| sender.input(MessageRowInput::SetRowHover(true)),
                connect_leave[sender] => move |_| sender.input(MessageRowInput::SetRowHover(false)),
            },

            // Drag a message onto a sidebar folder to move it there. The payload
            // carries one (account, source folder, UID, id) group per message —
            // the *whole* selection when this row is part of it, so dragging a
            // multi-selection moves every message, not just the row under the
            // pointer (#23).
            add_controller = gtk::DragSource {
                set_actions: gtk::gdk::DragAction::MOVE,
                connect_prepare[aid = self.msg.account_id, fid = self.msg.folder_id, uid = self.msg.uid, id = self.msg.id, keys = self.drag_keys.clone()] => move |src, _, _| {
                    let mut items = drag_selection(src, &keys);
                    // Dragging a row outside the selection (or before the list has
                    // published its keys) moves just that row.
                    if !items.iter().any(|k| k.0 == aid && k.3 == id) {
                        items = vec![(aid, fid, uid, id)];
                    }
                    let mut payload = String::from("vireo-move");
                    for (a, f, u, i) in items {
                        payload.push_str(&format!("\t{a}\t{f}\t{u}\t{i}"));
                    }
                    Some(gtk::gdk::ContentProvider::for_value(&payload.to_value()))
                },
            },

            // Thread replies animate open/shut by sliding, instead of the
            // list jumping to a new row count the instant a thread toggles
            // (see `revealed` / `MessageRowInput::SetRevealed`).
            #[wrap(Some)]
            set_child = &gtk::Revealer {
                set_transition_type: gtk::RevealerTransitionType::SlideDown,
                set_transition_duration: 200,
                #[watch]
                set_reveal_child: self.revealed,

            #[wrap(Some)]
            set_child = &gtk::Overlay {
            // The node dot where this member meets the group's dotted rail
            // (thread children only): overlaid at the row's left edge and
            // pulled onto the rail itself by .thread-node's negative margin.
            add_overlay = &gtk::Box {
                add_css_class: "thread-node",
                set_halign: gtk::Align::Start,
                set_valign: gtk::Align::Center,
                set_visible: self.is_thread_child,
            },

            #[wrap(Some)]
            set_child = &gtk::Box {
            set_orientation: gtk::Orientation::Horizontal,
            // Tighter than the row's left padding: the avatar sits well inside
            // the list's edge, and the unread dot's gutter is narrow enough that
            // the sender's name still reads as the start of the row.
            set_spacing: 8,
            set_css_classes: &self.content_css(),
            // A palette wider than the row it sits in is clipped here rather than
            // painted across the divider into the reader.
            set_overflow: gtk::Overflow::Hidden,

            adw::Avatar {
                set_size: 38,
                set_valign: gtk::Align::Center,
                set_show_initials: true,
                // Hidden, not faded: the point of turning these off is to get the
                // width back, so the row must give up the slot entirely (#29).
                set_visible: self.avatars,
                // Account colour ring (unified view only).
                set_css_classes: &self.ring_classes(),
                #[watch]
                set_text: Some(&self.face_name()),
                #[watch]
                set_custom_image: self.avatar_texture.as_ref(),
            },

            // Faded rather than hidden: a hidden widget gives up its slot in the
            // box, so every read row's sender and preview would sit 18px further
            // left than an unread one's and the column would jitter as mail is
            // read. The space is always reserved; only the dot's ink changes.
            gtk::Box {
                add_css_class: "unread-dot",
                set_valign: gtk::Align::Center,
                #[watch]
                set_opacity: if self.msg.unread || self.thread_unread { 1.0 } else { 0.0 },
            },

            gtk::Box {
                set_orientation: gtk::Orientation::Vertical,
                set_spacing: 2,
                set_hexpand: true,

                gtk::Box {
                    set_spacing: 6,
                    gtk::Label {
                        set_label: &self.name_line(),
                        set_halign: gtk::Align::Start,
                        set_hexpand: true,
                        set_ellipsize: gtk::pango::EllipsizeMode::End,
                        #[watch]
                        set_css_classes: &self.sender_classes(),
                    },
                    gtk::Image {
                        set_icon_name: Some("co.hyprlab.Vireo-mail-attachment-symbolic"),
                        #[watch]
                        set_visible: self.msg.has_attachment,
                        add_css_class: "dim-icon",
                    },
                    gtk::Image {
                        set_icon_name: Some("co.hyprlab.Vireo-starred-symbolic"),
                        #[watch]
                        set_visible: self.msg.starred,
                        add_css_class: "star-icon",
                    },
                    gtk::Label {
                        set_label: self.thread_date.as_deref().unwrap_or(&self.msg.datetime_list()),
                        set_halign: gtk::Align::End,
                        // Ellipsized so it stops being the row's floor: it is the
                        // one item on this line with no give, and it held the list
                        // ~40px wider than the palette needs (#29).
                        set_ellipsize: gtk::pango::EllipsizeMode::End,
                        add_css_class: "message-date",
                    },
                    // Conversation chip (thread heads only): the message count
                    // and the expand/collapse caret merged into one grey pill.
                    gtk::Button {
                        set_visible: self.thread_count > 1,
                        set_tooltip_text: Some("Show conversation"),
                        add_css_class: "flat",
                        add_css_class: "thread-chip",
                        set_valign: gtk::Align::Center,
                        connect_clicked[sender] => move |_| sender.input(MessageRowInput::ToggleThreadClicked),
                        gtk::Box {
                            set_spacing: 2,
                            gtk::Label {
                                set_label: &self.thread_count.to_string(),
                            },
                            gtk::Image {
                                set_icon_name: Some(if self.thread_expanded {
                                    "co.hyprlab.Vireo-pan-down-symbolic"
                                } else {
                                    "co.hyprlab.Vireo-pan-end-symbolic"
                                }),
                            },
                        },
                    },
                },

                gtk::Label {
                    set_label: &self.msg.subject,
                    set_halign: gtk::Align::Start,
                    set_ellipsize: gtk::pango::EllipsizeMode::End,
                    #[watch]
                    set_css_classes: &self.subject_classes(),
                },

                // The message's own text, at full width: nothing shares this line,
                // so it never reflows or gets covered.
                gtk::Label {
                    // 0 lines: previews are off, so the row gives them no space.
                    set_visible: self.preview_lines > 0,
                    set_label: &self.msg.preview,
                    set_halign: gtk::Align::Start,
                    set_hexpand: true,
                    set_xalign: 0.0,
                    // `lines` only means anything once wrapping is on; at one line
                    // it changes nothing, since the text is ellipsized before it
                    // can reach a second.
                    set_wrap: true,
                    set_wrap_mode: gtk::pango::WrapMode::WordChar,
                    set_ellipsize: gtk::pango::EllipsizeMode::End,
                    // A ceiling, not a reservation: a short message keeps a short
                    // row, so the list stays scannable and only long messages use
                    // the extra lines.
                    set_lines: self.preview_lines.max(1) as i32,
                    add_css_class: "message-preview",
                },

                // Below it, the Actions Palette on a line of its own, opening
                // rightward from the ⋯ button. The line reserves the palette's
                // width even while collapsed (see `.actions-line`), so the first
                // click doesn't shove the whole pane wider under the pointer.
                gtk::Box {
                    set_spacing: 2,
                    set_halign: gtk::Align::Start,
                    set_valign: gtk::Align::Center,
                    add_css_class: "actions-line",

                    // Actions toggle (⋯). Clicking it opens/closes the palette but
                    // does NOT select or open the message (it's a button, so the
                    // click is consumed before the row's selection gesture).
                    gtk::Button {
                        set_icon_name: "co.hyprlab.Vireo-view-more-horizontal-symbolic",
                        // Hidden until the row is hovered (or the palette is open);
                        // the .revealed class fades it in via a CSS transition.
                        #[watch]
                        set_css_classes: &self.chevron_classes(),
                        set_tooltip_text: Some("Actions"),
                        set_valign: gtk::Align::Center,
                        connect_clicked[sender] => move |_| sender.input(MessageRowInput::TogglePalette),
                    },

                    gtk::Revealer {
                        set_transition_type: gtk::RevealerTransitionType::SlideRight,
                        set_transition_duration: 180,
                        #[watch]
                        set_reveal_child: self.palette_open,

                        gtk::Box {
                            add_css_class: "actions-palette",
                            set_halign: gtk::Align::Start,
                            set_valign: gtk::Align::Center,
                            set_spacing: 0,

                            // Keep the palette open while the cursor is over it.
                            add_controller = gtk::EventControllerMotion {
                                connect_enter[sender] => move |_, _, _| sender.input(MessageRowInput::PaletteEnter),
                                connect_leave[sender] => move |_| sender.input(MessageRowInput::PaletteLeave),
                            },

                            gtk::Button {
                                set_icon_name: "co.hyprlab.Vireo-mail-reply-sender-symbolic",
                                set_tooltip_text: Some("Reply"),
                                add_css_class: "flat",
                                connect_clicked[sender] => move |_| sender.input(MessageRowInput::Action(RowAction::Reply)),
                            },
                            gtk::Button {
                                set_icon_name: "co.hyprlab.Vireo-mail-reply-all-symbolic",
                                set_tooltip_text: Some("Reply All"),
                                add_css_class: "flat",
                                connect_clicked[sender] => move |_| sender.input(MessageRowInput::Action(RowAction::ReplyAll)),
                            },
                            gtk::Button {
                                set_icon_name: "co.hyprlab.Vireo-mail-forward-symbolic",
                                set_tooltip_text: Some("Forward"),
                                add_css_class: "flat",
                                connect_clicked[sender] => move |_| sender.input(MessageRowInput::Action(RowAction::Forward)),
                            },
                            gtk::Button {
                                #[watch]
                                set_icon_name: if self.msg.starred { "co.hyprlab.Vireo-starred-symbolic" } else { "co.hyprlab.Vireo-non-starred-symbolic" },
                                #[watch]
                                set_tooltip_text: Some(if self.msg.starred { "Remove star" } else { "Star" }),
                                add_css_class: "flat",
                                connect_clicked[sender] => move |_| sender.input(MessageRowInput::Action(RowAction::ToggleStar)),
                            },
                            gtk::Button {
                                #[watch]
                                set_icon_name: if self.msg.unread { "co.hyprlab.Vireo-mail-read-symbolic" } else { "co.hyprlab.Vireo-mail-unread-symbolic" },
                                #[watch]
                                set_tooltip_text: Some(if self.msg.unread { "Mark as read" } else { "Mark as unread" }),
                                add_css_class: "flat",
                                connect_clicked[sender] => move |_| sender.input(MessageRowInput::Action(RowAction::ToggleRead)),
                            },
                            gtk::Button {
                                set_icon_name: "co.hyprlab.Vireo-mail-mark-junk-symbolic",
                                set_tooltip_text: Some("Mark as spam"),
                                add_css_class: "flat",
                                connect_clicked[sender] => move |_| sender.input(MessageRowInput::Action(RowAction::Spam)),
                            },
                            gtk::Button {
                                set_icon_name: "co.hyprlab.Vireo-mail-archive-symbolic",
                                set_tooltip_text: Some("Archive"),
                                add_css_class: "flat",
                                connect_clicked[sender] => move |_| sender.input(MessageRowInput::Action(RowAction::Archive)),
                            },
                            gtk::Button {
                                set_icon_name: "co.hyprlab.Vireo-user-trash-symbolic",
                                set_tooltip_text: Some("Delete"),
                                add_css_class: "flat",
                                connect_clicked[sender] => move |_| sender.input(MessageRowInput::Action(RowAction::Delete)),
                            },
                        },
                    },

                },
            },
            },
        },
        }
        }
    }

    fn init_model(init: Self::Init, _index: &DynamicIndex, sender: FactorySender<Self>) -> Self {
        let RowInit {
            msg,
            gravatar,
            avatars,
            sender_logos,
            preview_lines,
            ring_class,
            palette_collapse_secs,
            palette_hover,
            thread_count,
            is_thread_child,
            thread_expanded,
            thread_key,
            thread_date,
            thread_unread,
            drag_keys,
            show_recipient,
            revealed,
        } = init;
        let mut model = Self {
            msg,
            show_recipient,
            gravatar,
            avatars,
            sender_logos,
            preview_lines,
            avatar_texture: None,
            ring_class,
            row_hovered: false,
            palette_open: false,
            collapse_timer: None,
            palette_collapse_secs,
            palette_hover,
            thread_count,
            is_thread_child,
            thread_expanded,
            thread_key,
            thread_date,
            thread_unread,
            drag_keys,
            revealed,
        };

        // No point fetching anything for a circle that isn't drawn — and a
        // Gravatar lookup would send a hash of the sender's address for nothing.
        if model.avatars {
            model.load_face(&sender);
        }

        // A row that mounts already collapsed (a freshly-expanded reply)
        // flips to revealed on the next main-loop iteration, once it has been
        // measured at its natural size — animating it open instead of
        // starting from an already-final state.
        if !model.revealed {
            sender.input(MessageRowInput::SetRevealed(true));
        }

        model
    }

    fn update(&mut self, msg: Self::Input, sender: FactorySender<Self>) {
        match msg {
            MessageRowInput::SetRead(read) => self.msg.unread = !read,
            MessageRowInput::SetStarred(starred) => self.msg.starred = starred,
            MessageRowInput::SetHasAttachment(has) => self.msg.has_attachment = has,
            MessageRowInput::SetRowHover(over) => {
                self.row_hovered = over;
                // Hover mode: the palette slides open by itself on the row,
                // and arms the usual collapse timeout on leave.
                if self.palette_hover.get() {
                    if over {
                        self.palette_open = true;
                        self.cancel_collapse();
                    } else if self.palette_open {
                        self.arm_collapse(&sender);
                    }
                }
            }
            MessageRowInput::TogglePalette => {
                if self.palette_open {
                    self.palette_open = false;
                    self.cancel_collapse();
                } else {
                    self.palette_open = true;
                    // Persist briefly; moving onto the palette cancels this.
                    self.arm_collapse(&sender);
                }
            }
            MessageRowInput::PaletteEnter => self.cancel_collapse(),
            MessageRowInput::PaletteLeave => {
                if self.palette_open {
                    self.arm_collapse(&sender);
                }
            }
            MessageRowInput::CollapsePalette => {
                self.collapse_timer = None;
                self.palette_open = false;
            }
            MessageRowInput::Action(action) => {
                let _ = sender.output(MessageRowOutput::Action {
                    action,
                    message: Box::new(self.msg.clone()),
                });
            }
            MessageRowInput::ToggleThreadClicked => {
                if let Some(key) = self.thread_key.clone() {
                    let _ = sender.output(MessageRowOutput::ToggleThread(key));
                }
            }
            MessageRowInput::SetThreadUnread(unread) => self.thread_unread = unread,
            MessageRowInput::SetRevealed(revealed) => self.revealed = revealed,
            MessageRowInput::SetThreadExpanded(expanded) => self.thread_expanded = expanded,
        }
    }

    fn update_cmd(&mut self, cmd: Self::CommandOutput, sender: FactorySender<Self>) {
        match cmd {
            FaceCmd::Avatar { email, generation, mode, outcome, logo } => {
                // Record what came back before deciding what to draw — the
                // caches are shared, so the sender's other rows benefit even
                // when this row has been recycled to a different message.
                let retry_stale = crate::avatar::cache_result(&email, generation, mode, outcome);
                match logo {
                    Some(Some(bytes)) => {
                        crate::logo::decode_and_cache(&email, &bytes);
                    }
                    Some(None) => crate::logo::remember_missing(&email),
                    None => {}
                }
                if !self.face_email().eq_ignore_ascii_case(&email) {
                    return;
                }
                match crate::avatar::lookup(&email, self.gravatar) {
                    crate::avatar::CacheLookup::Texture(texture) => {
                        self.avatar_texture = Some(texture);
                    }
                    crate::avatar::CacheLookup::Missing => self.load_logo(&email, &sender),
                    crate::avatar::CacheLookup::Fetch { generation, mode } => {
                        self.avatar_texture = None;
                        // Only chase a result the EDS generation invalidated;
                        // a transient Gravatar failure waits for a later render.
                        if retry_stale {
                            let want_logo =
                                self.sender_logos && !crate::logo::known_missing(&email);
                            sender.oneshot_command(find_face(email, generation, mode, want_logo));
                        }
                    }
                }
            }
            FaceCmd::Logo { email, bytes } => {
                let texture = match bytes {
                    Some(bytes) => crate::logo::decode_and_cache(&email, &bytes),
                    None => {
                        // Remember the miss, so the sender's other rows and the
                        // next sync don't ask the same domain again.
                        crate::logo::remember_missing(&email);
                        None
                    }
                };
                if self.face_email().eq_ignore_ascii_case(&email) && self.sender_logos {
                    self.avatar_texture = texture;
                }
            }
        }
    }

    fn shutdown(&mut self, _widgets: &mut Self::Widgets, _output: relm4::Sender<Self::Output>) {
        // Cancel a pending collapse timer so it can't fire into this (now dropped)
        // component's shut-down runtime when the row is removed during a rebuild.
        self.cancel_collapse();
    }
}

/// Fire `DayChanged` shortly after the next local midnight (re-armed each time).
fn schedule_midnight_refresh(sender: &ComponentSender<MessageList>) {
    use chrono::Timelike;
    let secs = 86_400u32
        .saturating_sub(chrono::Local::now().num_seconds_from_midnight())
        .saturating_add(2);
    let input = sender.input_sender().clone();
    gtk::glib::timeout_add_seconds_local(secs, move || {
        let _ = input.send(MessageListInput::DayChanged);
        gtk::glib::ControlFlow::Break
    });
}

impl MessageRow {
    /// Cancel any pending auto-collapse (e.g. the cursor is now over the palette).
    fn cancel_collapse(&mut self) {
        if let Some(id) = self.collapse_timer.take() {
            id.remove();
        }
    }

    /// (Re)start the auto-collapse countdown from the shared preference (min 1s).
    fn arm_collapse(&mut self, sender: &FactorySender<Self>) {
        self.cancel_collapse();
        let secs = self.palette_collapse_secs.get().max(1);
        let s = sender.clone();
        self.collapse_timer = Some(gtk::glib::timeout_add_seconds_local_once(
            secs as u32,
            move || s.input(MessageRowInput::CollapsePalette),
        ));
    }

    /// Chevron classes: shown (`revealed`) while the row is hovered or the palette
    /// is open; a CSS opacity transition fades it in/out.
    fn chevron_classes(&self) -> Vec<&'static str> {
        let mut v = vec!["flat", "palette-toggle"];
        if self.row_hovered || self.palette_open {
            v.push("revealed");
        }
        v
    }

    fn ring_classes(&self) -> Vec<&str> {
        match &self.ring_class {
            Some(c) => vec![c.as_str()],
            None => Vec::new(),
        }
    }

    /// The row's name line: the sender — or, in a Sent folder, who the message
    /// went to, since every sender there is you (#27).
    fn name_line(&self) -> String {
        if !self.show_recipient {
            return self.msg.from_name.clone();
        }
        let names = recipient_names(&self.msg.to);
        if names.is_empty() {
            self.msg.from_name.clone()
        } else {
            format!("To: {names}")
        }
    }

    /// What the avatar's initials (and face lookups) key on: the first
    /// recipient in a Sent folder, the sender everywhere else.
    fn face_name(&self) -> String {
        if self.show_recipient {
            let names = recipient_names(&self.msg.to);
            if let Some(first) = names.split(',').next().map(str::trim) {
                if !first.is_empty() {
                    return first.to_string();
                }
            }
        }
        self.msg.from_name.clone()
    }

    /// The address face lookups run against — the first recipient's in a Sent
    /// folder, so the circle shows who the mail went to.
    fn face_email(&self) -> String {
        if self.show_recipient {
            if let Some(addr) = first_recipient_addr(&self.msg.to) {
                return addr;
            }
        }
        self.msg.from_addr.clone()
    }

    /// Fill the circle: a cached face if one is known, otherwise go and look.
    /// The chain is contact photo → Gravatar → domain icon → initials, each
    /// tier consulted only while its switch is on.
    fn load_face(&mut self, sender: &FactorySender<Self>) {
        let email = self.face_email();
        if email.is_empty() {
            return;
        }
        match crate::avatar::lookup(&email, self.gravatar) {
            crate::avatar::CacheLookup::Texture(texture) => {
                self.avatar_texture = Some(texture);
            }
            // Contact and Gravatar are definitively absent — the logo tier is
            // all that's left before initials.
            crate::avatar::CacheLookup::Missing => self.load_logo(&email, sender),
            crate::avatar::CacheLookup::Fetch { generation, mode } => {
                let want_logo = self.sender_logos && !crate::logo::known_missing(&email);
                sender.oneshot_command(find_face(email, generation, mode, want_logo));
            }
        }
    }

    /// The logo tier: only consulted when enabled, so switching "sender logos"
    /// off hides already-cached logos immediately. A domain already asked about
    /// is not asked again — one request a session, not one a row.
    fn load_logo(&mut self, email: &str, sender: &FactorySender<Self>) {
        if !self.sender_logos {
            self.avatar_texture = None;
            return;
        }
        if let Some(tex) = crate::logo::cached(email) {
            self.avatar_texture = Some(tex);
            return;
        }
        self.avatar_texture = None;
        if crate::logo::known_missing(email) {
            return;
        }
        sender.oneshot_command(find_logo(email.to_string()));
    }

    /// Classes for the row's content box. Without the sender circle the unread
    /// dot becomes the row's first element, and the wide inset that kept the
    /// circle clear of the list's edge would leave the dot lopsided — sitting
    /// twice as far from the edge as from the text beside it.
    fn content_css(&self) -> Vec<&'static str> {
        if self.avatars {
            vec!["message-row"]
        } else {
            vec!["message-row", "no-avatar"]
        }
    }

    /// Row classes: unread highlight plus a `thread-child` indent for replies.
    /// A thread head with unread messages anywhere in its conversation gets the
    /// heavier `thread-unread` highlight until every one of them is read.
    fn row_css(&self) -> Vec<&'static str> {
        let mut v = row_classes(&self.msg);
        if self.thread_unread {
            v.push("thread-unread");
        }
        if self.is_thread_child {
            v.push("thread-child");
        }
        v
    }

    /// Unread for display purposes: the message itself, or (on a thread head)
    /// any message hidden in its conversation.
    fn display_unread(&self) -> bool {
        self.msg.unread || self.thread_unread
    }

    fn sender_classes(&self) -> Vec<&'static str> {
        if self.display_unread() {
            vec!["message-sender", "unread"]
        } else {
            vec!["message-sender"]
        }
    }

    fn subject_classes(&self) -> Vec<&'static str> {
        if self.display_unread() {
            vec!["message-subject", "unread"]
        } else {
            vec!["message-subject"]
        }
    }
}

/// Order two messages by the chosen sort (ties fall back to date).
fn message_cmp(a: &Message, b: &Message, order: SortOrder) -> std::cmp::Ordering {
    let name_of = |m: &Message| {
        if m.from_name.trim().is_empty() {
            m.from_addr.to_lowercase()
        } else {
            m.from_name.to_lowercase()
        }
    };
    match order {
        SortOrder::DateNewest => b.timestamp.cmp(&a.timestamp),
        SortOrder::DateOldest => a.timestamp.cmp(&b.timestamp),
        SortOrder::Sender => name_of(a).cmp(&name_of(b)).then(b.timestamp.cmp(&a.timestamp)),
        SortOrder::Subject => normalize_subject(&a.subject)
            .cmp(&normalize_subject(&b.subject))
            .then(b.timestamp.cmp(&a.timestamp)),
        // `true` sorts after `false`, so compare b-vs-a to put unread/flagged first.
        SortOrder::UnreadFirst => b.unread.cmp(&a.unread).then(b.timestamp.cmp(&a.timestamp)),
        SortOrder::FlaggedFirst => b.starred.cmp(&a.starred).then(b.timestamp.cmp(&a.timestamp)),
    }
}

/// The conversation key a message belongs to: its owning account plus the
/// subject with reply/forward prefixes stripped. Messages with no subject get a
/// per-message key (by UID) so they never group together.
/// Lower-case the subject and strip any leading run of reply/forward prefixes
/// (`Re:`, `Fwd:`, …) so subject-sorting keeps a topic and its replies adjacent.
fn normalize_subject(subject: &str) -> String {
    const PREFIXES: &[&str] = &["re:", "fwd:", "fw:", "aw:", "sv:", "antw:", "wg:"];
    let mut s = subject.trim();
    loop {
        let lower = s.to_ascii_lowercase();
        let mut stripped = false;
        for p in PREFIXES {
            if lower.starts_with(p) {
                s = s[p.len()..].trim_start();
                stripped = true;
                break;
            }
        }
        if !stripped {
            break;
        }
    }
    s.trim().to_ascii_lowercase()
}

/// Group messages into conversations by their reply headers (Message-ID linked
/// via In-Reply-To / References), scoped per account. Returns each message's
/// thread key `(account_id, root)`. Messages with no reply relationship get a
/// unique key (a thread of one) — so unrelated messages that merely share a
/// subject are never threaded together.
///
/// Age plays no part: a message threads because its headers say what it answers,
/// and those are indexed with every message. Grouping runs over the rendered
/// window, so covering the whole mailbox costs no more than covering a day of
/// it; what a conversation costs to *open* is bounded separately, by
/// `THREAD_MEMBER_LIMIT`.
fn compute_thread_keys(
    msgs: &[Message],
    links: &[(u32, String, String)],
) -> std::collections::HashMap<(u32, u32), (u32, String)> {
    use std::collections::HashMap;

    // Union-find over message-id nodes (namespaced by account).
    let mut parent: HashMap<String, String> = HashMap::new();
    fn find(parent: &mut HashMap<String, String>, x: &str) -> String {
        let mut cur = x.to_string();
        while let Some(p) = parent.get(&cur) {
            if p == &cur {
                break;
            }
            cur = p.clone();
        }
        cur
    }
    fn union(parent: &mut HashMap<String, String>, a: &str, b: &str) {
        let ra = find(parent, a);
        let rb = find(parent, b);
        if ra != rb {
            parent.insert(ra, rb);
        }
    }
    // A message with its own Message-ID is a real node; one without gets a unique
    // node keyed by uid so it only links through its references (if any).
    let self_node = |m: &Message| -> String {
        if m.message_id.is_empty() {
            format!("{}\u{0}uid{}", m.account_id, m.uid)
        } else {
            format!("{}\u{0}{}", m.account_id, m.message_id)
        }
    };

    for m in msgs {
        let sn = self_node(m);
        parent.entry(sn.clone()).or_insert_with(|| sn.clone());
        for r in m.references.split_whitespace() {
            let rn = format!("{}\u{0}{}", m.account_id, r);
            parent.entry(rn.clone()).or_insert_with(|| rn.clone());
            union(&mut parent, &sn, &rn);
        }
    }

    // Messages from elsewhere in the account contribute their links but never
    // appear: a reply in the Inbox and the one before it are two answers to the
    // same message in Sent, and without that message nothing says so.
    for (aid, id, refs) in links {
        let sn = format!("{aid}\u{0}{id}");
        parent.entry(sn.clone()).or_insert_with(|| sn.clone());
        for r in refs.split_whitespace() {
            let rn = format!("{aid}\u{0}{r}");
            parent.entry(rn.clone()).or_insert_with(|| rn.clone());
            union(&mut parent, &sn, &rn);
        }
    }

    let mut out = HashMap::new();
    for m in msgs {
        let root = find(&mut parent, &self_node(m));
        out.insert((m.account_id, m.id), (m.account_id, root));
    }
    out
}

/// Style classes for a row: highlight unread messages with a pale accent.
fn row_classes(m: &Message) -> Vec<&'static str> {
    if m.unread {
        vec!["message-unread"]
    } else {
        Vec::new()
    }
}

pub struct MessageList {
    rows: FactoryVecDeque<MessageRow>,
    /// All messages for the current folder (full searchable index).
    all: Vec<Message>,
    /// Every folder's messages (all accounts), supplied by the app while a search
    /// is active, so `AllFolders` scope can filter across the whole mailbox. Empty
    /// when not searching.
    search_pool: Vec<Message>,
    /// Which messages the search field filters over.
    scope: SearchScope,
    /// The search field widget, kept so a folder switch can clear its text.
    search_entry: Option<gtk::SearchEntry>,
    /// Currently displayed (post-filter, capped) messages, aligned with rows.
    shown: Vec<Message>,
    /// Total messages matching the current filter (may exceed what's rendered).
    total_matches: usize,
    query: String,
    gravatar: bool,
    /// Lines of preview text per row (1–3), from Preferences.
    preview_lines: u32,
    /// Whether the coloured sender circles are drawn (#29).
    avatars: bool,
    /// Whether a sender's site icon may fill one (#30).
    sender_logos: bool,
    /// Tint each row by its account (used in the unified inbox view).
    colorize: bool,
    /// account_id → avatar colour, for tinting rows.
    account_colors: std::collections::HashMap<u32, String>,
    /// Display-wide provider with each account's pale row-tint rule.
    color_provider: gtk::CssProvider,
    /// Actions Palette collapse delay (seconds), shared with every row.
    palette_collapse_secs: std::rc::Rc<std::cell::Cell<u64>>,
    /// Shared with every row: open the palette on row hover.
    palette_hover: std::rc::Rc<std::cell::Cell<bool>>,
    /// The message currently being viewed, kept selected across list rebuilds.
    /// Keyed by (account_id, id) since UIDs collide across accounts in the
    /// unified "All Inboxes" view.
    selected_id: Option<(u32, u32)>,
    /// (account, message-id, references) for mail in the account's *other*
    /// folders. A conversation is often joined through messages that aren't on
    /// screen — every reply in an Inbox answers something in Sent — so those
    /// links are needed to see that the replies belong together.
    thread_links: Vec<(u32, String, String)>,
    /// The shown rows' (account, folder, uid, id) keys, handed to every row so a
    /// drag can carry the whole selection (#23).
    drag_keys: DragKeys,
    /// Every selected message key, so the whole selection survives list rebuilds
    /// (background syncs) until the user clicks away.
    selected_ids: Vec<(u32, u32)>,
    /// Selection changes still expected from a reader-driven selection, and what
    /// that selection is. GTK reports each `select_row`/`unselect_all` separately
    /// and a rebuild adds more, so a single flag would be consumed by the first
    /// and let a later one re-open the message; only a change that matches what
    /// the reader asked for is suppressed, and anything else ends it at once.
    from_reader: u8,
    reader_keys: Vec<(u32, u32)>,
    /// How many rows are currently selected (drives the bulk-action bar).
    selection_count: usize,
    /// Conversation keys the user has toggled away from the default state
    /// (expanded when the default is collapsed, and vice versa).
    expanded_threads: std::collections::HashSet<(u32, String)>,
    /// Whether conversations start expanded (user preference; collapsed default).
    default_expanded: bool,
    /// The open folder is Sent: rows name recipients instead of senders (#27).
    show_recipient: bool,
    /// Rendered thread membership: message key → conversation key, rebuilt with
    /// the rows. Lets a read-state change on a hidden reply refresh its head.
    msg_thread: std::collections::HashMap<(u32, u32), (u32, String)>,
    /// Conversation key → member message keys (multi-message threads only).
    thread_members: std::collections::HashMap<(u32, String), Vec<(u32, u32)>>,
    /// Messages actually rendered (after the render limit), independent of how
    /// many rows are visible once threads are collapsed.
    rendered_count: usize,
    /// How many messages to render — grows by `RENDER_CAP` each time the user
    /// scrolls to the bottom (infinite scroll). Reset on folder switch / search.
    render_limit: usize,
    /// Whether the folder's background index is fully loaded. When false, more
    /// rows may still stream in, so hitting the bottom shows a loading spinner.
    index_complete: bool,
    /// The list's scroller, kept so expand/collapse can preserve scroll position.
    scroller: Option<gtk::ScrolledWindow>,
    /// Current sort order for the list.
    sort: SortOrder,
    /// The last count string sent to the header bar, to emit only on change.
    last_count: String,
    /// Group messages into conversation threads (user preference).
    threading: bool,
    /// When `Some`, a spinner + this message overlays the list — shown while a
    /// large bulk action (archive/delete/spam) is applied.
    busy: Option<String>,
    /// Threads whose replies are sliding shut. The rows stay in `shown` until
    /// the paired timer fires and rebuilds the list without them — otherwise
    /// they'd simply vanish rather than animate away.
    collapsing_threads: std::collections::HashMap<(u32, String), gtk::glib::SourceId>,
}

/// How the message list is ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    DateNewest,
    DateOldest,
    Sender,
    Subject,
    UnreadFirst,
    FlaggedFirst,
}

impl SortOrder {
    pub fn from_key(key: &str) -> Self {
        match key {
            "date_oldest" => SortOrder::DateOldest,
            "sender" => SortOrder::Sender,
            "subject" => SortOrder::Subject,
            "unread" => SortOrder::UnreadFirst,
            "flagged" => SortOrder::FlaggedFirst,
            _ => SortOrder::DateNewest,
        }
    }
}

/// Which messages the search field filters over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchScope {
    /// Every folder of every account (the merged `search_pool`).
    AllFolders,
    /// Only the folder currently shown (the local `all` index).
    ThisFolder,
}

/// A bulk action applied to every selected message at once.
#[derive(Debug, Clone, Copy)]
pub enum BulkAction {
    MarkRead,
    MarkUnread,
    Flag,
    Archive,
    Spam,
    Delete,
}

#[derive(Debug)]
pub enum MessageListInput {
    SetMessages { messages: Vec<Message> },
    /// Merge more indexed messages into the current list (background backfill),
    /// preserving the current search query and view.
    AppendMessages { messages: Vec<Message> },
    SetLoading,
    SetThreading(bool),
    /// Reply headers from the account's other folders, so a conversation joined
    /// through a message that isn't on screen still groups.
    SetThreadLinks(Vec<(u32, String, String)>),
    /// Whether conversations start expanded (true) or collapsed (false).
    SetThreadsExpanded(bool),
    SetGravatar(bool),
    /// The GNOME Contacts photo index changed (EDS sync, or the first load
    /// finished) — refresh visible circles without losing the scroll position.
    ContactPhotosChanged,
    /// The open folder is (or stopped being) a Sent folder — rows name the
    /// recipient there instead of the sender (#27).
    SetShowRecipient(bool),
    /// Show or hide the coloured sender circles (#29).
    SetAvatars(bool),
    /// Fill them with senders' own site icons, or stop (#30).
    SetSenderLogos(bool),
    /// The date or clock preference changed: every row's date is built with the
    /// row, so they are built again (#32).
    RefreshDates,
    /// How many lines of preview text each row shows (1–3).
    SetPreviewLines(u32),
    SetColorize(bool),
    /// The local day rolled over — re-render rows so "Today" stays accurate.
    DayChanged,
    SetAccountColors(std::collections::HashMap<u32, String>),
    Search(String),
    /// Change the search scope (all folders vs. the current folder).
    SetScope(SearchScope),
    /// Replace the cross-folder search pool (all folders, all accounts). Sent by
    /// the app when a search begins; cleared to empty when it ends.
    SetSearchPool(Vec<Message>),
    /// The reader's selection: mirror whichever of these the list has rows for,
    /// so everything that acts on the list's selection acts on the messages the
    /// user pointed at. The reader is already showing them, so this must not
    /// re-open anything. Messages the list cannot represent — a reply read in
    /// from Sent, which belongs to another folder — are simply not mirrored;
    /// the reader keeps showing them selected regardless.
    SelectFromReader { keys: Vec<(u32, u32)> },
    /// The set of selected rows changed (single click, Ctrl/Shift multi-select).
    SelectionChanged,
    /// A row was activated (double-click / Enter): pop it out into its own window.
    RowActivated(i32),
    /// Apply a bulk action to every selected message.
    Bulk(BulkAction),
    /// Deselect everything.
    ClearSelection,
    /// Move the selection by `delta` rows (single-key j/k and the arrow keys).
    MoveSelection(i32),
    /// Add or remove the focused row from the selection, without opening it.
    ToggleSelection,
    /// Put keyboard focus on the list (so the arrow keys work again).
    FocusList,
    /// Put the cursor in the search field.
    FocusSearch,
    /// Expand/collapse a conversation thread.
    ToggleThread((u32, String)),
    /// A collapsing thread's replies have finished sliding shut — drop them
    /// from the list for real (see `start_collapse_thread`).
    FinishCollapseThread((u32, String)),
    /// Change the list sort order.
    SetSort(SortOrder),
    MarkRead(u32),
    SetRead { id: u32, read: bool },
    /// A hover-palette action for a specific message (forwarded to the app).
    RowAction { action: RowAction, message: Box<Message> },
    SetStarred { id: u32, starred: bool },
    /// Update a message's attachment indicator (e.g. clearing a false paperclip).
    SetHasAttachment { id: u32, has: bool },
    Remove(u32),
    /// Remove many messages in a single batch (bulk archive/delete/spam), so the
    /// list updates in one render pass instead of one per message.
    RemoveMany(Vec<u32>),
    /// Show (`Some(message)`) or hide (`None`) the busy spinner over the list.
    SetBusy(Option<String>),
    /// Secondary-click at (x, y) in the list: open the context menu.
    ContextMenu { x: f64, y: f64 },
    /// Set the Actions Palette auto-collapse delay (seconds).
    SetPaletteCollapse(u64),
    /// Open the Actions Palette on row hover, without the ⋯ click.
    SetPaletteHover(bool),
    /// Folder switch: reset infinite-scroll paging back to the first page and
    /// scroll to the top (a plain `SetMessages` now preserves paging for refreshes).
    ResetPaging,
    /// The list was scrolled to the bottom — render the next page of messages.
    LoadMore,
    /// Whether the current folder's background index is fully loaded.
    SetIndexComplete(bool),
    /// Mark which message is being viewed so it stays highlighted across
    /// rebuilds; `None` clears the selection (e.g. on folder switch).
    SetSelected(Option<u32>),
    /// Select a message by `(account_id, id)` AND load it in the reader — used to
    /// advance after the viewed message is removed by a background sync.
    SelectAndLoad((u32, u32)),
}

#[derive(Debug)]
pub enum MessageListOutput {
    /// A message was selected. `thread` holds the whole conversation (newest
    /// first) when the newest/head row was chosen, so the reader can show it as a
    /// scrollable conversation; otherwise it's just `[message]`.
    /// A message was opened. `thread` is the conversation to render (just
    /// `message` when it is not one). `solo` marks the deliberate case: the user
    /// picked one reply *inside* a conversation shown in the list, and wants
    /// only that message — so the reader must not go looking for its siblings.
    Selected { message: Message, thread: Vec<Message>, solo: bool },
    /// Every selected message, whenever that changes — the reader outlines the
    /// matching cards.
    SelectionKeys(Vec<(u32, u32)>),
    /// The header-bar count ("N" / "N of M") changed — app.rs shows it.
    CountChanged(String),
    /// A row was double-clicked: open the message in its own window.
    Activated(Message),
    /// A context-menu action chosen for a specific message.
    Action { action: RowAction, message: Box<Message> },
    /// A bulk action chosen for every currently-selected message.
    Bulk { action: BulkAction, messages: Vec<Message> },
    /// The viewed message was removed and no row remains to advance to, so the
    /// reader should clear.
    SelectionCleared,
    /// The search field became active (non-empty) or inactive (empty), so the app
    /// can supply or drop the cross-folder search pool.
    SearchActive(bool),
}

#[relm4::component(pub)]
impl SimpleComponent for MessageList {
    type Init = ();
    type Input = MessageListInput;
    type Output = MessageListOutput;

    view! {
        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,
            add_css_class: "message-list-pane",

            gtk::Box {
                add_css_class: "list-toolbar",
                set_orientation: gtk::Orientation::Vertical,
                set_spacing: 8,

                // The folder name, count and sort control live in the pane's
                // header bar now (app.rs) — only search needs this toolbar.
                gtk::Box {
                    set_spacing: 6,

                    #[name = "search_entry"]
                    gtk::SearchEntry {
                        set_hexpand: true,
                        // Its own default minimum is wider than the rows need.
                        set_width_chars: 3,
                        #[watch]
                        set_placeholder_text: Some(model.search_placeholder()),
                        connect_search_changed[sender] => move |entry| {
                            sender.input(MessageListInput::Search(entry.text().to_string()));
                        },
                    },

                    // Scope picker: 0 = All folders (default), 1 = This folder.
                    #[name = "scope_dropdown"]
                    gtk::DropDown {
                        set_valign: gtk::Align::Center,
                        set_tooltip_text: Some("Choose which folders to search"),
                        set_model: Some(&gtk::StringList::new(&["All folders", "This folder"])),
                        set_selected: 0,
                        connect_selected_notify[sender] => move |dd| {
                            let scope = if dd.selected() == 0 {
                                SearchScope::AllFolders
                            } else {
                                SearchScope::ThisFolder
                            };
                            sender.input(MessageListInput::SetScope(scope));
                        },
                    },
                },
            },

            // Bulk-action bar, revealed while more than one message is selected.
            gtk::Revealer {
                set_transition_type: gtk::RevealerTransitionType::SlideDown,
                #[watch]
                set_reveal_child: model.selection_count > 1,

                // A revealer sliding *down* still reserves its child's width while
                // collapsed, so this bar of seven buttons was setting the whole
                // pane's minimum width — 340px — however narrow the rows became
                // (#29). Scrolled, its minimum is nothing and its natural size is
                // unchanged, so it only clips when the list is genuinely too narrow
                // to hold it.
                gtk::ScrolledWindow {
                    set_vscrollbar_policy: gtk::PolicyType::Never,
                    set_hscrollbar_policy: gtk::PolicyType::External,
                    set_propagate_natural_width: true,
                    set_propagate_natural_height: true,

                gtk::Box {
                    add_css_class: "bulk-bar",
                    set_spacing: 2,

                    gtk::Label {
                        #[watch]
                        set_label: &format!("{} selected", model.selection_count),
                        set_hexpand: true,
                        set_halign: gtk::Align::Start,
                        set_ellipsize: gtk::pango::EllipsizeMode::End,
                        add_css_class: "bulk-count",
                    },
                    gtk::Button {
                        set_icon_name: "co.hyprlab.Vireo-mail-read-symbolic",
                        set_tooltip_text: Some("Mark as Read"),
                        add_css_class: "flat",
                        connect_clicked => MessageListInput::Bulk(BulkAction::MarkRead),
                    },
                    gtk::Button {
                        set_icon_name: "co.hyprlab.Vireo-mail-unread-symbolic",
                        set_tooltip_text: Some("Mark as Unread"),
                        add_css_class: "flat",
                        connect_clicked => MessageListInput::Bulk(BulkAction::MarkUnread),
                    },
                    gtk::Button {
                        set_icon_name: "co.hyprlab.Vireo-starred-symbolic",
                        set_tooltip_text: Some("Flag"),
                        add_css_class: "flat",
                        connect_clicked => MessageListInput::Bulk(BulkAction::Flag),
                    },
                    gtk::Button {
                        set_icon_name: "co.hyprlab.Vireo-mail-archive-symbolic",
                        set_tooltip_text: Some("Archive"),
                        add_css_class: "flat",
                        connect_clicked => MessageListInput::Bulk(BulkAction::Archive),
                    },
                    gtk::Button {
                        set_icon_name: "co.hyprlab.Vireo-mail-mark-junk-symbolic",
                        set_tooltip_text: Some("Mark as Spam"),
                        add_css_class: "flat",
                        connect_clicked => MessageListInput::Bulk(BulkAction::Spam),
                    },
                    gtk::Button {
                        set_icon_name: "co.hyprlab.Vireo-user-trash-symbolic",
                        set_tooltip_text: Some("Delete"),
                        add_css_class: "flat",
                        connect_clicked => MessageListInput::Bulk(BulkAction::Delete),
                    },
                    gtk::Separator {
                        set_orientation: gtk::Orientation::Vertical,
                    },
                    gtk::Button {
                        set_icon_name: "co.hyprlab.Vireo-edit-clear-symbolic",
                        set_tooltip_text: Some("Clear selection"),
                        add_css_class: "flat",
                        connect_clicked => MessageListInput::ClearSelection,
                    },
                },
                },
            },

            gtk::Overlay {
                #[wrap(Some)]
                #[name = "scroller"]
                set_child = &gtk::ScrolledWindow {
                set_vexpand: true,
                // External, not Never: with Never the widest row's minimum (the
                // Actions Palette reservation plus the avatar column) propagates
                // all the way up and becomes part of the window's minimum width,
                // which pushed it past half of a 1920px screen — at which point
                // GNOME refuses to tile the window to the left/right edge. Rows
                // ellipsize, so a narrow pane clips gracefully instead.
                set_hscrollbar_policy: gtk::PolicyType::External,
                // The pane's own floor, now that rows no longer set one: room
                // for a row's full Actions Palette (avatar + dot + the reserved
                // actions line), so opening the palette never needs to clip —
                // the narrow-window breakpoint rails the sidebar in time to
                // afford this even in a half-screen tile. (Grows by the thread
                // indent while a conversation is expanded — see the rebuild.)
                set_size_request: (LIST_MIN_WIDTH, -1),

                // Reaching the bottom pulls in the next page (and, if the index is
                // still loading, shows the spinner below until more arrive).
                connect_edge_reached[sender] => move |_, pos| {
                    if pos == gtk::PositionType::Bottom {
                        sender.input(MessageListInput::LoadMore);
                    }
                },

                gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,

                    #[local_ref]
                    row_box -> gtk::ListBox {
                        // Multiple selection: plain click selects one (shown in the
                        // reader), Ctrl/Shift extend the selection for bulk actions;
                        // double click (or Enter) pops a message out into its own window.
                        set_selection_mode: gtk::SelectionMode::Multiple,
                        set_activate_on_single_click: false,
                        add_css_class: "message-listbox",
                        connect_selected_rows_changed[sender] => move |_| {
                            sender.input(MessageListInput::SelectionChanged);
                        },
                        connect_row_activated[sender] => move |_, row| {
                            sender.input(MessageListInput::RowActivated(row.index()));
                        },
                    },

                    // Bottom loading indicator while the rest of the folder streams in.
                    gtk::Box {
                        add_css_class: "list-loading",
                        set_halign: gtk::Align::Center,
                        set_spacing: 8,
                        set_margin_top: 10,
                        set_margin_bottom: 14,
                        #[watch]
                        set_visible: model.is_loading_more(),

                        gtk::Spinner {
                            set_spinning: true,
                            set_width_request: 18,
                            set_height_request: 18,
                        },
                        gtk::Label {
                            set_label: "Loading more…",
                            add_css_class: "dim-label",
                        },
                    },
                },
                },

                add_overlay = &gtk::Box {
                    add_css_class: "bulk-busy",
                    set_halign: gtk::Align::Center,
                    set_valign: gtk::Align::Center,
                    set_orientation: gtk::Orientation::Vertical,
                    set_spacing: 12,
                    #[watch]
                    set_visible: model.busy.is_some(),

                    gtk::Spinner {
                        set_spinning: true,
                        set_width_request: 32,
                        set_height_request: 32,
                    },
                    gtk::Label {
                        #[watch]
                        set_label: model.busy.as_deref().unwrap_or(""),
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
        let rows = FactoryVecDeque::builder()
            .launch(gtk::ListBox::new())
            .forward(sender.input_sender(), |out| match out {
                MessageRowOutput::Action { action, message } => {
                    MessageListInput::RowAction { action, message }
                }
                MessageRowOutput::ToggleThread(key) => MessageListInput::ToggleThread(key),
            });

        let color_provider = gtk::CssProvider::new();
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &color_provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }

        let mut model = MessageList {
            rows,
            all: Vec::new(),
            search_pool: Vec::new(),
            scope: SearchScope::AllFolders,
            search_entry: None,
            shown: Vec::new(),
            total_matches: 0,
            render_limit: RENDER_CAP,
            index_complete: true,
            query: String::new(),
            gravatar: false,
            avatars: true,
            sender_logos: false,
            preview_lines: 1,
            colorize: false,
            account_colors: std::collections::HashMap::new(),
            color_provider,
            palette_collapse_secs: std::rc::Rc::new(std::cell::Cell::new(5)),
            palette_hover: std::rc::Rc::new(std::cell::Cell::new(
                crate::config::load_list_palette_hover(),
            )),
            thread_links: Vec::new(),
            drag_keys: DragKeys::default(),
            selected_id: None,
            selected_ids: Vec::new(),
            selection_count: 0,
            from_reader: 0,
            reader_keys: Vec::new(),
            expanded_threads: std::collections::HashSet::new(),
            show_recipient: false,
            default_expanded: false,
            msg_thread: std::collections::HashMap::new(),
            thread_members: std::collections::HashMap::new(),
            rendered_count: 0,
            scroller: None,
            sort: SortOrder::DateNewest,
            last_count: String::new(),
            threading: true,

            busy: None,
            collapsing_threads: std::collections::HashMap::new(),
        };

        let row_box = model.rows.widget();

        // Right-click (or long-press) a row to open its context menu.
        let click = gtk::GestureClick::new();
        click.set_button(gtk::gdk::BUTTON_SECONDARY);
        let cs = sender.clone();
        click.connect_pressed(move |_, _, x, y| {
            cs.input(MessageListInput::ContextMenu { x, y });
        });
        row_box.add_controller(click);

        // Delete / Backspace on a focused row deletes the selection (single or
        // multi). Scoped to the list, so typing in the search box is unaffected.
        let key = gtk::EventControllerKey::new();
        let ks = sender.clone();
        key.connect_key_pressed(move |_, keyval, _, _| {
            if matches!(keyval, gtk::gdk::Key::Delete | gtk::gdk::Key::BackSpace) {
                ks.input(MessageListInput::Bulk(BulkAction::Delete));
                gtk::glib::Propagation::Stop
            } else {
                gtk::glib::Propagation::Proceed
            }
        });
        row_box.add_controller(key);

        let widgets = view_output!();
        model.scroller = Some(widgets.scroller.clone());
        model.search_entry = Some(widgets.search_entry.clone());

        // The scope picker sizes itself to its widest entry ("All folders"), which
        // made it — not the messages — the narrowest the list could ever be (#29).
        // An ellipsizing label on the button lets it give way; the drop-down list
        // keeps its own factory, so the choices are still spelled out in full.
        let button_factory = gtk::SignalListItemFactory::new();
        button_factory.connect_setup(|_, item| {
            let label = gtk::Label::new(None);
            label.set_xalign(0.0);
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            if let Some(item) = item.downcast_ref::<gtk::ListItem>() {
                item.set_child(Some(&label));
            }
        });
        button_factory.connect_bind(|_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let text = item
                .item()
                .and_downcast::<gtk::StringObject>()
                .map(|s| s.string().to_string())
                .unwrap_or_default();
            if let Some(label) = item.child().and_downcast::<gtk::Label>() {
                label.set_label(&text);
            }
        });
        widgets.scope_dropdown.set_factory(Some(&button_factory));

        schedule_midnight_refresh(&sender);

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            MessageListInput::SetMessages { messages } => {
                self.all = messages;
                // Keep any active search query: this also fires for a background
                // re-sync of the folder you're viewing, which shouldn't drop your
                // search. Folder switches clear the query via `ResetPaging` first.
                self.rebuild_preserving_scroll();
            }
            MessageListInput::AppendMessages { messages } => {
                // Grow the searchable index in place. Dedup by (account, uid) since
                // UIDs collide across accounts in the unified inbox.
                let existing: std::collections::HashSet<(u32, u32)> =
                    self.all.iter().map(|m| (m.account_id, m.uid)).collect();
                let before = self.all.len();
                for m in messages {
                    if !existing.contains(&(m.account_id, m.uid)) {
                        self.all.push(m);
                    }
                }
                // Re-render when it could change what's visible: an active search, a
                // sort where older messages can surface at the top, or the user is
                // waiting at the bottom for more rows to fill the raised limit.
                let waiting_for_more = self.render_limit > self.rendered_count;
                if self.all.len() != before
                    && (!self.query.is_empty()
                        || self.sort != SortOrder::DateNewest
                        || waiting_for_more)
                {
                    self.rebuild_preserving_scroll();
                }
            }
            MessageListInput::SetLoading => {
                self.all.clear();
                self.clear_search();
                self.render_limit = RENDER_CAP;
                self.rebuild();
            }
            MessageListInput::ResetPaging => {
                // Folder switch: drop any active search, back to the first page,
                // scrolled to the top.
                self.clear_search();
                self.render_limit = RENDER_CAP;
                if let Some(s) = &self.scroller {
                    s.vadjustment().set_value(0.0);
                }
            }
            MessageListInput::LoadMore => {
                // Show more if the index already has more, or if it's still loading
                // (the spinner covers the wait, and appended rows fill in).
                if self.rendered_count < self.total_matches || !self.index_complete {
                    self.render_limit = self.render_limit.saturating_add(RENDER_CAP);
                    self.rebuild_preserving_scroll();
                }
            }
            MessageListInput::SetIndexComplete(complete) => {
                self.index_complete = complete;
            }
            MessageListInput::SetThreadLinks(links) => {
                if self.thread_links != links {
                    self.thread_links = links;
                    if self.threading {
                        self.rebuild_preserving_scroll();
                    }
                }
            }
            MessageListInput::SetThreading(on) => {
                if self.threading != on {
                    self.threading = on;
                    self.rebuild();
                }
            }
            MessageListInput::SetThreadsExpanded(on) => {
                if self.default_expanded != on {
                    self.default_expanded = on;
                    // Per-thread toggles were exceptions to the old default;
                    // drop them so everything follows the new one.
                    self.expanded_threads.clear();
                    self.rebuild();
                }
            }
            MessageListInput::RefreshDates => self.rebuild_preserving_scroll(),
            MessageListInput::SetSenderLogos(on) => {
                if self.sender_logos != on {
                    self.sender_logos = on;
                    // The circle is filled when the row is built.
                    self.rebuild_preserving_scroll();
                }
            }
            MessageListInput::SetAvatars(on) => {
                if self.avatars != on {
                    self.avatars = on;
                    // The circle is built with the row, so the rows have to be
                    // built again for the width to come back.
                    self.rebuild_preserving_scroll();

                }
            }
            MessageListInput::SetGravatar(on) => {
                if self.gravatar != on {
                    self.gravatar = on;
                    self.rebuild();
                }
            }
            MessageListInput::SetShowRecipient(on) => {
                if self.show_recipient != on {
                    self.show_recipient = on;
                    self.rebuild();
                }
            }
            MessageListInput::ContactPhotosChanged => {
                // Pointless when the circles aren't drawn; rows check the
                // fresh index as they are rebuilt.
                if self.avatars {
                    self.rebuild_preserving_scroll();
                }
            }
            MessageListInput::SetPreviewLines(lines) => {
                let lines = lines.min(3);
                if self.preview_lines != lines {
                    self.preview_lines = lines;
                    // Row height is set when the row is built, so the list has to
                    // be rebuilt rather than nudged.
                    self.rebuild();
                }
            }
            MessageListInput::SetColorize(on) => {
                if self.colorize != on {
                    self.colorize = on;
                    self.rebuild();
                }
            }
            MessageListInput::DayChanged => {
                // Re-render so relative labels like "Today" reflect the new date.
                self.rebuild();
                schedule_midnight_refresh(&sender);
            }
            MessageListInput::SetAccountColors(colors) => {
                self.account_colors = colors;
                self.refresh_tint_css();
                // Existing rows keep their classes; the rule update reaches them.
            }
            MessageListInput::Search(q) => {
                let was_active = self.searching();
                self.query = q;
                let now_active = self.searching();
                // On the empty↔non-empty edge, tell the app to supply or drop the
                // cross-folder pool.
                if was_active != now_active {
                    let _ = sender.output(MessageListOutput::SearchActive(now_active));
                }
                self.render_limit = RENDER_CAP;
                self.rebuild();
            }
            MessageListInput::SetScope(scope) => {
                if self.scope != scope {
                    self.scope = scope;
                    // Scope only affects the view while a query is present.
                    if self.searching() {
                        self.render_limit = RENDER_CAP;
                        self.rebuild();
                    }
                }
            }
            MessageListInput::SetSearchPool(pool) => {
                self.search_pool = pool;
                if self.searching() && self.scope == SearchScope::AllFolders {
                    self.render_limit = RENDER_CAP;
                    self.rebuild_preserving_scroll();
                }
            }
            MessageListInput::SelectFromReader { keys } => {
                // An empty set means the reader cleared its card selection (a
                // click on the document's empty space). The list keeps the
                // viewed message highlighted rather than losing its anchor —
                // the focus-within CSS dims the highlight instead.
                let keys = if keys.is_empty() {
                    self.selected_id.into_iter().collect()
                } else {
                    keys
                };
                // The list shows only a thread's head while it is collapsed, so
                // selecting a reply has to open the thread first — otherwise
                // there is no row to select and only the head would ever answer.
                let hidden: Vec<(u32, String)> = keys
                    .iter()
                    .filter(|k| !self.shown.iter().any(|m| (m.account_id, m.id) == **k))
                    .filter_map(|k| self.msg_thread.get(k).cloned())
                    .collect();
                if !hidden.is_empty() {
                    for thread_key in hidden {
                        // `expanded_threads` records the departure from the
                        // default, so which way to move it depends on that.
                        if self.default_expanded {
                            self.expanded_threads.remove(&thread_key);
                        } else {
                            self.expanded_threads.insert(thread_key);
                        }
                    }
                    self.rebuild_preserving_scroll();
                }
                let list = self.rows.widget();
                list.unselect_all();
                for key in &keys {
                    if let Some(idx) = self.shown.iter().position(|m| (m.account_id, m.id) == *key)
                    {
                        if let Some(row) = list.row_at_index(idx as i32) {
                            list.select_row(Some(&row));
                        }
                    }
                }
                // What the reader asked for, as this list can represent it, so
                // the changes GTK is about to report are recognised as ours
                // rather than the user's.
                self.reader_keys = list
                    .selected_rows()
                    .iter()
                    .filter_map(|r| self.shown.get(r.index() as usize).map(|m| (m.account_id, m.id)))
                    .collect();
                self.from_reader = 8;
                sender.input(MessageListInput::SelectionChanged);
            }
            MessageListInput::SelectionChanged => {
                let keys: Vec<(u32, u32)> = self
                    .rows
                    .widget()
                    .selected_rows()
                    .iter()
                    .filter_map(|r| self.shown.get(r.index() as usize).map(|m| (m.account_id, m.id)))
                    .collect();
                self.selection_count = keys.len();
                // Set from the reader, which is already showing these messages:
                // mirror the selection but leave the reader alone. Reporting it
                // back would also drop whatever the reader has selected that this
                // list has no row for.
                if self.from_reader > 0 {
                    self.from_reader -= 1;
                    if keys == self.reader_keys {
                        self.selected_ids = keys;
                        return;
                    }
                    // Something else moved the selection — stop expecting ours.
                    self.from_reader = 0;
                }
                // A selection the user made here; the reader outlines it.
                let _ = sender.output(MessageListOutput::SelectionKeys(keys.clone()));
                match keys.as_slice() {
                    [] => self.selected_id = None,
                    [key] => {
                        // Exactly one selected → show it in the reader. Skip when
                        // it's already the viewed row (e.g. programmatic restore
                        // after a rebuild) so it isn't needlessly reloaded.
                        if self.selected_id != Some(*key) {
                            self.selected_id = Some(*key);
                            if let Some(m) = self
                                .shown
                                .iter()
                                .find(|m| (m.account_id, m.id) == *key)
                                .cloned()
                            {
                                let (thread, solo) = self.conversation_for(&m);
                                let _ = sender.output(MessageListOutput::Selected {
                                    message: m,
                                    thread,
                                    solo,
                                });
                            }
                        }
                    }
                    // Multiple selected → keep the reader on the primary message.
                    _ => {}
                }
                self.selected_ids = keys;
            }
            MessageListInput::Bulk(action) => {
                let messages: Vec<Message> = self
                    .rows
                    .widget()
                    .selected_rows()
                    .iter()
                    .filter_map(|r| self.shown.get(r.index() as usize).cloned())
                    .collect();
                if !messages.is_empty() {
                    let _ = sender.output(MessageListOutput::Bulk { action, messages });
                }
                self.rows.widget().unselect_all();
                self.selected_id = None;
                self.selected_ids.clear();
                self.selection_count = 0;
            }
            MessageListInput::MoveSelection(delta) => {
                let list = self.rows.widget();
                if self.shown.is_empty() {
                    return;
                }
                // From the current row, or from the top/bottom when nothing is
                // selected yet, so the first keypress always lands somewhere.
                let current = list
                    .selected_rows()
                    .first()
                    .map(|r| r.index())
                    .unwrap_or(if delta > 0 { -1 } else { self.shown.len() as i32 });
                let next = (current + delta).clamp(0, self.shown.len() as i32 - 1);
                if let Some(row) = list.row_at_index(next) {
                    list.unselect_all();
                    list.select_row(Some(&row));
                    row.grab_focus();
                }
            }

            MessageListInput::ToggleSelection => {
                let list = self.rows.widget();
                let Some(row) = list.focus_child().and_downcast::<gtk::ListBoxRow>().or_else(|| {
                    list.selected_rows().first().cloned()
                }) else {
                    return;
                };
                if row.is_selected() {
                    list.unselect_row(&row);
                } else {
                    list.select_row(Some(&row));
                }
            }

            MessageListInput::FocusList => {
                let list = self.rows.widget();
                let row = list
                    .selected_rows()
                    .first()
                    .cloned()
                    .or_else(|| list.row_at_index(0));
                if let Some(row) = row {
                    row.grab_focus();
                }
            }

            MessageListInput::FocusSearch => {
                if let Some(entry) = &self.search_entry {
                    entry.grab_focus();
                }
            }

            MessageListInput::ClearSelection => {
                self.rows.widget().unselect_all();
                self.selected_id = None;
                self.selected_ids.clear();
                self.selection_count = 0;
            }
            MessageListInput::SetSort(order) => {
                if self.sort != order {
                    self.sort = order;
                    self.rebuild();
                }
            }
            MessageListInput::ToggleThread(key) => {
                let was_expanded = self.expanded_threads.contains(&key) != self.default_expanded;
                if was_expanded {
                    // Slide the replies shut in place; the list only drops
                    // them once that animation has actually finished (see
                    // `start_collapse_thread`) — dropping them right away
                    // would just make them vanish instead of collapsing.
                    self.start_collapse_thread(key, &sender);
                } else {
                    if !self.expanded_threads.remove(&key) {
                        self.expanded_threads.insert(key.clone());
                    }
                    // Insert just this thread's replies rather than rebuilding
                    // the whole list — on a long list a full rebuild tears
                    // down and recreates every row (up to RENDER_CAP), which
                    // stutters right as the reveal animation is trying to run.
                    self.expand_thread(&key);
                }
            }
            MessageListInput::FinishCollapseThread(key) => {
                self.collapsing_threads.remove(&key);
                if !self.expanded_threads.remove(&key) {
                    self.expanded_threads.insert(key.clone());
                }
                // Same reasoning as `expand_thread`: drop just these rows.
                self.collapse_thread_rows(&key);
            }
            MessageListInput::RowActivated(index) => {
                if let Some(m) = self.shown.get(index as usize) {
                    let _ = sender.output(MessageListOutput::Activated(m.clone()));
                }
            }
            MessageListInput::MarkRead(id) => {
                if let Some(m) = self.all.iter_mut().find(|m| m.id == id) {
                    m.unread = false;
                }
                if let Some(idx) = self.shown.iter().position(|m| m.id == id) {
                    self.shown[idx].unread = false;
                    self.rows.send(idx, MessageRowInput::SetRead(true));
                }
                self.refresh_thread_unread(id);
            }
            MessageListInput::SetRead { id, read } => {
                if let Some(m) = self.all.iter_mut().find(|m| m.id == id) {
                    m.unread = !read;
                }
                if let Some(idx) = self.shown.iter().position(|m| m.id == id) {
                    self.shown[idx].unread = !read;
                    self.rows.send(idx, MessageRowInput::SetRead(read));
                }
                self.refresh_thread_unread(id);
            }
            MessageListInput::SetStarred { id, starred } => {
                if let Some(m) = self.all.iter_mut().find(|m| m.id == id) {
                    m.starred = starred;
                }
                if let Some(idx) = self.shown.iter().position(|m| m.id == id) {
                    self.shown[idx].starred = starred;
                    self.rows.send(idx, MessageRowInput::SetStarred(starred));
                }
            }
            MessageListInput::SetHasAttachment { id, has } => {
                if let Some(m) = self.all.iter_mut().find(|m| m.id == id) {
                    m.has_attachment = has;
                }
                if let Some(idx) = self.shown.iter().position(|m| m.id == id) {
                    self.shown[idx].has_attachment = has;
                    self.rows.send(idx, MessageRowInput::SetHasAttachment(has));
                }
            }
            MessageListInput::Remove(id) => {
                // Was the removed message the one shown in the reader? If so we'll
                // advance to whatever row slides into its place.
                let was_viewed = self.selected_id.map(|(_, i)| i) == Some(id);
                if was_viewed {
                    self.selected_id = None;
                }
                self.selected_ids.retain(|(_, i)| *i != id);
                self.all.retain(|m| m.id != id);
                let removed_idx = self.shown.iter().position(|m| m.id == id);
                if let Some(idx) = removed_idx {
                    self.shown.remove(idx);
                    self.rows.guard().remove(idx);
                    self.publish_drag_keys();
                }

                if was_viewed {
                    match removed_idx {
                        // Select the row now at the removed slot (or the new last
                        // row if we deleted the bottom one). Selecting it fires
                        // SelectionChanged, which loads it in the reader.
                        Some(idx) if !self.shown.is_empty() => {
                            let next = idx.min(self.shown.len() - 1);
                            self.select_and_focus(next);
                        }
                        // Nothing left to show → clear the reader.
                        _ => {
                            let _ = sender.output(MessageListOutput::SelectionCleared);
                        }
                    }
                }
            }
            MessageListInput::RemoveMany(ids) => {
                if ids.is_empty() {
                    return;
                }
                let set: std::collections::HashSet<u32> = ids.into_iter().collect();
                let was_viewed = self.selected_id.map(|(_, i)| i).is_some_and(|i| set.contains(&i));
                if was_viewed {
                    self.selected_id = None;
                }
                self.selected_ids.retain(|(_, i)| !set.contains(i));
                self.all.retain(|m| !set.contains(&m.id));
                // Where the first removed row sat, so we can re-select in its place.
                let first_removed = self.shown.iter().position(|m| set.contains(&m.id));
                // Remove all matching rows in one guarded batch (a single widget
                // update) instead of one render cycle per message. Walk back-to-front
                // so indices stay valid.
                {
                    let mut guard = self.rows.guard();
                    let mut idx = self.shown.len();
                    while idx > 0 {
                        idx -= 1;
                        if set.contains(&self.shown[idx].id) {
                            self.shown.remove(idx);
                            guard.remove(idx);
                        }
                    }
                }
                self.publish_drag_keys();
                self.selection_count = self.selected_ids.len();
                if was_viewed {
                    match first_removed {
                        Some(idx) if !self.shown.is_empty() => {
                            let next = idx.min(self.shown.len() - 1);
                            self.select_and_focus(next);
                        }
                        _ => {
                            let _ = sender.output(MessageListOutput::SelectionCleared);
                        }
                    }
                }
            }
            MessageListInput::SetBusy(text) => {
                self.busy = text;
            }
            MessageListInput::RowAction { action, message } => {
                let _ = sender.output(MessageListOutput::Action { action, message });
            }
            MessageListInput::SetPaletteCollapse(secs) => self.palette_collapse_secs.set(secs),
            MessageListInput::SetPaletteHover(on) => self.palette_hover.set(on),
            MessageListInput::SetSelected(id) => {
                match id {
                    // Account-less id resolved against the shown list (the app
                    // only sends `None` today; `Some` kept for completeness).
                    Some(i) => {
                        if let Some(m) = self.shown.iter().find(|m| m.id == i) {
                            let key = (m.account_id, m.id);
                            self.selected_id = Some(key);
                            self.selected_ids = vec![key];
                            self.select_current();
                        }
                    }
                    None => {
                        self.selected_id = None;
                        self.selected_ids.clear();
                        self.selection_count = 0;
                        self.rows.widget().unselect_all();
                    }
                }
            }
            MessageListInput::SelectAndLoad(key) => {
                if let Some(m) = self.shown.iter().find(|m| (m.account_id, m.id) == key).cloned() {
                    self.selected_id = Some(key);
                    self.selected_ids = vec![key];
                    self.select_current();
                    let (thread, solo) = self.conversation_for(&m);
                    let _ = sender.output(MessageListOutput::Selected { message: m, thread, solo });
                }
            }
            MessageListInput::ContextMenu { x, y } => {
                if let Some(row) = self.rows.widget().row_at_y(y as i32) {
                    let list = self.rows.widget();
                    let selected = list.selected_rows();
                    let in_selection = selected.iter().any(|r| r.index() == row.index());
                    if selected.len() > 1 && in_selection {
                        // Right-clicked inside a multi-selection → bulk menu.
                        self.show_bulk_menu(x, y, &sender);
                    } else {
                        // Single-row menu acting on the clicked message. Crucially,
                        // don't select it — selecting would load it in the reader,
                        // and the user may just intend to move/archive/delete it.
                        if let Some(msg) = self.shown.get(row.index() as usize).cloned() {
                            self.show_context_menu(&msg, x, y, &sender);
                        }
                    }
                }
            }
        }
        // Whatever just happened, restate the header count if it moved — every
        // path that changes the visible rows funnels through here.
        let count = self.count_label();
        if count != self.last_count {
            self.last_count = count.clone();
            let _ = sender.output(MessageListOutput::CountChanged(count));
        }

    }
}

impl MessageList {
    /// Build and pop up the right-click menu for `msg` at the click location.
    fn show_context_menu(
        &self,
        msg: &Message,
        x: f64,
        y: f64,
        sender: &ComponentSender<Self>,
    ) {
        // Each entry carries the same icon as the reader-toolbar button (or
        // row-palette button) for that action, tying the two together.
        let item = |action: RowAction, label: &str, icon: &str| -> MenuEntry {
            let s = sender.clone();
            let m = msg.clone();
            MenuEntry::new(label, move || {
                let _ = s.output(MessageListOutput::Action {
                    action,
                    message: Box::new(m.clone()),
                });
            })
            .icon(icon)
        };

        let sections = vec![
            vec![
                item(RowAction::Reply, "Reply", "co.hyprlab.Vireo-mail-reply-sender-symbolic"),
                item(RowAction::ReplyAll, "Reply All", "co.hyprlab.Vireo-mail-reply-all-symbolic"),
                item(RowAction::Forward, "Forward", "co.hyprlab.Vireo-mail-forward-symbolic"),
            ],
            vec![
                if msg.starred {
                    item(RowAction::ToggleStar, "Remove Star", "co.hyprlab.Vireo-non-starred-symbolic")
                } else {
                    item(RowAction::ToggleStar, "Star", "co.hyprlab.Vireo-starred-symbolic")
                },
                if msg.unread {
                    item(RowAction::ToggleRead, "Mark as Read", "co.hyprlab.Vireo-mail-read-symbolic")
                } else {
                    item(RowAction::ToggleRead, "Mark as Unread", "co.hyprlab.Vireo-mail-unread-symbolic")
                },
            ],
            vec![
                item(RowAction::Spam, "Mark as Spam", "co.hyprlab.Vireo-mail-mark-junk-symbolic"),
                item(RowAction::Archive, "Archive", "co.hyprlab.Vireo-mail-archive-symbolic"),
                item(RowAction::Delete, "Delete", "co.hyprlab.Vireo-user-trash-symbolic"),
            ],
            vec![item(RowAction::ViewSource, "View Source", "co.hyprlab.Vireo-code-symbolic")],
        ];

        show_context_menu(self.rows.widget(), x, y, sections);
    }

    /// Build and pop up the bulk-action menu for the current multi-selection.
    fn show_bulk_menu(&self, x: f64, y: f64, sender: &ComponentSender<Self>) {
        let item = |action: BulkAction, label: &str, icon: &str| -> MenuEntry {
            let s = sender.clone();
            MenuEntry::new(label, move || s.input(MessageListInput::Bulk(action))).icon(icon)
        };

        let sections = vec![
            vec![
                item(BulkAction::MarkRead, "Mark as Read", "co.hyprlab.Vireo-mail-read-symbolic"),
                item(BulkAction::MarkUnread, "Mark as Unread", "co.hyprlab.Vireo-mail-unread-symbolic"),
                item(BulkAction::Flag, "Flag", "co.hyprlab.Vireo-starred-symbolic"),
            ],
            vec![
                item(BulkAction::Spam, "Mark as Spam", "co.hyprlab.Vireo-mail-mark-junk-symbolic"),
                item(BulkAction::Archive, "Archive", "co.hyprlab.Vireo-mail-archive-symbolic"),
                item(BulkAction::Delete, "Delete", "co.hyprlab.Vireo-user-trash-symbolic"),
            ],
        ];

        show_context_menu_with_header(
            self.rows.widget(),
            x,
            y,
            Some(&format!("{} selected", self.selection_count)),
            sections,
        );
    }

    /// Toolbar count: total matches, noting when more exist than are shown.
    fn count_label(&self) -> String {
        if self.total_matches > self.rendered_count {
            format!("{} of {}", self.rendered_count, self.total_matches)
        } else {
            format!("{}", self.total_matches)
        }
    }

    /// Whether the bottom loading spinner should show: the user has scrolled past
    /// what's loaded (`render_limit` exceeds the indexed count) and the folder's
    /// index is still streaming in.
    fn is_loading_more(&self) -> bool {
        self.render_limit > self.total_matches && !self.index_complete
    }

    /// A full rebuild that keeps the current scroll offset (a plain rebuild jumps
    /// to the top). Used when growing the list beneath the user.
    fn rebuild_preserving_scroll(&mut self) {
        let saved = self.scroller.as_ref().map(|s| s.vadjustment().value());
        self.rebuild();
        if let (Some(pos), Some(scroller)) = (saved, self.scroller.clone()) {
            let adj = scroller.vadjustment();
            gtk::glib::idle_add_local_once(move || adj.set_value(pos));
        }
    }

    /// A message's read state changed: recompute its conversation's aggregate
    /// unread flag and push it to the head row, so a collapsed thread's heavy
    /// highlight clears exactly when its last unread message is read.
    fn refresh_thread_unread(&mut self, id: u32) {
        let Some(key) = self
            .all
            .iter()
            .find(|m| m.id == id)
            .map(|m| (m.account_id, m.id))
        else {
            return;
        };
        let Some(tkey) = self.msg_thread.get(&key) else {
            return;
        };
        let Some(members) = self.thread_members.get(tkey).cloned() else {
            return;
        };
        let any_unread = members
            .iter()
            .any(|k| self.all.iter().any(|m| (m.account_id, m.id) == *k && m.unread));
        // The head is the first (and, when collapsed, only) member in `shown`.
        if let Some(idx) = self
            .shown
            .iter()
            .position(|m| members.contains(&(m.account_id, m.id)))
        {
            self.rows.send(idx, MessageRowInput::SetThreadUnread(any_unread));
        }
    }

    /// Begin collapsing a thread: slide its currently-visible replies shut
    /// (they stay exactly where they are in `self.rows`/`self.shown` — only
    /// their own Revealer closes), then rebuild the list without them once
    /// that animation has actually finished. Rebuilding right away — before
    /// the replies have shrunk — would just make them disappear outright.
    fn start_collapse_thread(&mut self, key: (u32, String), sender: &ComponentSender<Self>) {
        if let Some(members) = self.thread_members.get(&key).cloned() {
            // The head survives the toggle (only its replies are removed),
            // so its own chevron can rotate shut in place, in step with the
            // replies sliding closed beneath it.
            if let Some(&head_key) = members.first() {
                if let Some(idx) = self.shown.iter().position(|m| (m.account_id, m.id) == head_key)
                {
                    self.rows.send(idx, MessageRowInput::SetThreadExpanded(false));
                }
            }
            // `members` is head-first (see `rebuild`); only the replies
            // beneath it animate closed.
            for child_key in members.iter().skip(1) {
                if let Some(idx) =
                    self.shown.iter().position(|m| (m.account_id, m.id) == *child_key)
                {
                    self.rows.send(idx, MessageRowInput::SetRevealed(false));
                }
            }
        }
        if let Some(old) = self.collapsing_threads.remove(&key) {
            old.remove();
        }
        let s = sender.clone();
        let timer_key = key.clone();
        // Matches the Revealer's own transition duration, so the rows are
        // fully closed by the time they're actually dropped from the list.
        let timer = gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(200), move || {
            s.input(MessageListInput::FinishCollapseThread(timer_key));
        });
        self.collapsing_threads.insert(key, timer);
    }

    /// Insert a thread's replies right after its head, without touching any
    /// other row — the counterpart to `collapse_thread_rows`. Each new row
    /// mounts with `revealed: false` and animates open on its own (see
    /// `RowInit::revealed`).
    fn expand_thread(&mut self, key: &(u32, String)) {
        let Some(members) = self.thread_members.get(key).cloned() else { return };
        let Some(&head_key) = members.first() else { return };
        let Some(head_pos) = self.shown.iter().position(|m| (m.account_id, m.id) == head_key)
        else {
            return;
        };
        // `members` is already oldest-first (see `rebuild`); look the replies
        // up from the full index before borrowing `self.rows`/`self.shown`
        // mutably.
        let children: Vec<Message> = {
            let source = self.active_source();
            members
                .iter()
                .skip(1)
                .filter_map(|k| source.iter().find(|m| (m.account_id, m.id) == *k).cloned())
                .collect()
        };
        if children.is_empty() {
            return;
        }
        // The head survives the toggle (only its replies are inserted), so
        // its own chevron can rotate open in place.
        self.rows.send(head_pos, MessageRowInput::SetThreadExpanded(true));
        {
            let mut guard = self.rows.guard();
            for (i, msg) in children.iter().enumerate() {
                let ring_class = if self.colorize && self.account_colors.contains_key(&msg.account_id)
                {
                    Some(format!("vireo-acct-ring-{}", msg.account_id))
                } else {
                    None
                };
                guard.insert(head_pos + 1 + i, RowInit {
                    msg: msg.clone(),
                    gravatar: self.gravatar,
                    avatars: self.avatars,
                    sender_logos: self.sender_logos,
                    preview_lines: self.preview_lines,
                    ring_class,
                    palette_collapse_secs: self.palette_collapse_secs.clone(),
                    palette_hover: self.palette_hover.clone(),
                    thread_count: 0,
                    is_thread_child: true,
                    thread_expanded: false,
                    thread_key: None,
                    thread_date: None,
                    thread_unread: false,
                    drag_keys: self.drag_keys.clone(),
                    show_recipient: self.show_recipient,
                    revealed: false,
                });
            }
        }
        self.shown.splice(head_pos + 1..head_pos + 1, children);
        self.publish_drag_keys();
    }

    /// Drop a thread's reply rows (already slid shut by `start_collapse_thread`)
    /// without touching any other row — the counterpart to `expand_thread`.
    fn collapse_thread_rows(&mut self, key: &(u32, String)) {
        let Some(members) = self.thread_members.get(key).cloned() else { return };
        let mut indices: Vec<usize> = members
            .iter()
            .skip(1)
            .filter_map(|k| self.shown.iter().position(|m| (m.account_id, m.id) == *k))
            .collect();
        indices.sort_unstable();
        {
            let mut guard = self.rows.guard();
            // Remove back-to-front so earlier removals don't shift the
            // indices still to come.
            for &idx in indices.iter().rev() {
                guard.remove(idx);
            }
        }
        for &idx in indices.iter().rev() {
            self.shown.remove(idx);
        }
        self.publish_drag_keys();
    }

    /// Whether a search is currently active (the query is non-empty).
    fn searching(&self) -> bool {
        !self.query.trim().is_empty()
    }

    /// The message set the search filters over: the cross-folder pool while an
    /// `AllFolders` search is active (and the pool has arrived), otherwise the
    /// current folder's own index.
    fn active_source(&self) -> &[Message] {
        if self.searching()
            && self.scope == SearchScope::AllFolders
            && !self.search_pool.is_empty()
        {
            &self.search_pool
        } else {
            &self.all
        }
    }

    fn search_placeholder(&self) -> &'static str {
        match self.scope {
            SearchScope::AllFolders => "Search all folders",
            SearchScope::ThisFolder => "Search this folder",
        }
    }

    /// Drop any active search: clear the query and the entry text so a folder
    /// switch doesn't leave a stale term filtering the new folder.
    fn clear_search(&mut self) {
        if self.query.is_empty() {
            return;
        }
        self.query.clear();
        if let Some(e) = &self.search_entry {
            e.set_text("");
        }
    }

    fn rebuild(&mut self) {
        let q = self.query.to_lowercase();
        // Filter and sort by reference, and clone only the page that is actually
        // rendered. A folder's index holds every message ever synced, while
        // `render_limit` is a few hundred — cloning the whole match set first put
        // a copy of the entire mailbox through the allocator on every keystroke
        // and on the cache-backed load at startup.
        let mut matches: Vec<&Message> = self
            .active_source()
            .iter()
            .filter(|m| {
                q.is_empty()
                    || m.subject.to_lowercase().contains(&q)
                    || m.from_name.to_lowercase().contains(&q)
                    || m.from_addr.to_lowercase().contains(&q)
                    || m.preview.to_lowercase().contains(&q)
            })
            .collect();
        let sort = self.sort;
        matches.sort_by(|a, b| message_cmp(a, b, sort));
        let total_matches = matches.len();
        // Render up to the current limit; the rest stay indexed (for search) until
        // the user scrolls further and `LoadMore` raises the limit.
        let capped: Vec<Message> = matches.into_iter().take(self.render_limit).cloned().collect();
        self.total_matches = total_matches;
        self.rendered_count = capped.len();

        // Group into conversations by reply headers (Message-ID / In-Reply-To /
        // References), preserving newest-first order. Each thread shows its newest
        // message as the head; expanding reveals the older replies beneath it.
        // With threading off, every message is its own group.
        let keys = if self.threading {
            compute_thread_keys(&capped, &self.thread_links)
        } else {
            std::collections::HashMap::new()
        };
        let key_for = |m: &Message| -> (u32, String) {
            keys.get(&(m.account_id, m.id))
                .cloned()
                .unwrap_or_else(|| (m.account_id, format!("\u{0}uid{}", m.uid)))
        };
        let mut order: Vec<(u32, String)> = Vec::new();
        let mut groups: std::collections::HashMap<(u32, String), Vec<Message>> =
            std::collections::HashMap::new();
        for m in capped {
            let key = key_for(&m);
            if let Some(v) = groups.get_mut(&key) {
                v.push(m);
            } else {
                order.push(key.clone());
                groups.insert(key, vec![m]);
            }
        }

        // Flatten back into display order, recording per-row thread metadata.
        struct RowMeta {
            count: usize,
            is_child: bool,
            expanded: bool,
            key: Option<(u32, String)>,
            unread: bool,
            /// The newest member's display time (thread heads only): the head
            /// row says when the conversation last moved, not when it began.
            latest: Option<String>,
        }
        let mut shown: Vec<Message> = Vec::new();
        let mut metas: Vec<RowMeta> = Vec::new();
        self.msg_thread.clear();
        self.thread_members.clear();
        for key in &order {
            let mut msgs = groups.remove(key).unwrap();
            // A conversation reads like a transcript: the message that started it
            // is the row on screen, and its replies descend beneath it to the
            // newest. Where the *thread* sits among the other rows is still the
            // list's sort order — recent activity keeps it near the top — but
            // inside the thread, time only runs one way.
            msgs.sort_by(|a, b| a.timestamp.cmp(&b.timestamp).then(a.uid.cmp(&b.uid)));
            let count = msgs.len();
            // `expanded_threads` stores toggles away from the default state.
            let expanded = count > 1 && (self.expanded_threads.contains(key) != self.default_expanded);
            // The head stays marked unread while ANY message in its
            // conversation is unread — hidden replies included.
            let any_unread = count > 1 && msgs.iter().any(|m| m.unread);
            if count > 1 {
                let members: Vec<(u32, u32)> = msgs.iter().map(|m| (m.account_id, m.id)).collect();
                for k in &members {
                    self.msg_thread.insert(*k, key.clone());
                }
                self.thread_members.insert(key.clone(), members);
            }
            // The head is the thread's *oldest* message (see the sort above),
            // but its row should say when the conversation last moved — so the
            // newest member's time is carried alongside for display.
            let latest = (count > 1)
                .then(|| msgs.last().map(|m| m.datetime_list()))
                .flatten();
            let mut it = msgs.into_iter();
            let head = it.next().unwrap();
            shown.push(head);
            metas.push(RowMeta {
                count,
                is_child: false,
                expanded,
                key: if count > 1 { Some(key.clone()) } else { None },
                unread: any_unread,
                latest,
            });
            if expanded {
                for child in it {
                    shown.push(child);
                    metas.push(RowMeta {
                        count: 0,
                        is_child: true,
                        expanded: false,
                        key: None,
                        unread: false,
                        latest: None,
                    });
                }
            }
        }
        self.shown = shown;
        // Republish the row keys before the rows are built, so a drag starting on
        // any of them can map selected row indices back to messages.
        self.publish_drag_keys();

        // Expanded conversations indent their member cards; give the pane the
        // extra floor that needs while any thread is open, so nothing is
        // clipped at the right edge (see THREAD_EXPANDED_EXTRA).
        if let Some(s) = &self.scroller {
            let any_expanded = metas.iter().any(|meta| meta.is_child);
            let floor =
                LIST_MIN_WIDTH + if any_expanded { THREAD_EXPANDED_EXTRA } else { 0 };
            s.set_size_request(floor, -1);
        }

        {
            let mut guard = self.rows.guard();
            guard.clear();
            for (m, meta) in self.shown.iter().zip(metas.into_iter()) {
                let ring_class = if self.colorize && self.account_colors.contains_key(&m.account_id) {
                    Some(format!("vireo-acct-ring-{}", m.account_id))
                } else {
                    None
                };
                guard.push_back(RowInit {
                    msg: m.clone(),
                    gravatar: self.gravatar,
                    avatars: self.avatars,
                    sender_logos: self.sender_logos,
                    preview_lines: self.preview_lines,
                    ring_class,
                    palette_collapse_secs: self.palette_collapse_secs.clone(),
                    palette_hover: self.palette_hover.clone(),
                    thread_count: meta.count,
                    is_thread_child: meta.is_child,
                    thread_expanded: meta.expanded,
                    thread_key: meta.key,
                    thread_date: meta.latest,
                    thread_unread: meta.unread,
                    drag_keys: self.drag_keys.clone(),
                    show_recipient: self.show_recipient,
                    // A full rebuild never needs a row to mount closed —
                    // that's only for `expand_thread`'s surgical insert.
                    revealed: true,
                });
            }
        }

        // Restore the highlight on the message being viewed, if it's still shown.
        self.select_current();
    }

    /// Republish the shown rows' keys for drag-and-drop. Row indices shift
    /// whenever rows are added or removed, so this must follow every change to
    /// `shown` — a stale mapping would drag the wrong messages (#23).
    fn publish_drag_keys(&self) {
        *self.drag_keys.borrow_mut() = self
            .shown
            .iter()
            .map(|m| (m.account_id, m.folder_id, m.uid, m.id))
            .collect();
    }

    /// The conversation to show for a selected message: when `m` is the oldest
    /// (head) of a multi-message thread, every message in it (oldest first);
    /// otherwise just `m` (so opening an individual reply shows only that one).
    fn conversation_for(&self, m: &Message) -> (Vec<Message>, bool) {
        if !self.threading {
            return (vec![m.clone()], false);
        }
        // Thread within whatever set is on screen (the search pool while searching,
        // otherwise the current folder) so the conversation matches the rows shown.
        let source = self.active_source();
        let keys = compute_thread_keys(source, &self.thread_links);
        let Some(key) = keys.get(&(m.account_id, m.id)).cloned() else {
            return (vec![m.clone()], false);
        };
        let mut members: Vec<Message> = source
            .iter()
            .filter(|x| keys.get(&(x.account_id, x.id)) == Some(&key))
            .cloned()
            .collect();
        if members.len() <= 1 {
            // Nothing else here to thread with. It may still have siblings in
            // another folder, so this is *not* solo — the reader may look.
            return (vec![m.clone()], false);
        }
        // Oldest first, matching the rows: the head is the message that opened
        // the conversation, and opening it shows the whole thread in order.
        members.sort_by(|a, b| a.timestamp.cmp(&b.timestamp).then(a.uid.cmp(&b.uid)));
        let is_head = members
            .first()
            .is_some_and(|h| (h.account_id, h.id) == (m.account_id, m.id));
        if is_head {
            (members, false)
        } else {
            // A reply picked out of a conversation that is on screen: show it by
            // itself and leave it that way.
            (vec![m.clone()], true)
        }
    }

    /// Select row `idx` and put the keyboard focus on it.
    ///
    /// Focus matters after a removal: destroying the focused row leaves GTK to
    /// pick a fallback of its own, which can be the top of the list — and moving
    /// focus scrolls the viewport with it, so the list appears to jump away from
    /// where the user was working (#19). Taking focus deliberately also means the
    /// single-key shortcuts carry on from the row that is now selected.
    fn select_and_focus(&self, idx: usize) {
        let list = self.rows.widget();
        if let Some(row) = list.row_at_index(idx as i32) {
            list.select_row(Some(&row));
            row.grab_focus();
        }
    }

    /// Re-apply the whole selection (the viewed message plus any multi-selected
    /// rows) so it persists across rebuilds — background syncs included — until
    /// the user clicks away. Called after a rebuild, when rows are freshly built
    /// and nothing is selected yet.
    fn select_current(&self) {
        let list = self.rows.widget();
        if self.selected_ids.is_empty() {
            list.unselect_all();
            return;
        }
        for key in &self.selected_ids {
            if let Some(idx) = self.shown.iter().position(|m| (m.account_id, m.id) == *key) {
                if let Some(row) = list.row_at_index(idx as i32) {
                    list.select_row(Some(&row));
                }
            }
        }
    }

    /// Update the display-wide CSS that rings each account's avatar with its
    /// colour (used in the unified "All Inboxes" view to identify the account).
    fn refresh_tint_css(&self) {
        let mut css = String::new();
        for (id, color) in &self.account_colors {
            css.push_str(&format!(
                ".vireo-acct-ring-{0} {{ border-radius: 9999px; box-shadow: 0 0 0 3px {1}; }}\n",
                id, color
            ));
        }
        self.color_provider.load_from_data(&css);
    }
}

#[cfg(test)]
mod tests {
    use super::compute_thread_keys;
    use crate::models::Message;

    fn msg(id: u32, message_id: &str, references: &str) -> Message {
        Message {
            id,
            account_id: 1,
            folder_id: 1,
            uid: id,
            from_name: "X".into(),
            from_addr: "x@example.com".into(),
            to: String::new(),
            cc: String::new(),
            subject: "S".into(),
            preview: String::new(),
            body: String::new(),
            date: String::new(),
            timestamp: 1000,
            unread: false,
            starred: false,
            has_attachment: false,
            message_id: message_id.into(),
            references: references.into(),
        }
    }

    /// Two replies in an Inbox each answer a different message in Sent, and
    /// reference nothing else — so within the Inbox they share no id at all.
    /// They are one conversation, and the messages that say so are the ones in
    /// Sent, which the folder on screen never shows.
    #[test]
    fn a_conversation_joined_through_another_folder_still_groups() {
        let shown = [
            msg(1, "reply-a@them", "sent-1@us"),
            msg(2, "reply-b@them", "sent-2@us"),
        ];
        // Without the Sent messages there is nothing to join them.
        let alone = compute_thread_keys(&shown, &[]);
        assert_ne!(
            alone.get(&(1, 1)),
            alone.get(&(1, 2)),
            "nothing on screen links these two"
        );

        // Sent 2 replied to reply-a, which replied to Sent 1: one conversation.
        let links = vec![
            (1u32, "sent-1@us".to_string(), String::new()),
            (1u32, "sent-2@us".to_string(), "sent-1@us reply-a@them".to_string()),
        ];
        let joined = compute_thread_keys(&shown, &links);
        assert_eq!(
            joined.get(&(1, 1)),
            joined.get(&(1, 2)),
            "the messages in Sent say they belong together"
        );
    }

    /// A re-added account re-downloads its whole mailbox, so every message in it
    /// is older than the moment the account was added. Threading reads the reply
    /// headers, which say the same thing whenever the mail was sent — the three
    /// messages here are the shape iCloud delivered: a root with no References,
    /// and two replies naming it.
    #[test]
    fn mail_older_than_the_account_still_threads() {
        let old = 1_787_565_140i64; // long before this list was ever built
        let mut root = msg(1, "root@dccma.com", "");
        root.timestamp = old;
        let mut first = msg(2, "r1@dccma.com", "root@dccma.com sent-1@me.com");
        first.timestamp = old + 48;
        let mut second = msg(3, "r2@dccma.com", "root@dccma.com sent-2@me.com");
        second.timestamp = old + 224;

        let shown = [root, first, second];
        let keys = compute_thread_keys(&shown, &[]);
        let root_key = keys.get(&(1, 1)).cloned().expect("the root is threaded");
        assert_eq!(keys.get(&(1, 2)), Some(&root_key), "first reply joins");
        assert_eq!(keys.get(&(1, 3)), Some(&root_key), "second reply joins");
    }

    /// Links are evidence, not glue: unrelated mail must not be pulled in.
    #[test]
    fn links_do_not_merge_unrelated_conversations() {
        let shown = [msg(1, "a@x", ""), msg(2, "b@x", "")];
        let links = vec![(1u32, "c@x".to_string(), "a@x".to_string())];
        let keys = compute_thread_keys(&shown, &links);
        assert_ne!(keys.get(&(1, 1)), keys.get(&(1, 2)), "still two conversations");
    }

    /// A long conversation groups whole. What it may drag in from *other*
    /// folders is bounded by the cache's own per-thread limit; what is already
    /// in the folder on screen is shown in full.
    #[test]
    fn a_long_conversation_groups_every_message() {
        let n = 60usize;
        let mut members: Vec<Message> = Vec::new();
        for i in 0..n {
            let mut m = msg(i as u32 + 1, &format!("m{i}@x"), "root@x");
            m.timestamp = 1000 + i as i64;
            members.push(m);
        }
        // All one conversation by their shared reference.
        let keys = compute_thread_keys(&members, &[]);
        let root = keys.get(&(1, 1)).cloned().expect("threaded");
        assert!(
            members.iter().all(|m| keys.get(&(1, m.id)) == Some(&root)),
            "one conversation"
        );
        assert_eq!(
            members.iter().filter(|m| keys.get(&(1, m.id)) == Some(&root)).count(),
            n,
            "every message belongs to it, however long the thread runs"
        );
    }

    #[test]
    fn preview_lines_clamp_but_keep_zero() {
        // 0 is "off"; anything above 3 is a hand-edited file, not a setting.
        for (asked, expected) in [(0u32, 0u32), (1, 1), (3, 3), (9, 3)] {
            assert_eq!(asked.min(3), expected, "for {asked}");
        }
    }
}
