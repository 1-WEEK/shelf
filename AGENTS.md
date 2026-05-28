# shelf Agent Instructions

## Project Map

- `src/cli.rs`: user-facing CLI and command dispatch.
- `src/root_cli.rs` and `src/bin/shelf-root.rs`: privileged helper boundary.
- `src/apply.rs`, `src/mounts.rs`, `src/systemd.rs`: mount planning, runtime checks, and systemd integration.
- `src/config.rs`, `src/source.rs`, `src/remote_path.rs`: persisted config and user-facing source/path validation.
- `src/tui/`: Ratatui/Crossterm interactive control panel.

## Verification

Run the stable wrapper before handing off changes:

```bash
mise run check
```

`mise run check` runs:

- `cargo fmt --check`
- `cargo check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test`

## Boundaries

- Keep privileged system changes behind `shelf-root`.
- Preserve existing command-line behavior when changing TUI code.
- Do not expose SMB share splitting as user-facing product language.
- Use "source", "default source", "remote path", and "mount" for user-facing concepts.
- `Disconnect` unmounts but keeps Shelf config.
- `Remove from Shelf` removes Shelf config/systemd units and must not delete remote files.
- TUI code should stay an interaction layer over existing config/apply/status/root helpers.

## Safety

- Do not commit `.claude/settings.local.json` or other local agent/runtime settings.
- Do not store SMB passwords in project files; credentials must flow through the existing `shelf-root` credential boundary.
- Avoid destructive git or filesystem cleanup unless the user explicitly asks for it.
