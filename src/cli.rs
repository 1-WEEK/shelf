use std::io::IsTerminal;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use clap::{Args, Parser, Subcommand};
use dialoguer::{Confirm, Password, Select};

use crate::command::SystemRunner;
use crate::config::Config;
use crate::error::IoContext;
use crate::{paths, progress, status, Result, ShelfError};

#[derive(Debug, Parser)]
#[command(
    name = "shelf",
    about = "Mount local paths onto SMB-backed remote paths"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Source {
        #[command(subcommand)]
        command: SourceCommand,
    },
    Mount(MountArgs),
    Unmount(UnmountArgs),
    Apply,
    Remove,
    Status,
    Tui,
    Session {
        #[command(subcommand)]
        command: SessionCommand,
    },
    Run(RunArgs),
}

#[derive(Debug, Subcommand)]
enum SourceCommand {
    Add(SourceAddArgs),
    List,
    Default { id: String },
    Remove { id: Option<String> },
}

#[derive(Debug, Args)]
struct SourceAddArgs {
    address: String,
    #[arg(long)]
    username: String,
    #[arg(long)]
    name: Option<String>,
    /// Force this source to become the default. Without this flag a new
    /// source only becomes the default if no default is configured yet.
    #[arg(long = "default", default_value_t = false)]
    make_default: bool,
}

#[derive(Debug, Args)]
struct MountArgs {
    local_path: String,
    remote_path: String,
    #[arg(long)]
    source: Option<String>,
    #[arg(long, default_value_t = false)]
    temporary: bool,
    #[arg(long, default_value_t = false)]
    no_apply: bool,
}

#[derive(Debug, Args)]
struct UnmountArgs {
    local_path: String,
    #[arg(long, default_value_t = false)]
    temporary: bool,
}

#[derive(Debug, Subcommand)]
enum SessionCommand {
    Up,
    Down {
        #[arg(long)]
        local_path: Option<String>,
    },
}

#[derive(Debug, Args)]
struct RunArgs {
    #[arg(long = "mount", required = true)]
    mounts: Vec<String>,
    #[arg(long)]
    source: Option<String>,
    #[arg(last = true, required = true)]
    command: Vec<String>,
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        None | Some(Commands::Tui) => {
            if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
                return Err(ShelfError::Validation(
                    "shelf tui requires an interactive TTY; use shelf status, shelf mount, or shelf source for non-interactive workflows".into(),
                ));
            }
            crate::tui::run()
        }
        Some(Commands::Source { command }) => run_source(command),
        Some(Commands::Mount(args)) => run_mount(args),
        Some(Commands::Unmount(args)) => run_unmount(args),
        Some(Commands::Apply) => run_apply(),
        Some(Commands::Remove) => sudo_shelf_root(["remove"], None).map(|_| ()),
        Some(Commands::Status) => {
            let (config, _) = Config::load_user()?;
            let mut runner = SystemRunner;
            status::print_status(&config, &mut runner)
        }
        Some(Commands::Session { command }) => run_session(command),
        Some(Commands::Run(args)) => run_command(args),
    }
}

fn run_source(command: SourceCommand) -> Result<()> {
    let (mut config, path) = Config::load_user()?;
    match command {
        SourceCommand::Add(args) => {
            let password = Password::new()
                .with_prompt("SMB password")
                .interact()
                .map_err(|source| {
                    ShelfError::Validation(format!("failed to read password: {source}"))
                })?;
            let id = config.add_source(
                args.address,
                args.username.clone(),
                args.name,
                args.make_default,
            )?;
            progress::step(format!("storing credential for source {id}"));
            sudo_shelf_root(
                [
                    "store-credential".to_string(),
                    "--id".to_string(),
                    id.clone(),
                    "--username".to_string(),
                    args.username,
                ],
                Some(password.into_bytes()),
            )?;
            config.save(&path)?;
            progress::ok(format!("added source {id}"));
            Ok(())
        }
        SourceCommand::List => {
            if config.sources.is_empty() {
                println!("No sources configured.");
            } else {
                for source in config.sources.values() {
                    let marker = if config.default_source.as_deref() == Some(&source.id) {
                        " default"
                    } else {
                        ""
                    };
                    println!(
                        "{}{}: {} as {}",
                        source.id, marker, source.address, source.username
                    );
                }
            }
            Ok(())
        }
        SourceCommand::Default { id } => {
            config.set_default_source(&id)?;
            config.save(&path)?;
            println!("Default source set to {id}");
            Ok(())
        }
        SourceCommand::Remove { id } => {
            let id = match id {
                Some(id) => id,
                None => choose_source_to_remove(&config)?,
            };
            if !Confirm::new()
                .with_prompt(format!("Remove source {id}?"))
                .default(false)
                .interact()
                .map_err(|source| {
                    ShelfError::Validation(format!("failed to read confirmation: {source}"))
                })?
            {
                progress::step("source removal cancelled");
                return Ok(());
            }
            progress::step(format!("removing source {id}"));
            config.remove_source(&id)?;
            sudo_shelf_root(
                [
                    "remove-credential".to_string(),
                    "--id".to_string(),
                    id.clone(),
                ],
                None,
            )?;
            config.save(&path)?;
            progress::ok(format!("removed source {id}"));
            Ok(())
        }
    }
}

