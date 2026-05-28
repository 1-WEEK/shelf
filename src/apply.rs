use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::command::CommandRunner;
use crate::config::{Config, MountConfig, SourceConfig};
use crate::credentials;
use crate::error::IoContext;
use crate::remote_path::RemotePath;
use crate::{mounts, paths, progress, systemd, Result, ShelfError};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct RootState {
    #[serde(default)]
    pub persistent_mounts: Vec<RuntimeMount>,
    #[serde(default)]
    pub temporary_mounts: Vec<RuntimeMount>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct RuntimeMount {
    pub local_path: String,
    pub source_id: String,
    pub top_level: String,
    #[serde(default)]
    pub cifs_source: String,
    pub cifs_mount_root: String,
    pub bind_source: String,
    pub cifs_unit: Option<String>,
    pub bind_unit: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SessionMountRequest {
    pub source_id: String,
    pub address: String,
    pub owner_uid: u32,
    pub owner_gid: u32,
    pub local_path: PathBuf,
    pub remote_path: String,
}

#[derive(Debug, Clone)]
struct DesiredMount {
    source: SourceConfig,
    mount: MountConfig,
    remote: RemotePath,
    mount_root: PathBuf,
    bind_source: PathBuf,
    cifs_unit: String,
    bind_unit: String,
}

impl DesiredMount {
    fn runtime(&self) -> RuntimeMount {
        RuntimeMount {
            local_path: self.mount.local_path.clone(),
            source_id: self.mount.source_id.clone(),
            top_level: self.remote.top_level().to_string(),
            cifs_source: mounts::expected_cifs_source(
                &self.source.address,
                self.remote.top_level(),
            ),
            cifs_mount_root: self.mount_root.to_string_lossy().to_string(),
            bind_source: self.bind_source.to_string_lossy().to_string(),
            cifs_unit: Some(self.cifs_unit.clone()),
            bind_unit: Some(self.bind_unit.clone()),
        }
    }

    fn cifs_source(&self) -> String {
        mounts::expected_cifs_source(&self.source.address, self.remote.top_level())
    }

    fn fs_root(&self) -> String {
        mounts::expected_fs_root(self.remote.remainder())
    }
}

impl RootState {
    pub fn load() -> Result<Self> {
        let path = paths::state_file();
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path).with_path(&path)?;
        serde_json::from_str(&text).map_err(|source| {
            ShelfError::Validation(format!(
                "failed to parse root state {}: {source}",
                path.display()
            ))
        })
    }

    pub fn save(&self) -> Result<()> {
        let path = paths::state_file();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_path(parent)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|source| {
            ShelfError::Validation(format!("failed to encode root state: {source}"))
        })?;
        std::fs::write(&path, text).with_path(path)
    }
}

