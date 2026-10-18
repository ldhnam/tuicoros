//! Sign-in form (docs/UI_DESIGN.md §2.1).

use coros_api::{Credentials, Region};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use crate::action::{Edit, LoginMsg};
use crate::commands::login::regions_to_try;
use crate::service::Command;
use crate::theme::Theme;
use crate::widgets::{TextInput, centered, popup, spinner};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Email,
    Password,
    Region,
    Remember,
    Submit,
}

const FIELDS: [Field; 5] = [
    Field::Email,
    Field::Password,
    Field::Region,
    Field::Remember,
    Field::Submit,
];

/// `None` = try the preferred region first, then the others.
const REGION_CHOICES: [Option<Region>; 4] =
    [None, Some(Region::Us), Some(Region::Asia), Some(Region::Eu)];

#[derive(Debug, Clone)]
pub struct LoginScreen {
    email: TextInput,
    password: TextInput,
    region: usize,
    /// Region tried first in "auto" mode (from config).
    preferred: Region,
    remember: bool,
    focus: Field,
    submitting: bool,
    error: Option<String>,
    /// Shown above the form, e.g. "Your session expired".
    notice: Option<String>,
}

impl LoginScreen {
    pub fn new(email: Option<&str>, preferred: Region, notice: Option<String>) -> Self {
        let email = email.map(TextInput::with_value).unwrap_or_default();
        let focus = if email.is_empty() {
            Field::Email
        } else {
            Field::Password
        };
        Self {
            email,
            password: TextInput::default(),
            region: 0,
            preferred,
            remember: false,
            focus,
            submitting: false,
            error: None,
            notice,
        }
    }

    pub fn is_submitting(&self) -> bool {
        self.submitting
    }

