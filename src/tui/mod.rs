use std::io::{self, Stdout};
use std::panic;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::{Result, ShelfError};

pub mod actions;
pub mod app;
pub mod events;
pub mod view;

type TuiTerminal = Terminal<CrosstermBackend<Stdout>>;

pub fn run() -> Result<()> {
    let mut terminal = TerminalGuard::enter()?;
    let (tx, rx) = mpsc::channel();
    let mut app = app::App::new(tx);
    app.refresh();

    let mut last_tick = Instant::now();
    loop {
        while let Ok(message) = rx.try_recv() {
            app.handle_task_message(message);
        }

        terminal
            .draw(|frame| view::render(frame, &app))
            .map_err(tui_io_error)?;

        if app.should_quit {
            break;
        }

        if let Some(action) = app.pending_sudo_action.take() {
            terminal.suspend()?;
            let ok = sudo_auth();
            terminal.resume()?;
            if ok {
                app.execute_privileged_action(action);
            } else {
                app.modal = Some(app::Modal::Error(
                    "sudo authentication failed. The operation was cancelled.".into(),
                ));
            }
        }

        let timeout = Duration::from_millis(200).saturating_sub(last_tick.elapsed());
        if events::poll(timeout)? {
            let event = events::read()?;
            events::handle(event, &mut app)?;
        }
        let now = Instant::now();
        if last_tick.elapsed() >= Duration::from_millis(200) {
            last_tick = now;
            if let Some(dismiss_at) = app.auto_dismiss_at {
                if now >= dismiss_at {
                    app.dismiss_success();
                }
            }
        }
    }

    Ok(())
}

fn sudo_auth() -> bool {
    Command::new("sudo")
        .arg("-v")
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

struct TerminalGuard {
    terminal: TuiTerminal,
    suspended: bool,
}

impl TerminalGuard {
    fn enter() -> Result<Self> {
        enable_raw_mode().map_err(tui_io_error)?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture).map_err(tui_io_error)?;
        let terminal = Terminal::new(CrosstermBackend::new(stdout)).map_err(tui_io_error)?;

        let hook = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
            hook(info);
        }));

        Ok(Self {
            terminal,
            suspended: false,
        })
    }

    fn suspend(&mut self) -> Result<()> {
        if self.suspended {
            return Ok(());
        }
        execute!(
            self.terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )
        .map_err(tui_io_error)?;
        let _ = disable_raw_mode();
        let _ = self.terminal.show_cursor();
        self.suspended = true;
        Ok(())
    }

    fn resume(&mut self) -> Result<()> {
        if !self.suspended {
            return Ok(());
        }
        enable_raw_mode().map_err(tui_io_error)?;
        execute!(
            self.terminal.backend_mut(),
            EnterAlternateScreen,
            EnableMouseCapture
        )
        .map_err(tui_io_error)?;
        self.terminal.clear().map_err(tui_io_error)?;
        self.suspended = false;
        Ok(())
    }
}

fn tui_io_error(source: std::io::Error) -> ShelfError {
    ShelfError::Validation(format!("terminal I/O failed: {source}"))
}

impl std::ops::Deref for TerminalGuard {
    type Target = TuiTerminal;

    fn deref(&self) -> &Self::Target {
        &self.terminal
    }
}

impl std::ops::DerefMut for TerminalGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.terminal
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if self.suspended {
            return;
        }
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        );
        let _ = self.terminal.show_cursor();
    }
}
