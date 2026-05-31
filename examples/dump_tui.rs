use std::collections::BTreeMap;
use std::sync::mpsc;

use ratatui::backend::TestBackend;
use ratatui::Terminal;

use shelf::config::{MountConfig, SourceConfig};
use shelf::tui::actions::MountHealth;
use shelf::tui::app::{App, Screen, StatusRow};
use shelf::tui::view;

fn seed(app: &mut App) {
    app.config.default_source = Some("nas-home".into());
    app.config.sources = BTreeMap::from([
        (
            "nas-home".into(),
            SourceConfig {
                id: "nas-home".into(),
                address: "192.168.1.10".into(),
                username: "alice".into(),
                owner_uid: 1000,
                owner_gid: 1000,
            },
        ),
        (
            "nas-cold".into(),
            SourceConfig {
                id: "nas-cold".into(),
                address: "10.0.0.5".into(),
                username: "bob".into(),
                owner_uid: 1000,
                owner_gid: 1000,
            },
        ),
    ]);
    app.config.mounts = vec![
        MountConfig {
            local_path: "/home/alice/Videos".into(),
            source_id: "nas-home".into(),
            remote_path: "/media/movies".into(),
        },
        MountConfig {
            local_path: "/home/alice/Music".into(),
            source_id: "nas-cold".into(),
            remote_path: "/music".into(),
        },
    ];
    app.status_rows = vec![
        StatusRow {
            mount: app.config.mounts[0].clone(),
            health: MountHealth::Ready,
        },
        StatusRow {
            mount: app.config.mounts[1].clone(),
            health: MountHealth::NeedsAttention,
        },
    ];
}

fn dump(label: &str, screen: Screen) {
    let (tx, _) = mpsc::channel();
    let mut app = App::new(tx);
    seed(&mut app);
    app.screen = screen;

    let backend = TestBackend::new(80, 18);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| view::render(frame, &app)).unwrap();
    let buffer = terminal.backend().buffer().clone();

    println!("─── {label} ───────────────────────────────────────────────────────────────");
    for y in 0..18 {
        let row: String = (0..80)
            .map(|x| buffer[(x, y)].symbol().to_string())
            .collect();
        println!("{}", row.trim_end());
    }
    println!();
}

fn dump_wizard(label: &str, step: shelf::tui::app::WizardStep) {
    let (tx, _) = mpsc::channel();
    let mut app = App::new(tx);
    seed(&mut app);
    app.screen = Screen::AddMount;
    app.add_mount.step = step;
    app.add_mount.local_folder = "/home/alice/Photos".into();
    app.add_mount.remote_path = "/media/photos".into();

    let backend = TestBackend::new(80, 18);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| view::render(frame, &app)).unwrap();
    let buffer = terminal.backend().buffer().clone();

    println!("─── {label} ───────────────────────────────────────────────────────────────");
    for y in 0..18 {
        let row: String = (0..80)
            .map(|x| buffer[(x, y)].symbol().to_string())
            .collect();
        println!("{}", row.trim_end());
    }
    println!();
}

fn main() {
    dump("Home", Screen::Home);
    dump("MountDetail", Screen::MountDetail);
    dump("Sources", Screen::Sources);
    dump("SourceDetail", Screen::SourceDetail);
    dump_wizard("AddMount @ step 1", shelf::tui::app::WizardStep::LocalFolder);
    dump_wizard("AddMount @ step 3", shelf::tui::app::WizardStep::LoginSource);
}
