use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;
use std::thread;

use crate::command::SystemRunner;
use crate::config::{Config, MountConfig};
use crate::error::IoContext;
use crate::remote_path::RemotePath;
use crate::{mounts, paths, Result, ShelfError};

use super::app::{AddMountForm, SourceForm};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskKind {
    Refresh,
    AddMount,
    AddSource,
    SetDefaultSource,
    Disconnect,
    RemoveMount,
    RemoveSource,
    Apply,
    Repair,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskStep {
    pub label: String,
    pub done: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MountHealth {
    Ready,
    NeedsAttention,
    Missing,
    Broken,
}

#[derive(Debug)]
pub enum TaskMessage {
    Status {
        config: Config,
        path: PathBuf,
        rows: Vec<(MountConfig, MountHealth)>,
    },
    Started {
        kind: TaskKind,
        steps: Vec<TaskStep>,
    },
    Step {
        index: usize,
        label: String,
    },
    Done {
        steps: Vec<TaskStep>,
    },
    Failed {
        steps: Vec<TaskStep>,
        error: String,
    },
}

pub fn steps_for(kind: TaskKind) -> Vec<TaskStep> {
    let labels = match kind {
        TaskKind::Refresh => vec!["load config", "check mount state"],
        TaskKind::AddMount => vec![
            "validate config",
            "check credentials",
            "mount remote path",
            "create local folder",
            "write systemd units",
            "start bind mount",
            "test write access",
        ],
        TaskKind::AddSource => vec!["validate login source", "store credential", "save config"],
        TaskKind::SetDefaultSource => vec!["validate login source", "save config"],
        TaskKind::Disconnect => vec![
            "clean stacked mounts",
            "disconnect local mount",
            "refresh state",
        ],
        TaskKind::RemoveMount => vec![
            "disconnect local mount",
            "remove shelf config",
            "remove systemd unit",
            "refresh state",
        ],
        TaskKind::RemoveSource => vec!["validate source usage", "remove credential", "save config"],
        TaskKind::Apply => vec![
            "validate config",
            "check credentials",
            "mount remote path",
            "create local folder",
            "write systemd units",
            "start bind mount",
            "test write access",
        ],
        TaskKind::Repair => vec![
            "clean stacked mounts",
            "validate config",
            "check credentials",
            "mount remote path",
            "write systemd units",
            "start bind mount",
            "test write access",
        ],
    };
    labels
        .into_iter()
        .map(|label| TaskStep {
            label: label.into(),
            done: false,
        })
        .collect()
}

pub fn spawn_refresh(tx: Sender<TaskMessage>) {
    spawn_task(tx, TaskKind::Refresh, |progress| {
        progress(0, "load config")?;
        let (config, path) = Config::load_user()?;
        progress(1, "check mount state")?;
        let rows = status_rows(&config)?;
        Ok(TaskMessage::Status { config, path, rows })
    });
}

pub fn spawn_add_mount(tx: Sender<TaskMessage>, form: AddMountForm) {
    spawn_task(tx, TaskKind::AddMount, move |progress| {
        progress(0, "validate config")?;
        let (mut config, path) = Config::load_user()?;
        let source_id = selected_source_id(&config, form.source_index)?;
        let local_path = paths::expand_local_path(&form.local_folder)?;
        config.add_mount(local_path, form.remote_path, source_id)?;
        config.save(&path)?;
        progress(1, "check credentials")?;
        run_sudo_shelf_root(
            [
                "apply".to_string(),
                "--config".to_string(),
                path.to_string_lossy().to_string(),
            ],
            None,
        )?;
        let steps = steps_for(TaskKind::AddMount);
        for (index, step) in steps.iter().enumerate().take(7).skip(2) {
            progress(index, &step.label)?;
        }
        Ok(TaskMessage::Done {
            steps: done_steps(TaskKind::AddMount),
        })
    });
}

pub fn spawn_add_source(tx: Sender<TaskMessage>, form: SourceForm) {
    spawn_task(tx, TaskKind::AddSource, move |progress| {
        progress(0, "validate login source")?;
        let (mut config, path) = Config::load_user()?;
        let name = if form.id.trim().is_empty() {
            None
        } else {
            Some(form.id)
        };
        let id = config.add_source(form.address, form.username.clone(), name, true)?;
        progress(1, "store credential")?;
        run_sudo_shelf_root(
            [
                "store-credential".to_string(),
                "--id".to_string(),
                id,
                "--username".to_string(),
                form.username,
            ],
            Some(form.password.into_bytes()),
        )?;
        progress(2, "save config")?;
        config.save(&path)?;
        Ok(TaskMessage::Done {
            steps: done_steps(TaskKind::AddSource),
        })
    });
}

pub fn spawn_set_default_source(tx: Sender<TaskMessage>, id: String) {
    spawn_task(tx, TaskKind::SetDefaultSource, move |progress| {
        progress(0, "validate login source")?;
        let (mut config, path) = Config::load_user()?;
        config.set_default_source(&id)?;
        progress(1, "save config")?;
        config.save(&path)?;
        Ok(TaskMessage::Done {
            steps: done_steps(TaskKind::SetDefaultSource),
        })
    });
}

pub fn spawn_disconnect(tx: Sender<TaskMessage>, mount: MountConfig) {
    spawn_task(tx, TaskKind::Disconnect, move |progress| {
        progress(0, "clean stacked mounts")?;
        progress(1, "disconnect local mount")?;
        run_sudo_shelf_root(
            [
                "unmount".to_string(),
                "--local-path".to_string(),
                mount.local_path,
            ],
            None,
        )?;
        progress(2, "refresh state")?;
        Ok(TaskMessage::Done {
            steps: done_steps(TaskKind::Disconnect),
        })
    });
}

pub fn spawn_remove_mount(tx: Sender<TaskMessage>, mount: MountConfig) {
    spawn_task(tx, TaskKind::RemoveMount, move |progress| {
        progress(0, "disconnect local mount")?;
        run_sudo_shelf_root(
            [
                "unmount".to_string(),
                "--local-path".to_string(),
                mount.local_path.clone(),
            ],
            None,
        )?;
        progress(1, "remove shelf config")?;
        let (mut config, path) = Config::load_user()?;
        config.remove_mount(Path::new(&mount.local_path))?;
        config.save(&path)?;
        progress(2, "remove systemd unit")?;
        progress(3, "refresh state")?;
        Ok(TaskMessage::Done {
            steps: done_steps(TaskKind::RemoveMount),
        })
    });
}

pub fn spawn_remove_source(tx: Sender<TaskMessage>, id: String) {
    spawn_task(tx, TaskKind::RemoveSource, move |progress| {
        progress(0, "validate source usage")?;
        let (mut config, path) = Config::load_user()?;
        config.remove_source(&id)?;
        progress(1, "remove credential")?;
        run_sudo_shelf_root(
            [
                "remove-credential".to_string(),
                "--id".to_string(),
                id.clone(),
            ],
            None,
        )?;
        progress(2, "save config")?;
        config.save(&path)?;
        Ok(TaskMessage::Done {
            steps: done_steps(TaskKind::RemoveSource),
        })
    });
}

pub fn spawn_apply(tx: Sender<TaskMessage>) {
    spawn_task(tx, TaskKind::Apply, |progress| {
        let (_, path) = Config::load_user()?;
        progress(0, "validate config")?;
        progress(1, "check credentials")?;
        run_sudo_shelf_root(
            [
                "apply".to_string(),
                "--config".to_string(),
                path.to_string_lossy().to_string(),
            ],
            None,
        )?;
        let steps = steps_for(TaskKind::Apply);
        for (index, step) in steps.iter().enumerate().take(7).skip(2) {
            progress(index, &step.label)?;
        }
        Ok(TaskMessage::Done {
            steps: done_steps(TaskKind::Apply),
        })
    });
}

pub fn spawn_repair(tx: Sender<TaskMessage>, mount: Option<MountConfig>) {
    spawn_task(tx, TaskKind::Repair, move |progress| {
        progress(0, "clean stacked mounts")?;
        if let Some(mount) = mount {
            let _ = run_sudo_shelf_root(
                [
                    "unmount".to_string(),
                    "--local-path".to_string(),
                    mount.local_path,
                ],
                None,
            );
        }
        let (_, path) = Config::load_user()?;
        progress(1, "validate config")?;
        progress(2, "check credentials")?;
        run_sudo_shelf_root(
            [
                "apply".to_string(),
                "--config".to_string(),
                path.to_string_lossy().to_string(),
            ],
            None,
        )?;
        let steps = steps_for(TaskKind::Repair);
        for (index, step) in steps.iter().enumerate().take(7).skip(3) {
            progress(index, &step.label)?;
        }
        Ok(TaskMessage::Done {
            steps: done_steps(TaskKind::Repair),
        })
    });
}

fn spawn_task<F>(tx: Sender<TaskMessage>, kind: TaskKind, work: F)
where
    F: FnOnce(&mut dyn FnMut(usize, &str) -> Result<()>) -> Result<TaskMessage> + Send + 'static,
{
    thread::spawn(move || {
        let steps = steps_for(kind);
        let _ = tx.send(TaskMessage::Started {
            kind,
            steps: steps.clone(),
        });
        let mut task_steps = steps;
        let tx_for_progress = tx.clone();
        let mut progress = |index: usize, label: &str| -> Result<()> {
            if let Some(step) = task_steps.get_mut(index) {
                step.done = true;
                step.label = label.to_string();
            }
            tx_for_progress
                .send(TaskMessage::Step {
                    index,
                    label: label.to_string(),
                })
                .map_err(|source| {
                    ShelfError::Validation(format!("TUI channel closed: {source}"))
                })?;
            Ok(())
        };
        match work(&mut progress) {
            Ok(TaskMessage::Done { steps }) => {
                let _ = tx.send(TaskMessage::Done { steps });
            }
            Ok(message) => {
                let _ = tx.send(message);
                let _ = tx.send(TaskMessage::Done { steps: task_steps });
            }
            Err(error) => {
                let _ = tx.send(TaskMessage::Failed {
                    steps: task_steps,
                    error: error.to_string(),
                });
            }
        }
    });
}

fn status_rows(config: &Config) -> Result<Vec<(MountConfig, MountHealth)>> {
    let mut runner = SystemRunner;
    config
        .mounts
        .iter()
        .map(|mount| {
            let remote = RemotePath::parse(&mount.remote_path)?;
            let mount_root = paths::mount_root_for(&mount.source_id, remote.top_level());
            let local_path = PathBuf::from(&mount.local_path);
            let cifs = mounts::find_mountpoint(&mut runner, &mount_root)?;
            let local = mounts::find_mountpoint(&mut runner, &local_path)?;
            let health = match (cifs, local) {
                (Some(cifs), Some(_)) if cifs.fs_type == "cifs" => MountHealth::Ready,
                (Some(_), Some(_)) | (Some(_), None) | (None, Some(_)) => {
                    MountHealth::NeedsAttention
                }
                (None, None) => MountHealth::Missing,
            };
            Ok((mount.clone(), health))
        })
        .collect()
}

fn selected_source_id(config: &Config, index: usize) -> Result<String> {
    config
        .sources
        .values()
        .nth(index)
        .map(|source| source.id.clone())
        .or_else(|| config.default_source.clone())
        .ok_or(ShelfError::MissingDefaultSource)
}

fn done_steps(kind: TaskKind) -> Vec<TaskStep> {
    steps_for(kind)
        .into_iter()
        .map(|mut step| {
            step.done = true;
            step
        })
        .collect()
}

fn run_sudo_shelf_root<I, S>(args: I, stdin: Option<Vec<u8>>) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let root = shelf_root_program()?;
    let mut command = Command::new("sudo");
    command.arg(root);
    let args = args.into_iter().map(Into::into).collect::<Vec<String>>();
    command.args(&args);
    command.stdout(Stdio::null()).stderr(Stdio::piped());
    if stdin.is_some() {
        command.stdin(Stdio::piped());
    }

    let mut child = command.spawn().map_err(|source| ShelfError::CommandIo {
        program: "sudo".into(),
        source,
    })?;
    if let Some(stdin) = stdin {
        let mut child_stdin = child.stdin.take().ok_or_else(|| ShelfError::CommandIo {
            program: "sudo".into(),
            source: std::io::Error::new(std::io::ErrorKind::BrokenPipe, "sudo stdin unavailable"),
        })?;
        child_stdin
            .write_all(&stdin)
            .map_err(|source| ShelfError::CommandIo {
                program: "sudo".into(),
                source,
            })?;
    }
    let output = child
        .wait_with_output()
        .map_err(|source| ShelfError::CommandIo {
            program: "sudo".into(),
            source,
        })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(ShelfError::CommandFailed {
            program: "sudo".into(),
            args,
            status: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        })
    }
}

fn shelf_root_program() -> Result<String> {
    let current = std::env::current_exe().with_path("<current_exe>")?;
    let sibling = current.with_file_name("shelf-root");
    if sibling.exists() {
        Ok(sibling.to_string_lossy().to_string())
    } else {
        Ok("shelf-root".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repair_steps_keep_expected_order() {
        let labels = steps_for(TaskKind::Repair)
            .into_iter()
            .map(|step| step.label)
            .collect::<Vec<_>>();
        assert_eq!(
            labels,
            vec![
                "clean stacked mounts",
                "validate config",
                "check credentials",
                "mount remote path",
                "write systemd units",
                "start bind mount",
                "test write access",
            ]
        );
    }
}
