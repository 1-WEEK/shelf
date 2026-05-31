# Shelf TUI Interaction Spec

This document defines the interaction design of the Shelf TUI. Any change to behavior, flow, or feedback must update this spec first.

## Design Principles

1. **Fail forward**: destructive actions require confirmation; everything else executes immediately
2. **State tells the truth**: the UI reflects the actual system state, not the config file
3. **One screen, one job**: no screen mixes list management with detail editing
4. **Success is silent, failure is loud**: completed operations auto-dismiss; errors stay until acknowledged
5. **q always escapes**: `q` / `Esc` navigates up one level or exits; never triggers action

## Screen Model

The TUI is a stack of screens with a single global modal layer.

```
Modal (overlay, blocks all input)
  |
Screen (exactly one visible at a time)
  |
Footer (context-sensitive key hints)
```

### Screens

| Screen | Purpose | Parent |
|--------|---------|--------|
| Home | Browse mount list, launch actions | (root) |
| MountDetail | Inspect one mount, act on it | Home |
| Sources | Browse source list | Home |
| SourceDetail | Inspect one source, set default | Sources |
| AddMount | Wizard: create a new mount | Home |
| Help | Static reference | caller |

### Navigation Rules

- `Esc` and `q` behave identically unless typing in a text field
- From any screen: go to its Parent via `Esc` / `q`
- From Home: `Esc` / `q` quits the application
- `previous_screen` is tracked only for Help (returns to caller)
- Forward navigation is always explicit (`Enter`, `a`, `s`, `?`)

## Modal System

Modals are the only overlay. They capture all keyboard input until closed.

### Modal Types

| Type | Trigger | Dismiss | After-dismiss behavior |
|------|---------|---------|----------------------|
| Confirm | User initiates privileged action | `Enter` confirms, `Esc`/`n` cancels | Confirm spawns background task; cancel returns to caller screen. Destructive actions (`RemoveMount`, `RemoveSource`) render with a red border and a "Confirm destructive action" title; routine confirmations use the iris accent. Classification lives on `ConfirmAction::is_destructive`. |
| Progress | Background task starts (non-Refresh) | Implicit: replaced by Success or Error | Never dismissed by user input |
| Success | Mutating task completes successfully | Any key, or auto-dismiss after 1s | Navigates to list screen (see Auto-Navigation) |
| Error | Validation failure or task failure | `Esc` / `Enter` | Returns to caller screen, no navigation |

### Auto-Navigation on Success

After these operations complete, the Success modal shows for 1 second, then automatically closes and navigates:

| Operation | Navigate to |
|-----------|-------------|
| AddMount | Home |
| AddSource | Sources (list view) |
| RemoveMount | Home |
| RemoveSource | Sources |

Operations that do **not** auto-navigate: Disconnect, Apply, Repair, SetDefaultSource. These close the Progress modal immediately and refresh state in place.

Reasoning: wizard completions should return the user to the list they started from. In-place operations (Disconnect, Repair) preserve context so the user can see the state change.

## Key Binding Philosophy

### Global keys (available when no modal is open and not in a text field)

| Key | Action | Rationale |
|-----|--------|-----------|
| `j` / `k` | move selection | Vim convention; works in every list |
| `Enter` | confirm / enter detail | Universal accept action |
| `q` / `Esc` | back / quit | Escape is the affordance; `q` mirrors CLI quit |
| `?` | show Help | Standard TUI help key |

### Screen-local keys

| Screen | Key | Action |
|--------|-----|--------|
| Home | `a` | AddMount wizard |
| Home | `s` | Sources screen |
| Home | `r` | Refresh status |
| Home | `p` | Apply full config |
| Home | `Enter` | MountDetail for selected mount |
| MountDetail | `x` | Disconnect selected mount |
| MountDetail | `d` | Remove selected mount from Shelf |
| MountDetail | `p` | Repair selected mount |
| Sources | `a` | AddSource flow |
| Sources | `d` | Remove selected source |
| Sources | `Enter` | SourceDetail for selected source |
| SourceDetail | `Enter` | Set selected source as default |
| SourceDetail | `d` | Remove this source |
| AddMount | `Enter` | Advance wizard step |
| AddMount | `Esc` | Go back one step (or exit from step 1) |
| AddMount | `Tab` | Cycle source selection (LoginSource step only) |