pub fn apply_config(
    config: &Config,
    owner_uid: u32,
    owner_gid: u32,
    runner: &mut impl CommandRunner,
) -> Result<()> {
    progress::step("checking tools, config, and existing mounts");
    config.validate()?;
    mounts::ensure_dependencies(runner)?;
    let desired = build_desired(config, runner)?;
    let mut state = RootState::load()?;

    preflight_desired(&state, &desired, runner)?;

    progress::step("mounting SMB sources");
    let newly_mounted = ensure_desired_cifs(&desired, owner_uid, owner_gid, runner)?;

    progress::step("checking remote directories and write access");
    if let Err(err) = preflight_remote_directories(&desired) {
        rollback_new_cifs(&newly_mounted, runner);
        return Err(err);
    }
    if let Err(err) = ensure_local_directories(&desired) {
        rollback_new_cifs(&newly_mounted, runner);
        return Err(err);
    }

    progress::step("removing stale mounts");
    cleanup_stale_persistent(&mut state, &desired, runner)?;

    progress::step("writing systemd mount units");
    let mut written_cifs = HashSet::new();
    for mount in &desired {
        std::fs::create_dir_all(&mount.mount_root).with_path(&mount.mount_root)?;
        if written_cifs.insert((
            mount.source.id.clone(),
            mount.remote.top_level().to_string(),
        )) {
            let unit = systemd::UnitFile {
                name: mount.cifs_unit.clone(),
                path: systemd::unit_path(&mount.cifs_unit),
                content: systemd::cifs_unit_content(
                    &mount.source.address,
                    mount.remote.top_level(),
                    &mount.mount_root,
                    &credentials::credential_path(&mount.source.id)?,
                    owner_uid,
                    owner_gid,
                ),
            };
            systemd::write_unit(&unit)?;
        }
    }

    systemd::daemon_reload(runner)?;

    progress::step("starting SMB mount units");
    let mut started_cifs = HashSet::new();
    for mount in &desired {
        if started_cifs.insert((
            mount.source.id.clone(),
            mount.remote.top_level().to_string(),
        )) {
            systemd::enable_now(runner, &mount.cifs_unit)?;
            mounts::ensure_cifs_mounted(
                runner,
                &mount.source.address,
                mount.remote.top_level(),
                &mount.mount_root,
                &credentials::credential_path(&mount.source.id)?,
                owner_uid,
                owner_gid,
            )?;
        }
    }

    progress::step("writing local bind mount units");
    for mount in &desired {
        let local_path = PathBuf::from(&mount.mount.local_path);

        let unit = systemd::UnitFile {
            name: mount.bind_unit.clone(),
            path: systemd::unit_path(&mount.bind_unit),
            content: systemd::bind_unit_content(&mount.bind_source, &local_path, &mount.cifs_unit),
        };
        systemd::write_unit(&unit)?;
    }

    systemd::daemon_reload(runner)?;

    progress::step("starting local bind mounts");
    for mount in &desired {
        systemd::enable_now(runner, &mount.bind_unit)?;
        mounts::ensure_bind_mounted(
            runner,
            &mount.bind_source,
            &PathBuf::from(&mount.mount.local_path),
            &mount.cifs_source(),
            &mount.fs_root(),
        )?;
    }

    state.persistent_mounts = desired.iter().map(DesiredMount::runtime).collect();
    state.save()?;
    progress::ok("system state matches shelf config");
    Ok(())
}

pub fn remove_all(runner: &mut impl CommandRunner) -> Result<()> {
    progress::step("removing all shelf-managed mounts");
    let mut state = RootState::load()?;
    for mount in state
        .persistent_mounts
        .iter()
        .chain(state.temporary_mounts.iter())
        .collect::<Vec<_>>()
    {
        if let Some(unit) = &mount.bind_unit {
            systemd::disable_now_best_effort(runner, unit);
            remove_unit_file(unit)?;
        }
        mounts::unmount(runner, Path::new(&mount.local_path))?;
    }

    let mut cifs_roots = HashSet::new();
    for mount in state
        .persistent_mounts
        .iter()
        .chain(state.temporary_mounts.iter())
    {
        cifs_roots.insert((mount.cifs_mount_root.clone(), mount.cifs_unit.clone()));
    }
    for (root, unit) in cifs_roots {
        if let Some(unit) = unit {
            systemd::disable_now_best_effort(runner, &unit);
            remove_unit_file(&unit)?;
        }
        mounts::unmount(runner, Path::new(&root))?;
    }

    state.persistent_mounts.clear();
    state.temporary_mounts.clear();
    state.save()?;
    progress::ok("removed all shelf-managed mounts");
    Ok(())
}

pub fn unmount_persistent_target(local_path: &Path, runner: &mut impl CommandRunner) -> Result<()> {
    progress::step(format!("unmounting {}", local_path.display()));
    let mut state = RootState::load()?;
    let local = local_path.to_string_lossy().to_string();
    let mut removed = Vec::new();
    state.persistent_mounts.retain(|mount| {
        if mount.local_path == local {
            removed.push(mount.clone());
            false
        } else {
            true
        }
    });

    if removed.is_empty() {
        mounts::unmount(runner, local_path)?;
        state.save()?;
        progress::ok(format!("unmounted {}", local_path.display()));
        return Ok(());
    }

    for mount in &removed {
        if let Some(unit) = &mount.bind_unit {
            systemd::disable_now_best_effort(runner, unit);
            remove_unit_file(unit)?;
        }
        mounts::unmount(runner, Path::new(&mount.local_path))?;
    }

    for mount in removed {
        if !runtime_uses_cifs(&state, &mount.cifs_mount_root) {
            if let Some(unit) = &mount.cifs_unit {
                systemd::disable_now_best_effort(runner, unit);
                remove_unit_file(unit)?;
            }
            mounts::unmount(runner, Path::new(&mount.cifs_mount_root))?;
        }
    }

    systemd::daemon_reload(runner)?;
    state.save()?;
    progress::ok(format!("unmounted {}", local_path.display()));
    Ok(())
}

