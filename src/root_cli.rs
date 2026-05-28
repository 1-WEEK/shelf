use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use nix::unistd::Uid;

use crate::command::SystemRunner;
use crate::config::Config;
use crate::{apply, credentials, Result, ShelfError};

#[derive(Debug, Parser)]
#[command(name = "shelf-root", about = "Privileged helper for shelf")]
struct RootCli {
    #[command(subcommand)]
    command: RootCommand,
}

#[derive(Debug, Subcommand)]
enum RootCommand {
    StoreCredential {
        #[arg(long)]
        id: String,
        #[arg(long)]
        username: String,
    },
    RemoveCredential {
        #[arg(long)]
        id: String,
    },
    Apply {
        #[arg(long)]
        config: PathBuf,
    },
    Remove,
    Unmount {
        #[arg(long)]
        local_path: PathBuf,
    },
    SessionUp {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        source_id: String,
        #[arg(long)]
        local_path: PathBuf,
        #[arg(long)]
        remote_path: String,
    },
    SessionDown {
        #[arg(long)]
        local_path: Option<PathBuf>,
    },
}

pub fn run() -> Result<()> {
    let cli = RootCli::parse();
    ensure_root()?;
    let mut runner = SystemRunner;

    match cli.command {
        RootCommand::StoreCredential { id, username } => {
            let password = credentials::read_password_from_stdin()?;
            credentials::write_credential(&id, &username, &password)?;
            Ok(())
        }
        RootCommand::RemoveCredential { id } => credentials::remove_credential(&id),
        RootCommand::Apply { config } => {
            let (owner_uid, owner_gid) = config_owner(&config)?;
            let config = Config::load(&config)?;
            apply::apply_config(&config, owner_uid, owner_gid, &mut runner)
        }
        RootCommand::Remove => apply::remove_all(&mut runner),
        RootCommand::Unmount { local_path } => {
            apply::unmount_persistent_target(&local_path, &mut runner)
        }
        RootCommand::SessionUp {
            config,
            source_id,
            local_path,
            remote_path,
        } => {
            let (owner_uid, owner_gid) = config_owner(&config)?;
            let config = Config::load(&config)?;
            let source = config.source(&source_id)?;
            apply::session_up(
                apply::SessionMountRequest {
                    source_id,
                    address: source.address.clone(),
                    owner_uid,
                    owner_gid,
                    local_path,
                    remote_path,
                },
                &mut runner,
            )
        }
        RootCommand::SessionDown { local_path } => {
            apply::session_down(local_path.as_deref(), &mut runner)
        }
    }
}

fn config_owner(path: &Path) -> Result<(u32, u32)> {
    let metadata = std::fs::metadata(path).map_err(|source| ShelfError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok((metadata.uid(), metadata.gid()))
}

fn ensure_root() -> Result<()> {
    if Uid::effective().is_root() {
        Ok(())
    } else {
        Err(ShelfError::RequiresRoot)
    }
}
