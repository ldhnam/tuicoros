//! Top-level app: tabs, navigation, overlays, the event loop.
//!
//! `update` is synchronous and does no I/O (ADR-004): it changes state and
//! returns `Command`s for the `DataService`. Rendering only reads state.

use chrono::{DateTime, Local, NaiveDate, Utc};
use color_eyre::Result;
use coros_api::Region;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::DefaultTerminal;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Tabs, Wrap};
use tokio::sync::mpsc::UnboundedReceiver;

use crate::action::{Action, RequestIds, UiErrorKind};
use crate::event::{Event, Events};
use crate::keymap::{self, Context};
use crate::screens::activities::{ActivitiesScreen, Effect};
use crate::screens::detail::{DetailEffect, DetailScreen};
use crate::screens::login::LoginScreen;
use crate::service::{Command, DataService};
use crate::theme::Theme;
use crate::widgets::{centered, popup, spinner};

/// Below this size we show a "terminal too small" message instead of the UI.
pub const MIN_WIDTH: u16 = 60;
pub const MIN_HEIGHT: u16 = 15;

const BRAND: &str = " tuicoros ";
/// Minimum empty cells between the header's tabs and its right-hand text.
const HEADER_GAP: u16 = 2;
/// Toasts stay this many ticks (250 ms each).
const TOAST_TICKS: u64 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Dashboard,
    Activities,
    Analysis,
    Calendar,
    Workouts,
}

impl Tab {
    pub const ALL: [Tab; 5] = [
        Tab::Dashboard,
        Tab::Activities,
        Tab::Analysis,
        Tab::Calendar,
        Tab::Workouts,
    ];

    pub const fn title(self) -> &'static str {
        match self {
            Tab::Dashboard => "Dashboard",
            Tab::Activities => "Activities",
            Tab::Analysis => "Analysis",
            Tab::Calendar => "Calendar",
            Tab::Workouts => "Workouts",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }

    /// `1` → Dashboard … `5` → Workouts.
    pub fn from_number(n: u32) -> Option<Tab> {
        let i = usize::try_from(n).ok()?.checked_sub(1)?;
        Tab::ALL.get(i).copied()
    }

    pub fn next(self) -> Tab {
        Tab::ALL[(self.index() + 1) % Tab::ALL.len()]
    }

    pub fn prev(self) -> Tab {
        Tab::ALL[(self.index() + Tab::ALL.len() - 1) % Tab::ALL.len()]
    }

    /// Placeholder text for screens not built yet: (what it will show, phase).
    const fn placeholder(self) -> (&'static str, &'static str) {
        match self {
            Tab::Dashboard => (
                "HRV, resting HR, fatigue, load and recent activities.",
                "Phase 3",
            ),
            Tab::Activities => (
                "Every activity, filterable, with laps and details.",
                "Phase 2",
            ),
            Tab::Analysis => (
                "EvoLab: training load, fatigue, HRV and RHR trends.",
                "Phase 3",
            ),
            Tab::Calendar => ("Planned workouts next to completed activities.", "Phase 4"),
            Tab::Workouts => ("Your workout library with structured steps.", "Phase 4"),
        }
    }
}

/// Settings the app needs from config.
#[derive(Debug, Clone)]
pub struct AppOptions {
    /// Tried first when signing in with "auto" region.
    pub region: Region,
    pub page_size: u32,
    pub default_range_days: u32,
    /// One-line footer message (e.g. a config warning).
    pub notice: Option<String>,
    /// Pre-filled on the login form.
    pub last_email: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Toast {
    text: String,
    expires_at: u64,
}

pub struct App {
    tab: Tab,
    running: bool,
    dirty: bool,
    theme: Theme,
    options: AppOptions,
    /// (display name, region) once signed in.
    account: Option<(String, Region)>,
    login: Option<LoginScreen>,
    activities: ActivitiesScreen,
    detail: Option<DetailScreen>,
    help: bool,
    toasts: Vec<Toast>,
    ids: RequestIds,
    tick: u64,
    now: DateTime<Utc>,
}

impl App {
    /// A signed-out app showing the login form. Call [`App::signed_in`] to
    /// start with an existing session.
    pub fn new(theme: Theme, options: AppOptions, today: NaiveDate, now: DateTime<Utc>) -> Self {
        let login = LoginScreen::new(options.last_email.as_deref(), options.region, None);
        Self {
            tab: Tab::Activities,
            running: true,
            dirty: true,
            theme,
            activities: ActivitiesScreen::new(options.page_size, options.default_range_days, today),
            options,
            account: None,
            login: Some(login),
            detail: None,
            help: false,
            toasts: Vec::new(),
            ids: RequestIds::default(),
            tick: 0,
            now,
        }
    }

