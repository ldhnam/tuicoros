//! Colours and text styles. Respects `NO_COLOR` (<https://no-color.org>).

use ratatui::style::{Color, Modifier, Style};

use crate::config::ThemeName;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    pub text: Style,
    pub muted: Style,
    pub accent: Style,
    pub title: Style,
    pub border: Style,
    pub tab: Style,
    pub tab_active: Style,
    pub warning: Style,
}

impl Theme {
    pub fn new(name: ThemeName, no_color: bool) -> Self {
        if no_color {
            return Self::plain();
        }
        match name {
            ThemeName::Dark => Self::dark(),
            ThemeName::Light => Self::light(),
        }
    }

    pub fn dark() -> Self {
        Self {
            text: Style::new().fg(Color::Gray),
            muted: Style::new().fg(Color::DarkGray),
            accent: Style::new().fg(Color::LightGreen),
            title: Style::new().fg(Color::White).add_modifier(Modifier::BOLD),
            border: Style::new().fg(Color::DarkGray),
            tab: Style::new().fg(Color::Gray),
            tab_active: Style::new()
                .fg(Color::Black)
                .bg(Color::LightGreen)
                .add_modifier(Modifier::BOLD),
            warning: Style::new().fg(Color::Yellow),
        }
    }

    pub fn light() -> Self {
        Self {
            text: Style::new().fg(Color::Black),
            muted: Style::new().fg(Color::DarkGray),
            accent: Style::new().fg(Color::Green),
            title: Style::new().fg(Color::Black).add_modifier(Modifier::BOLD),
            border: Style::new().fg(Color::Gray),
            tab: Style::new().fg(Color::DarkGray),
            tab_active: Style::new()
                .fg(Color::White)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD),
            warning: Style::new().fg(Color::Red),
        }
    }

    /// No colours at all, only bold/reverse. Used for `NO_COLOR` and in tests.
    pub fn plain() -> Self {
        Self {
            text: Style::new(),
            muted: Style::new(),
            accent: Style::new().add_modifier(Modifier::BOLD),
            title: Style::new().add_modifier(Modifier::BOLD),
            border: Style::new(),
            tab: Style::new(),
            tab_active: Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD),
            warning: Style::new().add_modifier(Modifier::BOLD),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_color_wins_over_theme_choice() {
        assert_eq!(Theme::new(ThemeName::Dark, true), Theme::plain());
        assert_eq!(Theme::new(ThemeName::Light, true), Theme::plain());
        assert_eq!(Theme::new(ThemeName::Light, false), Theme::light());
    }

    #[test]
    fn plain_theme_uses_no_colours() {
        let t = Theme::plain();
        for style in [
            t.text,
            t.muted,
            t.accent,
            t.title,
            t.border,
            t.tab,
            t.tab_active,
            t.warning,
        ] {
            assert_eq!(style.fg, None);
            assert_eq!(style.bg, None);
        }
    }
}
