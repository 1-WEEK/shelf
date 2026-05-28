use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::command::{checked, CommandRunner, CommandSpec};
use crate::{Result, ShelfError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountInfo {
    pub target: PathBuf,
    pub source: String,
    pub fs_type: String,
    pub fs_root: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CifsMountOutcome {
    AlreadyMounted,
    Mounted,
}

#[derive(Debug, Deserialize)]
struct FindmntOutput {
    #[serde(default)]
    filesystems: Vec<FindmntEntry>,
}

#[derive(Debug, Deserialize)]
struct FindmntEntry {
    target: String,
    source: String,
    #[serde(rename = "fstype")]
    fs_type: String,
    #[serde(default, rename = "fsroot")]
    fs_root: String,
}

pub fn ensure_dependencies(runner: &mut impl CommandRunner) -> Result<()> {
    for spec in [
        CommandSpec::new("mount.cifs").arg("--version"),
        CommandSpec::new("findmnt").arg("--version"),
        CommandSpec::new("systemctl").arg("--version"),
        CommandSpec::new("systemd-escape").arg("--version"),
    ] {
        let output = runner.run(spec.clone())?;
        checked(spec, output)?;
    }
    Ok(())
}

pub fn find_mountpoint(
    runner: &mut impl CommandRunner,
    target: &Path,
) -> Result<Option<MountInfo>> {
    let spec = CommandSpec::new("findmnt").args([
        "--json".to_string(),
        "--mountpoint".to_string(),
        target.to_string_lossy().to_string(),
        "--output".to_string(),
        "TARGET,SOURCE,FSTYPE,FSROOT".to_string(),
    ]);
    let output = runner.run(spec)?;
    if !output.success() {
        return Ok(None);
    }
    let parsed: FindmntOutput = serde_json::from_str(&output.stdout).map_err(|source| {
        ShelfError::Validation(format!("failed to parse findmnt output: {source}"))
    })?;
    Ok(parsed
        .filesystems
        .into_iter()
        .next()
        .map(|entry| MountInfo {
            target: PathBuf::from(entry.target),
            source: entry.source,
            fs_type: entry.fs_type,
            fs_root: entry.fs_root,
        }))
}

pub fn ensure_cifs_mounted(
    runner: &mut impl CommandRunner,
    address: &str,
    top_level: &str,
    mount_root: &Path,
    credential_file: &Path,
    owner_uid: u32,
    owner_gid: u32,
) -> Result<CifsMountOutcome> {
    let expected_source = expected_cifs_source(address, top_level);
    if let Some(info) = find_mountpoint(runner, mount_root)? {
        if info.fs_type == "cifs" && info.source == expected_source {
            return Ok(CifsMountOutcome::AlreadyMounted);
        }
        return Err(ShelfError::Validation(format!(
            "mount point exists but is not expected CIFS source: {} (found {} {}, expected {})",
            mount_root.display(),
            info.fs_type,
            info.source,
            expected_source
        )));
    }

    let options = format!(
        "credentials={},uid={},gid={},file_mode=0660,dir_mode=0770,noserverino",
        credential_file.display(),
        owner_uid,
        owner_gid
    );
    let spec = CommandSpec::new("mount.cifs")
        .arg(expected_source.clone())
        .arg(mount_root.to_string_lossy().to_string())
        .arg("-o")
        .arg(options);
    let output = runner.run(spec.clone())?;
    checked(spec, output)?;

    match find_mountpoint(runner, mount_root)? {
        Some(info) if info.fs_type == "cifs" && info.source == expected_source => {
            Ok(CifsMountOutcome::Mounted)
        }
        Some(info) => Err(ShelfError::Validation(format!(
            "expected CIFS source {} at {}, found {} {}",
            expected_source,
            mount_root.display(),
            info.fs_type,
            info.source
        ))),
        None => Err(ShelfError::Validation(format!(
            "mount.cifs reported success but {} is not mounted",
            mount_root.display()
        ))),
    }
}

pub fn expected_cifs_source(address: &str, top_level: &str) -> String {
    format!("//{address}/{top_level}")
}

pub fn ensure_bind_mounted(
    runner: &mut impl CommandRunner,
    source: &Path,
    target: &Path,
    expected_cifs_source: &str,
    expected_fs_root: &str,
) -> Result<()> {
    if let Some(info) = find_mountpoint(runner, target)? {
        if is_expected_bind_mount(&info, source, expected_cifs_source, expected_fs_root) {
            return Ok(());
        }
        return Err(ShelfError::Validation(format!(
            "local path is already a mount point from another source: {} -> {}",
            target.display(),
            info.source
        )));
    }

    let spec = CommandSpec::new("mount")
        .arg("--bind")
        .arg(source.to_string_lossy().to_string())
        .arg(target.to_string_lossy().to_string());
    let output = runner.run(spec.clone())?;
    checked(spec, output)?;
    Ok(())
}

pub fn is_expected_bind_mount(
    info: &MountInfo,
    source: &Path,
    expected_cifs_source: &str,
    expected_fs_root: &str,
) -> bool {
    Path::new(&info.source) == source
        || (info.fs_type == "cifs"
            && info.source == expected_cifs_source
            && normalize_fs_root(&info.fs_root) == normalize_fs_root(expected_fs_root))
}

pub fn expected_fs_root(remainder: &[String]) -> String {
    if remainder.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", remainder.join("/"))
    }
}

fn normalize_fs_root(root: &str) -> String {
    if root.is_empty() {
        "/".to_string()
    } else {
        root.to_string()
    }
}

pub fn unmount(runner: &mut impl CommandRunner, target: &Path) -> Result<()> {
    for _ in 0..16 {
        if find_mountpoint(runner, target)?.is_none() {
            return Ok(());
        }
        let spec = CommandSpec::new("umount").arg(target.to_string_lossy().to_string());
        let output = runner.run(spec.clone())?;
        checked(spec, output)?;
    }
    Err(ShelfError::Validation(format!(
        "mount point still exists after repeated unmount attempts: {}",
        target.display()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::tests::MockRunner;

    #[test]
    fn treats_missing_mountpoint_as_none() {
        let mut runner = MockRunner::default();
        runner.push_failure("not mounted");
        let info = find_mountpoint(&mut runner, Path::new("/mnt/missing")).unwrap();
        assert!(info.is_none());
    }

    #[test]
    fn parses_findmnt_json() {
        let mut runner = MockRunner::default();
        runner.push_success(
            r#"{"filesystems":[{"target":"/mnt/shelf","source":"//nas/media","fstype":"cifs","fsroot":"/"}]}"#,
        );
        let info = find_mountpoint(&mut runner, Path::new("/mnt/shelf"))
            .unwrap()
            .unwrap();
        assert_eq!(info.fs_type, "cifs");
        assert_eq!(info.source, "//nas/media");
        assert_eq!(info.fs_root, "/");
    }

    #[test]
    fn rejects_existing_cifs_mount_from_wrong_source() {
        let mut runner = MockRunner::default();
        runner.push_success(
            r#"{"filesystems":[{"target":"/mnt/shelf","source":"//nas/other","fstype":"cifs","fsroot":"/"}]}"#,
        );
        let err = ensure_cifs_mounted(
            &mut runner,
            "nas",
            "media",
            Path::new("/mnt/shelf"),
            Path::new("/etc/shelf/credentials/home.cred"),
            1000,
            1000,
        )
        .unwrap_err();
        assert!(err.to_string().contains("expected CIFS source"));
    }

    #[test]
    fn recognizes_cifs_bind_mount_reported_as_remote_source() {
        let info = MountInfo {
            target: PathBuf::from("/tmp/shelf-videos"),
            source: "//nas/media".into(),
            fs_type: "cifs".into(),
            fs_root: "/movies".into(),
        };
        assert!(is_expected_bind_mount(
            &info,
            Path::new("/mnt/shelf/sources/home/media/movies"),
            "//nas/media",
            "/movies"
        ));
        assert!(!is_expected_bind_mount(
            &info,
            Path::new("/mnt/shelf/sources/home/media/other"),
            "//nas/media",
            "/other"
        ));
    }

    #[test]
    fn unmount_repeats_until_mountpoint_is_clear() {
        let mut runner = MockRunner::default();
        runner.push_success(
            r#"{"filesystems":[{"target":"/tmp/shelf-videos","source":"//nas/media","fstype":"cifs","fsroot":"/"}]}"#,
        );
        runner.push_success("");
        runner.push_success(
            r#"{"filesystems":[{"target":"/tmp/shelf-videos","source":"//nas/media","fstype":"cifs","fsroot":"/"}]}"#,
        );
        runner.push_success("");
        runner.push_failure("not mounted");

        unmount(&mut runner, Path::new("/tmp/shelf-videos")).unwrap();

        let umount_count = runner
            .calls
            .iter()
            .filter(|call| call.program == "umount")
            .count();
        assert_eq!(umount_count, 2);
    }
}
