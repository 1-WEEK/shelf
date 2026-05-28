use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};

use crate::{Result, ShelfError};

use super::app::{App, Modal, Screen, SourceMode, WizardStep};

pub fn poll(timeout: Duration) -> Result<bool> {
    event::poll(timeout).map_err(tui_io_error)
}

pub fn read() -> Result<Event> {
    event::read().map_err(tui_io_error)
}

pub fn handle(event: Event, app: &mut App) -> Result<()> {
    let Event::Key(key) = event else {
        return Ok(());
    };
    if key.kind != KeyEventKind::Press {
        return Ok(());
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.should_quit = true;
        return Ok(());
    }

    if matches!(app.modal, Some(Modal::Confirm { .. })) {
        match key.code {
            KeyCode::Enter => app.enter(),
            KeyCode::Esc | KeyCode::Char('n') => app.modal = None,
            _ => {}
        }
        return Ok(());
    }
    if app.modal.is_some() {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => app.modal = None,
            _ => {}
        }
        return Ok(());
    }

    match key.code {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Char('?') => app.show_help(),
        KeyCode::Esc => app.back(),
        KeyCode::Enter => {
            if matches!(app.screen, Screen::Sources)
                && matches!(app.source_mode, SourceMode::Add { .. })
            {
                app.source_add_next_field();
            } else {
                app.enter();
            }
        }
        KeyCode::Up | KeyCode::Char('k') => app.move_selection(-1),
        KeyCode::Down | KeyCode::Char('j') => app.move_selection(1),
        KeyCode::Char('a')
            if app.screen == Screen::Sources || app.screen == Screen::SourceDetail =>
        {
            app.start_add_source()
        }
        KeyCode::Char('a') => app.goto(Screen::AddMount),
        KeyCode::Char('r') => app.refresh(),
        KeyCode::Char('x') => app.disconnect_selected(),
        KeyCode::Char('d') => {
            if app.screen == Screen::Sources || app.screen == Screen::SourceDetail {
                app.remove_selected_source();
            } else {
                app.remove_selected_mount();
            }
        }
        KeyCode::Char('p') => {
            if app.screen == Screen::Repair {
                app.repair_selected_or_all();
            } else {
                app.apply_config();
            }
        }
        KeyCode::Char('s') => app.goto(Screen::Sources),
        KeyCode::Char(ch) if should_capture_text(app) => app.input_char(ch),
        KeyCode::Backspace => app.backspace(),
        KeyCode::Tab
            if app.screen == Screen::AddMount && app.add_mount.step == WizardStep::LoginSource =>
        {
            app.move_selection(1);
            app.add_mount.source_index = app.selected_source;
        }
        _ => {}
    }
    Ok(())
}

fn should_capture_text(app: &App) -> bool {
    matches!(app.screen, Screen::AddMount)
        && matches!(
            app.add_mount.step,
            WizardStep::LocalFolder | WizardStep::RemotePath
        )
        || matches!(app.source_mode, SourceMode::Add { .. })
}

fn tui_io_error(source: std::io::Error) -> ShelfError {
    ShelfError::Validation(format!("terminal I/O failed: {source}"))
}
