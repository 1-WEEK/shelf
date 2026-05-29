use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Clear, List, ListItem, Paragraph, Row, Table, TableState, Wrap,
};
use ratatui::Frame;

use super::actions::MountHealth;
use super::app::{App, Modal, Screen, SourceMode, TaskState, WizardStep};

// ── Style helpers ──────────────────────────────────────────────────────

fn border_style() -> Style {
    Style::default().fg(Color::DarkGray)
}

fn title_style() -> Style {
    Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

fn bordered_block<'a>(title: &'a str) -> Block<'a> {
    Block::default()
        .title(title)
        .title_style(title_style())
        .borders(Borders::ALL)
        .border_style(border_style())
}

fn dim_style() -> Style {
    Style::default().fg(Color::DarkGray)
}

// ── Render entry ───────────────────────────────────────────────────────

pub fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
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
    let line = Line::from(vec![
        Span::styled("( ͠° ͟ʖ ͡°)", Style::default().fg(Color::White)),
        Span::styled(
            " 𝙨𝙝𝙚𝙡𝙛",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" · remote storage control panel", dim_style()),
        Span::styled(format!("   default: {default_source}"), dim_style()),
    ]);
    frame.render_widget(
        Paragraph::new(line)
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
        let vchunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(35), Constraint::Min(3)])
            .split(area);
        let text = vec![
            Line::from("No mounts configured."),
            Line::from(""),
            Line::from("Press s to add a login source, then a to add a mount."),
        ];
        frame.render_widget(
            Paragraph::new(text)
                .block(bordered_block("Home"))
                .wrap(Wrap { trim: true })
                .alignment(Alignment::Center),
            vchunks[1],
        );
        return;
    }

    let rows = app.status_rows.iter().map(|row| {
        let state = health_label(row.health);
        Row::new(vec![
            row.mount.local_path.clone(),
            format!("{}:{}", row.mount.source_id, row.mount.remote_path),
            state.0.to_string(),
        ])
        .style(Style::default().fg(state.1))
    });
    let mut state = TableState::default().with_selected(Some(app.selected_mount));
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(42),
            Constraint::Percentage(42),
            Constraint::Length(16),
        ],
    )
    .header(
        Row::new(vec!["Local folder", "Remote path", "State"]).style(
            Style::default()
                .fg(Color::Gray)
                .add_modifier(Modifier::BOLD),
        ),
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
    let step_line = steps
        .iter()
        .map(|(label, step)| {
            let is_active = *step == app.add_mount.step;
            let style = if is_active {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            } else {
                dim_style()
            };
            Span::styled(format!(" {label} "), style)
        })
        .collect::<Vec<_>>();

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
            let marker = if app.config.default_source.as_deref() == Some(&source.id) {
                " default"
            } else {
                ""
            };
            ListItem::new(format!(
                "{}{}  {} as {}",
                source.id, marker, source.address, source.username
            ))
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
    let check_dim = Style::default().fg(Color::DarkGray);
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
        Line::from(Span::styled(
            format!("State:   {label}"),
            Style::default().fg(color),
        )),
        Line::from("Startup: enabled by Shelf apply"),
        Line::from("Writable: tested during apply and repair"),
        Line::from(""),
        Line::from("x disconnect keeps config; d remove from Shelf keeps remote files; p repair."),
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
            let marker = if app.config.default_source.as_deref() == Some(&source.id) {
                " default"
            } else {
                ""
            };
            ListItem::new(format!(
                "{}{}  {} as {}",
                source.id, marker, source.address, source.username
            ))
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
        Line::from("Enter sets this as default. d removes the source when no mounts use it."),
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
    let term = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let text = vec![
        Line::from(vec![
            Span::styled("Source", term),
            Span::from(": a saved login source for remote storage."),
        ]),
        Line::from(vec![
            Span::styled("Remote path", term),
            Span::from(": the path inside that remote storage, for example /media/movies."),
        ]),
        Line::from(vec![
            Span::styled("Mount", term),
            Span::from(": the mapping from a local folder to source:remote path."),
        ]),
        Line::from(vec![
            Span::styled("Disconnect", term),
            Span::from(": unmounts the local folder and keeps Shelf config."),
        ]),
        Line::from(vec![
            Span::styled("Remove from Shelf", term),
            Span::from(
                ": removes Shelf config and systemd units. It does not delete remote files.",
            ),
        ]),
        Line::from(vec![
            Span::styled("Repair", term),
            Span::from(": cleans stacked mounts, reapplies config, and tests write access."),
        ]),
    ];
    frame.render_widget(
        Paragraph::new(text)
            .block(bordered_block("Help"))
            .wrap(Wrap { trim: true }),
        area,
    );
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
        Modal::Confirm { message, .. } => {
            let confirm_title = Span::styled("Confirm", title_style());
            frame.render_widget(
                Paragraph::new(format!("{message}\n\nEnter confirms. Esc cancels."))
                    .block(
                        Block::default()
                            .title(confirm_title)
                            .borders(Borders::ALL)
                            .border_style(border_style()),
                    )
                    .wrap(Wrap { trim: true }),
                block,
            );
        }
        Modal::Error(message) => {
            let error_title = Span::styled(
                "Needs Attention",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            );
            frame.render_widget(
                Paragraph::new(format!("{message}\n\nEsc closes this message."))
                    .block(
                        Block::default()
                            .title(error_title)
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::Red)),
                    )
                    .style(Style::default().fg(Color::Red))
                    .wrap(Wrap { trim: true }),
                block,
            );
        }
        Modal::Progress => render_progress_modal(frame, block, app),
        Modal::Success(message) => {
            let success_title = Span::styled(
                "Done",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            );
            frame.render_widget(
                Paragraph::new(format!("{message}\n\nPress any key to dismiss."))
                    .block(
                        Block::default()
                            .title(success_title)
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::Green)),
                    )
                    .style(Style::default().fg(Color::Green))
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
    let check_on = Style::default().fg(Color::Green);
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
        MountHealth::Ready => ("mounted", Color::Green),
        MountHealth::NeedsAttention => ("needs attention", Color::Yellow),
        MountHealth::Missing => ("not mounted", Color::Yellow),
        MountHealth::Broken => ("broken", Color::Red),
    }
}

fn selection_style() -> Style {
    Style::default()
        .fg(Color::Rgb(0, 0, 0))
        .bg(Color::Cyan)
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

        assert_eq!(style.fg, Some(Color::Rgb(0, 0, 0)));
        assert_eq!(style.bg, Some(Color::Cyan));
        assert!(style.add_modifier.contains(Modifier::BOLD));
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