    /// Switch to the signed-in state and start loading.
    pub fn signed_in(&mut self, who: String, region: Region) -> Vec<Command> {
        self.account = Some((who, region));
        self.login = None;
        self.tab = Tab::Activities;
        self.dirty = true;
        vec![self.activities.start(&mut self.ids, false)]
    }

    pub fn set_clock(&mut self, now: DateTime<Utc>, today: NaiveDate) {
        self.now = now;
        self.activities.set_today(today);
    }

    /// Main loop: draw when dirty, wait for input or results, update, dispatch.
    pub async fn run(
        mut self,
        terminal: &mut DefaultTerminal,
        mut events: Events,
        mut results: UnboundedReceiver<Action>,
        service: DataService,
    ) -> Result<()> {
        while self.running {
            if self.dirty {
                terminal.draw(|frame| self.render(frame))?;
                self.dirty = false;
            }
            let action = tokio::select! {
                event = events.next() => self.handle_event(&event),
                Some(action) = results.recv() => Some(action),
            };
            let Some(action) = action else { continue };
            if action == Action::Tick {
                self.set_clock(Utc::now(), Local::now().date_naive());
            } else {
                tracing::debug!(action = action_name(&action), "action");
            }
            for command in self.update(action) {
                service.dispatch(command);
            }
        }
        Ok(())
    }

    pub fn handle_event(&self, event: &Event) -> Option<Action> {
        match event {
            Event::Key(key) => self.map_key(*key),
            Event::Resize(..) => Some(Action::Resize),
            Event::Tick => Some(Action::Tick),
            Event::Terminate => Some(Action::Quit),
        }
    }

    fn map_key(&self, key: KeyEvent) -> Option<Action> {
        if key.kind == KeyEventKind::Release {
            return None;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Some(Action::Quit);
        }
        if self.help {
            return matches!(key.code, KeyCode::Char('?' | 'q') | KeyCode::Esc)
                .then_some(Action::ToggleHelp);
        }
        if let Some(login) = &self.login {
            return login.map_key(key).map(Action::Login);
        }
        if self.tab == Tab::Activities {
            if let Some(detail) = &self.detail {
                if let Some(msg) = detail.map_key(key) {
                    return Some(Action::Detail(msg));
                }
            } else {
                if let Some(msg) = self.activities.map_key(key) {
                    return Some(Action::Activities(msg));
                }
                if self.activities.captures_input() {
                    return None;
                }
            }
        }
        global_key(key)
    }

    pub fn update(&mut self, action: Action) -> Vec<Command> {
        if action == Action::Tick {
            self.tick += 1;
            let before = self.toasts.len();
            self.toasts.retain(|t| t.expires_at > self.tick);
            if self.toasts.len() != before || self.busy() {
                self.dirty = true;
            }
            return Vec::new();
        }
        self.dirty = true;
        match action {
            Action::Tick | Action::Resize => {}
            Action::Quit => self.running = false,
            Action::ToggleHelp => self.help = !self.help,
            Action::SelectTab(tab) if self.login.is_none() => self.tab = tab,
            Action::NextTab if self.login.is_none() => self.tab = self.tab.next(),
            Action::PrevTab if self.login.is_none() => self.tab = self.tab.prev(),
            Action::SelectTab(_) | Action::NextTab | Action::PrevTab => {}

            Action::Login(msg) => {
                if let Some(login) = &mut self.login {
                    return login.update(msg).into_iter().collect();
                }
            }
            Action::LoginSucceeded { who, region } => return self.signed_in(who, region),
            Action::LoginFailed { message } => {
                if let Some(login) = &mut self.login {
                    login.on_failed(message);
                }
            }

            Action::Activities(msg) => match self.activities.update(msg, &mut self.ids) {
                Some(Effect::Command(command)) => return vec![command],
                Some(Effect::Open(summary)) => {
                    let (detail, command) = DetailScreen::open(*summary, &mut self.ids);
                    self.detail = Some(detail);
                    return vec![command];
                }
                None => {}
            },
            Action::Detail(msg) => {
                if let Some(detail) = &mut self.detail {
                    match detail.update(msg, &mut self.ids) {
                        Some(DetailEffect::Back) => self.detail = None,
                        Some(DetailEffect::Command(command)) => return vec![command],
                        None => {}
                    }
                }
            }

            Action::ActivitiesLoaded {
                req,
                page,
                fetched_at,
                from_cache,
                refreshing,
            } => {
                self.activities
                    .on_loaded(req, page, fetched_at, from_cache, refreshing);
            }
            Action::DetailLoaded {
                req,
                detail,
                fetched_at,
                from_cache,
            } => {
                if let Some(screen) = &mut self.detail {
                    screen.on_loaded(req, detail, fetched_at, from_cache);
                }
            }
            Action::RequestFailed { req, error } => {
                let list_had_rows = !self.activities.visible().is_empty();
                let mine_list = self.activities.on_failed(req, error.clone());
                let mine_detail = self
                    .detail
                    .as_mut()
                    .is_some_and(|d| d.on_failed(req, error.clone()));
                if error.kind == UiErrorKind::SessionExpired {
                    self.session_expired(&error.message);
                } else if (mine_list && list_had_rows) || (!mine_list && !mine_detail) {
                    // Errors the screen can't show inline become toasts.
                    self.toast(error.message);
                }
            }
        }
        Vec::new()
    }

