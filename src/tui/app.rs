use std::path::PathBuf;
use std::sync::mpsc::Sender;

use crate::config::{Config, MountConfig, SourceConfig};
use crate::{paths, ShelfError};

use super::actions::{self, MountHealth, TaskKind, TaskMessage, TaskStep};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Home,
    AddMount,
    MountDetail,
    Sources,
    SourceDetail,
    Repair,
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Modal {
    Confirm {
        action: ConfirmAction,
        message: String,
    },
    Error(String),
    Progress,
    Input,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmAction {
    Disconnect,
    RemoveMount,
    RemoveSource,
    Apply,
    Repair,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskState {
    Idle,
    Running {
        kind: TaskKind,
        steps: Vec<TaskStep>,
    },
    Failed {
        steps: Vec<TaskStep>,
        error: String,
    },
    Done {
        steps: Vec<TaskStep>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WizardStep {
    LocalFolder,
    RemotePath,
    LoginSource,
    Review,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddMountForm {
    pub step: WizardStep,
    pub local_folder: String,
    pub remote_path: String,
    pub source_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceMode {
    List,
    Add { field: usize, form: SourceForm },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SourceForm {
    pub address: String,
    pub username: String,
    pub id: String,
    pub password: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusRow {
    pub mount: MountConfig,
    pub health: MountHealth,
}

pub struct App {
    pub screen: Screen,
    pub previous_screen: Screen,
    pub modal: Option<Modal>,
    pub config: Config,
    pub config_path: Option<PathBuf>,
    pub status_rows: Vec<StatusRow>,
    pub selected_mount: usize,
    pub selected_source: usize,
    pub add_mount: AddMountForm,
    pub source_mode: SourceMode,
    pub task: TaskState,
    pub should_quit: bool,
    task_tx: Sender<TaskMessage>,
}

impl App {
    pub fn new(task_tx: Sender<TaskMessage>) -> Self {
        Self {
            screen: Screen::Home,
            previous_screen: Screen::Home,
            modal: None,
            config: Config::default(),
            config_path: None,
            status_rows: Vec::new(),
            selected_mount: 0,
            selected_source: 0,
            add_mount: AddMountForm {
                step: WizardStep::LocalFolder,
                local_folder: String::new(),
                remote_path: String::new(),
                source_index: 0,
            },
            source_mode: SourceMode::List,
            task: TaskState::Idle,
            should_quit: false,
            task_tx,
        }
    }

    pub fn refresh(&mut self) {
        self.start_task(TaskKind::Refresh);
        actions::spawn_refresh(self.task_tx.clone());
    }

    pub fn move_selection(&mut self, delta: isize) {
        let len = match self.screen {
            Screen::Home | Screen::MountDetail | Screen::Repair => self.status_rows.len(),
            Screen::Sources | Screen::SourceDetail => self.config.sources.len(),
            Screen::AddMount if self.add_mount.step == WizardStep::LoginSource => {
                self.config.sources.len()
            }
            _ => 0,
        };
        if len == 0 {
            return;
        }
        let current = match self.screen {
            Screen::Sources | Screen::SourceDetail | Screen::AddMount => &mut self.selected_source,
            _ => &mut self.selected_mount,
        };
        *current = move_index(*current, len, delta);
        if self.screen == Screen::AddMount && self.add_mount.step == WizardStep::LoginSource {
            self.add_mount.source_index = self.selected_source;
        }
    }

    pub fn enter(&mut self) {
        if self.modal.is_some() {
            self.confirm_modal();
            return;
        }
        match self.screen {
            Screen::Home if !self.status_rows.is_empty() => self.goto(Screen::MountDetail),
            Screen::Sources if !self.config.sources.is_empty() => self.goto(Screen::SourceDetail),
            Screen::AddMount => self.advance_add_mount(),
            Screen::SourceDetail => self.set_selected_source_default(),
            _ => {}
        }
    }

    pub fn back(&mut self) {
        if self.modal.is_some() {
            self.modal = None;
            return;
        }
        match self.screen {
            Screen::Home => self.should_quit = true,
            Screen::Help => self.screen = self.previous_screen,
            Screen::AddMount => self.back_add_mount(),
            Screen::MountDetail | Screen::Sources | Screen::Repair => self.goto(Screen::Home),
            Screen::SourceDetail => self.goto(Screen::Sources),
        }
    }

    pub fn goto(&mut self, screen: Screen) {
        self.previous_screen = self.screen;
        self.screen = screen;
        if screen == Screen::AddMount {
            self.add_mount = AddMountForm {
                step: WizardStep::LocalFolder,
                local_folder: String::new(),
                remote_path: String::new(),
                source_index: self.selected_source,
            };
        }
    }

    pub fn show_help(&mut self) {
        if self.screen != Screen::Help {
            self.previous_screen = self.screen;
            self.screen = Screen::Help;
        }
    }

    pub fn disconnect_selected(&mut self) {
        if let Some(mount) = self.selected_mount().cloned() {
            self.modal = Some(Modal::Confirm {
                action: ConfirmAction::Disconnect,
                message: format!(
                    "Disconnect {}?\n\nThis keeps the shelf config and will not delete remote files.",
                    mount.local_path
                ),
            });
        }
    }

    pub fn remove_selected_mount(&mut self) {
        if let Some(mount) = self.selected_mount().cloned() {
            self.modal = Some(Modal::Confirm {
                action: ConfirmAction::RemoveMount,
                message: format!(
                    "Remove {} from Shelf?\n\nThis removes shelf config and systemd units. This will not delete remote files.",
                    mount.local_path
                ),
            });
        }
    }

    pub fn repair_selected_or_all(&mut self) {
        self.modal = Some(Modal::Confirm {
            action: ConfirmAction::Repair,
            message: "Repair mount state?\n\nShelf will clean stacked mounts, apply config, and test write access where possible.".into(),
        });
    }

    pub fn apply_config(&mut self) {
        self.modal = Some(Modal::Confirm {
            action: ConfirmAction::Apply,
            message: "Apply Shelf configuration?\n\nThis will mount remote paths, create local folders, write systemd units, and test write access.".into(),
        });
    }

    pub fn start_add_source(&mut self) {
        self.screen = Screen::Sources;
        self.source_mode = SourceMode::Add {
            field: 0,
            form: SourceForm::default(),
        };
    }

    pub fn source_add_next_field(&mut self) {
        if let SourceMode::Add { field, form } = &mut self.source_mode {
            if *field < 3 {
                *field += 1;
            } else {
                let form = form.clone();
                self.start_task(TaskKind::AddSource);
                actions::spawn_add_source(self.task_tx.clone(), form);
                self.source_mode = SourceMode::List;
            }
        }
    }

    pub fn remove_selected_source(&mut self) {
        if let Some(source) = self.selected_source().cloned() {
            self.modal = Some(Modal::Confirm {
                action: ConfirmAction::RemoveSource,
                message: format!(
                    "Remove login source {}?\n\nMounts using this source must be removed first. Remote files are not touched.",
                    source.id
                ),
            });
        }
    }

    pub fn input_char(&mut self, ch: char) {
        match self.screen {
            Screen::AddMount => match self.add_mount.step {
                WizardStep::LocalFolder => self.add_mount.local_folder.push(ch),
                WizardStep::RemotePath => self.add_mount.remote_path.push(ch),
                WizardStep::LoginSource | WizardStep::Review => {}
            },
            Screen::Sources => {
                if let SourceMode::Add { field, form } = &mut self.source_mode {
                    match *field {
                        0 => form.address.push(ch),
                        1 => form.username.push(ch),
                        2 => form.id.push(ch),
                        3 => form.password.push(ch),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    pub fn backspace(&mut self) {
        match self.screen {
            Screen::AddMount => match self.add_mount.step {
                WizardStep::LocalFolder => {
                    self.add_mount.local_folder.pop();
                }
                WizardStep::RemotePath => {
                    self.add_mount.remote_path.pop();
                }
                WizardStep::LoginSource | WizardStep::Review => {}
            },
            Screen::Sources => {
                if let SourceMode::Add { field, form } = &mut self.source_mode {
                    match *field {
                        0 => {
                            form.address.pop();
                        }
                        1 => {
                            form.username.pop();
                        }
                        2 => {
                            form.id.pop();
                        }
                        3 => {
                            form.password.pop();
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    pub fn handle_task_message(&mut self, message: TaskMessage) {
        match message {
            TaskMessage::Status { config, path, rows } => {
                self.config = config;
                self.config_path = Some(path);
                self.status_rows = rows
                    .into_iter()
                    .map(|(mount, health)| StatusRow { mount, health })
                    .collect();
                clamp_index(&mut self.selected_mount, self.status_rows.len());
                clamp_index(&mut self.selected_source, self.config.sources.len());
            }
            TaskMessage::Started { kind, steps } => {
                self.task = TaskState::Running { kind, steps };
                if kind != TaskKind::Refresh {
                    self.modal = Some(Modal::Progress);
                }
            }
            TaskMessage::Step { index, label } => {
                if let TaskState::Running { steps, .. } = &mut self.task {
                    if let Some(step) = steps.get_mut(index) {
                        step.done = true;
                        step.label = label;
                    }
                }
            }
            TaskMessage::Done { steps } => {
                let should_refresh = matches!(
                    self.task,
                    TaskState::Running {
                        kind: TaskKind::AddMount
                            | TaskKind::AddSource
                            | TaskKind::SetDefaultSource
                            | TaskKind::Disconnect
                            | TaskKind::RemoveMount
                            | TaskKind::RemoveSource
                            | TaskKind::Apply
                            | TaskKind::Repair,
                        ..
                    }
                );
                self.task = TaskState::Done { steps };
                if should_refresh {
                    self.modal = Some(Modal::Progress);
                } else if matches!(self.modal, Some(Modal::Progress)) {
                    self.modal = None;
                }
                if should_refresh {
                    self.refresh();
                }
            }
            TaskMessage::Failed { steps, error } => {
                self.task = TaskState::Failed {
                    steps,
                    error: error.clone(),
                };
                self.modal = Some(Modal::Error(error));
            }
        }
    }

    pub fn selected_mount(&self) -> Option<&MountConfig> {
        self.status_rows
            .get(self.selected_mount)
            .map(|row| &row.mount)
    }

    pub fn selected_source(&self) -> Option<&SourceConfig> {
        self.config.sources.values().nth(self.selected_source)
    }

    fn advance_add_mount(&mut self) {
        match self.add_mount.step {
            WizardStep::LocalFolder => {
                if let Err(err) = validate_local_input(&self.add_mount.local_folder) {
                    self.modal = Some(Modal::Error(err.to_string()));
                } else {
                    self.add_mount.step = WizardStep::RemotePath;
                }
            }
            WizardStep::RemotePath => {
                if self.add_mount.remote_path.trim().is_empty()
                    || !self.add_mount.remote_path.starts_with('/')
                {
                    self.modal = Some(Modal::Error(
                        "remote path must start with '/', for example /media/movies".into(),
                    ));
                } else {
                    self.add_mount.step = WizardStep::LoginSource;
                }
            }
            WizardStep::LoginSource => {
                if self.config.sources.is_empty() {
                    self.modal = Some(Modal::Error(
                        "add a login source before adding mounts".into(),
                    ));
                } else {
                    self.add_mount.step = WizardStep::Review;
                }
            }
            WizardStep::Review => {
                self.start_task(TaskKind::AddMount);
                actions::spawn_add_mount(self.task_tx.clone(), self.add_mount.clone());
            }
        }
    }

    fn back_add_mount(&mut self) {
        self.add_mount.step = match self.add_mount.step {
            WizardStep::LocalFolder => {
                self.goto(Screen::Home);
                return;
            }
            WizardStep::RemotePath => WizardStep::LocalFolder,
            WizardStep::LoginSource => WizardStep::RemotePath,
            WizardStep::Review => WizardStep::LoginSource,
        };
    }

    fn confirm_modal(&mut self) {
        let action = match &self.modal {
            Some(Modal::Confirm { action, .. }) => *action,
            Some(Modal::Error(_)) | Some(Modal::Progress) | Some(Modal::Input) | None => {
                self.modal = None;
                return;
            }
        };
        self.modal = None;
        match action {
            ConfirmAction::Disconnect => {
                if let Some(mount) = self.selected_mount().cloned() {
                    self.start_task(TaskKind::Disconnect);
                    actions::spawn_disconnect(self.task_tx.clone(), mount);
                }
            }
            ConfirmAction::RemoveMount => {
                if let Some(mount) = self.selected_mount().cloned() {
                    self.start_task(TaskKind::RemoveMount);
                    actions::spawn_remove_mount(self.task_tx.clone(), mount);
                }
            }
            ConfirmAction::RemoveSource => {
                if let Some(source) = self.selected_source().cloned() {
                    self.start_task(TaskKind::RemoveSource);
                    actions::spawn_remove_source(self.task_tx.clone(), source.id);
                }
            }
            ConfirmAction::Apply => {
                self.start_task(TaskKind::Apply);
                actions::spawn_apply(self.task_tx.clone());
            }
            ConfirmAction::Repair => {
                let mount = self.selected_mount().cloned();
                self.start_task(TaskKind::Repair);
                actions::spawn_repair(self.task_tx.clone(), mount);
            }
        }
    }

    fn set_selected_source_default(&mut self) {
        if let Some(source) = self.selected_source().cloned() {
            self.start_task(TaskKind::SetDefaultSource);
            actions::spawn_set_default_source(self.task_tx.clone(), source.id);
        }
    }

    fn start_task(&mut self, kind: TaskKind) {
        self.task = TaskState::Running {
            kind,
            steps: actions::steps_for(kind),
        };
        if kind != TaskKind::Refresh {
            self.modal = Some(Modal::Progress);
        }
    }
}

fn validate_local_input(input: &str) -> Result<(), ShelfError> {
    if input.trim().is_empty() {
        return Err(ShelfError::Validation(
            "local folder cannot be empty".into(),
        ));
    }
    let path = paths::expand_local_path(input)?;
    paths::validate_local_path(&path)
}

fn move_index(current: usize, len: usize, delta: isize) -> usize {
    let last = len.saturating_sub(1) as isize;
    (current as isize + delta).clamp(0, last) as usize
}

fn clamp_index(index: &mut usize, len: usize) {
    if len == 0 {
        *index = 0;
    } else if *index >= len {
        *index = len - 1;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    #[test]
    fn navigates_home_to_help_and_back() {
        let (tx, _) = mpsc::channel();
        let mut app = App::new(tx);
        app.show_help();
        assert_eq!(app.screen, Screen::Help);
        app.back();
        assert_eq!(app.screen, Screen::Home);
    }

    #[test]
    fn add_mount_wizard_validates_local_folder() {
        let (tx, _) = mpsc::channel();
        let mut app = App::new(tx);
        app.goto(Screen::AddMount);
        app.enter();
        assert!(matches!(app.modal, Some(Modal::Error(_))));
    }

    #[test]
    fn selection_clamps_at_bounds() {
        let (tx, _) = mpsc::channel();
        let mut app = App::new(tx);
        app.status_rows = vec![
            StatusRow {
                mount: MountConfig {
                    local_path: "/tmp/a".into(),
                    source_id: "home".into(),
                    remote_path: "/a".into(),
                },
                health: MountHealth::Missing,
            },
            StatusRow {
                mount: MountConfig {
                    local_path: "/tmp/b".into(),
                    source_id: "home".into(),
                    remote_path: "/b".into(),
                },
                health: MountHealth::Missing,
            },
        ];
        app.move_selection(10);
        assert_eq!(app.selected_mount, 1);
        app.move_selection(-10);
        assert_eq!(app.selected_mount, 0);
    }

    #[test]
    fn refresh_done_does_not_start_another_refresh() {
        let (tx, rx) = mpsc::channel();
        let mut app = App::new(tx);
        app.start_task(TaskKind::Refresh);
        app.modal = Some(Modal::Progress);

        app.handle_task_message(TaskMessage::Done {
            steps: steps_done_for_test(TaskKind::Refresh),
        });

        assert!(matches!(app.task, TaskState::Done { .. }));
        assert!(app.modal.is_none());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn mutating_task_done_starts_refresh() {
        let (tx, _) = mpsc::channel();
        let mut app = App::new(tx);
        app.start_task(TaskKind::Apply);

        app.handle_task_message(TaskMessage::Done {
            steps: steps_done_for_test(TaskKind::Apply),
        });

        assert!(matches!(
            app.task,
            TaskState::Running {
                kind: TaskKind::Refresh,
                ..
            }
        ));
    }

    fn steps_done_for_test(kind: TaskKind) -> Vec<TaskStep> {
        actions::steps_for(kind)
            .into_iter()
            .map(|mut step| {
                step.done = true;
                step
            })
            .collect()
    }
}
