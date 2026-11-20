use std::{
    collections::BTreeMap,
    env,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use ataxia_scripting::Interface;
use ataxia_types::Character;
use iced::{
    Element, Length, Subscription, Task, Theme,
    futures::{SinkExt, Stream},
    stream,
    widget::{Container, button, column, container, row, text},
    window::{self, Id},
};
use log::warn;
use notify_debouncer_full::{
    DebounceEventResult, new_debouncer,
    notify::{EventKind, RecursiveMode},
};
use rfd::FileHandle;
use serde::{Deserialize, Serialize};
use tokio::{fs, io, task};

use crate::views::{BasicMessage, BasicState, InventoryState};

mod views;

#[derive(Clone, Debug)]
pub enum Message {
    Window(WindowMessage),
    Basic(BasicMessage),
    Meter(MeterMessage),
    OpenFilePicker,
    FilePicked(Option<FileHandle>),
    FileLoaded {
        main: Arc<io::Result<String>>,
        backup: Option<Arc<io::Result<String>>>,
        state: Arc<io::Result<String>>,
    },
    FileModified,
    WatcherCreated(tokio::sync::mpsc::Sender<PathBuf>),
    AutoSave(Instant),
    ViewChanged(View, Id),
}

#[derive(Clone, Debug)]
pub enum WindowMessage {
    WindowClosed(Id),
    OpenWindow(Id),
}

#[derive(Clone, Debug)]
pub enum MeterMessage {
    Spend(String),
    Restore(String),
}

#[derive(Default, Debug)]
pub struct State {
    view: View,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Copy)]
pub enum View {
    #[default]
    Basic,
    Inventory,
    Spells,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Rhai,
    Json,
}

/// The serializable ui state, saved next to the sheet so it can be restored.
///
/// Window-specific state is not included because `iced::window::Id`
/// is not serializable.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UiState {
    pub basic: BasicState,
    pub inventory: InventoryState,
}

#[derive(Debug)]
pub struct App {
    windows: BTreeMap<Id, State>,
    // global: Option<Arc<Global>>,
    sheet: Option<Character>,
    basic_state: BasicState,
    inventory_state: InventoryState,
    sheet_path: Option<PathBuf>,
    sheet_mode: Mode,
    rhai_interface: Interface,
}

