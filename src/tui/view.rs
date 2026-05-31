use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Cell, Clear, List, ListItem, Paragraph, Row, Table, TableState,
    Wrap,
};
use ratatui::Frame;

use super::actions::MountHealth;
use super::app::{App, Modal, Screen, SourceMode, TaskState, WizardStep};

// ── Rose Pine palette (main variant) ──────────────────────────────────
// Source: https://github.com/rose-pine/rose-pine-palette/blob/main/palette.json

const RP_BASE: Color = Color::Rgb(0x19, 0x17, 0x24);
const RP_MUTED: Color = Color::Rgb(0x6e, 0x6a, 0x86);
const RP_SUBTLE: Color = Color::Rgb(0x90, 0x8c, 0xaa);
const RP_LOVE: Color = Color::Rgb(0xeb, 0x6f, 0x92);
const RP_GOLD: Color = Color::Rgb(0xf6, 0xc1, 0x77);
const RP_FOAM: Color = Color::Rgb(0x9c, 0xcf, 0xd8);
const RP_IRIS: Color = Color::Rgb(0xc4, 0xa7, 0xe7);

// ── Style helpers ──────────────────────────────────────────────────────

fn border_style() -> Style {
    Style::default().fg(RP_MUTED)
}

fn title_style() -> Style {
    Style::default().fg(RP_IRIS).add_modifier(Modifier::BOLD)
}

fn bordered_block<'a>(title: &'a str) -> Block<'a> {
    Block::default()
        .title(title)
        .title_style(title_style())
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style())
}

fn modal_block<'a>(title: Span<'a>, accent: Color) -> Block<'a> {
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(accent))
}

fn dim_style() -> Style {
    Style::default().fg(RP_SUBTLE)
}

fn key_style() -> Style {
    Style::default().fg(RP_GOLD).add_modifier(Modifier::BOLD)
}

fn action_hint<'a>(key: &'a str, body: &'a str) -> Line<'a> {
    Line::from(vec![
        Span::styled(key, key_style()),
        Span::raw("  "),
        Span::raw(body),
    ])
}

fn wizard_step_order(step: WizardStep) -> usize {
    match step {
        WizardStep::LocalFolder => 0,
        WizardStep::RemotePath => 1,
        WizardStep::LoginSource => 2,
        WizardStep::Review => 3,
    }
}

fn step_glyph(index: usize, done: bool) -> &'static str {
    if done {
        return "✓";
    }
    match index {
        0 => "①",
        1 => "②",
        2 => "③",
        _ => "④",
    }
}

fn source_list_item(source: &crate::config::SourceConfig, is_default: bool) -> ListItem<'static> {
    let mut spans = vec![
        Span::raw(source.id.clone()),
        Span::styled(
            format!("  {}@{}", source.username, source.address),
            dim_style(),
        ),
    ];
    if is_default {
        spans.push(Span::styled("  [default]", key_style()));
    }
    ListItem::new(Line::from(spans))
}

// ── Render entry ───────────────────────────────────────────────────────

pub fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(4),
            Constraint::Length(2),
        ])
        .split(area);

    render_header(frame, chunks[0], app);
    match app.screen {
        Screen::Home => render_home(frame, chunks[1], app),
        Screen::AddMount => render_add_mount(frame, chunks[1], app),
        Screen::MountDetail => render_mount_detail(frame, chunks[1], app),
        Screen::Sources | Screen::SourceDetail => render_sources(frame, chunks[1], app),
        Screen::Help => render_help(frame, chunks[1]),
    }
    render_footer(frame, chunks[2], app);

    if let Some(modal) = &app.modal {
        render_modal(frame, area, app, modal);
    }
}

fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let default_source = app.config.default_source.as_deref().unwrap_or("none");
    let wordmark = Style::default().fg(RP_IRIS).add_modifier(Modifier::BOLD);
    let dim = dim_style();
    let lines = vec![
        Line::from(vec![
            Span::styled(" ╭─╮ ╷ ╷ ╭─╮ ╷   ╭─╮     ", wordmark),
            Span::styled("remote storage control panel", dim),
        ]),
        Line::from(vec![
            Span::styled(" ╰─╮ ├─┤ ├─╴ │   ├─╴     ", wordmark),
            Span::styled(format!("default: {default_source}"), dim),
        ]),
        Line::from(vec![
            Span::styled(" ╰─╯ ╵ ╵ ╰─╯ ╰─╴ ╵       ", wordmark),
            Span::styled(format!("v{}", env!("CARGO_PKG_VERSION")), dim),
        ]),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::BOTTOM)
                    .border_style(border_style()),
            )
            .alignment(Alignment::Left),
        area,
    );
}

// ── Home ───────────────────────────────────────────────────────────────

fn render_home(frame: &mut Frame, area: Rect, app: &App) {
    if app.status_rows.is_empty() {
        let needs_source = app.config.sources.is_empty();
        let prompt = if needs_source {
            Line::from(vec![
                Span::raw("Press "),
                Span::styled("s", key_style()),
                Span::raw(" to add a login source, then "),
                Span::styled("a", key_style()),
                Span::raw(" to add a mount."),
            ])
        } else {
            Line::from(vec![
                Span::raw("Press "),
                Span::styled("a", key_style()),
                Span::raw(" to add your first mount."),
            ])
        };
        let text = vec![
            Line::from(""),
            Line::from(Span::styled("No mounts yet.", dim_style())),
            Line::from(""),
            prompt,
        ];
        frame.render_widget(
            Paragraph::new(text)
                .block(bordered_block("Home"))
                .wrap(Wrap { trim: true })
                .alignment(Alignment::Center),
            area,
        );
        return;
    }

    let rows = app.status_rows.iter().map(|row| {
        let (label, color) = health_label(row.health);
        Row::new(vec![
            Cell::from(row.mount.local_path.clone()),
            Cell::from(format!("{}:{}", row.mount.source_id, row.mount.remote_path)),
            Cell::from(format!("● {label}")).style(Style::default().fg(color)),
        ])
    });
    let mut state = TableState::default().with_selected(Some(app.selected_mount));
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(42),
            Constraint::Percentage(42),
            Constraint::Length(18),
        ],
    )
    .header(
        Row::new(vec!["Local folder", "Remote path", "State"])
            .style(Style::default().fg(RP_SUBTLE).add_modifier(Modifier::BOLD)),
    )
    .block(bordered_block("Home"))
    .row_highlight_style(selection_style());
    frame.render_stateful_widget(table, area, &mut state);
}

// ── Add Mount Wizard ───────────────────────────────────────────────────

fn render_add_mount(frame: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(7), Constraint::Min(5)])
        .split(area);
    let steps = [
        ("Local folder", WizardStep::LocalFolder),
        ("Remote path", WizardStep::RemotePath),
        ("Login source", WizardStep::LoginSource),
        ("Review", WizardStep::Review),
    ];
    let current = wizard_step_order(app.add_mount.step);
    let mut step_line: Vec<Span> = Vec::new();
    for (i, (label, _step)) in steps.iter().enumerate() {
        if i > 0 {
            step_line.push(Span::styled("───", dim_style()));
        }
        let (glyph_style, label_style) = match i.cmp(&current) {
            std::cmp::Ordering::Less => {
                (Style::default().fg(RP_FOAM), Style::default().fg(RP_FOAM))
            }
            std::cmp::Ordering::Equal => (
                Style::default().fg(RP_IRIS).add_modifier(Modifier::BOLD),
                Style::default().fg(RP_IRIS).add_modifier(Modifier::BOLD),
            ),
            std::cmp::Ordering::Greater => (dim_style(), dim_style()),
        };
        let glyph = step_glyph(i, i < current);
        step_line.push(Span::styled(format!(" {glyph} "), glyph_style));
        step_line.push(Span::styled(format!("{label} "), label_style));
    }

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(step_line),
            Line::from(""),
            Line::from(format!("Local folder: {}", app.add_mount.local_folder)),
            Line::from(format!("Remote path:  {}", app.add_mount.remote_path)),
        ])
        .block(bordered_block("Add Mount Wizard")),
        chunks[0],
    );

    match app.add_mount.step {
        WizardStep::LocalFolder => render_text_prompt(
            frame,
            chunks[1],
            "Local folder",
            "Type an absolute or ~/ local folder path, then press Enter.",
        ),
        WizardStep::RemotePath => render_text_prompt(
            frame,
            chunks[1],
            "Remote path",
            "Type a remote path such as /media/movies, then press Enter.",
        ),
        WizardStep::LoginSource => render_source_picker(frame, chunks[1], app),
        WizardStep::Review => render_review(frame, chunks[1], app),
    }
}