    fn session_expired(&mut self, message: &str) {
        if self.login.is_some() {
            return;
        }
        self.account = None;
        self.help = false;
        self.login = Some(LoginScreen::new(
            self.options.last_email.as_deref(),
            self.options.region,
            Some(message.to_owned()),
        ));
    }

    fn toast(&mut self, text: String) {
        self.toasts.push(Toast {
            text,
            expires_at: self.tick + TOAST_TICKS,
        });
        if self.toasts.len() > 3 {
            self.toasts.remove(0);
        }
    }

    /// Something is animating (spinner) and needs redraws on ticks.
    fn busy(&self) -> bool {
        self.login.as_ref().is_some_and(LoginScreen::is_submitting)
            || self.activities.is_loading()
            || self.detail.as_ref().is_some_and(DetailScreen::is_loading)
    }

    fn context(&self) -> Context {
        if self.help {
            Context::Help
        } else if self.login.is_some() {
            Context::Login
        } else if self.tab != Tab::Activities {
            Context::Placeholder
        } else if self.detail.is_some() {
            Context::Detail
        } else if self.activities.has_filter_open() {
            Context::Filter
        } else if self.activities.is_searching() {
            Context::Search
        } else {
            Context::Activities
        }
    }

    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();
        if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
            self.render_too_small(frame, area);
            return;
        }
        let [header, body, footer] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .areas(area);
        self.render_header(frame, header);
        if let Some(login) = &self.login {
            login.render(frame, body, &self.theme, self.tick);
        } else if self.tab == Tab::Activities {
            match &self.detail {
                Some(detail) => detail.render(frame, body, &self.theme, self.tick),
                None => self.activities.render(frame, body, &self.theme, self.tick),
            }
        } else {
            self.render_placeholder(frame, body);
        }
        self.render_footer(frame, footer);
        self.render_toasts(frame, body);
        if self.help {
            self.render_help(frame, body);
        }
    }

    fn render_header(&self, frame: &mut Frame, area: Rect) {
        let full: Vec<String> = Tab::ALL
            .iter()
            .enumerate()
            .map(|(i, t)| format!("{} {}", i + 1, t.title()))
            .collect();
        let (right_long, right_short) = match &self.account {
            Some((who, region)) => {
                let region = region.as_str().to_uppercase();
                (format!("{who} · {region} "), format!("{region} "))
            }
            None => ("not signed in ".to_owned(), String::new()),
        };

        // Shrink in steps: drop the brand, shorten the right text, then show
        // only the active tab's name.
        let titles = if area.width >= tabs_width(&full) + text_width(&right_short) + HEADER_GAP {
            full
        } else {
            Tab::ALL
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    if *t == self.tab {
                        format!("{} {}", i + 1, t.title())
                    } else {
                        (i + 1).to_string()
                    }
                })
                .collect()
        };
        let tabs_w = tabs_width(&titles);
        let right = if area.width >= tabs_w + text_width(&right_long) + HEADER_GAP {
            right_long
        } else {
            right_short
        };
        let right_w = text_width(&right);
        let brand_w = if area.width >= text_width(BRAND) + tabs_w + right_w + HEADER_GAP {
            text_width(BRAND)
        } else {
            0
        };

        let [brand_area, tabs_area, _, right_area] = Layout::horizontal([
            Constraint::Length(brand_w),
            Constraint::Length(tabs_w),
            Constraint::Fill(1),
            Constraint::Length(right_w),
        ])
        .areas(area);

        frame.render_widget(Line::from(BRAND).style(self.theme.accent), brand_area);
        let tabs = Tabs::new(titles)
            .select(self.tab.index())
            .style(self.theme.tab)
            .highlight_style(self.theme.tab_active)
            .divider("│")
            .padding(" ", " ");
        frame.render_widget(tabs, tabs_area);
        frame.render_widget(
            Line::from(right).style(self.theme.muted).right_aligned(),
            right_area,
        );
    }

    fn render_placeholder(&self, frame: &mut Frame, area: Rect) {
        let block = Block::bordered()
            .border_style(self.theme.border)
            .title(Line::from(format!(" {} ", self.tab.title())).style(self.theme.title));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let (what, phase) = self.tab.placeholder();
        crate::widgets::message(
            frame,
            inner,
            vec![
                Line::from(self.tab.title()).style(self.theme.title),
                Line::from(""),
                Line::from(what).style(self.theme.text),
                Line::from(format!("Coming in {phase} · see docs/PLAN.md")).style(self.theme.muted),
            ],
        );
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let freshness = if self.login.is_none() && self.tab == Tab::Activities {
            match &self.detail {
                Some(d) => d.freshness(self.now),
                None => self.activities.freshness(self.now),
            }
        } else {
            None
        };
        let status = if self.login.as_ref().is_some_and(LoginScreen::is_submitting) {
            Span::styled(
                format!(" {} Signing in…", spinner(self.tick)),
                self.theme.accent,
            )
        } else if let Some(age) = freshness {
            Span::styled(format!(" {age}"), self.theme.muted)
        } else if let Some(notice) = &self.options.notice {
            Span::styled(format!(" {notice}"), self.theme.warning)
        } else {
            Span::styled(format!(" v{}", env!("CARGO_PKG_VERSION")), self.theme.muted)
        };

        let status_w = text_width(&status.content);
        let hints = fit_hints(
            &keymap::footer(self.context()),
            area.width.saturating_sub(status_w + 2),
        );
        let [left, right] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(text_width(&hints))])
                .areas(area);
        frame.render_widget(Line::from(status), left);
        frame.render_widget(
            Line::from(hints).style(self.theme.muted).right_aligned(),
            right,
        );
    }

    fn render_toasts(&self, frame: &mut Frame, body: Rect) {
        let mut y = body.y + 1;
        for toast in self.toasts.iter().rev() {
            let width = (text_width(&toast.text) + 4)
                .min(body.width.saturating_sub(4))
                .max(10);
            let height = 3;
            if y + height > body.bottom() || width + 2 > body.width {
                break;
            }
            let rect = Rect::new(body.right().saturating_sub(width + 2), y, width, height);
            frame.render_widget(Clear, rect);
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    toast.text.clone(),
                    self.theme.error,
                )))
                .block(Block::bordered().border_style(self.theme.error))
                .wrap(Wrap { trim: true }),
                rect,
            );
            y += height;
        }
    }

    fn render_help(&self, frame: &mut Frame, area: Rect) {
        let mut lines: Vec<Line<'static>> = Vec::new();
        let section = |title: &str, hints: &[keymap::KeyHint], lines: &mut Vec<Line<'static>>| {
            if hints.is_empty() {
                return;
            }
            lines.push(Line::from(Span::styled(
                format!(" {title}"),
                self.theme.header,
            )));
            for h in hints {
                lines.push(Line::from(vec![
                    Span::styled(format!("   {:<14}", h.keys), self.theme.accent),
                    Span::styled(h.action.to_owned(), self.theme.text),
                ]));
            }
            lines.push(Line::from(""));
        };
        let context = if self.login.is_some() {
            Context::Login
        } else if self.tab != Tab::Activities {
            Context::Placeholder
        } else if self.detail.is_some() {
            Context::Detail
        } else {
            Context::Activities
        };
        section("This screen", keymap::for_context(context), &mut lines);
        if context == Context::Activities {
            section("While searching", keymap::SEARCH, &mut lines);
        }
        section("Everywhere", keymap::GLOBAL, &mut lines);
        lines.push(Line::from(Span::styled(
            " Esc or ? closes this help",
            self.theme.muted,
        )));
        let height = u16::try_from(lines.len())
            .unwrap_or(u16::MAX)
            .saturating_add(2);
        let inner = popup(frame, centered(area, 48, height), "Keys", &self.theme);
        frame.render_widget(Paragraph::new(lines), inner);
    }

    fn render_too_small(&self, frame: &mut Frame, area: Rect) {
        crate::widgets::message(
            frame,
            area,
            vec![
                Line::from("Terminal too small").style(self.theme.warning),
                Line::from(format!(
                    "Need {MIN_WIDTH}×{MIN_HEIGHT}, have {}×{}",
                    area.width, area.height
                )),
                Line::from("Resize, or press Ctrl+C to quit").style(self.theme.muted),
            ],
        );
    }
}

