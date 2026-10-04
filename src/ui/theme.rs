//! Shared terminal colors and widget styling.
use ratatui::{
    style::{Color, Modifier, Style},
    widgets::Block,
};

#[derive(Clone, Copy, Default, clap::ValueEnum)]
pub enum Theme {
    #[default]
    Dark,
    Light,
    Mono,
}

#[derive(Clone, Copy)]
pub(super) struct Palette {
    pub(super) bg: Color,
    pub(super) panel: Color,
    pub(super) text: Color,
    pub(super) muted: Color,
    pub(super) line: Color,
    pub(super) accent: Color,
    pub(super) good: Color,
    pub(super) warn: Color,
    pub(super) bad: Color,
    pub(super) selection: Color,
}

impl Palette {
    pub(super) fn new(theme: Theme) -> Self {
        match theme {
            Theme::Dark => Self {
                bg: Color::Rgb(12, 17, 23),
                panel: Color::Rgb(17, 24, 32),
                text: Color::Rgb(214, 224, 235),
                muted: Color::Rgb(132, 151, 169),
                line: Color::Rgb(42, 57, 71),
                accent: Color::Rgb(97, 214, 208),
                good: Color::Rgb(139, 209, 166),
                warn: Color::Rgb(237, 195, 124),
                bad: Color::Rgb(242, 143, 153),
                selection: Color::Rgb(30, 48, 60),
            },
            Theme::Light => Self {
                bg: Color::Rgb(245, 247, 250),
                panel: Color::Rgb(255, 255, 255),
                text: Color::Rgb(28, 43, 57),
                muted: Color::Rgb(86, 106, 125),
                line: Color::Rgb(199, 210, 221),
                accent: Color::Rgb(0, 106, 113),
                good: Color::Rgb(27, 119, 68),
                warn: Color::Rgb(143, 91, 0),
                bad: Color::Rgb(173, 39, 61),
                selection: Color::Rgb(221, 237, 241),
            },
            Theme::Mono => Self {
                bg: Color::Reset,
                panel: Color::Reset,
                text: Color::Reset,
                muted: Color::Reset,
                line: Color::Reset,
                accent: Color::Reset,
                good: Color::Reset,
                warn: Color::Reset,
                bad: Color::Reset,
                selection: Color::Reset,
            },
        }
    }

    pub(super) fn style(self) -> Style {
        Style::default().fg(self.text).bg(self.bg)
    }
    pub(super) fn muted(self) -> Style {
        Style::default().fg(self.muted)
    }
    pub(super) fn accent(self) -> Style {
        Style::default()
            .fg(self.accent)
            .add_modifier(Modifier::BOLD)
    }
    pub(super) fn block(self, title: impl Into<String>) -> Block<'static> {
        Block::bordered()
            .title(format!(" {} ", title.into()))
            .border_type(ratatui::widgets::BorderType::Rounded)
            .border_style(Style::default().fg(self.line))
            .title_style(self.muted())
            .style(Style::default().bg(self.panel))
    }
}
