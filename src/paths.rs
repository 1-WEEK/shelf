use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use crate::error::IoContext;
use crate::{Result, ShelfError};

pub fn user_config_path() -> Result<PathBuf> {
    if let Some(config_home) = std::env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(config_home).join("shelf").join("config.toml"));
    }
    let home =
        std::env::var_os("HOME").ok_or_else(|| ShelfError::Validation("HOME is not set".into()))?;
    Ok(PathBuf::from(home)
        .join(".config")
        .join("shelf")
        .join("config.toml"))
}

pub fn expand_local_path(input: &str) -> Result<PathBuf> {
    let expanded = if input == "~" {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| ShelfError::Validation("HOME is not set".into()))?
    } else if let Some(rest) = input.strip_prefix("~/") {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| ShelfError::Validation("HOME is not set".into()))?;
        home.join(rest)
    } else {
        PathBuf::from(input)
    };

    if expanded.is_absolute() {
        Ok(expanded)
    } else {
        Ok(std::env::current_dir().with_path(".")?.join(expanded))
    }
}

pub fn validate_local_path(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        return Err(ShelfError::Validation(format!(
            "local path must resolve to an absolute path: {}",
            path.display()
        )));
    }
    validate_systemd_path_text(path)?;
    if path.exists() && !path.is_dir() {
        return Err(ShelfError::Validation(format!(
            "local path exists but is not a directory: {}",
            path.display()
        )));
    }
    Ok(())
}

pub fn validate_systemd_path_text(path: &Path) -> Result<()> {
    let bytes = path.as_os_str().as_bytes();
    if bytes
        .iter()
        .any(|byte| matches!(*byte, b'\n' | b'\r' | 0) || *byte < 0x20)
    {
        return Err(ShelfError::Validation(format!(
            "path cannot contain control characters: {}",
            path.display()
        )));
    }
    if bytes.contains(&b'%') {
        return Err(ShelfError::Validation(format!(
            "path cannot contain '%' because systemd expands percent specifiers: {}",
            path.display()
        )));
    }
    Ok(())
}

pub fn ensure_empty_or_mountable(path: &Path, is_already_mounted: bool) -> Result<()> {
    validate_local_path(path)?;
    if !path.exists() {
        return Ok(());
    }
    if is_already_mounted {
        return Ok(());
    }
    let mut entries = std::fs::read_dir(path).with_path(path)?;
    if entries.next().transpose().with_path(path)?.is_some() {
        return Err(ShelfError::Validation(format!(
            "local path is not empty; refusing to hide existing data behind a mount: {}",
            path.display()
        )));
    }
    Ok(())
}

pub fn mount_root_for(source_id: &str, top_level: &str) -> PathBuf {
    PathBuf::from("/mnt/shelf/sources")
        .join(source_id)
        .join(top_level)
}

pub fn credential_file_for(source_id: &str) -> PathBuf {
    PathBuf::from("/etc/shelf/credentials").join(format!("{source_id}.cred"))
}

pub fn state_file() -> PathBuf {
    PathBuf::from("/var/lib/shelf/state.json")
}

pub fn systemd_unit_dir() -> PathBuf {
    PathBuf::from("/etc/systemd/system")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_stable_system_paths() {
        assert_eq!(
            mount_root_for("home", "media"),
            PathBuf::from("/mnt/shelf/sources/home/media")
        );
        assert_eq!(
            credential_file_for("home"),
            PathBuf::from("/etc/shelf/credentials/home.cred")
        );
    }

    #[test]
    fn rejects_systemd_unsafe_local_paths() {
        assert!(validate_local_path(Path::new("/tmp/shelf-ok")).is_ok());
        assert!(validate_local_path(Path::new("/tmp/shelf\nbad")).is_err());
        assert!(validate_local_path(Path::new("/tmp/shelf-%n-bad")).is_err());
    }
}
