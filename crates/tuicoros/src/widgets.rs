//! Small reusable UI pieces.

use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};

use crate::action::Edit;
use crate::theme::Theme;

/// A single-line text field. Cursor positions count characters, not bytes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextInput {
    value: String,
    cursor: usize,
}

impl TextInput {
    pub fn with_value(value: &str) -> Self {
        Self {
            value: value.to_owned(),
            cursor: value.chars().count(),
        }
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }

    /// Empty the field, overwriting the old contents first (passwords).
    pub fn wipe(&mut self) {
        // Overwrite in place so the old bytes don't linger in this allocation.
        let len = self.value.len();
        self.value.clear();
        self.value.extend(std::iter::repeat_n('\0', len));
        self.value.clear();
        self.cursor = 0;
    }

    fn byte_index(&self, char_index: usize) -> usize {
        self.value
            .char_indices()
            .nth(char_index)
            .map_or(self.value.len(), |(i, _)| i)
    }

    pub fn apply(&mut self, edit: Edit) {
        let len = self.value.chars().count();
        match edit {
            Edit::Insert(c) if !c.is_control() => {
                let at = self.byte_index(self.cursor);
                self.value.insert(at, c);
                self.cursor += 1;
            }
            Edit::Insert(_) => {}
            Edit::Backspace if self.cursor > 0 => {
                let at = self.byte_index(self.cursor - 1);
                self.value.remove(at);
                self.cursor -= 1;
            }
            Edit::Delete if self.cursor < len => {
                let at = self.byte_index(self.cursor);
                self.value.remove(at);
            }
            Edit::Backspace | Edit::Delete => {}
            Edit::Left => self.cursor = self.cursor.saturating_sub(1),
            Edit::Right => self.cursor = (self.cursor + 1).min(len),
            Edit::Home => self.cursor = 0,
            Edit::End => self.cursor = len,
            Edit::ClearToStart => {
                let at = self.byte_index(self.cursor);
                self.value.replace_range(..at, "");
                self.cursor = 0;
            }
        }
    }

    /// Render as `[value   ]`, masked if `secret`, with a block cursor when focused.
    pub fn line(&self, width: u16, secret: bool, focused: bool, theme: &Theme) -> Line<'static> {
        let shown: Vec<char> = if secret {
            std::iter::repeat_n('•', self.value.chars().count()).collect()
        } else {
            self.value.chars().collect()
        };
        let width = usize::from(width.max(1));
        // Scroll so the cursor stays visible.
        let start = (self.cursor + 1).saturating_sub(width);
        let style = if focused {
            theme.input_focused
        } else {
            theme.input
        };
        let mut spans = Vec::new();
        for (i, c) in shown.iter().enumerate().skip(start).take(width) {
            let s = if focused && i == self.cursor {
                style.patch(theme.selected)
            } else {
                style
            };
            spans.push(Span::styled(c.to_string(), s));
        }
        let used = shown.len().saturating_sub(start).min(width);
        if used < width {
            let cursor_here = focused && self.cursor >= shown.len();
            if cursor_here {
                spans.push(Span::styled(" ", style.patch(theme.selected)));
            }
            let pad = width - used - usize::from(cursor_here);
            spans.push(Span::styled(" ".repeat(pad), style));
        }
        Line::from(spans)
    }
}

/// Braille spinner, one frame per tick.
pub fn spinner(tick: u64) -> &'static str {
    const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    FRAMES[(tick % FRAMES.len() as u64) as usize]
}

/// A `width`×`height` rectangle centred in `area` (clamped to fit).
pub fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let [row] = Layout::vertical([Constraint::Length(height.min(area.height))])
        .flex(Flex::Center)
        .areas(area);
    let [rect] = Layout::horizontal([Constraint::Length(width.min(area.width))])
        .flex(Flex::Center)
        .areas(row);
    rect
}

/// Clear `area` and draw a bordered popup; returns the inner area.
pub fn popup(frame: &mut Frame, area: Rect, title: &str, theme: &Theme) -> Rect {
    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .border_style(theme.border_focused)
        .title(Line::from(format!(" {title} ")).style(theme.title));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    inner
}

/// A short centred message (empty, loading, error states).
pub fn message(frame: &mut Frame, area: Rect, lines: Vec<Line<'static>>) {
    let height = u16::try_from(lines.len()).unwrap_or(u16::MAX);
    let [_, middle, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height),
        Constraint::Fill(1),
    ])
    .areas(area);
    frame.render_widget(
        Paragraph::new(lines).centered().wrap(Wrap { trim: true }),
        middle,
    );
}

/// A horizontal bar of `fraction` (0–1) of `width` cells.
pub fn bar(fraction: f64, width: u16, style: Style) -> Span<'static> {
    let cells = (fraction.clamp(0.0, 1.0) * f64::from(width)).round() as usize;
    Span::styled("█".repeat(cells), style)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(s: &str) -> TextInput {
        let mut t = TextInput::default();
        for c in s.chars() {
            t.apply(Edit::Insert(c));
        }
        t
    }

    #[test]
    fn editing_handles_multibyte_text() {
        let mut t = typed("Tân");
        assert_eq!(t.value(), "Tân");
        t.apply(Edit::Left);
        t.apply(Edit::Backspace);
        assert_eq!(t.value(), "Tn");
        t.apply(Edit::Insert('â'));
        assert_eq!(t.value(), "Tân");
        t.apply(Edit::Home);
        t.apply(Edit::Delete);
        assert_eq!(t.value(), "ân");
        t.apply(Edit::End);
        t.apply(Edit::Backspace);
        t.apply(Edit::Backspace);
        t.apply(Edit::Backspace);
        assert!(t.is_empty());
    }

    #[test]
    fn clear_to_start_and_wipe() {
        let mut t = typed("hello world");
        for _ in 0..5 {
            t.apply(Edit::Left);
        }
        t.apply(Edit::ClearToStart);
        assert_eq!(t.value(), "world");
        t.wipe();
        assert!(t.is_empty());
        t.apply(Edit::Insert('\n'));
        assert!(t.is_empty(), "control characters are ignored");
    }

    #[test]
    fn rendered_field_is_masked_and_fixed_width() {
        let t = typed("secret");
        let line = t.line(10, true, false, &Theme::plain());
        assert_eq!(line.to_string(), "••••••    ");
        let long = typed("a-very-long-email@example.com");
        assert_eq!(
            long.line(8, false, true, &Theme::plain()).width(),
            8,
            "scrolls to the cursor"
        );
    }

    #[test]
    fn centering_clamps_to_area() {
        let area = Rect::new(0, 0, 20, 10);
        assert_eq!(centered(area, 10, 4), Rect::new(5, 3, 10, 4));
        assert_eq!(centered(area, 50, 50), area);
    }
}