impl<'a> App {
    pub fn new() -> (Self, Task<Message>) {
        let (_, open) = window::open(window::Settings::default());
        (
            Self {
                windows: BTreeMap::new(),
                // global: None,
                sheet: None,
                basic_state: Default::default(),
                sheet_path: None,
                sheet_mode: Default::default(),
                rhai_interface: Interface::new(),
                inventory_state: Default::default(),
            },
            open.map(WindowMessage::OpenWindow).map(Message::Window),
        )
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Window(window_message) => match window_message {
                WindowMessage::WindowClosed(id) => {
                    self.windows.remove(&id);
                    if self.windows.is_empty() {
                        println!("Closing window");
                        if let Some(ref s) = self.sheet {
                            let path = self
                                .sheet_path
                                .as_ref()
                                .expect("Always a path when a character exists");
                            let c = get_backup_path(path);
                            let state_path = get_state_path(path);
                            let contents =
                                serde_json::to_string_pretty(s).expect("Should never fail");
                            let state_contents =
                                serde_json::to_string_pretty(&self.snapshot_ui_state())
                                    .expect("Should never fail");
                            println!("Saving file");

                            self.sheet_path = None;

                            let start = Task::future(async move {
                                if let Err(e) = fs::write(c, contents).await {
                                    log::error!("Failed to write backup file: {e}");
                                }
                                if let Err(e) = fs::write(state_path, state_contents).await {
                                    log::error!("Failed to write ui state file: {e}");
                                }
                            });
                            // TODO kill the notify thread and exit properly
                            start.then(|()| std::process::exit(0))
                        } else {
                            std::process::exit(0)
                        }
                    } else {
                        Task::none()
                    }
                }
                WindowMessage::OpenWindow(id) => {
                    self.windows.insert(id, State::default());
                    Task::none()
                }
            },
            Message::Basic(basic_message) => {
                if let Some(sheet) = &mut self.sheet {
                    self.basic_state.update(basic_message, sheet)
                } else {
                    Task::none()
                }
            }
            Message::OpenFilePicker => {
                let current_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                let t = rfd::AsyncFileDialog::new()
                    .add_filter("rhai", &["rhai"])
                    .add_filter("json", &["json"])
                    .set_title("Select Character")
                    .set_directory(current_dir)
                    .pick_file();
                println!("Opening file picker");
                Task::perform(t, Message::FilePicked)
            }
            Message::FilePicked(f) => {
                println!("File picked");
                if let Some(f) = f {
                    match f.path().extension().and_then(|f| f.to_str()) {
                        Some("json") => {
                            self.sheet_mode = Mode::Json;
                            let f = f.path().to_owned();
                            self.sheet_path = Some(f.clone());

                            let t = async move {
                                let main = fs::read_to_string(f.clone()).await;
                                let state = fs::read_to_string(get_state_path(f)).await;
                                (main, state)
                            };
                            Task::perform(t, |(main, state)| Message::FileLoaded {
                                main: Arc::new(main),
                                backup: None,
                                state: Arc::new(state),
                            })
                        }
                        Some(_) => {
                            let f = f.path().to_owned();
                            self.sheet_path = Some(f.clone());

                            let t2 = fs::read_to_string(get_backup_path(f.as_path()));
                            let t3 = fs::read_to_string(get_state_path(f.as_path()));
                            let t = fs::read_to_string(f);
                            let t = async move {
                                let t = t.await;
                                let t2 = t2.await;
                                let t3 = t3.await;
                                (t, t2, t3)
                            };
                            Task::perform(t, |(main, backup, state)| Message::FileLoaded {
                                main: Arc::new(main),
                                backup: Some(Arc::new(backup)),
                                state: Arc::new(state),
                            })
                        }
                        None => {
                            println!("Invalid file");
                            Task::none()
                        }
                    }
                } else {
                    println!("No file chosen");
                    Task::none()
                }
            }
            Message::FileLoaded {
                main,
                backup,
                state,
            } => {
                println!("File loaded");
                let loaded = match &*main {
                    Ok(f) => match self.sheet_mode {
                        Mode::Rhai => {
                            let second = if let Some(backup) = backup
                                && let Ok(b) = Arc::try_unwrap(backup)
                                    .expect("There should only ever be one copy")
                            {
                                match serde_json::from_str(&b) {
                                    Ok(b) => Some(b),
                                    Err(e) => {
                                        println!("{e:?}");
                                        None
                                    }
                                }
                            } else {
                                warn!("Failed to load file backup");
                                self.sheet.clone()
                            };
                            match self.rhai_interface.execute(f, second) {
                                Ok(sheet) => {
                                    self.sheet = Some(sheet);
                                    true
                                }
                                Err(e) => {
                                    println!("Rhai failed: {e:?}");
                                    false
                                }
                            }
                        }
                        Mode::Json => match serde_json::from_str::<Character>(f) {
                            Ok(c) => {
                                self.sheet = Some(c);
                                true
                            }
                            Err(e) => {
                                println!("Json failed: {e:?}");
                                false
                            }
                        },
                    },
                    Err(e) => {
                        println!("{e:?}");
                        false
                    }
                };
                if loaded {
                    self.load_ui_state(state);
                }
                Task::none()
            }
            Message::FileModified => {
                println!("Reloading file");

                if let Some(e) = &self.sheet_path {
                    let e = e.to_owned();
                    let t2 = fs::read_to_string(get_state_path(e.as_path()));
                    let t = fs::read_to_string(e);
                    let t = async move {
                        let main = t.await;
                        let state = t2.await;
                        (main, state)
                    };
                    Task::perform(t, |(main, state)| Message::FileLoaded {
                        main: Arc::new(main),
                        backup: None,
                        state: Arc::new(state),
                    })
                } else {
                    Task::none()
                }
            }
            Message::WatcherCreated(sender) => {
                if let Some(f) = &self.sheet_path {
                    let f = f.to_owned();
                    Task::future(async move {
                        let _ = sender.send(f).await;
                    })
                    .discard()
                } else {
                    println!("No path to send to watcher!");
                    Task::none()
                }
            }
            Message::Meter(meter_message) => {
                let Some(sheet) = &mut self.sheet else {
                    return Task::none();
                };

                let meters = &mut sheet.meters;
                match meter_message {
                    MeterMessage::Spend(s) => {
                        if let Some(m) = meters.meters.get_mut(&s) {
                            m.spend();
                        }
                    }
                    MeterMessage::Restore(s) => {
                        if let Some(m) = meters.meters.get_mut(&s) {
                            m.restore();
                        }
                    }
                }

                Task::none()
            }
            Message::AutoSave(_instant) => {
                let Some(ref sheet) = self.sheet else {
                    return Task::none();
                };
                let Ok(r) = serde_json::to_string_pretty(&sheet) else {
                    warn!("Json tostring failed");
                    return Task::none();
                };
                let Ok(s) = serde_json::to_string_pretty(&self.snapshot_ui_state()) else {
                    warn!("Ui state tostring failed");
                    return Task::none();
                };
                let path = self
                    .sheet_path
                    .as_ref()
                    .expect("We always have this when we have a character")
                    .clone();
                let path = get_backup_path(&path);
                let state_path = get_state_path(&path);
                let task = async move {
                    if let Err(e) = fs::write(path, r).await {
                        log::error!("Failed to write backup file: {e}");
                    }
                    if let Err(e) = fs::write(state_path, s).await {
                        log::error!("Failed to write ui state file: {e}");
                    }
                };
                Task::future(task).discard()
            }
            Message::ViewChanged(view, id) => {
                if let Some(w) = self.windows.get_mut(&id) {
                    w.view = view;
                    Task::none()
                } else {
                    Task::none()
                }
            }
        }
    }

    fn snapshot_ui_state(&self) -> UiState {
        UiState {
            basic: self.basic_state.clone(),
            inventory: self.inventory_state.clone(),
        }
    }

    /// Applies the ui state loaded alongside a sheet.
    ///
    /// A missing state file (e.g. the sheet has never been saved by this
    /// app) resets the ui state so stale state from another sheet is not
    /// shown.
    fn load_ui_state(&mut self, state: Arc<io::Result<String>>) {
        let ui_state = match Arc::try_unwrap(state).expect("There should only ever be one copy") {
            Ok(contents) => match serde_json::from_str::<UiState>(&contents) {
                Ok(ui_state) => ui_state,
                Err(e) => {
                    warn!("Failed to load ui state: {e}");
                    UiState::default()
                }
            },
            // Expected on first load of a sheet, so no warning.
            Err(_) => UiState::default(),
        };
        self.basic_state = ui_state.basic;
        self.inventory_state = ui_state.inventory;
    }

    pub fn view(&'_ self, window_id: Id) -> Element<'_, Message> {
        let Some(state) = self.windows.get(&window_id) else {
            return text!("Some error occured").into();
        };

        let Some(sheet) = &self.sheet else {
            let b = button("Select file").on_press(Message::OpenFilePicker);
            let c = column![text("No sheet loaded"), b];
            let c = container(c).center(Length::Fill);
            return c.into();
        };

        let c = match state.view {
            View::Basic => self.basic_state.view(sheet),
            View::Inventory => self.inventory_state.view(sheet),
            View::Spells => todo!(),
        };
        let c = column![mode_switcher(self, window_id), c].spacing(8);
        container(c).center(Length::Fill).into()
    }

    pub const fn theme(&self, _window_id: Id) -> Option<Theme> {
        Some(Theme::CatppuccinMacchiato)
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let w = window::close_events()
            .map(WindowMessage::WindowClosed)
            .map(Message::Window);
        let watcher = if self.sheet_path.is_some() {
            Subscription::run(create_file_watcher)
        } else {
            Subscription::none()
        };

        let auto_save = iced::time::every(Duration::from_secs(5)).map(Message::AutoSave);

        Subscription::batch([w, watcher, auto_save])
    }
}

