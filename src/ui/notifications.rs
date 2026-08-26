//! Top-of-window status bar (notification area).
//!
//! A transient bar slides down for new messages and auto-collapses after a few
//! seconds. Messages that need attention (errors) are also kept in a list that
//! can be expanded into toast-like cards via a button in the header. The bar is
//! tinted with the desktop accent colour (`@accent_bg_color`, default GNOME
//! blue) and turns amber for errors.

use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;
use relm4::factory::FactoryVecDeque;
use relm4::prelude::*;

const TRANSIENT_SECS: u64 = 5;

/// A single persisted notification (currently always an error).
#[derive(Debug, Clone)]
pub struct Notification {
    pub id: u32,
    pub text: String,
    /// Connection/sync error — cleared automatically once connectivity returns.
    pub connectivity: bool,
}

/// One expandable card.
pub struct NotificationCard {
    note: Notification,
}

#[derive(Debug)]
pub enum CardOutput {
    Dismiss(u32),
}

#[relm4::factory(pub)]
impl FactoryComponent for NotificationCard {
    type Init = Notification;
    type Input = ();
    type Output = CardOutput;
    type CommandOutput = ();
    type ParentWidget = gtk::Box;

    view! {
        gtk::Box {
            set_spacing: 10,
            add_css_class: "toast-card",

            gtk::Image {
                set_icon_name: Some("co.hyprlab.Vireo-dialog-warning-symbolic"),
                set_valign: gtk::Align::Start,
                add_css_class: "toast-card-icon",
            },

            gtk::Label {
                set_label: &self.note.text,
                set_wrap: true,
                set_xalign: 0.0,
                set_hexpand: true,
                set_halign: gtk::Align::Start,
            },

            gtk::Button {
                set_icon_name: "co.hyprlab.Vireo-window-close-symbolic",
                set_valign: gtk::Align::Start,
                add_css_class: "flat",
                add_css_class: "circular",
                connect_clicked[sender, id = self.note.id] => move |_| {
                    let _ = sender.output(CardOutput::Dismiss(id));
                },
            },
        }
    }

    fn init_model(note: Self::Init, _index: &DynamicIndex, _sender: FactorySender<Self>) -> Self {
        Self { note }
    }
}

pub struct NotificationCenter {
    cards: FactoryVecDeque<NotificationCard>,
    /// Card ids, index-aligned with the factory (newest first).
    ids: Vec<u32>,
    next_id: u32,
    /// Bumped on each new transient message so stale timers are ignored.
    epoch: u32,
    transient_text: String,
    transient_error: bool,
    transient_visible: bool,
    /// Current ongoing activity (connecting/syncing). Never auto-shown; only
    /// visible when the user opens the panel.
    status_text: String,
    panel_open: bool,
}

#[derive(Debug)]
pub enum NotifyInput {
    /// Auto-shows the transient bar for a few seconds; errors also add a card.
    Push {
        text: String,
        error: bool,
        connectivity: bool,
    },
    /// Updates the ongoing-activity text without auto-showing anything.
    SetStatus(String),
    /// Connectivity restored: drop any connection/sync error cards.
    ClearConnectivity,
    TogglePanel,
    ClearAll,
    Dismiss(u32),
    CollapseTransient(u32),
}

#[derive(Debug)]
pub enum NotifyOutput {
    CountChanged(usize),
}

#[relm4::component(pub)]
impl SimpleComponent for NotificationCenter {
    type Init = ();
    type Input = NotifyInput;
    type Output = NotifyOutput;

