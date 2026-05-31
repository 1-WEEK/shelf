use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use nix::unistd::{getgid, getuid};
use serde::{Deserialize, Serialize};

use crate::error::IoContext;
use crate::remote_path::RemotePath;
use crate::source::{generate_source_id, validate_address, validate_source_id};
use crate::{credentials, paths, Result, ShelfError};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct Config {
    pub default_source: Option<String>,
    #[serde(default)]
    pub sources: BTreeMap<String, SourceConfig>,
    #[serde(default)]
    pub mounts: Vec<MountConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceConfig {
    pub id: String,
    pub address: String,
    pub username: String,
    pub owner_uid: u32,
    pub owner_gid: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MountConfig {
    pub local_path: String,
    pub source_id: String,
    pub remote_path: String,
}

impl Config {
    pub fn load_or_default(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        Self::load(path)
    }

    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(ShelfError::ConfigNotFound(path.to_path_buf()));
        }
        let text = std::fs::read_to_string(path).with_path(path)?;
        let config: Self = toml::from_str(&text).map_err(|source| ShelfError::TomlParse {
            path: path.to_path_buf(),
            source,
        })?;
        config.validate()?;
        Ok(config)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_path(parent)?;
        }
        let text = toml::to_string_pretty(self)?;
        std::fs::write(path, text).with_path(path)
    }

    pub fn load_user() -> Result<(Self, PathBuf)> {
        let path = paths::user_config_path()?;
        let config = Self::load_or_default(&path)?;
        Ok((config, path))
    }

    pub fn validate(&self) -> Result<()> {
        if self.sources.is_empty() {
            if self.default_source.is_some() {
                return Err(ShelfError::Validation(
                    "default_source is set but no sources exist".into(),
                ));
            }
            if !self.mounts.is_empty() {
                return Err(ShelfError::Validation(
                    "mounts require at least one source".into(),
                ));
            }
            return Ok(());
        }

        let default = self
            .default_source
            .as_ref()
            .ok_or(ShelfError::MissingDefaultSource)?;
        if !self.sources.contains_key(default) {
            return Err(ShelfError::SourceNotFound(default.clone()));
        }

        for (id, source) in &self.sources {
            validate_source_id(id)?;
            if id != &source.id {
                return Err(ShelfError::Validation(format!(
                    "source key {id} does not match source id {}",
                    source.id
                )));
            }
            validate_address(&source.address)?;
            credentials::validate_credential_value("username", &source.username)?;
            if source.username.trim().is_empty() {
                return Err(ShelfError::Validation(format!(
                    "source {id} has an empty username"
                )));
            }
        }

        let mut seen_local_paths = HashSet::new();
        for mount in &self.mounts {
            if !self.sources.contains_key(&mount.source_id) {
                return Err(ShelfError::SourceNotFound(mount.source_id.clone()));
            }
            RemotePath::parse(&mount.remote_path)?;
            let local = Path::new(&mount.local_path);
            if !local.is_absolute() {
                return Err(ShelfError::Validation(format!(
                    "local path must be absolute in config: {}",
                    mount.local_path
                )));
            }
            if !seen_local_paths.insert(mount.local_path.clone()) {
                return Err(ShelfError::Validation(format!(
                    "duplicate local path in config: {}",
                    mount.local_path
                )));
            }
        }

        Ok(())
    }

    pub fn add_source(
        &mut self,
        address: String,
        username: String,
        name: Option<String>,
        make_default: bool,
    ) -> Result<String> {
        validate_address(&address)?;
        credentials::validate_credential_value("username", &username)?;
        if username.trim().is_empty() {
            return Err(ShelfError::Validation("username cannot be empty".into()));
        }

        let id = match name {
            Some(name) => {
                validate_source_id(&name)?;
                name
            }
            None => generate_source_id(&address, &username),
        };

        if self.sources.contains_key(&id) {
            return Err(ShelfError::Validation(format!(
                "source already exists: {id}"
            )));
        }

        let source = SourceConfig {
            id: id.clone(),
            address,
            username,
            owner_uid: getuid().as_raw(),
            owner_gid: getgid().as_raw(),
        };
        self.sources.insert(id.clone(), source);
        if make_default || self.default_source.is_none() {
            self.default_source = Some(id.clone());
        }
        self.validate()?;
        Ok(id)
    }

    pub fn remove_source(&mut self, id: &str) -> Result<()> {
        if self.mounts.iter().any(|mount| mount.source_id == id) {
            return Err(ShelfError::Validation(format!(
                "source {id} is still used by one or more mounts"
            )));
        }
        self.sources
            .remove(id)
            .ok_or_else(|| ShelfError::SourceNotFound(id.to_string()))?;
        if self.default_source.as_deref() == Some(id) {
            self.default_source = self.sources.keys().next().cloned();
        }
        self.validate()
    }

    pub fn set_default_source(&mut self, id: &str) -> Result<()> {
        if !self.sources.contains_key(id) {
            return Err(ShelfError::SourceNotFound(id.to_string()));
        }
        self.default_source = Some(id.to_string());
        self.validate()
    }

    pub fn resolve_source_id(&self, explicit: Option<&str>) -> Result<String> {
        match explicit {
            Some(id) if self.sources.contains_key(id) => Ok(id.to_string()),
            Some(id) => Err(ShelfError::SourceNotFound(id.to_string())),
            None => self
                .default_source
                .clone()
                .ok_or(ShelfError::MissingDefaultSource),
        }
    }

    pub fn source(&self, id: &str) -> Result<&SourceConfig> {
        self.sources
            .get(id)
            .ok_or_else(|| ShelfError::SourceNotFound(id.to_string()))
    }

    pub fn add_mount(
        &mut self,
        local_path: PathBuf,
        remote_path: String,
        source_id: String,
    ) -> Result<()> {
        let remote = RemotePath::parse(&remote_path)?;
        crate::paths::validate_local_path(&local_path)?;
        let local = local_path.to_string_lossy().to_string();
        if let Some(existing) = self
            .mounts
            .iter_mut()
            .find(|mount| mount.local_path == local)
        {
            existing.remote_path = remote.raw().to_string();
            existing.source_id = source_id;
        } else {
            self.mounts.push(MountConfig {
                local_path: local,
                source_id,
                remote_path: remote.raw().to_string(),
            });
        }
        self.validate()
    }

    pub fn remove_mount(&mut self, local_path: &Path) -> Result<MountConfig> {
        let local = local_path.to_string_lossy();
        let index = self
            .mounts
            .iter()
            .position(|mount| mount.local_path == local)
            .ok_or_else(|| ShelfError::MountNotFound(local.to_string()))?;
        let removed = self.mounts.remove(index);
        self.validate()?;
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_source_auto_becomes_default_without_flag() {
        let mut config = Config::default();
        let id = config
            .add_source("192.168.1.10".into(), "alice".into(), None, false)
            .unwrap();
        assert_eq!(id, "192.168.1.10-alice");
        assert_eq!(config.default_source.as_deref(), Some("192.168.1.10-alice"));
    }

    #[test]
    fn second_source_does_not_steal_default_without_flag() {
        let mut config = Config::default();
        config
            .add_source("192.168.1.10".into(), "alice".into(), None, false)
            .unwrap();
        config
            .add_source("192.168.1.20".into(), "bob".into(), None, false)
            .unwrap();
        assert_eq!(config.default_source.as_deref(), Some("192.168.1.10-alice"));
    }

    #[test]
    fn explicit_default_flag_overrides_existing_default() {
        let mut config = Config::default();
        config
            .add_source("192.168.1.10".into(), "alice".into(), None, false)
            .unwrap();
        config
            .add_source("192.168.1.10".into(), "bob".into(), None, true)
            .unwrap();
        assert!(config.sources.contains_key("192.168.1.10-alice"));
        assert!(config.sources.contains_key("192.168.1.10-bob"));
        assert_eq!(config.default_source.as_deref(), Some("192.168.1.10-bob"));
    }

    #[test]
    fn mount_uses_default_source_when_resolved_by_cli() {
        let mut config = Config::default();
        config
            .add_source(
                "192.168.1.10".into(),
                "alice".into(),
                Some("home".into()),
                true,
            )
            .unwrap();
        let source_id = config.resolve_source_id(None).unwrap();
        config
            .add_mount(
                PathBuf::from("/tmp/shelf-videos"),
                "/media/movies".into(),
                source_id,
            )
            .unwrap();
        assert_eq!(config.mounts[0].source_id, "home");
    }

    #[test]
    fn config_roundtrip() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        let mut config = Config::default();
        config
            .add_source(
                "nas.local".into(),
                "alice".into(),
                Some("home".into()),
                true,
            )
            .unwrap();
        config
            .add_mount(
                PathBuf::from("/tmp/shelf-videos"),
                "/media/movies".into(),
                "home".into(),
            )
            .unwrap();
        config.save(&path).unwrap();
        let loaded = Config::load(&path).unwrap();
        assert_eq!(loaded, config);
    }
}
