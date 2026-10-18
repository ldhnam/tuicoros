//! Colours and text styles. Respects `NO_COLOR` (<https://no-color.org>).
//! Colour is never the only signal: zones carry labels, the fastest lap a `*`.

use ratatui::style::{Color, Modifier, Style};

use crate::config::ThemeName;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    pub text: Style,
    pub muted: Style,
    pub accent: Style,
    pub title: Style,
    pub border: Style,
    pub border_focused: Style,
    pub tab: Style,
    pub tab_active: Style,
    pub warning: Style,
    pub error: Style,
    /// The highlighted row in lists and tables.
    pub selected: Style,
    pub header: Style,
    pub input: Style,
    pub input_focused: Style,
    pub fastest: Style,
    /// Heart-rate zones 0–5.
    pub zones: [Style; 6],
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
            border_focused: Style::new().fg(Color::LightGreen),
            tab: Style::new().fg(Color::Gray),
            tab_active: Style::new()
                .fg(Color::Black)
                .bg(Color::LightGreen)
                .add_modifier(Modifier::BOLD),
            warning: Style::new().fg(Color::Yellow),
            error: Style::new().fg(Color::LightRed),
            selected: Style::new()
                .bg(Color::Rgb(40, 60, 45))
                .add_modifier(Modifier::BOLD),
            header: Style::new().fg(Color::White).add_modifier(Modifier::BOLD),
            input: Style::new().fg(Color::Gray).bg(Color::Rgb(30, 30, 30)),
            input_focused: Style::new().fg(Color::White).bg(Color::Rgb(45, 55, 48)),
            fastest: Style::new()
                .fg(Color::LightGreen)
                .add_modifier(Modifier::BOLD),
            zones: Self::zone_colours(),
        }
    }

    pub fn light() -> Self {
        Self {
            text: Style::new().fg(Color::Black),
            muted: Style::new().fg(Color::DarkGray),
            accent: Style::new().fg(Color::Green),
            title: Style::new().fg(Color::Black).add_modifier(Modifier::BOLD),
            border: Style::new().fg(Color::Gray),
            border_focused: Style::new().fg(Color::Green),
            tab: Style::new().fg(Color::DarkGray),
            tab_active: Style::new()
                .fg(Color::White)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD),
            warning: Style::new().fg(Color::Red),
            error: Style::new().fg(Color::Red),
            selected: Style::new()
                .bg(Color::Rgb(215, 235, 220))
                .add_modifier(Modifier::BOLD),
            header: Style::new().fg(Color::Black).add_modifier(Modifier::BOLD),
            input: Style::new().fg(Color::Black).bg(Color::Rgb(235, 235, 235)),
            input_focused: Style::new().fg(Color::Black).bg(Color::Rgb(215, 235, 220)),
            fastest: Style::new().fg(Color::Green).add_modifier(Modifier::BOLD),
            zones: Self::zone_colours(),
        }
    }

    fn zone_colours() -> [Style; 6] {
        [
            Style::new().fg(Color::DarkGray),
            Style::new().fg(Color::Gray),
            Style::new().fg(Color::Blue),
            Style::new().fg(Color::Green),
            Style::new().fg(Color::Yellow),
            Style::new().fg(Color::Red),
        ]
    }

    /// No colours at all, only bold/reverse/underline. Used for `NO_COLOR` and in tests.
    pub fn plain() -> Self {
        Self {
            text: Style::new(),
            muted: Style::new(),
            accent: Style::new().add_modifier(Modifier::BOLD),
            title: Style::new().add_modifier(Modifier::BOLD),
            border: Style::new(),
            border_focused: Style::new().add_modifier(Modifier::BOLD),
            tab: Style::new(),
            tab_active: Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD),
            warning: Style::new().add_modifier(Modifier::BOLD),
            error: Style::new().add_modifier(Modifier::BOLD),
            selected: Style::new().add_modifier(Modifier::REVERSED),
            header: Style::new().add_modifier(Modifier::BOLD),
            input: Style::new().add_modifier(Modifier::UNDERLINED),
            input_focused: Style::new().add_modifier(Modifier::UNDERLINED | Modifier::BOLD),
            fastest: Style::new().add_modifier(Modifier::BOLD),
            zones: [Style::new(); 6],
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
        let mut styles = vec![
            t.text,
            t.muted,
            t.accent,
            t.title,
            t.border,
            t.border_focused,
            t.tab,
            t.tab_active,
            t.warning,
            t.error,
            t.selected,
            t.header,
            t.input,
            t.input_focused,
            t.fastest,
        ];
        styles.extend(t.zones);
        for style in styles {
            assert_eq!((style.fg, style.bg), (None, None));
        }
    }
}