pub fn session_up(request: SessionMountRequest, runner: &mut impl CommandRunner) -> Result<()> {
    progress::step(format!(
        "starting temporary mount {} -> {}:{}",
        request.local_path.display(),
        request.source_id,
        request.remote_path
    ));
    mounts::ensure_dependencies(runner)?;
    let remote = RemotePath::parse(&request.remote_path)?;
    let mount_root = paths::mount_root_for(&request.source_id, remote.top_level());
    let bind_source = remote.bind_source_under(&mount_root);
    let credential_file = credentials::credential_path(&request.source_id)?;

    std::fs::create_dir_all(&mount_root).with_path(&mount_root)?;
    mounts::ensure_cifs_mounted(
        runner,
        &request.address,
        remote.top_level(),
        &mount_root,
        &credential_file,
        request.owner_uid,
        request.owner_gid,
    )?;
    ensure_remote_directory(&bind_source)?;
    ensure_remote_writable(&bind_source)?;
    let is_already_mounted = mounts::find_mountpoint(runner, &request.local_path)?.is_some();
    paths::ensure_empty_or_mountable(&request.local_path, is_already_mounted)?;
    std::fs::create_dir_all(&request.local_path).with_path(&request.local_path)?;
    mounts::ensure_bind_mounted(
        runner,
        &bind_source,
        &request.local_path,
        &mounts::expected_cifs_source(&request.address, remote.top_level()),
        &mounts::expected_fs_root(remote.remainder()),
    )?;

    let mut state = RootState::load()?;
    let runtime = RuntimeMount {
        local_path: request.local_path.to_string_lossy().to_string(),
        source_id: request.source_id,
        top_level: remote.top_level().to_string(),
        cifs_source: mounts::expected_cifs_source(&request.address, remote.top_level()),
        cifs_mount_root: mount_root.to_string_lossy().to_string(),
        bind_source: bind_source.to_string_lossy().to_string(),
        cifs_unit: None,
        bind_unit: None,
    };
    if !state.temporary_mounts.contains(&runtime) {
        state.temporary_mounts.push(runtime);
    }
    state.save()?;
    progress::ok("temporary mount ready");
    Ok(())
}

pub fn session_down(local_path: Option<&Path>, runner: &mut impl CommandRunner) -> Result<()> {
    progress::step("tearing down temporary mounts");
    let mut state = RootState::load()?;
    let filter = local_path.map(|path| path.to_string_lossy().to_string());
    let mut removed = Vec::new();
    state.temporary_mounts.retain(|mount| {
        let should_remove = filter
            .as_ref()
            .is_none_or(|local| mount.local_path == *local);
        if should_remove {
            removed.push(mount.clone());
            false
        } else {
            true
        }
    });

    for mount in &removed {
        mounts::unmount(runner, Path::new(&mount.local_path))?;
    }
    for mount in removed {
        if !runtime_uses_cifs(&state, &mount.cifs_mount_root) {
            mounts::unmount(runner, Path::new(&mount.cifs_mount_root))?;
        }
    }
    state.save()?;
    progress::ok("temporary mounts removed");
    Ok(())
}

fn build_desired(config: &Config, runner: &mut impl CommandRunner) -> Result<Vec<DesiredMount>> {
    let mut desired = Vec::new();
    for mount in &config.mounts {
        let source = config.source(&mount.source_id)?.clone();
        let remote = RemotePath::parse(&mount.remote_path)?;
        let mount_root = paths::mount_root_for(&source.id, remote.top_level());
        let bind_source = remote.bind_source_under(&mount_root);
        let cifs_unit = systemd::mount_unit_name_for(runner, &mount_root)?;
        let bind_unit = systemd::mount_unit_name_for(runner, Path::new(&mount.local_path))?;
        desired.push(DesiredMount {
            source,
            mount: mount.clone(),
            remote,
            mount_root,
            bind_source,
            cifs_unit,
            bind_unit,
        });
    }
    Ok(desired)
}

