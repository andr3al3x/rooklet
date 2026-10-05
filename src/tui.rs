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
    app::{App, ProfileOperation},
    model::Snapshot,
    ui::{self, Theme},
};

pub(crate) fn run(theme: Theme) -> Result<()> {
    let mut worker = Worker::start()?;
    let mut terminal = TerminalSession::start()?;
    let mut app = App::new(Snapshot {
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
                        match authenticate_in_terminal(&mut terminal)? {
                            Ok(()) => app.notify("Administrator session authorized".into(), false),
                            Err(error) => app.notify(error.to_string(), true),
                        }
                    }
                    if let Some(request) = effect.terminate {
                        worker.terminate(request)?;
                    }
                    if effect.update_geoip {
                        worker.update_geoip()?;
                    }
                    if let Some(operation) = effect.profile {
                        if matches!(operation, ProfileOperation::Apply(_)) {
                            match authenticate_in_terminal(&mut terminal)? {
                                Ok(()) => worker.profile(operation)?,
                                Err(error) => app.operation_failed(error.to_string()),
                            }
                        } else {
                            worker.profile(operation)?;
                        }
                    }
                    if let Some(mutation) = effect.mutation {
                        match authenticate_in_terminal(&mut terminal)? {
                            Ok(()) => worker.submit(mutation)?,
                            Err(error) => app.operation_failed(error.to_string()),
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
    if let Some(error) = update.operation_error {
        let message = match update.result {
            Ok(snapshot) => {
                app.update(snapshot, false);
                format!("{error:#}")
            }
            Err(refresh) => {
                let message = format!("{error:#}; status refresh failed: {refresh:#}");
                app.invalidate_observation(&message);
                message
            }
        };
        app.operation_failed(message.clone());
        if closing {
            anyhow::bail!("{message}");
        }
        return Ok(());
    }
    if let Some(outcome) = update.profile {
        match update.result {
            Ok(snapshot) => app.profiles_finished(snapshot, outcome),
            Err(error) => {
                app.profile_completed(outcome);
                let summary = app
                    .notice
                    .as_ref()
                    .map(|notice| notice.text.clone())
                    .unwrap_or_default();
                app.observation_failed(format!("{summary}; status refresh failed: {error:#}"));
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
                app.observation_failed(format!("{summary}; activity refresh failed: {error:#}"));
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
        Err(error) => {
            let message = format!("{error:#}");
            if update.kind == UpdateKind::Observation {
                app.observation_failed(message);
            } else {
                app.invalidate_observation(&message);
                app.operation_failed(message);
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests;