fn choose_source_to_remove(config: &Config) -> Result<String> {
    if config.sources.is_empty() {
        return Err(ShelfError::Validation("no sources configured".into()));
    }

    let sources = config.sources.values().collect::<Vec<_>>();
    let labels = sources
        .iter()
        .map(|source| {
            let default_marker = if config.default_source.as_deref() == Some(&source.id) {
                " default"
            } else {
                ""
            };
            format!(
                "{}{}: {} as {}",
                source.id, default_marker, source.address, source.username
            )
        })
        .collect::<Vec<_>>();

    let index = Select::new()
        .with_prompt("Select source to remove")
        .items(&labels)
        .default(0)
        .interact()
        .map_err(|source| {
            ShelfError::Validation(format!("failed to read source selection: {source}"))
        })?;
    Ok(sources[index].id.clone())
}

fn run_mount(args: MountArgs) -> Result<()> {
    let (mut config, path) = Config::load_user()?;
    let source_id = config.resolve_source_id(args.source.as_deref())?;
    let local_path = paths::expand_local_path(&args.local_path)?;
    paths::validate_local_path(&local_path)?;

    if args.temporary {
        let source = config.source(&source_id)?;
        progress::step(format!(
            "mounting {} -> {}:{} temporarily",
            local_path.display(),
            source_id,
            args.remote_path
        ));
        sudo_session_up(&path, source, &local_path, &args.remote_path)?;
        progress::ok("temporary mount ready");
        return Ok(());
    }

    config.add_mount(
        local_path.clone(),
        args.remote_path.clone(),
        source_id.clone(),
    )?;
    config.save(&path)?;
    if !args.no_apply {
        progress::step("applying persistent mount configuration");
        run_apply()?;
    }
    progress::ok(format!(
        "configured mount {} -> {}:{}",
        local_path.display(),
        source_id,
        args.remote_path
    ));
    Ok(())
}

fn run_unmount(args: UnmountArgs) -> Result<()> {
    let (mut config, path) = Config::load_user()?;
    let local_path = paths::expand_local_path(&args.local_path)?;

    if args.temporary {
        progress::step(format!(
            "unmounting temporary mount {}",
            local_path.display()
        ));
        sudo_shelf_root(
            [
                "session-down".to_string(),
                "--local-path".to_string(),
                local_path.to_string_lossy().to_string(),
            ],
            None,
        )?;
        progress::ok(format!(
            "unmounted temporary mount {}",
            local_path.display()
        ));
        return Ok(());
    }

    progress::step(format!("unmounting {}", local_path.display()));
    sudo_shelf_root(
        [
            "unmount".to_string(),
            "--local-path".to_string(),
            local_path.to_string_lossy().to_string(),
        ],
        None,
    )?;
    config.remove_mount(&local_path)?;
    config.save(&path)?;
    progress::ok(format!("unmounted {}", local_path.display()));
    Ok(())
}

fn run_apply() -> Result<()> {
    let (_, path) = Config::load_user()?;
    progress::step("running privileged apply");
    sudo_shelf_root(
        [
            "apply".to_string(),
            "--config".to_string(),
            path.to_string_lossy().to_string(),
        ],
        None,
    )?;
    progress::ok("applied shelf configuration");
    Ok(())
}

fn run_session(command: SessionCommand) -> Result<()> {
    match command {
        SessionCommand::Up => {
            let (config, path) = Config::load_user()?;
            for mount in &config.mounts {
                let source = config.source(&mount.source_id)?;
                progress::step(format!(
                    "mounting {} -> {}:{} temporarily",
                    mount.local_path, mount.source_id, mount.remote_path
                ));
                sudo_session_up(
                    &path,
                    source,
                    &PathBuf::from(&mount.local_path),
                    &mount.remote_path,
                )?;
            }
            progress::ok("temporary session is up");
            Ok(())
        }
        SessionCommand::Down { local_path } => {
            let mut args = vec!["session-down".to_string()];
            if let Some(local_path) = local_path {
                args.push("--local-path".to_string());
                args.push(
                    paths::expand_local_path(&local_path)?
                        .to_string_lossy()
                        .to_string(),
                );
            }
            progress::step("tearing down temporary session");
            sudo_shelf_root(args, None)?;
            progress::ok("temporary session is down");
            Ok(())
        }
    }
}

