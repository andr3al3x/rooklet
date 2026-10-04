//! Terminal lifecycle and event routing.
mod terminal;
mod worker;

use crate::auth::authenticate;
use anyhow::Result;
use crossterm::event::{self, Event, KeyEventKind};
use std::time::Duration;
use terminal::TerminalSession;
use worker::{Update, UpdateKind, Worker};
use xield::{
    app::App,
    model::Snapshot,
    ui::{self, Theme},
};

pub(crate) fn run(demo: bool, theme: Theme) -> Result<()> {
    let mut worker = Worker::start(demo)?;
    let mut terminal = TerminalSession::start()?;
    let mut app = App::new(Snapshot {
        demo,
        notices: vec!["Reading macOS firewall and traffic statistics…".into()],
        ..Default::default()
    });
    let result = (|| -> Result<()> {
        let mut dirty = true;
        let mut closing = false;
        let mut hit_map = None;
        let mut ui_state = ui::State::default();
        loop {
            for update in worker.drain() {
                apply_update(&mut app, update, closing)?;
                dirty = true;
            }
            if closing && !app.busy {
                break;
            }
            if dirty {
                terminal.draw(|frame| {
                    hit_map = Some(ui::draw_interactive(frame, &app, theme, &mut ui_state))
                })?;
                dirty = false;
            }
            if event::poll(Duration::from_millis(100))? {
                let effect = match event::read()? {
                    Event::Key(key) if key.kind != KeyEventKind::Release => Some(app.handle(key)),
                    Event::Mouse(mouse) => hit_map
                        .as_ref()
                        .and_then(|hits| hits.action(mouse))
                        .map(|action| app.handle_mouse(action)),
                    Event::Resize(..) => {
                        dirty = true;
                        None
                    }
                    _ => None,
                };
                if let Some(effect) = effect {
                    if effect.quit {
                        if app.busy {
                            closing = true;
                            app.notify(
                                "Finishing the current operation before quitting…".into(),
                                false,
                            );
                        } else {
                            break;
                        }
                    }
                    if effect.authenticate {
                        if demo {
                            app.notify("Demo mode; no administrator access needed".into(), false);
                        } else {
                            match authenticate_in_terminal(&mut terminal)? {
                                Ok(()) => {
                                    app.notify("Administrator session authorized".into(), false)
                                }
                                Err(error) => app.notify(error.to_string(), true),
                            }
                        }
                    }
                    if let Some(request) = effect.terminate {
                        worker.terminate(request)?;
                    }
                    if effect.update_geoip {
                        worker.update_geoip()?;
                    }
                    if let Some(mutation) = effect.mutation {
                        let authorized = if demo {
                            Ok(())
                        } else {
                            authenticate_in_terminal(&mut terminal)?
                        };
                        match authorized {
                            Ok(()) => worker.submit(mutation)?,
                            Err(error) => app.failed(error.to_string(), true),
                        }
                    }
                    dirty = true;
                }
            }
        }
        Ok(())
    })();
    let restored = terminal.suspend();
    drop(terminal);
    drop(worker);
    result.and(restored)
}

// Terminal failures stop the TUI; authentication denial remains a displayable result.
fn authenticate_in_terminal(terminal: &mut TerminalSession) -> Result<Result<()>> {
    terminal.suspend()?;
    let result = authenticate();
    terminal.resume()?;
    Ok(result)
}

fn apply_update(app: &mut App, update: Update, closing: bool) -> Result<()> {
    if let Some(report) = update.termination {
        match update.result {
            Ok(snapshot) => app.termination_finished(snapshot, &report),
            Err(error) => {
                app.termination_report(&report);
                let summary = app
                    .notice
                    .as_ref()
                    .map(|notice| notice.text.clone())
                    .unwrap_or_default();
                // A failed observation cannot make the cached state fresh or healthy.
                app.failed(
                    format!("{summary}; activity refresh failed: {error:#}"),
                    false,
                );
            }
        }
        if closing
            && let Some(notice) = &app.notice
            && notice.error
        {
            anyhow::bail!("{}", notice.text);
        }
        return Ok(());
    }
    match update.result {
        Ok(snapshot) if update.kind == UpdateKind::GeoIp => app.geoip_updated(snapshot),
        Ok(snapshot) => app.update(snapshot, update.kind == UpdateKind::Firewall),
        Err(error) if closing && update.kind != UpdateKind::Observation => return Err(error),
        Err(error) => app.failed(format!("{error:#}"), update.kind != UpdateKind::Observation),
    }
    Ok(())
}
#[cfg(test)]
mod tests;
