use std::path::PathBuf;

use crate::command::CommandRunner;
use crate::config::Config;
use crate::remote_path::RemotePath;
use crate::{mounts, paths, Result};

pub fn print_status(config: &Config, runner: &mut impl CommandRunner) -> Result<()> {
    if config.sources.is_empty() {
        println!("No sources configured.");
        return Ok(());
    }

    println!("Sources:");
    for source in config.sources.values() {
        let marker = if config.default_source.as_deref() == Some(&source.id) {
            " default"
        } else {
            ""
        };
        println!(
            "  {}{}: {} as {}",
            source.id, marker, source.address, source.username
        );
    }

    if config.mounts.is_empty() {
        println!("Mounts: none");
        return Ok(());
    }

    println!("Mounts:");
    for mount in &config.mounts {
        let remote = RemotePath::parse(&mount.remote_path)?;
        let mount_root = paths::mount_root_for(&mount.source_id, remote.top_level());
        let local_path = PathBuf::from(&mount.local_path);
        let cifs = mounts::find_mountpoint(runner, &mount_root)?;
        let local = mounts::find_mountpoint(runner, &local_path)?;

        let cifs_status = match cifs {
            Some(info) if info.fs_type == "cifs" => "cifs-mounted".to_string(),
            Some(info) => format!("wrong-fs:{}", info.fs_type),
            None => "not-mounted".to_string(),
        };
        let local_status = match local {
            Some(info) => format!("mounted-from:{}", info.source),
            None => "not-mounted".to_string(),
        };

        println!(
            "  {} -> {}:{} [{}; {}]",
            mount.local_path, mount.source_id, mount.remote_path, cifs_status, local_status
        );
    }

    Ok(())
}
