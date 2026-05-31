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

    if let Some(modal) = &app.modal {
        match modal {
            Modal::Success(_) => {
                app.dismiss_success();
                return Ok(());
            }
            Modal::Confirm { .. } => {
                match key.code {
                    KeyCode::Enter => app.enter(),
                    KeyCode::Esc | KeyCode::Char('n') => app.modal = None,
                    _ => {}
                }
                return Ok(());
            }
            Modal::Progress => {
                return Ok(());
            }
            Modal::Error(_) | Modal::Input => {
                match key.code {
                    KeyCode::Esc | KeyCode::Enter => app.modal = None,
                    _ => {}
                }
                return Ok(());
            }
        }
    }

    match key.code {
        KeyCode::Char(ch) if should_capture_text(app) => app.input_char(ch),
        KeyCode::Char('q') if !should_capture_text(app) => app.back(),
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
            if app.screen == Screen::MountDetail {
                app.repair_selected_or_all();
            } else {
                app.apply_config();
            }
        }
        KeyCode::Char('s') => app.goto(Screen::Sources),
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

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

    use super::super::app::{App, Modal, Screen, SourceMode, WizardStep};
    use super::handle;

    fn key_event(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::empty()))
    }

    #[test]
    fn text_input_in_add_mount_does_not_trigger_shortcuts() {
        let (tx, _) = mpsc::channel();
        let mut app = App::new(tx);
        app.goto(Screen::AddMount);
        assert_eq!(app.add_mount.step, WizardStep::LocalFolder);

        handle(key_event(KeyCode::Char('q')), &mut app).unwrap();

        assert!(!app.should_quit, "'q' in text field must not quit");
        assert_eq!(app.add_mount.local_folder, "q");
    }

    #[test]
    fn text_input_in_add_source_does_not_trigger_shortcuts() {
        let (tx, _) = mpsc::channel();
        let mut app = App::new(tx);
        app.screen = Screen::Sources;
        app.source_mode = SourceMode::Add {
            field: 0,
            form: Default::default(),
        };

        handle(key_event(KeyCode::Char('a')), &mut app).unwrap();

        assert_eq!(
            app.screen,
            Screen::Sources,
            "'a' in text field must not navigate"
        );
        assert_eq!(
            app.source_mode,
            SourceMode::Add {
                field: 0,
                form: super::super::app::SourceForm {
                    address: "a".into(),
                    ..Default::default()
                },
            }
        );
    }

    #[test]
    fn shortcuts_work_when_not_in_text_input() {
        let (tx, _) = mpsc::channel();
        let mut app = App::new(tx);

        handle(key_event(KeyCode::Char('q')), &mut app).unwrap();

        assert!(app.should_quit, "'q' outside text field must quit");
    }

    #[test]
    fn progress_modal_cannot_be_dismissed_by_keys() {
        let (tx, _) = mpsc::channel();
        let mut app = App::new(tx);
        app.modal = Some(Modal::Progress);

        handle(key_event(KeyCode::Esc), &mut app).unwrap();
        assert!(matches!(app.modal, Some(Modal::Progress)));

        handle(key_event(KeyCode::Enter), &mut app).unwrap();
        assert!(matches!(app.modal, Some(Modal::Progress)));
    }
}
