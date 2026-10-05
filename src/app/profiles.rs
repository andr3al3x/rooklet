//! Managed profile selection, export, and reviewed application.
use super::{App, ConfirmedAction, Effect, Popup, clean};
use crossterm::event::{KeyCode, KeyEvent};
use rooklet_core::model::Snapshot;
use rooklet_macos::profile::Prepared;

#[derive(Debug, Clone)]
pub enum ProfileOperation {
    List,
    Prepare(String),
    Export(String),
    Apply(Box<Prepared>),
}

#[derive(Debug, Clone)]
pub enum ProfileOutcome {
    Listed(Vec<String>),
    Prepared {
        name: String,
        prepared: Box<Prepared>,
    },
    Exported {
        name: String,
        entries: Vec<String>,
    },
    Applied,
}

pub(super) enum Pending {
    List,
    Prepare(String),
    Export(String),
}

impl App {
    pub(super) fn open_profiles(&mut self) -> Effect {
        self.popup = Some(Popup::Profiles {
            entries: Vec::new(),
            selected: 0,
            loading: true,
        });
        self.pending_profile = Some(Pending::List);
        self.busy = true;
        Effect {
            profile: Some(ProfileOperation::List),
            ..Default::default()
        }
    }

    pub(super) fn handle_profile_popup(&mut self, mut popup: Popup, key: KeyEvent) -> Effect {
        if !self.busy {
            match &mut popup {
                Popup::Profiles {
                    entries,
                    selected,
                    loading,
                } if !*loading => match key.code {
                    KeyCode::Up | KeyCode::Char('k') => *selected = selected.saturating_sub(1),
                    KeyCode::Down | KeyCode::Char('j') => {
                        *selected = selected
                            .saturating_add(1)
                            .min(entries.len().saturating_sub(1))
                    }
                    KeyCode::PageUp => *selected = selected.saturating_sub(8),
                    KeyCode::PageDown => {
                        *selected = selected
                            .saturating_add(8)
                            .min(entries.len().saturating_sub(1))
                    }
                    KeyCode::Home => *selected = 0,
                    KeyCode::End => *selected = entries.len().saturating_sub(1),
                    KeyCode::Char('e') => {
                        if self.snapshot.firewall.is_some()
                            && self.snapshot.applications_available
                            && self.snapshot.network.rules_available
                        {
                            self.popup = Some(Popup::ProfileName {
                                name: String::new(),
                            });
                            return Effect::default();
                        }
                        self.notify("Complete firewall status is required to export. Close this dialog and press u to authenticate and refresh.".into(), true);
                    }
                    KeyCode::Enter => {
                        if let Some(name) = entries.get(*selected).cloned() {
                            self.busy = true;
                            *loading = true;
                            self.pending_profile = Some(Pending::Prepare(name.clone()));
                            self.popup = Some(popup);
                            return Effect {
                                profile: Some(ProfileOperation::Prepare(name)),
                                ..Default::default()
                            };
                        }
                    }
                    _ => {}
                },
                Popup::ProfileName { name } => match key.code {
                    KeyCode::Enter if !name.trim().is_empty() => {
                        let name = name.trim().to_owned();
                        self.pending_profile = Some(Pending::Export(name.clone()));
                        self.busy = true;
                        self.popup = Some(popup);
                        return Effect {
                            profile: Some(ProfileOperation::Export(name)),
                            ..Default::default()
                        };
                    }
                    code => super::editor::edit_text(Some(name), code, 64),
                },
                _ => {}
            }
        }
        self.popup = Some(popup);
        Effect::default()
    }

    pub fn profiles_finished(&mut self, snapshot: Snapshot, outcome: ProfileOutcome) {
        self.update(snapshot, false);
        self.profile_completed(outcome);
    }
    pub fn profile_completed(&mut self, outcome: ProfileOutcome) {
        self.busy = false;
        let pending = self.pending_profile.take();
        match outcome {
            ProfileOutcome::Listed(entries)
                if matches!(pending, Some(Pending::List))
                    && matches!(self.popup, Some(Popup::Profiles { .. })) =>
            {
                self.popup = Some(Popup::Profiles {
                    entries,
                    selected: 0,
                    loading: false,
                });
            }
            ProfileOutcome::Prepared { name, prepared }
                if matches!(&pending, Some(Pending::Prepare(expected)) if *expected == name)
                    && matches!(self.popup, Some(Popup::Profiles { .. })) =>
            {
                self.popup = Some(Popup::Confirm {
                    title: format!("Apply profile {}", clean(&name)),
                    body: prepared
                        .review()
                        .lines()
                        .map(clean)
                        .collect::<Vec<_>>()
                        .join("\n"),
                    scroll: Default::default(),
                    action: ConfirmedAction::Profile(prepared),
                });
            }
            ProfileOutcome::Exported { name, entries } => {
                if matches!(&pending, Some(Pending::Export(expected)) if *expected == name)
                    && matches!(self.popup, Some(Popup::ProfileName { .. }))
                {
                    let selected = entries.iter().position(|entry| *entry == name).unwrap_or(0);
                    self.popup = Some(Popup::Profiles {
                        entries,
                        selected,
                        loading: false,
                    });
                }
                self.notify(
                    format!(
                        "Exported current firewall scopes to profile {}",
                        clean(&name)
                    ),
                    false,
                );
            }
            ProfileOutcome::Applied => {
                self.notify("Profile applied; backend settings read back".into(), false)
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;
    use rooklet_core::model::NetworkStatus;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }
    fn snapshot() -> Snapshot {
        Snapshot {
            firewall: Some(Default::default()),
            applications_available: true,
            network: NetworkStatus {
                rules_available: true,
                ..Default::default()
            },
            ..Default::default()
        }
    }
    fn ready(app: &mut App) {
        assert!(matches!(
            app.handle(key(KeyCode::Char('p'))).profile,
            Some(ProfileOperation::List)
        ));
        app.profiles_finished(snapshot(), ProfileOutcome::Listed(vec!["work".into()]));
    }
    #[test]
    fn export_name_is_bounded_and_requires_complete_snapshot() {
        let mut app = App::new(snapshot());
        ready(&mut app);
        app.snapshot.network.rules_available = false;
        app.handle(key(KeyCode::Char('e')));
        assert!(matches!(app.popup, Some(Popup::Profiles { .. })));
        app.snapshot.network.rules_available = true;
        app.handle(key(KeyCode::Char('e')));
        for _ in 0..70 {
            app.handle(key(KeyCode::Char('a')));
        }
        assert!(matches!(&app.popup, Some(Popup::ProfileName { name }) if name.len() == 64));
        assert!(
            matches!(app.handle(key(KeyCode::Enter)).profile, Some(ProfileOperation::Export(name)) if name.len() == 64)
        );
    }
    #[test]
    fn failed_prepare_allows_retry_and_mouse_selection_respects_bounds() {
        let mut app = App::new(snapshot());
        ready(&mut app);
        app.handle(key(KeyCode::Enter));
        app.operation_failed("unavailable".into());
        assert!(matches!(
            app.popup,
            Some(Popup::Profiles { loading: false, .. })
        ));
        app.handle_mouse(super::super::MouseAction::ProfileRow(99));
        assert!(matches!(
            app.popup,
            Some(Popup::Profiles { selected: 0, .. })
        ));
        assert!(matches!(
            app.handle(key(KeyCode::Enter)).profile,
            Some(ProfileOperation::Prepare(_))
        ));
    }
}