fn run_command(args: RunArgs) -> Result<()> {
    let (config, config_path) = Config::load_user()?;
    let mut mounted_locals = Vec::new();
    for spec in &args.mounts {
        let (local, remote) = parse_run_mount(spec)?;
        let source_id = config.resolve_source_id(args.source.as_deref())?;
        let source = config.source(&source_id)?;
        let local_path = paths::expand_local_path(local)?;
        progress::step(format!(
            "mounting {} -> {}:{} for command",
            local_path.display(),
            source_id,
            remote
        ));
        sudo_session_up(&config_path, source, &local_path, remote)?;
        mounted_locals.push(local_path);
    }

    let mut child = Command::new(&args.command[0]);
    child.args(&args.command[1..]);
    let status = child.status().map_err(|source| ShelfError::CommandIo {
        program: args.command[0].clone(),
        source,
    });

    let mut cleanup_errors = Vec::new();
    for local_path in mounted_locals.iter().rev() {
        if let Err(err) = sudo_shelf_root(
            [
                "session-down".to_string(),
                "--local-path".to_string(),
                local_path.to_string_lossy().to_string(),
            ],
            None,
        ) {
            cleanup_errors.push(err);
        }
    }

    if let Some(err) = aggregate_cleanup_errors(cleanup_errors) {
        return Err(err);
    }

    let status = status?;
    if status.success() {
        Ok(())
    } else {
        Err(ShelfError::CommandFailed {
            program: args.command[0].clone(),
            args: args.command[1..].to_vec(),
            status: status.code().unwrap_or(-1),
            stderr: "child command exited unsuccessfully".into(),
        })
    }
}

fn aggregate_cleanup_errors(errors: Vec<ShelfError>) -> Option<ShelfError> {
    match errors.len() {
        0 => None,
        1 => errors.into_iter().next(),
        _ => {
            let count = errors.len();
            let lines: Vec<String> = errors.iter().map(|err| err.to_string()).collect();
            Some(ShelfError::Validation(format!(
                "{count} cleanup operations failed:\n  - {}",
                lines.join("\n  - ")
            )))
        }
    }
}

fn parse_run_mount(spec: &str) -> Result<(&str, &str)> {
    let (local, remote) = spec
        .split_once(':')
        .ok_or_else(|| ShelfError::InvalidRunMount(spec.to_string()))?;
    if local.is_empty() || remote.is_empty() {
        return Err(ShelfError::InvalidRunMount(spec.to_string()));
    }
    Ok((local, remote))
}

fn sudo_session_up(
    config_path: &Path,
    source: &crate::config::SourceConfig,
    local_path: &Path,
    remote_path: &str,
) -> Result<()> {
    sudo_shelf_root(
        [
            "session-up".to_string(),
            "--config".to_string(),
            config_path.to_string_lossy().to_string(),
            "--source-id".to_string(),
            source.id.clone(),
            "--local-path".to_string(),
            local_path.to_string_lossy().to_string(),
            "--remote-path".to_string(),
            remote_path.to_string(),
        ],
        None,
    )?;
    Ok(())
}

fn sudo_shelf_root<I, S>(args: I, stdin: Option<Vec<u8>>) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let root = shelf_root_program()?;
    let mut command = Command::new("sudo");
    command.arg(root);
    for arg in args {
        command.arg(arg.into());
    }
    command.stdout(Stdio::inherit()).stderr(Stdio::inherit());
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
    let status = child.wait().map_err(|source| ShelfError::CommandIo {
        program: "sudo".into(),
        source,
    })?;
    if !status.success() {
        return Err(ShelfError::CommandFailed {
            program: "sudo".into(),
            args: Vec::new(),
            status: status.code().unwrap_or(-1),
            stderr: "privileged helper exited unsuccessfully".into(),
        });
    }
    Ok(())
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
    fn parses_run_mount_spec() {
        assert_eq!(
            parse_run_mount("/tmp/videos:/media/movies").unwrap(),
            ("/tmp/videos", "/media/movies")
        );
        assert!(parse_run_mount("/tmp/videos").is_err());
    }

    #[test]
    fn aggregate_cleanup_errors_returns_none_when_empty() {
        assert!(aggregate_cleanup_errors(Vec::new()).is_none());
    }

    #[test]
    fn aggregate_cleanup_errors_passes_through_single_error() {
        let err = ShelfError::Validation("solo".into());
        let aggregated = aggregate_cleanup_errors(vec![err]).unwrap();
        assert_eq!(aggregated.to_string(), "solo");
    }

    #[test]
    fn aggregate_cleanup_errors_lists_every_failure_when_many() {
        let errors = vec![
            ShelfError::Validation("first failure".into()),
            ShelfError::Validation("second failure".into()),
        ];
        let aggregated = aggregate_cleanup_errors(errors).unwrap().to_string();
        assert!(aggregated.contains("2 cleanup operations failed"));
        assert!(aggregated.contains("first failure"));
        assert!(aggregated.contains("second failure"));
    }
}
