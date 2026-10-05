//! Export actual Ratatui cells for visual QA without starting a terminal or filter.
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Terminal,
    backend::TestBackend,
    style::{Color, Modifier},
};
use serde_json::json;
use xield::{
    app::{App, Popup, View},
    backend::Backend,
    ui::{self, Theme},
};

fn color(color: Color) -> String {
    match color {
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        _ => "#d6e0eb".into(),
    }
}

fn main() -> Result<()> {
    let name = std::env::args().nth(1).unwrap_or("activity".into());
    let mut backend = Backend::new(true)?;
    let mut app = App::new(backend.snapshot()?);
    for _ in 0..8 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        app.update(backend.snapshot()?, false);
    }
    match name.as_str() {
        "sorted" => {
            app.handle(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        }
        "filtered" => {
            app.filters[0] = "country:US proto:tcp".into();
        }
        "peer-rule" | "rule-review" => {
            app.snapshot.network.rules = vec![xield::model::NetworkRule {
                id: "allow-tcp".into(),
                name: "Allow TCP".into(),
                destination: "any".into(),
                port: None,
                protocol: xield::model::Protocol::Tcp,
                direction: xield::model::Direction::Outbound,
                action: xield::model::Action::Allow,
                interface: None,
                enabled: true,
            }];
            app.handle(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            app.handle(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
            app.handle(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
            if let Some(Popup::Network { draft, .. }) = &mut app.popup {
                draft.id = "selected-peer".into();
            }
            if name == "rule-review" {
                app.handle(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            }
        }
        "explain" => {
            app.view = View::Network;
            app.handle(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE));
            if let Some(Popup::Explain { draft, .. }) = &mut app.popup {
                draft.remote = "203.0.113.5".into();
                draft.port = "443".into();
                draft.interface = "en0".into();
            }
        }
        "network" => app.view = View::Network,
        "settings" => app.view = View::Settings,
        "applications" => app.view = View::Applications,
        "editor" => {
            app.view = View::Network;
            app.handle(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        }
        "terminate" | "force-kill" => {
            app.handle(KeyEvent::new(
                KeyCode::Char(if name == "terminate" { 'x' } else { 'X' }),
                KeyModifiers::NONE,
            ));
        }
        "help" => app.popup = Some(Popup::Help),
        "permissions" | "permissions-dialog" => {
            let identity = app.snapshot.activity[0].identities[0].clone();
            let mut helper = identity.clone();
            helper.path = format!(
                "{}/Contents/Helpers/helper",
                identity.bundle_path.as_deref().unwrap()
            );
            app.snapshot.activity[0].identities.push(helper.clone());
            app.snapshot.applications = vec![
                xield::model::Application {
                    path: identity.path,
                    name: "Main executable".into(),
                    blocked: false,
                },
                xield::model::Application {
                    path: helper.path,
                    name: "Helper".into(),
                    blocked: true,
                },
            ];
            if name == "permissions-dialog" {
                app.handle(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
            }
        }
        "dialog" => {
            app.handle(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        }
        _ => {
            app.handle(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        }
    }
    let width = std::env::args()
        .nth(2)
        .map(|value| value.parse())
        .transpose()?
        .unwrap_or(120);
    let height = std::env::args()
        .nth(3)
        .map(|value| value.parse())
        .transpose()?
        .unwrap_or(34);
    let mut terminal = Terminal::new(TestBackend::new(width, height))?;
    terminal.draw(|frame| ui::draw(frame, &app, Theme::Dark))?;
    let buffer = terminal.backend().buffer();
    let cells=buffer.content.iter().map(|cell|json!({"symbol":cell.symbol(),"fg":color(cell.fg),"bg":color(cell.bg),"bold":cell.modifier.contains(Modifier::BOLD)})).collect::<Vec<_>>();
    println!("{}", json!({"width":width,"height":height,"cells":cells}));
    Ok(())
}
