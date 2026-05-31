# shelf

Mount SMB shares on Linux without the ceremony.

Define a "source" (an SMB server + credentials) and a "mount" (a local folder pointing to a path on that server). Shelf handles the rest: `mount.cifs`, bind mounts, systemd units for reboot survival, and credential isolation. There is no config for you to write — everything lives in `~/.config/shelf/config.toml`, and your passwords never leave root-owned files in `/etc/shelf/credentials/`.

It ships with a terminal UI for browsing, adding, and repairing mounts.

## Installation

Requires Rust toolchain. Build from source:

```
cargo build --release
```

This produces two binaries:

- `shelf` — the user-facing CLI and TUI.
- `shelf-root` — a privileged helper invoked via `sudo`. It handles mounts, credential storage, and systemd unit management.

Drop both into your `PATH`.

## Quick start

Add an SMB source:

```
shelf source add nas.local --username alice
```

You will be prompted for the SMB password. This creates a source with an auto-generated ID like `nas-local-alice` (set one explicitly with `--name`).

Mount a path from that source to a local folder:

```
shelf mount ~/Videos /media/movies
```

If you only have one source, it becomes the default. Otherwise pass `--source nas-local-alice`.

Apply the config (this is the step that actually mounts):

```
shelf apply
```

Your mount now survives reboots. Check status:

```
shelf status
```

## CLI highlights

| Command | What it does |
|---|---|
| `shelf` | Launch the TUI |
| `shelf source list` | List configured servers |
| `shelf source default <id>` | Set the default login |
| `shelf mount <local> <remote>` | Wire a local folder to a server path |
| `shelf unmount <local>` | Unmount and remove from config |
| `shelf apply` | Mount everything defined in config |
| `shelf remove` | Tear down all mounts and clean up |
| `shelf run --mount ~/A:/x -- -c 'ls'` | Mount, run a command, unmount |
| `shelf session up` | Start all mounts without systemd persistence |
| `shelf session down` | Tear down temporary session mounts |

## TUI

Run `shelf` with no arguments to enter the terminal interface. Navigate with `j`/`k`, press `?` for help. The wizard walks you through adding mounts step by step — pick a local folder, a remote path, a source, and confirm.

## Architecture

Shelf keeps a hard line between user and root.

- **User config** lives in `~/.config/shelf/config.toml`. No passwords here.
- **Credentials** live in `/etc/shelf/credentials/*.cred`, owned by root with `0600` permissions.
- **Mounts** use a two-layer setup: a CIFS mount at `/mnt/shelf/sources/{id}/{share}`, then a bind mount from the specific subdirectory to your local folder. Each layer gets its own systemd `.mount` unit, ordered so the CIFS mount starts first.
- **State** is tracked in `/var/lib/shelf/state.json` so stale mounts can be cleaned up when config changes.

All privileged work — mounting, credential file writes, systemd unit management — flows through `shelf-root` via `sudo`.

## Safety checks

- Refuses to mount over a non-empty directory.
- Validates all paths for control characters and systemd-percent-specifier safety.
- Tests write access with a probe file on every apply.
- Rolls back CIFS mounts if an apply fails partway through.
- Skips re-mounting already-mounted paths.

## Development

```
mise run check
```

Runs `cargo fmt --check`, `cargo check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test`.