fn cleanup_stale_persistent(
    state: &mut RootState,
    desired: &[DesiredMount],
    runner: &mut impl CommandRunner,
) -> Result<()> {
    let desired_runtimes: HashSet<RuntimeMount> =
        desired.iter().map(DesiredMount::runtime).collect();
    let desired_cifs: HashSet<String> = desired
        .iter()
        .map(|mount| mount.mount_root.to_string_lossy().to_string())
        .collect();

    let mut stale = Vec::new();
    state.persistent_mounts.retain(|mount| {
        if desired_runtimes.contains(mount) {
            true
        } else {
            stale.push(mount.clone());
            false
        }
    });

    for mount in &stale {
        if let Some(unit) = &mount.bind_unit {
            systemd::disable_now_best_effort(runner, unit);
            remove_unit_file(unit)?;
        }
        mounts::unmount(runner, Path::new(&mount.local_path))?;
    }

    for mount in stale {
        if !desired_cifs.contains(&mount.cifs_mount_root)
            && !runtime_uses_cifs(state, &mount.cifs_mount_root)
        {
            if let Some(unit) = &mount.cifs_unit {
                systemd::disable_now_best_effort(runner, unit);
                remove_unit_file(unit)?;
            }
            mounts::unmount(runner, Path::new(&mount.cifs_mount_root))?;
        }
    }

    Ok(())
}

fn runtime_uses_cifs(state: &RootState, cifs_mount_root: &str) -> bool {
    state
        .persistent_mounts
        .iter()
        .chain(state.temporary_mounts.iter())
        .any(|mount| mount.cifs_mount_root == cifs_mount_root)
}

fn ensure_remote_directory(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path).with_path(path)
}

fn ensure_remote_writable(path: &Path) -> Result<()> {
    let probe = path.join(format!(".shelf-write-test-{}", std::process::id()));
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .with_path(&probe)?;
    std::fs::remove_file(&probe).with_path(probe)
}

fn remove_unit_file(unit: &str) -> Result<()> {
    let path = systemd::unit_path(unit);
    if path.exists() {
        std::fs::remove_file(&path).with_path(path)?;
    }
    Ok(())
}

fn preflight_desired(
    state: &RootState,
    desired: &[DesiredMount],
    runner: &mut impl CommandRunner,
) -> Result<()> {
    let mut seen_credentials = HashSet::new();
    for mount in desired {
        paths::validate_systemd_path_text(&mount.mount_root)?;
        paths::validate_systemd_path_text(&mount.bind_source)?;

        if seen_credentials.insert(mount.source.id.clone()) {
            let credential_path = credentials::credential_path(&mount.source.id)?;
            if !credential_path.exists() {
                return Err(ShelfError::Validation(format!(
                    "credential file is missing for source {}: {}",
                    mount.source.id,
                    credential_path.display()
                )));
            }
        }

        preflight_local_target(state, mount, runner)?;
    }
    Ok(())
}

fn preflight_local_target(
    state: &RootState,
    desired: &DesiredMount,
    runner: &mut impl CommandRunner,
) -> Result<()> {
    let local_path = PathBuf::from(&desired.mount.local_path);
    paths::validate_local_path(&local_path)?;
    if let Some(info) = mounts::find_mountpoint(runner, &local_path)? {
        if mounts::is_expected_bind_mount(
            &info,
            &desired.bind_source,
            &desired.cifs_source(),
            &desired.fs_root(),
        ) {
            return Ok(());
        }
        let recorded_stale = state.persistent_mounts.iter().any(|mount| {
            mount.local_path == desired.mount.local_path
                && (mount.bind_source == info.source
                    || (mount.cifs_source == info.source
                        && mount.top_level == desired.remote.top_level()))
        });
        if recorded_stale {
            return Ok(());
        }
        return Err(ShelfError::Validation(format!(
            "local path is already mounted from an unmanaged source: {} -> {}",
            local_path.display(),
            info.source
        )));
    }
    paths::ensure_empty_or_mountable(&local_path, false)
}

