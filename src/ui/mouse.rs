//! Pointer targets are collected from the actual rendered widget geometry.
use super::theme::Palette;
use crate::app::{MouseAction, View};
use crossterm::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Margin, Position, Rect};
use ratatui::{
    Frame,
    text::{Line, Span},
    widgets::Paragraph,
};

struct Region {
    area: Rect,
    action: MouseAction,
    wheel: bool,
}

pub(super) fn shortcuts(
    frame: &mut Frame,
    area: Rect,
    choices: &[(&str, KeyCode)],
    palette: Palette,
    hits: &mut HitMap,
) {
    let mut x = area.x;
    let mut spans = Vec::new();
    for (index, (label, key)) in choices.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw("  "));
            x = x.saturating_add(2);
        }
        let width = Line::raw(*label).width() as u16;
        hits.key(Rect::new(x, area.y, width, 1).intersection(area), *key);
        spans.push(Span::styled(*label, palette.accent()));
        x = x.saturating_add(width);
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

pub struct HitMap {
    area: Rect,
    regions: Vec<Region>,
}

impl HitMap {
    pub(super) fn new(area: Rect) -> Self {
        Self {
            area,
            regions: Vec::new(),
        }
    }

    pub fn action(&self, event: MouseEvent) -> Option<MouseAction> {
        if !event.modifiers.is_empty() {
            return None;
        }
        let delta = match event.kind {
            MouseEventKind::ScrollUp => -3,
            MouseEventKind::ScrollDown => 3,
            MouseEventKind::Down(MouseButton::Left) => 0,
            _ => return None,
        };
        let region = self.regions.iter().rev().find(|region| {
            region.wheel == (delta != 0)
                && region.area.contains(Position::new(event.column, event.row))
        })?;
        match &region.action {
            MouseAction::Scroll { view, .. } => Some(MouseAction::Scroll { view: *view, delta }),
            MouseAction::DialogScroll(_) => Some(MouseAction::DialogScroll(delta)),
            action => Some(action.clone()),
        }
    }

    pub(super) fn click(&mut self, area: Rect, action: MouseAction) {
        let area = area.intersection(self.area);
        if !area.is_empty() {
            self.regions.push(Region {
                area,
                action,
                wheel: false,
            });
        }
    }

    pub(super) fn dialog_scroll(&mut self, area: Rect) {
        let area = area.intersection(self.area);
        if !area.is_empty() {
            self.regions.push(Region {
                area,
                action: MouseAction::DialogScroll(0),
                wheel: true,
            });
        }
    }

    pub(super) fn key(&mut self, area: Rect, code: KeyCode) {
        self.click(area, MouseAction::Key(code));
    }

    pub(super) fn table(
        &mut self,
        area: Rect,
        header: u16,
        offset: usize,
        view: View,
        keys: &[String],
    ) {
        let inner = area.inner(Margin::new(1, 1));
        let header = header.min(inner.height);
        let body = Rect::new(
            inner.x,
            inner.y + header,
            inner.width,
            inner.height - header,
        );
        if body.is_empty() {
            return;
        }
        self.regions.push(Region {
            area: body,
            action: MouseAction::Scroll { view, delta: 0 },
            wheel: true,
        });
        for (row, key) in keys
            .iter()
            .skip(offset)
            .take(body.height.into())
            .enumerate()
        {
            let area = Rect::new(body.x, body.y + row as u16, body.width, 1);
            self.click(
                area,
                MouseAction::Row {
                    view,
                    key: key.clone(),
                    activate: false,
                },
            );
            if view == View::Activity {
                self.click(
                    Rect::new(area.x, area.y, area.width.min(4), 1),
                    MouseAction::Row {
                        view,
                        key: key.clone(),
                        activate: true,
                    },
                );
            }
        }
    }

    pub(super) fn modal(&mut self) {
        self.regions.clear();
    }
}