/// Keys that work everywhere a screen doesn't use them.
fn global_key(key: KeyEvent) -> Option<Action> {
    if key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
    {
        return None;
    }
    match key.code {
        KeyCode::Char('q') => Some(Action::Quit),
        KeyCode::Char('?') => Some(Action::ToggleHelp),
        KeyCode::Char(c @ '1'..='9') => c
            .to_digit(10)
            .and_then(Tab::from_number)
            .map(Action::SelectTab),
        KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => Some(Action::NextTab),
        KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => Some(Action::PrevTab),
        _ => None,
    }
}

/// Drop trailing hints (at ` · ` boundaries) until the text fits.
fn fit_hints(hints: &str, width: u16) -> String {
    let mut parts: Vec<&str> = hints.split(" · ").collect();
    while !parts.is_empty() {
        let text = format!("{} ", parts.join(" · "));
        if text_width(&text) <= width {
            return text;
        }
        parts.pop();
    }
    String::new()
}

/// For logs: the variant name only (no data, no secrets).
fn action_name(action: &Action) -> String {
    let debug = format!("{action:?}");
    debug.split(['(', ' ', '{']).next().unwrap_or("").to_owned()
}

fn text_width(s: &str) -> u16 {
    u16::try_from(Line::from(s).width()).unwrap_or(u16::MAX)
}