fn ensure_desired_cifs(
    desired: &[DesiredMount],
    owner_uid: u32,
    owner_gid: u32,
    runner: &mut impl CommandRunner,
) -> Result<Vec<PathBuf>> {
    let mut handled = HashSet::new();
    let mut newly_mounted = Vec::new();
    for mount in desired {
        if !handled.insert((
            mount.source.id.clone(),
            mount.remote.top_level().to_string(),
        )) {
            continue;
        }
        std::fs::create_dir_all(&mount.mount_root).with_path(&mount.mount_root)?;
        let outcome = mounts::ensure_cifs_mounted(
            runner,
            &mount.source.address,
            mount.remote.top_level(),
            &mount.mount_root,
            &credentials::credential_path(&mount.source.id)?,
            owner_uid,
            owner_gid,
        )?;
        if outcome == mounts::CifsMountOutcome::Mounted {
            newly_mounted.push(mount.mount_root.clone());
        }
    }
    Ok(newly_mounted)
}

fn preflight_remote_directories(desired: &[DesiredMount]) -> Result<()> {
    for mount in desired {
        ensure_remote_directory(&mount.bind_source)?;
        ensure_remote_writable(&mount.bind_source)?;
    }
    Ok(())
}

fn ensure_local_directories(desired: &[DesiredMount]) -> Result<()> {
    for mount in desired {
        let local_path = PathBuf::from(&mount.mount.local_path);
        std::fs::create_dir_all(&local_path).with_path(&local_path)?;
    }
    Ok(())
}

fn rollback_new_cifs(newly_mounted: &[PathBuf], runner: &mut impl CommandRunner) {
    for mount_root in newly_mounted.iter().rev() {
        let _ = mounts::unmount(runner, mount_root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::tests::MockRunner;

    #[test]
    fn build_desired_splits_remote_path_internally() {
        let mut config = Config::default();
        config
            .add_source(
                "192.168.1.10".into(),
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

        let mut runner = MockRunner::default();
        runner.push_success("mnt-shelf-sources-home-media.mount\n");
        runner.push_success("tmp-shelf\\x2dvideos.mount\n");

        let desired = build_desired(&config, &mut runner).unwrap();
        assert_eq!(desired[0].remote.top_level(), "media");
        assert_eq!(
            desired[0].bind_source,
            PathBuf::from("/mnt/shelf/sources/home/media/movies")
        );
        assert_eq!(desired[0].cifs_unit, "mnt-shelf-sources-home-media.mount");
    }

    #[test]
    fn stale_cleanup_uses_full_runtime_identity_not_only_local_path() {
        let source = SourceConfig {
            id: "home".into(),
            address: "nas".into(),
            username: "alice".into(),
            owner_uid: 1000,
            owner_gid: 1000,
        };
        let desired = DesiredMount {
            source,
            mount: MountConfig {
                local_path: "/tmp/shelf-videos".into(),
                source_id: "home".into(),
                remote_path: "/media/new".into(),
            },
            remote: RemotePath::parse("/media/new").unwrap(),
            mount_root: PathBuf::from("/mnt/shelf/sources/home/media"),
            bind_source: PathBuf::from("/mnt/shelf/sources/home/media/new"),
            cifs_unit: "mnt-shelf-sources-home-media.mount".into(),
            bind_unit: "tmp-shelf-videos.mount".into(),
        };
        let mut state = RootState {
            persistent_mounts: vec![RuntimeMount {
                local_path: "/tmp/shelf-videos".into(),
                source_id: "home".into(),
                top_level: "media".into(),
                cifs_source: "//nas/media".into(),
                cifs_mount_root: "/mnt/shelf/sources/home/media".into(),
                bind_source: "/mnt/shelf/sources/home/media/old".into(),
                cifs_unit: Some("mnt-shelf-sources-home-media.mount".into()),
                bind_unit: Some("tmp-shelf-videos.mount".into()),
            }],
            temporary_mounts: Vec::new(),
        };
        let mut runner = MockRunner::default();
        runner.push_success("");
        runner.push_failure("not mounted");

        cleanup_stale_persistent(&mut state, &[desired], &mut runner).unwrap();

        assert!(state.persistent_mounts.is_empty());
    }
}