    pub fn map_key(&self, key: KeyEvent) -> Option<LoginMsg> {
        if self.submitting {
            return None;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let text_field = matches!(self.focus, Field::Email | Field::Password);
        Some(match key.code {
            KeyCode::Tab | KeyCode::Down => LoginMsg::NextField,
            KeyCode::BackTab | KeyCode::Up => LoginMsg::PrevField,
            KeyCode::Enter => LoginMsg::Submit,
            KeyCode::Char('u') if ctrl && text_field => LoginMsg::Edit(Edit::ClearToStart),
            _ if ctrl => return None,
            KeyCode::Left if self.focus == Field::Region => LoginMsg::CycleRegion(-1),
            KeyCode::Right | KeyCode::Char(' ') if self.focus == Field::Region => {
                LoginMsg::CycleRegion(1)
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ')
                if self.focus == Field::Remember =>
            {
                LoginMsg::Toggle
            }
            KeyCode::Char(' ') if self.focus == Field::Submit => LoginMsg::Submit,
            KeyCode::Char(c) if text_field => LoginMsg::Edit(Edit::Insert(c)),
            KeyCode::Backspace if text_field => LoginMsg::Edit(Edit::Backspace),
            KeyCode::Delete if text_field => LoginMsg::Edit(Edit::Delete),
            KeyCode::Left if text_field => LoginMsg::Edit(Edit::Left),
            KeyCode::Right if text_field => LoginMsg::Edit(Edit::Right),
            KeyCode::Home if text_field => LoginMsg::Edit(Edit::Home),
            KeyCode::End if text_field => LoginMsg::Edit(Edit::End),
            _ => return None,
        })
    }

    pub fn update(&mut self, msg: LoginMsg) -> Option<Command> {
        let index = FIELDS.iter().position(|f| *f == self.focus).unwrap_or(0);
        match msg {
            LoginMsg::NextField => self.focus = FIELDS[(index + 1) % FIELDS.len()],
            LoginMsg::PrevField => self.focus = FIELDS[(index + FIELDS.len() - 1) % FIELDS.len()],
            LoginMsg::CycleRegion(step) => {
                let n = REGION_CHOICES.len() as i64;
                self.region = (self.region as i64 + i64::from(step)).rem_euclid(n) as usize;
            }
            LoginMsg::Toggle => self.remember = !self.remember,
            LoginMsg::Edit(edit) => match self.focus {
                Field::Email => self.email.apply(edit),
                Field::Password => self.password.apply(edit),
                _ => {}
            },
            LoginMsg::Submit => return self.submit(),
        }
        None
    }

    fn submit(&mut self) -> Option<Command> {
        let email = self.email.value().trim().to_owned();
        if email.is_empty() || self.password.is_empty() {
            self.error = Some("Enter your COROS email and password.".to_owned());
            self.focus = if email.is_empty() {
                Field::Email
            } else {
                Field::Password
            };
            return None;
        }
        let credentials = Credentials::from_password(email, self.password.value());
        self.password.wipe();
        self.submitting = true;
        self.error = None;
        let regions = match REGION_CHOICES[self.region] {
            Some(region) => vec![region],
            None => regions_to_try(self.preferred, false),
        };
        Some(Command::Login {
            credentials,
            regions,
            remember: self.remember,
        })
    }

    pub fn on_failed(&mut self, message: String) {
        self.submitting = false;
        self.error = Some(message);
        self.focus = Field::Password;
    }

    fn region_label(&self) -> String {
        match REGION_CHOICES[self.region] {
            Some(region) => format!("{} ({})", region.as_str().to_uppercase(), region.label()),
            None => {
                let order: Vec<String> = regions_to_try(self.preferred, false)
                    .iter()
                    .map(|r| r.as_str().to_uppercase())
                    .collect();
                format!("Auto ({})", order.join(" → "))
            }
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme, tick: u64) {
        let width = area.width.min(66);
        let field_w = width.saturating_sub(16);
        let error_lines = self.error.as_ref().map_or(0, |e| {
            (e.chars().count() as u16 / width.saturating_sub(4).max(1))
                + e.lines().count() as u16
                + 1
        });
        let height = (13 + u16::from(self.notice.is_some()) * 2 + error_lines).min(area.height);
        let inner = popup(
            frame,
            centered(area, width, height),
            "Sign in to COROS",
            theme,
        );

        let label = |field: Field, text: &str| {
            let focused = self.focus == field && !self.submitting;
            let marker = if focused { "› " } else { "  " };
            Span::styled(
                format!("{marker}{text:<10}"),
                if focused { theme.accent } else { theme.text },
            )
        };
        let focused = |field| self.focus == field && !self.submitting;

        let mut lines = Vec::new();
        if let Some(notice) = &self.notice {
            lines.push(Line::from(Span::styled(
                format!(" {notice}"),
                theme.warning,
            )));
            lines.push(Line::from(""));
        }
        lines.push(Line::from(""));
        let mut email = vec![label(Field::Email, "Email")];
        email.extend(
            self.email
                .line(field_w, false, focused(Field::Email), theme)
                .spans,
        );
        lines.push(Line::from(email));
        let mut password = vec![label(Field::Password, "Password")];
        password.extend(
            self.password
                .line(field_w, true, focused(Field::Password), theme)
                .spans,
        );
        lines.push(Line::from(password));
        let region_style = if focused(Field::Region) {
            theme.input_focused
        } else {
            theme.text
        };
        lines.push(Line::from(vec![
            label(Field::Region, "Region"),
            Span::styled(format!("‹ {} ›", self.region_label()), region_style),
        ]));
        let check = if self.remember { "[x]" } else { "[ ]" };
        lines.push(Line::from(vec![
            label(Field::Remember, "Remember"),
            Span::styled(
                format!("{check} keep password hash for automatic sign-in"),
                if focused(Field::Remember) {
                    theme.input_focused
                } else {
                    theme.text
                },
            ),
        ]));
        lines.push(Line::from(""));
        let button = if self.submitting {
            Span::styled(format!("{} Signing in…", spinner(tick)), theme.accent)
        } else if focused(Field::Submit) {
            Span::styled("[ Sign in ]", theme.selected)
        } else {
            Span::styled("[ Sign in ]", theme.title)
        };
        lines.push(Line::from(button).centered());
        lines.push(Line::from(""));
        if let Some(error) = &self.error {
            for l in error.lines() {
                lines.push(Line::from(Span::styled(format!(" {l}"), theme.error)));
            }
            lines.push(Line::from(""));
        }
        lines.push(Line::from(Span::styled(
            " Your password is hashed immediately and never stored.",
            theme.muted,
        )));
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
    }
}

#[cfg(test)]
mod tests {
    use secrecy::ExposeSecret;

    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn type_text(screen: &mut LoginScreen, text: &str) {
        for c in text.chars() {
            let msg = screen.map_key(key(KeyCode::Char(c))).unwrap();
            screen.update(msg);
        }
    }

    #[test]
    fn typing_and_submitting_hashes_the_password_and_wipes_the_field() {
        let mut s = LoginScreen::new(None, Region::Us, None);
        type_text(&mut s, "runner@example.com");
        s.update(LoginMsg::NextField);
        type_text(&mut s, "password");
        let Some(Command::Login {
            credentials,
            regions,
            remember,
        }) = s.update(LoginMsg::Submit)
        else {
            panic!("no login command");
        };
        assert_eq!(credentials.account, "runner@example.com");
        assert_eq!(
            credentials.password_md5().expose_secret(),
            "5f4dcc3b5aa765d61d8327deb882cf99"
        );
        assert_eq!(
            regions,
            vec![Region::Us, Region::Asia, Region::Eu],
            "auto = all regions, preferred first"
        );
        assert!(!remember, "remember me is opt-in");
        assert!(s.password.is_empty(), "plaintext wiped");
        assert!(s.is_submitting());
        assert_eq!(
            s.map_key(key(KeyCode::Char('x'))),
            None,
            "input locked while signing in"
        );
    }

    #[test]
    fn region_and_remember_controls() {
        let mut s = LoginScreen::new(Some("a@b.c"), Region::Us, None);
        assert_eq!(
            s.focus,
            Field::Password,
            "email prefilled → start at password"
        );
        type_text(&mut s, "pw");
        s.update(LoginMsg::NextField);
        assert_eq!(
            s.map_key(key(KeyCode::Right)),
            Some(LoginMsg::CycleRegion(1))
        );
        s.update(LoginMsg::CycleRegion(1));
        assert_eq!(s.region_label(), "US (Americas / rest of world)");
        s.update(LoginMsg::NextField);
        assert_eq!(s.map_key(key(KeyCode::Char(' '))), Some(LoginMsg::Toggle));
        s.update(LoginMsg::Toggle);
        let Some(Command::Login {
            regions, remember, ..
        }) = s.update(LoginMsg::Submit)
        else {
            panic!()
        };
        assert_eq!((regions, remember), (vec![Region::Us], true));
    }

    #[test]
    fn missing_fields_and_failures_show_errors() {
        let mut s = LoginScreen::new(None, Region::Us, None);
        assert!(s.update(LoginMsg::Submit).is_none());
        assert!(s.error.as_deref().unwrap().contains("email and password"));
        s.submitting = true;
        s.on_failed("Incorrect password".into());
        assert!(!s.is_submitting());
        assert_eq!(s.focus, Field::Password);
    }

    #[test]
    fn field_navigation_wraps() {
        let mut s = LoginScreen::new(None, Region::Eu, None);
        s.update(LoginMsg::PrevField);
        assert_eq!(s.focus, Field::Submit);
        s.update(LoginMsg::NextField);
        assert_eq!(s.focus, Field::Email);
        assert_eq!(s.region_label(), "Auto (EU → ASIA → US)");
    }
}
