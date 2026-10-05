//! Modal confirmation and text/choice editing.
use super::{App, ConfirmedAction, Effect, Popup, clean};
use crossterm::event::{KeyCode, KeyEvent};
use rooklet_core::model::Mutation;

impl App {
    pub(super) fn handle_popup(&mut self, mut popup: Popup, key: KeyEvent) -> Effect {
        if key.code == KeyCode::Esc {
            self.pending_profile = None;
            return Effect::default();
        }
        if matches!(popup, Popup::Profiles { .. } | Popup::ProfileName { .. }) {
            return self.handle_profile_popup(popup, key);
        }
        match &mut popup {
            Popup::Help | Popup::Inspect { .. } if key.code == KeyCode::Enter => {
                return Effect::default();
            }
            Popup::Confirm { action, .. } if key.code == KeyCode::Enter => {
                if !self.busy {
                    self.busy = true;
                    return match action {
                        ConfirmedAction::Firewall(mutation) => Effect {
                            mutation: Some(mutation.clone()),
                            ..Default::default()
                        },
                        ConfirmedAction::Terminate(request) => Effect {
                            terminate: Some(request.clone()),
                            ..Default::default()
                        },
                        ConfirmedAction::Profile(prepared) => Effect {
                            profile: Some(super::ProfileOperation::Apply(prepared.clone())),
                            ..Default::default()
                        },
                    };
                }
            }
            Popup::Confirm { scroll, .. } | Popup::Inspect { scroll, .. } => match key.code {
                KeyCode::Up => scroll.set(scroll.get().saturating_sub(1)),
                KeyCode::Down => scroll.set(scroll.get().saturating_add(1)),
                KeyCode::PageUp => scroll.set(scroll.get().saturating_sub(8)),
                KeyCode::PageDown => scroll.set(scroll.get().saturating_add(8)),
                KeyCode::Home => scroll.set(0),
                KeyCode::End => scroll.set(u16::MAX),
                _ => {}
            },
            Popup::Application { path } => match key.code {
                KeyCode::Enter if !path.trim().is_empty() => {
                    let path = path.clone();
                    self.confirm(
                        "Add incoming application",
                        format!("Add {} to the application firewall?", clean(&path)),
                        Mutation::AddApplication(path),
                    );
                    return Effect::default();
                }
                code => edit_text(Some(path), code, 4096),
            },
            Popup::Network { draft, field } => match key.code {
                KeyCode::Tab | KeyCode::Down => *field = (*field + 1) % 7,
                KeyCode::BackTab | KeyCode::Up => *field = (*field + 6) % 7,
                KeyCode::Left | KeyCode::Right => draft.cycle(*field),
                KeyCode::Char(' ') if draft.text_mut(*field).is_none() => draft.cycle(*field),
                KeyCode::Enter => match draft.rule() {
                    Ok(rule) => {
                        let mut rules = self.snapshot.network.rules.clone();
                        if let Some(index) = rules.iter().position(|r| r.id == rule.id) {
                            rules[index] = rule;
                        } else {
                            rules.push(rule);
                        }
                        self.review_network_rules(rules);
                        return Effect::default();
                    }
                    Err(error) => self.notify(error.to_string(), true),
                },
                code => edit_text(draft.text_mut(*field), code, 256),
            },
            Popup::Explain { draft, field } => match key.code {
                KeyCode::Enter => return Effect::default(),
                KeyCode::Tab | KeyCode::Down => *field = (*field + 1) % 5,
                KeyCode::BackTab | KeyCode::Up => *field = (*field + 4) % 5,
                KeyCode::Left | KeyCode::Right => draft.cycle(*field),
                KeyCode::Char(' ') if draft.text_mut(*field).is_none() => draft.cycle(*field),
                code => edit_text(draft.text_mut(*field), code, 256),
            },
            _ => {}
        }
        self.popup = Some(popup);
        Effect::default()
    }
}

pub(super) fn edit_text(text: Option<&mut String>, code: KeyCode, max_bytes: usize) {
    let Some(text) = text else { return };
    match code {
        KeyCode::Backspace => {
            text.pop();
        }
        KeyCode::Char(c) if !c.is_control() && text.len() + c.len_utf8() <= max_bytes => {
            text.push(c)
        }
        _ => {}
    }
}