    view! {
        gtk::Revealer {
            set_transition_type: gtk::RevealerTransitionType::SlideDown,
            #[watch]
            set_reveal_child: model.transient_visible || model.panel_open,

            gtk::Box {
                set_orientation: gtk::Orientation::Vertical,
                #[watch]
                set_css_classes: &model.area_classes(),

                // A WindowHandle so the bar can drag the window, like a title bar.
                // Interactive children (the buttons) still receive their clicks.
                gtk::WindowHandle {
                    gtk::Box {
                        add_css_class: "notify-bar",
                        set_spacing: 8,

                        gtk::Image {
                            #[watch]
                            set_icon_name: Some(model.bar_icon()),
                        },

                        gtk::Label {
                            #[watch]
                            set_label: &model.bar_label(),
                            set_hexpand: true,
                            set_halign: gtk::Align::Start,
                            set_ellipsize: gtk::pango::EllipsizeMode::End,
                        },

                        gtk::Button {
                            #[watch]
                            set_visible: model.panel_open && !model.ids.is_empty(),
                            set_label: "Clear all",
                            add_css_class: "flat",
                            connect_clicked => NotifyInput::ClearAll,
                        },

                        gtk::Button {
                            #[watch]
                            set_icon_name: if model.panel_open { "co.hyprlab.Vireo-pan-up-symbolic" } else { "co.hyprlab.Vireo-pan-down-symbolic" },
                            set_tooltip_text: Some("Collapse status bar"),
                            add_css_class: "flat",
                            connect_clicked => NotifyInput::TogglePanel,
                        },
                    },
                },

                // Only the cards section expands; an empty panel stays at bar
                // height (just shows the status / "Status bar (0)" row).
                gtk::Revealer {
                    set_transition_type: gtk::RevealerTransitionType::SlideDown,
                    #[watch]
                    set_reveal_child: model.panel_open && !model.ids.is_empty(),

                    gtk::ScrolledWindow {
                        set_propagate_natural_height: true,
                        set_max_content_height: 320,
                        set_hscrollbar_policy: gtk::PolicyType::Never,

                        #[local_ref]
                        cards_box -> gtk::Box {
                            set_orientation: gtk::Orientation::Vertical,
                            set_spacing: 8,
                            add_css_class: "notify-cards",
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
        let cards = FactoryVecDeque::builder()
            .launch(gtk::Box::default())
            .forward(sender.input_sender(), |out| match out {
                CardOutput::Dismiss(id) => NotifyInput::Dismiss(id),
            });

        let model = NotificationCenter {
            cards,
            ids: Vec::new(),
            next_id: 1,
            epoch: 0,
            transient_text: String::new(),
            transient_error: false,
            transient_visible: false,
            status_text: String::new(),
            panel_open: false,
        };

        let cards_box = model.cards.widget();
        let widgets = view_output!();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            NotifyInput::Push {
                text,
                error,
                connectivity,
            } => {
                self.transient_text = text.clone();
                self.transient_error = error;
                self.transient_visible = true;
                self.epoch = self.epoch.wrapping_add(1);

                if error {
                    // Keep at most one connectivity error card around.
                    if connectivity {
                        self.remove_connectivity_cards();
                    }
                    let id = self.next_id;
                    self.next_id += 1;
                    self.ids.insert(0, id);
                    self.cards.guard().push_front(Notification {
                        id,
                        text,
                        connectivity,
                    });
                    let _ = sender.output(NotifyOutput::CountChanged(self.ids.len()));
                }

                // Auto-collapse the transient bar after a few seconds, unless a
                // newer message (or the expanded panel) supersedes it.
                let epoch = self.epoch;
                let sender = sender.clone();
                glib::timeout_add_local_once(Duration::from_secs(TRANSIENT_SECS), move || {
                    sender.input(NotifyInput::CollapseTransient(epoch));
                });
            }

            NotifyInput::SetStatus(text) => {
                self.status_text = text;
            }

            NotifyInput::ClearConnectivity => {
                let before = self.ids.len();
                self.remove_connectivity_cards();
                if self.ids.len() != before {
                    if self.ids.is_empty() {
                        self.panel_open = false;
                    }
                    let _ = sender.output(NotifyOutput::CountChanged(self.ids.len()));
                }
            }

            NotifyInput::CollapseTransient(epoch) => {
                if epoch == self.epoch {
                    self.transient_visible = false;
                }
            }

            NotifyInput::TogglePanel => {
                self.panel_open = !self.panel_open;
            }

            NotifyInput::Dismiss(id) => {
                if let Some(pos) = self.ids.iter().position(|x| *x == id) {
                    self.ids.remove(pos);
                    self.cards.guard().remove(pos);
                    if self.ids.is_empty() {
                        self.panel_open = false;
                    }
                    let _ = sender.output(NotifyOutput::CountChanged(self.ids.len()));
                }
            }

            NotifyInput::ClearAll => {
                self.cards.guard().clear();
                self.ids.clear();
                self.panel_open = false;
                self.transient_visible = false;
                let _ = sender.output(NotifyOutput::CountChanged(0));
            }
        }
    }
}

impl NotificationCenter {
    /// Remove every connectivity-flagged error card (newest-first indices).
    fn remove_connectivity_cards(&mut self) {
        let to_remove: Vec<usize> = (0..self.ids.len())
            .filter(|&i| self.cards.get(i).is_some_and(|c| c.note.connectivity))
            .collect();
        if to_remove.is_empty() {
            return;
        }
        {
            let mut guard = self.cards.guard();
            for &i in to_remove.iter().rev() {
                guard.remove(i);
            }
        }
        for &i in to_remove.iter().rev() {
            self.ids.remove(i);
        }
    }

    fn is_error_state(&self) -> bool {
        // Amber only when something error-related is actually present: an active
        // error transient, or existing error cards. Never the stale flag, which
        // used to tint the bar amber while collapsing an already-cleared panel.
        (self.transient_visible && self.transient_error) || !self.ids.is_empty()
    }

    fn area_classes(&self) -> Vec<&'static str> {
        if self.is_error_state() {
            vec!["notify-area", "error"]
        } else {
            vec!["notify-area"]
        }
    }

    fn bar_icon(&self) -> &'static str {
        if self.is_error_state() {
            "co.hyprlab.Vireo-dialog-warning-symbolic"
        } else {
            "co.hyprlab.Vireo-dialog-information-symbolic"
        }
    }

    fn bar_label(&self) -> String {
        if self.panel_open {
            // Reveal the current activity when the user opens the panel.
            if !self.status_text.is_empty() {
                self.status_text.clone()
            } else {
                format!("Status bar ({})", self.ids.len())
            }
        } else {
            self.transient_text.clone()
        }
    }
}