Note: `p` is context-sensitive. On Home it means "apply globally"; on MountDetail it means "repair this one mount".

## AddMount Wizard

Four linear steps. No branching. No skipping.

1. **LocalFolder** -- text input, validated via `paths::expand_local_path` on advance
2. **RemotePath** -- text input, must start with `/`
3. **LoginSource** -- list selection from configured sources
4. **Review** -- static summary, no editable fields

Advance: `Enter`. Retreat: `Esc`. Exit from step 1: `Esc` returns to Home.

The wizard state is stored in `App.add_mount`. It resets when the user enters the wizard.

## Task Feedback Flow

All privileged work runs in a background thread. The main loop polls a channel for `TaskMessage`.

```
User action
    |
    v
App method sets pending_sudo_action
    |
    v
Main loop detects pending_sudo_action -> suspends TUI -> sudo -v
    |
    v
On success: App::execute_privileged_action -> actions::spawn_*(tx, ...)
    |
    v
Background thread runs -> sends TaskMessage via channel
    |
    v
Main loop rx.try_recv() -> App::handle_task_message(msg)
```

### TaskMessage handling

- `Status`: only sent by Refresh. Updates `config`, `config_path`, `status_rows`
- `Started { kind, steps }`: sets `TaskState::Running`, opens Progress modal
- `Step { index, label }`: marks step done, updates label
- `Done { steps }`: sets `TaskState::Done`. For AddMount/AddSource/RemoveMount/RemoveSource, opens Success modal + sets auto-dismiss timer. For others, closes Progress modal and triggers Refresh.
- `Failed { steps, error }`: sets `TaskState::Failed`, opens Error modal

## Sudo Boundary

The TUI process never runs privileged code. All privileged operations delegate to `shelf-root` via `sudo`.

Flow:
1. User confirms in Confirm modal
2. `pending_sudo_action` is set
3. Main loop suspends the terminal (leaves alternate screen)
4. `sudo -v` is executed interactively
5. On success, the task thread runs `sudo --non-interactive shelf-root <subcommand>`
6. Terminal resumes

Password entry happens outside the TUI, in the normal terminal.

## Empty States

| Screen | Empty condition | Message |
|--------|-----------------|---------|
| Home (no sources) | no mounts and no sources configured | "No mounts yet." + "Press s to add a login source, then a to add a mount." |
| Home (with sources) | no mounts but at least one source | "No mounts yet." + "Press a to add your first mount." |
| Sources | no sources configured | (list renders empty; footer still shows `a` to add) |

The `s` and `a` keys in the prompt are rendered with `key_style` (gold + bold) so the next action is visually obvious.

## Health States

Rendered in Home list and MountDetail. Each state colour matches the [Rose Pine palette](https://github.com/rose-pine/rose-pine-palette/blob/main/palette.json) (main variant); see `src/tui/view.rs` for the constants.

| State | Role | Meaning |
|-------|------|---------|
| mounted | foam | CIFS and bind mount both active |
| needs attention | gold | partial mount state (mismatch) |
| not mounted | gold | neither mount present |
| broken | love | validation or runtime error |

The Home table prefixes each state label with a `●` glyph styled in the same role colour. The table row body itself stays in default text so paths remain readable on broken mounts.

## Add Mount Wizard Progress

Each wizard step renders inside the panel header as a circled number (`①` `②` `③` `④`) followed by its label, joined by `───` separators. Completed steps switch to a `✓` glyph and the foam colour, the active step is iris bold, and future steps are subtle dim. The state is computed from `WizardStep` via `wizard_step_order`.

## Constraints for Future Changes

- Do not add new screens without updating `back()` navigation
- Do not add new modal types without updating events.rs intercept logic
- Do not add new privileged operations without adding to `ConfirmAction`, `PrivilegedAction`, `TaskKind`, and `steps_for()`
- Footer text must reflect actual key bindings; keep it under one line
- Text input fields must be guarded by `should_capture_text()` to prevent shortcut misfire
