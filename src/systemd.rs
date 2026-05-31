use std::path::{Path, PathBuf};

use crate::command::{checked, CommandRunner, CommandSpec};
use crate::error::IoContext;
use crate::{mounts, paths, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitFile {
    pub name: String,
    pub path: PathBuf,
    pub content: String,
}

pub fn mount_unit_name_for(runner: &mut impl CommandRunner, path: &Path) -> Result<String> {
    let spec = CommandSpec::new("systemd-escape")
        .arg("--path")
        .arg("--suffix=mount")
        .arg(path.to_string_lossy().to_string());
    let output = runner.run(spec.clone())?;
    let output = checked(spec, output)?;
    Ok(output.stdout.trim().to_string())
}

pub fn cifs_unit_content(
    address: &str,
    top_level: &str,
    where_path: &Path,
    credential_file: &Path,
    owner_uid: u32,
    owner_gid: u32,
) -> String {
    let options = mounts::cifs_mount_options(credential_file, owner_uid, owner_gid);
    format!(
        "[Unit]\n\
Description=shelf SMB mount //{address}/{top_level}\n\
After=network-online.target\n\
Wants=network-online.target\n\
\n\
[Mount]\n\
What=//{address}/{top_level}\n\
Where={}\n\
Type=cifs\n\
Options={options}\n\
TimeoutSec=30\n\
\n\
[Install]\n\
WantedBy=multi-user.target\n",
        where_path.display()
    )
}

pub fn bind_unit_content(source: &Path, target: &Path, requires_unit: &str) -> String {
    format!(
        "[Unit]\n\
Description=shelf bind mount {} -> {}\n\
Requires={requires_unit}\n\
After={requires_unit}\n\
\n\
[Mount]\n\
What={}\n\
Where={}\n\
Type=none\n\
Options=bind\n\
\n\
[Install]\n\
WantedBy=multi-user.target\n",
        target.display(),
        source.display(),
        source.display(),
        target.display()
    )
}

pub fn write_unit(unit: &UnitFile) -> Result<()> {
    if let Some(parent) = unit.path.parent() {
        std::fs::create_dir_all(parent).with_path(parent)?;
    }
    std::fs::write(&unit.path, &unit.content).with_path(&unit.path)
}

pub fn unit_path(name: &str) -> PathBuf {
    paths::systemd_unit_dir().join(name)
}

pub fn daemon_reload(runner: &mut impl CommandRunner) -> Result<()> {
    run_systemctl_checked(runner, ["daemon-reload"])
}

pub fn enable_now(runner: &mut impl CommandRunner, unit: &str) -> Result<()> {
    run_systemctl_checked(runner, ["enable", "--now", unit])
}

pub fn disable_now_best_effort(runner: &mut impl CommandRunner, unit: &str) {
    let _ = runner.run(CommandSpec::new("systemctl").args(["disable", "--now", unit]));
}

fn run_systemctl_checked<const N: usize>(
    runner: &mut impl CommandRunner,
    args: [&str; N],
) -> Result<()> {
    let spec = CommandSpec::new("systemctl").args(args);
    let output = runner.run(spec.clone())?;
    checked(spec, output)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cifs_unit_contains_remote_and_credentials() {
        let content = cifs_unit_content(
            "192.168.1.10",
            "media",
            Path::new("/mnt/shelf/sources/home/media"),
            Path::new("/etc/shelf/credentials/home.cred"),
            1000,
            1000,
        );
        assert!(content.contains("What=//192.168.1.10/media"));
        assert!(content.contains("credentials=/etc/shelf/credentials/home.cred"));
        assert!(content.contains("uid=1000,gid=1000"));
    }

    #[test]
    fn bind_unit_depends_on_cifs_unit() {
        let content = bind_unit_content(
            Path::new("/mnt/shelf/sources/home/media/movies"),
            Path::new("/home/alice/Videos"),
            "mnt-shelf-sources-home-media.mount",
        );
        assert!(content.contains("Requires=mnt-shelf-sources-home-media.mount"));
        assert!(content.contains("Options=bind"));
    }
}
