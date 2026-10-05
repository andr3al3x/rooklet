//! Semantic pointer actions reuse the keyboard confirmation and mutation flow.
use super::{App, Effect, Popup, View};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MouseAction {
    View(View),
    Row {
        view: View,
        key: String,
        activate: bool,
    },
    Scroll {
        view: View,
        delta: isize,
    },
    Key(KeyCode),
    DialogField(usize),
    DialogScroll(isize),
    ProfileRow(usize),
}

impl App {
    pub fn handle_mouse(&mut self, action: MouseAction) -> Effect {
        match action {
            MouseAction::Key(code) => {
                if self.popup.is_none() {
                    self.searching = false;
                }
                return self.handle(KeyEvent::new(code, KeyModifiers::NONE));
            }
            MouseAction::DialogScroll(delta) => {
                if let Some(Popup::Confirm { scroll, .. }) = &self.popup {
                    scroll.set(
                        scroll
                            .get()
                            .saturating_add_signed(delta.clamp(-3, 3) as i16),
                    );
                }
                if !self.busy
                    && let Some(Popup::Profiles {
                        entries, selected, ..
                    }) = &mut self.popup
                {
                    *selected = selected
                        .saturating_add_signed(delta.clamp(-3, 3))
                        .min(entries.len().saturating_sub(1));
                }
            }
            MouseAction::ProfileRow(index) if !self.busy => {
                if let Some(Popup::Profiles {
                    entries,
                    selected,
                    loading: false,
                }) = &mut self.popup
                    && index < entries.len()
                {
                    *selected = index;
                }
            }
            MouseAction::DialogField(index) if !self.busy && index < 7 => {
                if let Some(Popup::Network { draft, field }) = &mut self.popup {
                    *field = index;
                    if matches!(index, 2..=4) {
                        draft.cycle(index);
                    }
                }
                if let Some(Popup::Explain { draft, field }) = &mut self.popup
                    && index < 5
                {
                    *field = index;
                    draft.cycle(index);
                }
            }
            MouseAction::View(view) if self.popup.is_none() => {
                self.last_mouse_click = None;
                self.searching = false;
                self.view = view;
                self.reconcile();
            }
            MouseAction::Scroll { view, delta } if self.popup.is_none() && self.view == view => {
                self.last_mouse_click = None;
                self.searching = false;
                self.navigate(delta.clamp(-3, 3));
            }
            MouseAction::Row {
                view,
                key,
                activate,
            } if self.popup.is_none() && self.view == view && self.keys().contains(&key) => {
                let double_click = self.last_mouse_click.as_ref().is_some_and(
                    |(previous_view, previous_key, at)| {
                        *previous_view == view
                            && *previous_key == key
                            && at.elapsed() <= Duration::from_millis(400)
                    },
                );
                self.selection[view.index()] = Some(key.clone());
                self.searching = false;
                self.last_mouse_click = Some((view, key, Instant::now()));
                if activate || double_click {
                    let code = if view == View::Applications {
                        let blocked = self
                            .applications()
                            .get(self.selected_index().unwrap_or(0))
                            .is_some_and(|app| app.blocked);
                        KeyCode::Char(if blocked { 'a' } else { 'b' })
                    } else {
                        KeyCode::Enter
                    };
                    return self.handle(KeyEvent::new(code, KeyModifiers::NONE));
                }
            }
            _ => {}
        }
        Effect::default()
    }
}