fn create_file_watcher() -> impl Stream<Item = Message> {
    println!("Creating watcher");
    stream::channel(100, async move |mut output| {
        let path = {
            let (tx, mut rx) = tokio::sync::mpsc::channel(1);
            output
                .send(Message::WatcherCreated(tx))
                .await
                .expect("Got file");
            rx.recv().await
        };

        println!("Watcher got file");

        let Some(path) = path else {
            println!("Watcher init failed");
            return;
        };

        // Spawn a task that owns the watcher and bridges its blocking std::sync channel
        // into an async tokio channel. This avoids blocking the main async task,
        // which would prevent output.send() from being polled.
        let (bridge_tx, mut bridge_rx) = tokio::sync::mpsc::channel(16);
        task::spawn_blocking(move || {
            let (event_tx, event_rx) = std::sync::mpsc::channel();

            let Ok(mut debouncer) = new_debouncer(
                Duration::from_secs(1),
                None,
                move |res: DebounceEventResult| {
                    if let Ok(event) = res {
                        let _ = event_tx.send(event);
                    }
                },
            ) else {
                log::error!("File watcher failed!");
                return;
            };

            if let Err(e) = debouncer.watch(&path, RecursiveMode::NonRecursive) {
                println!("Watcher failed: {e:?}");
                return;
            }

            while let Ok(event) = event_rx.recv() {
                if bridge_tx.blocking_send(event).is_err() {
                    break;
                }
            }
        });

        while let Some(events) = bridge_rx.recv().await {
            for event in events {
                let send = match event.kind {
                    EventKind::Modify(_) => true,
                    EventKind::Create(_) => true,
                    EventKind::Remove(_) => true,
                    _ => false,
                };

                // println!("{event:?}");
                if send {
                    let _ = output.send(Message::FileModified).await;
                }
            }
        }
    })
}
fn get_backup_path(f: impl AsRef<Path>) -> String {
    format!("{}.json", f.as_ref().display())
}