fn render_text_prompt(frame: &mut Frame, area: Rect, title: &str, body: &str) {
    frame.render_widget(
        Paragraph::new(body)
            .block(bordered_block(title))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_source_picker(frame: &mut Frame, area: Rect, app: &App) {
    let items = app
        .config
        .sources
        .values()
        .map(|source| {
            let is_default = app.config.default_source.as_deref() == Some(&source.id);
            source_list_item(source, is_default)
        })
        .collect::<Vec<_>>();
    let mut state = ratatui::widgets::ListState::default().with_selected(Some(app.selected_source));
    let list = List::new(items)
        .block(bordered_block("Login Source"))
        .highlight_style(selection_style());
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_review(frame: &mut Frame, area: Rect, app: &App) {
    let source = app
        .config
        .sources
        .values()
        .nth(app.add_mount.source_index)
        .map(|source| source.id.as_str())
        .unwrap_or("default");
    let check_dim = Style::default().fg(RP_SUBTLE);
    let text = vec![
        Line::from(format!("Local folder: {}", app.add_mount.local_folder)),
        Line::from(format!("Remote path:  {}", app.add_mount.remote_path)),
        Line::from(format!("Login source: {source}")),
        Line::from(""),
        Line::from("Shelf will:"),
        Line::from(vec![
            Span::styled("  [ ] ", check_dim),
            Span::from("check login source"),
        ]),
        Line::from(vec![
            Span::styled("  [ ] ", check_dim),
            Span::from("mount remote path"),
        ]),
        Line::from(vec![
            Span::styled("  [ ] ", check_dim),
            Span::from("create local folder if missing"),
        ]),
        Line::from(vec![
            Span::styled("  [ ] ", check_dim),
            Span::from("bind local folder"),
        ]),
        Line::from(vec![
            Span::styled("  [ ] ", check_dim),
            Span::from("enable startup restore"),
        ]),
        Line::from(vec![
            Span::styled("  [ ] ", check_dim),
            Span::from("test write access"),
        ]),
    ];
    frame.render_widget(
        Paragraph::new(text)
            .block(bordered_block("Review"))
            .wrap(Wrap { trim: true }),
        area,
    );
}

// ── Mount Detail ───────────────────────────────────────────────────────

fn render_mount_detail(frame: &mut Frame, area: Rect, app: &App) {
    let Some(row) = app.status_rows.get(app.selected_mount) else {
        render_home(frame, area, app);
        return;
    };
    let (label, color) = health_label(row.health);
    let text = vec![
        Line::from(format!("Local:   {}", row.mount.local_path)),
        Line::from(format!(
            "Remote:  {}:{}",
            row.mount.source_id, row.mount.remote_path
        )),
        Line::from(format!("Source:  {}", row.mount.source_id)),
        Line::from(vec![
            Span::raw("State:   "),
            Span::styled(format!("● {label}"), Style::default().fg(color)),
        ]),
        Line::from("Startup: enabled by Shelf apply"),
        Line::from("Writable: tested during apply and repair"),
        Line::from(""),
        action_hint("x", "disconnect (unmount, keep Shelf config)"),
        action_hint("d", "remove from Shelf (clean units, keep remote files)"),
        action_hint("p", "repair this mount"),
    ];
    frame.render_widget(
        Paragraph::new(text)
            .block(bordered_block("Mount Detail"))
            .wrap(Wrap { trim: true }),
        area,
    );
}

// ── Sources ────────────────────────────────────────────────────────────

fn render_sources(frame: &mut Frame, area: Rect, app: &App) {
    if app.screen == Screen::SourceDetail {
        render_source_detail(frame, area, app);
        return;
    }

    if let SourceMode::Add { field, form } = &app.source_mode {
        let password = if form.password.is_empty() {
            String::new()
        } else {
            "*".repeat(form.password.chars().count())
        };
        let labels = ["Address", "Username", "Source id", "Password"];
        let text = vec![
            Line::from(format!(
                "{}: {}",
                field_marker(*field, 0, labels[0]),
                form.address
            )),
            Line::from(format!(
                "{}: {}",
                field_marker(*field, 1, labels[1]),
                form.username
            )),
            Line::from(format!(
                "{}: {}",
                field_marker(*field, 2, labels[2]),
                form.id
            )),
            Line::from(format!(
                "{}: {}",
                field_marker(*field, 3, labels[3]),
                password
            )),
            Line::from(""),
            Line::from("Enter moves to the next field. Source id can be blank."),
        ];
        frame.render_widget(
            Paragraph::new(text).block(bordered_block("Add Login Source")),
            area,
        );
        return;
    }

    let items = app
        .config
        .sources
        .values()
        .map(|source| {
            let is_default = app.config.default_source.as_deref() == Some(&source.id);
            source_list_item(source, is_default)
        })
        .collect::<Vec<_>>();
    let mut state = ratatui::widgets::ListState::default().with_selected(Some(app.selected_source));
    let list = List::new(items)
        .block(bordered_block("Manage Sources"))
        .highlight_style(selection_style());
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_source_detail(frame: &mut Frame, area: Rect, app: &App) {
    let Some(source) = app.config.sources.values().nth(app.selected_source) else {
        frame.render_widget(
            Paragraph::new("No login source selected.").block(bordered_block("Source Detail")),
            area,
        );
        return;
    };

    let default = if app.config.default_source.as_deref() == Some(&source.id) {
        "yes"
    } else {
        "no"
    };
    let mount_count = app
        .config
        .mounts
        .iter()
        .filter(|mount| mount.source_id == source.id)
        .count();
    let text = vec![
        Line::from(format!("Id:       {}", source.id)),
        Line::from(format!("Address:  {}", source.address)),
        Line::from(format!("Username: {}", source.username)),
        Line::from(format!("Default:  {default}")),
        Line::from(format!("Mounts:   {mount_count}")),
        Line::from(format!(
            "Owner:    {}:{}",
            source.owner_uid, source.owner_gid
        )),
        Line::from(""),
        action_hint("Enter", "set as default"),
        action_hint("d", "remove (only when no mounts use this source)"),
    ];
    frame.render_widget(
        Paragraph::new(text)
            .block(bordered_block("Source Detail"))
            .wrap(Wrap { trim: true }),
        area,
    );
}

// ── Help ───────────────────────────────────────────────────────────────

fn render_help(frame: &mut Frame, area: Rect) {
    let term = Style::default().fg(RP_GOLD).add_modifier(Modifier::BOLD);
    let section = Style::default().fg(RP_IRIS).add_modifier(Modifier::BOLD);

    let term_line = |label: &'static str, body: &'static str| -> Line<'static> {
        Line::from(vec![Span::styled(label, term), Span::raw(body)])
    };

    let text = vec![
        Line::from(Span::styled("Terms", section)),
        term_line("Source", ": a saved login source for remote storage."),
        term_line(
            "Remote path",
            ": the path inside that remote storage, for example /media/movies.",
        ),
        term_line(
            "Mount",
            ": the mapping from a local folder to source:remote path.",
        ),
        term_line(
            "Disconnect",
            ": unmounts the local folder and keeps Shelf config.",
        ),
        term_line(
            "Remove from Shelf",
            ": removes Shelf config and systemd units. It does not delete remote files.",
        ),
        term_line(
            "Repair",
            ": cleans stacked mounts, reapplies config, and tests write access.",
        ),
        Line::from(""),
        Line::from(Span::styled("Keys", section)),
        key_row(
            "Global",
            &[
                ("q/Esc", "back"),
                ("?", "help"),
                ("j/k", "move"),
                ("Enter", "accept"),
            ],
        ),
        key_row(
            "Home",
            &[
                ("a", "add mount"),
                ("s", "sources"),
                ("r", "refresh"),
                ("p", "apply"),
            ],
        ),
        key_row(
            "Mount",
            &[
                ("x", "disconnect"),
                ("d", "remove from Shelf"),
                ("p", "repair"),
            ],
        ),
        key_row(
            "Sources",
            &[
                ("a", "add"),
                ("d", "remove"),
                ("Enter", "detail / set default"),
            ],
        ),
        key_row(
            "Wizard",
            &[
                ("Enter", "advance"),
                ("Esc", "back"),
                ("Tab", "cycle source"),
            ],
        ),
    ];
    frame.render_widget(
        Paragraph::new(text)
            .block(bordered_block("Help"))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn key_row(scope: &'static str, pairs: &[(&'static str, &'static str)]) -> Line<'static> {
    let mut spans = vec![Span::styled(format!("  {scope:<10}"), dim_style())];
    for (i, (key, action)) in pairs.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("   "));
        }
        spans.push(Span::styled(*key, key_style()));
        spans.push(Span::raw(format!(" {action}")));
    }
    Line::from(spans)
}

// ── Footer ─────────────────────────────────────────────────────────────

fn render_footer(frame: &mut Frame, area: Rect, app: &App) {
    let keys = match app.screen {
        Screen::Home => {
            "q quit  ? help  j/k move  Enter detail  a add  s sources  r refresh  p apply"
        }
        Screen::AddMount => "Enter continue  Esc back  Tab source",
        Screen::MountDetail => "Esc back  x disconnect  d remove  p repair",
        Screen::Sources | Screen::SourceDetail => {
            "Esc back  a add source  d remove source  Enter set default/detail"
        }
        Screen::Help => "Esc back",
    };
    frame.render_widget(
        Paragraph::new(keys).style(dim_style()).block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(border_style()),
        ),
        area,
    );
}

// ── Modal ──────────────────────────────────────────────────────────────

fn render_modal(frame: &mut Frame, area: Rect, app: &App, modal: &Modal) {
    let block = centered_rect(64, 45, area);
    frame.render_widget(Clear, block);
    match modal {
        Modal::Confirm { action, message } => {
            let (title_text, accent) = if action.is_destructive() {
                ("Confirm destructive action", RP_LOVE)
            } else {
                ("Confirm", RP_IRIS)
            };
            let confirm_title = Span::styled(
                title_text,
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            );
            frame.render_widget(
                Paragraph::new(format!("{message}\n\nEnter confirms. Esc cancels."))
                    .block(modal_block(confirm_title, accent))
                    .wrap(Wrap { trim: true }),
                block,
            );
        }
        Modal::Error(message) => {
            let error_title = Span::styled(
                "Needs Attention",
                Style::default().fg(RP_LOVE).add_modifier(Modifier::BOLD),
            );
            frame.render_widget(
                Paragraph::new(format!("{message}\n\nEsc closes this message."))
                    .block(modal_block(error_title, RP_LOVE))
                    .style(Style::default().fg(RP_LOVE))
                    .wrap(Wrap { trim: true }),
                block,
            );
        }
        Modal::Progress => render_progress_modal(frame, block, app),
        Modal::Success(message) => {
            let success_title = Span::styled(
                "Done",
                Style::default().fg(RP_FOAM).add_modifier(Modifier::BOLD),
            );
            frame.render_widget(
                Paragraph::new(format!("{message}\n\nPress any key to dismiss."))
                    .block(modal_block(success_title, RP_FOAM))
                    .style(Style::default().fg(RP_FOAM))
                    .wrap(Wrap { trim: true }),
                block,
            );
        }
        Modal::Input => {}
    }
}

fn render_progress_modal(frame: &mut Frame, area: Rect, app: &App) {
    let lines = match &app.task {
        TaskState::Idle => vec![Line::from("Idle")],
        TaskState::Running { steps, .. } => checklist_lines("Running", steps, None),
        TaskState::Done { steps } => checklist_lines("Done", steps, None),
        TaskState::Failed { steps, error } => checklist_lines("Failed", steps, Some(error)),
    };
    frame.render_widget(
        Paragraph::new(lines)
            .block(bordered_block("Progress"))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn checklist_lines<'a>(
    title: &'a str,
    steps: &'a [super::actions::TaskStep],
    error: Option<&'a str>,
) -> Vec<Line<'a>> {
    let check_on = Style::default().fg(RP_FOAM);
    let check_off = dim_style();
    let mut lines = vec![Line::from(title)];
    lines.extend(steps.iter().map(|step| {
        let (mark, style) = if step.done {
            ("[x]", check_on)
        } else {
            ("[ ]", check_off)
        };
        Line::from(vec![
            Span::styled(mark, style),
            Span::from(format!(" {}", step.label)),
        ])
    }));
    if let Some(error) = error {
        lines.push(Line::from(""));
        lines.push(Line::from(format!("Action: {error}")));
    }
    lines
}

// ── Health / Selection ─────────────────────────────────────────────────

fn health_label(health: MountHealth) -> (&'static str, Color) {
    match health {
        MountHealth::Ready => ("mounted", RP_FOAM),
        MountHealth::NeedsAttention => ("needs attention", RP_GOLD),
        MountHealth::Missing => ("not mounted", RP_GOLD),
        MountHealth::Broken => ("broken", RP_LOVE),
    }
}

fn selection_style() -> Style {
    Style::default()
        .fg(RP_BASE)
        .bg(RP_IRIS)
        .add_modifier(Modifier::BOLD)
}

// ── Field marker ───────────────────────────────────────────────────────

fn field_marker(current: usize, target: usize, label: &str) -> String {
    if current == target {
        format!("> {label}")
    } else {
        format!("  {label}")
    }
}

// ── Centered rect helper ───────────────────────────────────────────────

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::mpsc;

    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    use crate::config::SourceConfig;

    use super::*;
    use crate::tui::app::{App, Screen};

    #[test]
    fn selected_rows_use_readable_explicit_contrast() {
        let style = selection_style();

        assert_eq!(style.fg, Some(RP_BASE));
        assert_eq!(style.bg, Some(RP_IRIS));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn header_wordmark_renders_three_rows_with_metadata() {
        let (tx, _) = mpsc::channel();
        let mut app = App::new(tx);
        app.config.default_source = Some("nas-home".into());
        app.config.sources = BTreeMap::from([(
            "nas-home".into(),
            SourceConfig {
                id: "nas-home".into(),
                address: "nas.local".into(),
                username: "alice".into(),
                owner_uid: 1000,
                owner_gid: 1000,
            },
        )]);

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, &app)).unwrap();
        let buffer = terminal.backend().buffer().clone();

        let row = |y: u16| -> String {
            (0..80)
                .map(|x| buffer[(x, y)].symbol().to_string())
                .collect::<String>()
                .trim_end()
                .to_string()
        };

        assert_eq!(
            row(0),
            " \u{256d}\u{2500}\u{256e} \u{2577} \u{2577} \u{256d}\u{2500}\u{256e} \u{2577}   \u{256d}\u{2500}\u{256e}     remote storage control panel"
        );
        assert!(row(1).contains("default: nas-home"));
        assert!(row(2).starts_with(" \u{2570}\u{2500}\u{256f}"));
    }

    #[test]
    fn source_detail_renders_selected_source_fields() {
        let (tx, _) = mpsc::channel();
        let mut app = App::new(tx);
        app.screen = Screen::SourceDetail;
        app.config.default_source = Some("home".into());
        app.config.sources = BTreeMap::from([(
            "home".into(),
            SourceConfig {
                id: "home".into(),
                address: "nas.local".into(),
                username: "alice".into(),
                owner_uid: 1000,
                owner_gid: 1000,
            },
        )]);

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, &app)).unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(rendered.contains("Source Detail"));
        assert!(rendered.contains("nas.local"));
        assert!(rendered.contains("alice"));
        assert!(rendered.contains("Default:  yes"));
    }
}