/// Width of a `Tabs` widget with one space of padding each side and a 1-cell divider.
fn tabs_width(titles: &[String]) -> u16 {
    let cells: u16 = titles.iter().map(|t| text_width(t) + 2).sum();
    let dividers = u16::try_from(titles.len().saturating_sub(1)).unwrap_or(0);
    cells + dividers
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyEventState;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::action::{ActivitiesMsg, DetailMsg, FilterMsg, LoginMsg, Move, RequestId, UiError};
    use crate::testdata;

    fn options() -> AppOptions {
        AppOptions {
            region: Region::Us,
            page_size: 20,
            default_range_days: 90,
            notice: None,
            last_email: None,
        }
    }

    fn signed_out() -> App {
        App::new(
            Theme::plain(),
            options(),
            testdata::today(),
            testdata::now(),
        )
    }

    fn sign_in(app: &mut App) -> RequestId {
        let commands = app.update(Action::LoginSucceeded {
            who: "Runner".into(),
            region: Region::Us,
        });
        let Some(Command::LoadActivities { req, .. }) = commands.into_iter().next() else {
            panic!("no load")
        };
        req
    }

    /// Signed in, with one page of activities loaded (from a 3-minute-old cache).
    fn browsing() -> App {
        let mut app = signed_out();
        let req = sign_in(&mut app);
        app.update(Action::ActivitiesLoaded {
            req,
            page: testdata::page(1, 20, 2),
            fetched_at: testdata::now() - chrono::Duration::minutes(3),
            from_cache: true,
            refreshing: false,
        });
        app
    }

    /// Browsing, with the second activity's detail open and loaded.
    fn in_detail() -> App {
        let mut app = browsing();
        app.update(Action::Activities(ActivitiesMsg::Move(Move::Down)));
        let Some(Command::LoadDetail { req, .. }) = app
            .update(Action::Activities(ActivitiesMsg::Open))
            .into_iter()
            .next()
        else {
            panic!("open should load the detail");
        };
        app.update(Action::DetailLoaded {
            req,
            detail: Box::new(testdata::detail()),
            fetched_at: testdata::now(),
            from_cache: false,
        });
        app
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn press(app: &mut App, code: KeyCode) -> Vec<Command> {
        match app.handle_event(&Event::Key(key(code))) {
            Some(action) => app.update(action),
            None => Vec::new(),
        }
    }

    fn draw(app: &App, width: u16, height: u16) -> Terminal<TestBackend> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| app.render(frame)).unwrap();
        terminal
    }

    #[test]
    fn tab_numbers_and_cycling() {
        assert_eq!(Tab::from_number(1), Some(Tab::Dashboard));
        assert_eq!(Tab::from_number(0), None);
        assert_eq!(Tab::Workouts.next(), Tab::Dashboard);
        for tab in Tab::ALL {
            assert_eq!(tab.next().prev(), tab);
        }
    }

    #[test]
    fn starts_on_login_and_only_ctrl_c_quits_there() {
        let mut app = signed_out();
        assert_eq!(app.context(), Context::Login);
        press(&mut app, KeyCode::Char('q'));
        assert!(app.running, "q is typed into the email field");
        let quit = app.handle_event(&Event::Key(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        )));
        assert_eq!(quit, Some(Action::Quit));
    }

    #[test]
    fn login_flow() {
        let mut app = signed_out();
        for c in "a@b.c".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Tab);
        for c in "pw".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        let commands = press(&mut app, KeyCode::Enter);
        assert!(matches!(&commands[..], [Command::Login { .. }]));
        assert!(app.busy(), "spinner while signing in");

        app.update(Action::LoginFailed {
            message: "Incorrect password".into(),
        });
        assert!(app.login.is_some() && !app.busy());

        let commands = app.update(Action::LoginSucceeded {
            who: "Runner".into(),
            region: Region::Us,
        });
        assert!(app.login.is_none());
        assert_eq!(app.tab, Tab::Activities);
        assert!(matches!(&commands[..], [Command::LoadActivities { .. }]));
    }

    #[test]
    fn opening_and_leaving_an_activity() {
        let mut app = in_detail();
        assert_eq!(app.context(), Context::Detail);
        press(&mut app, KeyCode::Tab);
        assert_eq!(
            app.tab,
            Tab::Activities,
            "Tab switches detail tabs, not app tabs"
        );
        press(&mut app, KeyCode::Esc);
        assert!(app.detail.is_none());
        assert_eq!(app.context(), Context::Activities);
    }

    #[test]
    fn search_swallows_global_keys() {
        let mut app = browsing();
        press(&mut app, KeyCode::Char('/'));
        assert_eq!(app.context(), Context::Search);
        press(&mut app, KeyCode::Char('q'));
        press(&mut app, KeyCode::Char('3'));
        assert!(
            app.running && app.tab == Tab::Activities,
            "typed into search instead"
        );
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.context(), Context::Activities);
    }

    #[test]
    fn expired_session_returns_to_login_with_a_notice() {
        let mut app = browsing();
        let Some(Command::LoadActivities { req, .. }) =
            press(&mut app, KeyCode::Char('r')).into_iter().next()
        else {
            panic!()
        };
        app.update(Action::RequestFailed {
            req,
            error: UiError {
                kind: UiErrorKind::SessionExpired,
                message: "Your session expired. Sign in again.".into(),
            },
        });
        assert_eq!(app.context(), Context::Login);
        assert!(app.account.is_none());
    }

    #[test]
    fn errors_with_data_on_screen_become_toasts_that_expire() {
        let mut app = browsing();
        let Some(Command::LoadActivities { req, .. }) =
            press(&mut app, KeyCode::Char('r')).into_iter().next()
        else {
            panic!()
        };
        app.update(Action::RequestFailed {
            req,
            error: UiError::other("COROS answered HTTP 502. Try again later."),
        });
        assert_eq!(app.toasts.len(), 1);
        for _ in 0..TOAST_TICKS {
            app.update(Action::Tick);
        }
        assert!(app.toasts.is_empty());
    }

    #[test]
    fn stale_results_are_ignored() {
        let mut app = browsing();
        let before = app.activities.visible().len();
        app.update(Action::ActivitiesLoaded {
            req: RequestId(999),
            page: testdata::page(1, 3, 1),
            fetched_at: testdata::now(),
            from_cache: false,
            refreshing: false,
        });
        assert_eq!(app.activities.visible().len(), before);
        assert!(app.toasts.is_empty());
    }

    #[test]
    fn help_overlay_captures_keys() {
        let mut app = browsing();
        press(&mut app, KeyCode::Char('?'));
        assert_eq!(app.context(), Context::Help);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char('2'));
        assert!(app.help);
        press(&mut app, KeyCode::Esc);
        assert!(!app.help);
    }

    #[test]
    fn idle_ticks_do_not_redraw() {
        let mut app = browsing();
        app.dirty = false;
        app.update(Action::Tick);
        assert!(!app.dirty);
        press(&mut app, KeyCode::Char('r'));
        app.dirty = false;
        app.update(Action::Tick);
        assert!(app.dirty, "spinner animates while loading");
    }

    #[test]
    fn key_release_is_ignored() {
        let app = browsing();
        let release = KeyEvent {
            code: KeyCode::Char('q'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Release,
            state: KeyEventState::NONE,
        };
        assert_eq!(app.handle_event(&Event::Key(release)), None);
    }

    /// Every key listed in the help (keymap.rs) must do something on its screen.
    #[test]
    fn documented_keys_are_bound() {
        fn keys(spec: &str) -> Vec<KeyEvent> {
            let k = |code| KeyEvent::new(code, KeyModifiers::NONE);
            spec.split_whitespace()
                .flat_map(|token| -> Vec<KeyEvent> {
                    match token {
                        "Enter" => vec![k(KeyCode::Enter)],
                        "Esc" => vec![k(KeyCode::Esc)],
                        "Tab" => vec![k(KeyCode::Tab)],
                        "PgUp" => vec![k(KeyCode::PageUp)],
                        "PgDn" => vec![k(KeyCode::PageDown)],
                        "Space" => vec![k(KeyCode::Char(' '))],
                        "Shift+Tab" => vec![KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT)],
                        "Ctrl+U" => vec![KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)],
                        "Ctrl+C" => vec![KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)],
                        "1–5" => vec![k(KeyCode::Char('1')), k(KeyCode::Char('5'))],
                        "type" => vec![k(KeyCode::Char('x'))],
                        other => other
                            .chars()
                            .map(|c| match c {
                                '↑' => k(KeyCode::Up),
                                '↓' => k(KeyCode::Down),
                                '←' => k(KeyCode::Left),
                                '→' => k(KeyCode::Right),
                                c => k(KeyCode::Char(c)),
                            })
                            .collect(),
                    }
                })
                .collect()
        }
        let check = |app: &App, hints: &[keymap::KeyHint]| {
            for hint in hints {
                for key in keys(hint.keys) {
                    assert!(
                        app.map_key(key).is_some(),
                        "{:?} ({}) does nothing",
                        key.code,
                        hint.action
                    );
                }
            }
        };
        let mut login = signed_out();
        check(&login, keymap::LOGIN);
        login.update(Action::Login(LoginMsg::NextField));
        login.update(Action::Login(LoginMsg::NextField)); // region: ← → change it
        check(&login, &keymap::LOGIN[3..4]);
        let mut app = browsing();
        check(&app, keymap::ACTIVITIES);
        check(&app, keymap::GLOBAL);
        app.update(Action::Activities(ActivitiesMsg::StartSearch));
        check(&app, keymap::SEARCH);
        app.update(Action::Activities(ActivitiesMsg::EndSearch(false)));
        app.update(Action::Activities(ActivitiesMsg::OpenFilter));
        check(&app, keymap::FILTER);
        check(&in_detail(), keymap::DETAIL);
    }

    // ----- render snapshots (plain theme; styles don't show in text snapshots) -----

    #[test]
    fn snapshot_login() {
        insta::assert_snapshot!("login_80x24", draw(&signed_out(), 80, 24).backend());
        let mut app = signed_out();
        app.update(Action::Login(LoginMsg::Submit));
        insta::assert_snapshot!("login_missing_fields_60x20", draw(&app, 60, 20).backend());
        let mut app = signed_out();
        app.login = Some(LoginScreen::new(
            Some("runner@example.com"),
            Region::Us,
            Some("Your session expired. Sign in again.".into()),
        ));
        app.update(Action::LoginFailed {
            message: "could not sign in.\n  us: COROS returned error 1030 on account/login: Incorrect password".into(),
        });
        insta::assert_snapshot!(
            "login_expired_and_failed_100x30",
            draw(&app, 100, 30).backend()
        );
    }

    #[test]
    fn snapshot_activities() {
        let app = browsing();
        insta::assert_snapshot!("activities_100x30", draw(&app, 100, 30).backend());
        insta::assert_snapshot!("activities_80x24", draw(&app, 80, 24).backend());
        insta::assert_snapshot!("activities_60x15", draw(&app, 60, 15).backend());
    }

    #[test]
    fn snapshot_activities_states() {
        let mut loading = signed_out();
        sign_in(&mut loading);
        insta::assert_snapshot!("activities_loading", draw(&loading, 80, 24).backend());

        let mut empty = signed_out();
        let req = sign_in(&mut empty);
        empty.update(Action::ActivitiesLoaded {
            req,
            page: testdata::page(1, 0, 0),
            fetched_at: testdata::now(),
            from_cache: false,
            refreshing: false,
        });
        insta::assert_snapshot!("activities_empty", draw(&empty, 80, 24).backend());

        let mut failed = signed_out();
        let req = sign_in(&mut failed);
        failed.update(Action::RequestFailed {
            req,
            error: UiError {
                kind: UiErrorKind::Network,
                message: "Can't reach COROS. Check your connection and retry.".into(),
            },
        });
        insta::assert_snapshot!("activities_error", draw(&failed, 80, 24).backend());
    }

    #[test]
    fn snapshot_search_filter_help_and_sort() {
        let mut app = browsing();
        press(&mut app, KeyCode::Char('/'));
        for c in "inter".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        insta::assert_snapshot!("activities_search", draw(&app, 100, 30).backend());

        let mut app = browsing();
        press(&mut app, KeyCode::Char('s'));
        press(&mut app, KeyCode::Char('f'));
        app.update(Action::Activities(ActivitiesMsg::Filter(FilterMsg::Move(
            Move::Down,
        ))));
        insta::assert_snapshot!(
            "activities_sorted_with_filter_open",
            draw(&app, 100, 30).backend()
        );

        let mut app = browsing();
        press(&mut app, KeyCode::Char('?'));
        insta::assert_snapshot!("help_activities", draw(&app, 100, 30).backend());
    }

    #[test]
    fn snapshot_detail() {
        let mut app = in_detail();
        insta::assert_snapshot!("detail_summary_100x30", draw(&app, 100, 30).backend());
        insta::assert_snapshot!("detail_summary_80x24", draw(&app, 80, 24).backend());
        app.update(Action::Detail(DetailMsg::NextTab));
        insta::assert_snapshot!("detail_laps_100x30", draw(&app, 100, 30).backend());
        app.update(Action::Detail(DetailMsg::CycleLapGroup));
        app.update(Action::Detail(DetailMsg::CycleLapGroup));
        insta::assert_snapshot!("detail_km_laps_80x24", draw(&app, 80, 24).backend());

        let mut loading = browsing();
        loading.update(Action::Activities(ActivitiesMsg::Open));
        insta::assert_snapshot!("detail_loading_80x24", draw(&loading, 80, 24).backend());
    }

    #[test]
    fn snapshot_placeholder_tab_and_too_small() {
        let mut app = browsing();
        press(&mut app, KeyCode::Char('1'));
        insta::assert_snapshot!("dashboard_placeholder_80x24", draw(&app, 80, 24).backend());
        insta::assert_snapshot!("too_small_40x12", draw(&app, 40, 12).backend());
    }

    #[test]
    fn never_panics_at_any_size() {
        let mut search = browsing();
        search.update(Action::Activities(ActivitiesMsg::StartSearch));
        let mut filter = browsing();
        filter.update(Action::Activities(ActivitiesMsg::OpenFilter));
        let mut laps = in_detail();
        laps.update(Action::Detail(DetailMsg::NextTab));
        let mut help = browsing();
        help.update(Action::ToggleHelp);
        let mut toast = browsing();
        toast.toast("A long error message that has to wrap or be cut somewhere sensible".into());
        let apps = [
            signed_out(),
            browsing(),
            search,
            filter,
            in_detail(),
            laps,
            help,
            toast,
        ];
        for app in &apps {
            for width in (0..=140).step_by(7) {
                for height in (0..=45).step_by(4) {
                    draw(app, width, height);
                }
            }
        }
    }

    #[test]
    fn hints_shrink_to_fit() {
        assert_eq!(fit_hints("a 1 · b 2 · c 3", 100), "a 1 · b 2 · c 3 ");
        assert_eq!(fit_hints("a 1 · b 2 · c 3", 10), "a 1 · b 2 ");
        assert_eq!(fit_hints("a 1 · b 2", 2), "");
    }

    #[test]
    fn logged_action_names_carry_no_data() {
        let typed = Action::Login(LoginMsg::Edit(crate::action::Edit::Insert('x')));
        assert_eq!(action_name(&typed), "Login");
        assert_eq!(
            action_name(&Action::LoginFailed {
                message: "secret-ish".into()
            }),
            "LoginFailed"
        );
    }
}