fn get_state_path(f: impl AsRef<Path>) -> String {
    format!("{}.state.json", f.as_ref().display())
}

fn mode_switcher(app: &'_ App, id: Id) -> Container<'_, Message> {
    let b1 = button("Main").on_press_maybe(if app.windows[&id].view == View::Basic {
        None
    } else {
        Some(Message::ViewChanged(View::Basic, id))
    });
    let b2 = button("Inventory").on_press_maybe(if app.windows[&id].view == View::Inventory {
        None
    } else {
        Some(Message::ViewChanged(View::Inventory, id))
    });

    container(row![b1, b2].spacing(8)).padding(12)
}

#[cfg(test)]
mod tests {
    use super::UiState;

    #[test]
    fn ui_state_roundtrip() {
        let mut state = UiState::default();
        state.basic.reaction = true;
        state.basic.hp_input = "3".to_string();

        let json = serde_json::to_string(&state).expect("should serialize");
        let back: UiState = serde_json::from_str(&json).expect("should deserialize");

        assert!(back.basic.reaction);
        assert_eq!(back.basic.hp_input, "3");
        assert!(back.basic.pinned.is_empty());
    }

    #[test]
    fn ui_state_missing_fields_default() {
        let state: UiState = serde_json::from_str("{}").expect("should deserialize");
        assert!(!state.basic.reaction);
        assert!(state.basic.pinned.is_empty());
    }
}

// fn mode_style(theme: &Theme, status: iced::widget::button::Status) -> iced::widget::button::Style {
//     if status == iced::widget::button::Status::Disabled {
//         Style::default().
//     }
// }
