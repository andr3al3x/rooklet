//! Keyboard routing, confirmation, and dialog editing.
use super::{ActivityRow, App, Effect, NetworkDraft, Popup, SETTINGS, View, clean, process_key};
use crate::model::{Action, Mutation};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

impl App {
    pub(super) fn confirm(&mut self, title: &str, body: String, mutation: Mutation) {
        self.popup = Some(Popup::Confirm {
            title: title.into(),
            scroll: Default::default(),
            body,
            action: super::ConfirmedAction::Firewall(mutation),
        });
    }
    pub fn handle(&mut self, key: KeyEvent) -> Effect {
        self.last_mouse_click = None;
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Effect {
                quit: true,
                ..Default::default()
            };
        }
        if let Some(popup) = self.popup.take() {
            return self.handle_popup(popup, key);
        }
        if self.searching {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => self.searching = false,
                code => {
                    super::editor::edit_text(Some(&mut self.filters[self.view.index()]), code, 256)
                }
            }
            self.reconcile();
            return Effect::default();
        }
        match key.code {
            KeyCode::Char('w') if !self.busy && self.view == View::Network => {
                if self.snapshot.network.rules_available {
                    self.popup = Some(Popup::Explain {
                        draft: super::RuleProbe::default(),
                        field: 0,
                    });
                } else {
                    self.notify(
                        "Saved rules are unavailable. Press u to authenticate and refresh.".into(),
                        true,
                    );
                }
            }
            KeyCode::Char('s') if self.view == View::Activity => {
                self.activity_sort = self.activity_sort.next();
                self.reconcile();
            }
            KeyCode::Char(c @ ('x' | 'X')) if !self.busy && self.view == View::Activity => {
                self.termination_action(if c == 'x' {
                    crate::process::TerminationMode::Terminate
                } else {
                    crate::process::TerminationMode::ForceKill
                });
            }
            KeyCode::Char('g') if !self.busy && self.view == View::Settings => {
                self.busy = true;
                self.notify("Updating country database…".into(), false);
                return Effect {
                    update_geoip: true,
                    ..Default::default()
                };
            }
            KeyCode::Char('u') if !self.busy => {
                return Effect {
                    authenticate: true,
                    ..Default::default()
                };
            }
            KeyCode::Char('q') => {
                return Effect {
                    quit: true,
                    ..Default::default()
                };
            }
            KeyCode::Tab | KeyCode::BackTab => {
                let offset = if key.code == KeyCode::Tab { 1 } else { 3 };
                self.view = View::ALL[(self.view.index() + offset) % 4];
                self.reconcile();
            }
            KeyCode::Char(c @ '1'..='4') => {
                self.view = View::ALL[(c as usize) - ('1' as usize)];
                self.reconcile();
            }
            KeyCode::Down | KeyCode::Char('j') => self.navigate(1),
            KeyCode::Up | KeyCode::Char('k') => self.navigate(-1),
            KeyCode::Char('/') => self.searching = true,
            KeyCode::Esc => {
                self.filters[self.view.index()].clear();
                self.reconcile();
            }
            KeyCode::Char('?') => self.popup = Some(Popup::Help),
            KeyCode::Char(' ') if self.view == View::Activity => {
                self.paused = !self.paused;
                if !self.paused {
                    self.snapshot.activity = self.live_activity.clone();
                    self.reconcile();
                }
            }
            KeyCode::Char('a')
                if !self.busy && matches!(self.view, View::Activity | View::Applications) =>
            {
                self.application_action(Action::Allow)
            }
            KeyCode::Char('b')
                if !self.busy && matches!(self.view, View::Activity | View::Applications) =>
            {
                self.application_action(Action::Block)
            }
            KeyCode::Char('n') if !self.busy && self.view == View::Applications => {
                self.popup = Some(Popup::Application {
                    path: String::new(),
                })
            }
            KeyCode::Char('d') if !self.busy && self.view == View::Applications => {
                if let Some(path) = self.selected_key().map(str::to_owned) {
                    self.confirm(
                        "Remove application",
                        format!(
                            "Remove the incoming permission entry for:\n\n{}",
                            clean(&path)
                        ),
                        Mutation::RemoveApplication(path),
                    );
                }
            }
            KeyCode::Char('n')
                if !self.busy && matches!(self.view, View::Activity | View::Network) =>
            {
                if !self.snapshot.network.rules_available {
                    self.notify(
                        "Saved rules are unavailable. Press u to authenticate and refresh.".into(),
                        true,
                    );
                } else if !self.snapshot.network.configured {
                    self.notify("Set up PF first: sudo xield network setup".into(), true);
                } else {
                    let draft = if self.view == View::Activity {
                        let rows = self.activity_rows();
                        let Some(ActivityRow::Connection(_, flow)) =
                            self.selected_index().and_then(|index| rows.get(index))
                        else {
                            self.notify(
                                "Expand an app and select a peer to create a rule draft.".into(),
                                false,
                            );
                            return Effect::default();
                        };
                        NetworkDraft::from_peer(flow)
                    } else {
                        NetworkDraft::new(String::new())
                    };
                    self.popup = Some(Popup::Network { draft, field: 0 });
                }
            }
            KeyCode::Char('e') | KeyCode::Enter if !self.busy && self.view == View::Network => {
                if let Some(rule) = self.rules().get(self.selected_index().unwrap_or(0)) {
                    self.popup = Some(Popup::Network {
                        draft: NetworkDraft::from_rule(rule),
                        field: 0,
                    });
                }
            }
            KeyCode::Char('d') | KeyCode::Char('t') | KeyCode::Char('+') | KeyCode::Char('-')
                if !self.busy && self.view == View::Network =>
            {
                if let Some(id) = self.selected_key().map(str::to_owned) {
                    let mut rules = self.snapshot.network.rules.clone();
                    if let Some(index) = rules.iter().position(|r| r.id == id) {
                        match key.code {
                            KeyCode::Char('d') => {
                                rules.remove(index);
                            }
                            KeyCode::Char('t') => rules[index].enabled = !rules[index].enabled,
                            KeyCode::Char('+') if index > 0 => rules.swap(index, index - 1),
                            KeyCode::Char('-') if index + 1 < rules.len() => {
                                rules.swap(index, index + 1)
                            }
                            _ => return Effect::default(),
                        }
                        self.review_network_rules(rules);
                    }
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') if !self.busy && self.view == View::Settings => {
                if let (Some(index), Some(settings)) =
                    (self.selected_index(), self.snapshot.firewall.as_ref())
                {
                    let (setting, label) = SETTINGS[index];
                    let value = !settings.get(setting);
                    self.confirm(
                        "Change firewall setting",
                        format!(
                            "Set {label} to {}?\n\nThese settings control incoming connections.",
                            if value { "on" } else { "off" }
                        ),
                        Mutation::Setting(setting, value),
                    );
                }
            }
            KeyCode::Enter if self.view == View::Activity => {
                if let Some(row) = self.activity_rows().get(self.selected_index().unwrap_or(0)) {
                    match row {
                        ActivityRow::Process(process) => {
                            let identity = process_key(process);
                            if !self.expanded.remove(&identity) {
                                self.expanded.insert(identity);
                            }
                        }
                        ActivityRow::Connection(_, _) => {
                            self.popup = Some(Popup::Inspect(row.key()))
                        }
                    }
                }
            }
            _ => {}
        }
        Effect::default()
    }
}
