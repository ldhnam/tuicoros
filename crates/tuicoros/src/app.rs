//! Top-level app state, input mapping and rendering.
//!
//! `update` is synchronous and does no I/O (ADR-004), so it is tested without
//! a terminal. Rendering is tested with ratatui's `TestBackend` + insta snapshots.

use color_eyre::Result;
use coros_api::Region;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::DefaultTerminal;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph, Tabs, Wrap};

use crate::action::Action;
use crate::event::{Event, Events};
use crate::theme::Theme;

/// Below this size we show a "terminal too small" message instead of the UI.
pub const MIN_WIDTH: u16 = 60;
pub const MIN_HEIGHT: u16 = 15;

const BRAND: &str = " tuicoros ";
const KEY_HINTS: &str = "1-5 tab  ←→ switch  q quit ";
/// Minimum empty cells between the header's tabs and its right-hand text.
const HEADER_GAP: u16 = 2;

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

    /// Placeholder text until the real screen exists: (what it will show, phase).
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

pub struct App {
    tab: Tab,
    running: bool,
    /// Set when state changed and the screen needs a redraw.
    dirty: bool,
    region: Region,
    theme: Theme,
    /// One-line message for the footer (e.g. a config warning).
    notice: Option<String>,
}

impl App {
    pub fn new(region: Region, theme: Theme, notice: Option<String>) -> Self {
        Self {
            tab: Tab::default(),
            running: true,
            dirty: true,
            region,
            theme,
            notice,
        }
    }

    /// Main loop: draw when dirty, wait for the next event, update.
    pub async fn run(mut self, terminal: &mut DefaultTerminal, mut events: Events) -> Result<()> {
        while self.running {
            if self.dirty {
                terminal.draw(|frame| self.render(frame))?;
                self.dirty = false;
            }
            let event = events.next().await;
            if let Some(action) = self.handle_event(&event) {
                tracing::debug!(?action, "action");
                self.update(action);
            }
        }
        Ok(())
    }

    pub fn handle_event(&self, event: &Event) -> Option<Action> {
        match event {
            Event::Key(key) => map_key(*key),
            Event::Resize(..) => Some(Action::Resize),
            Event::Tick => None,
            Event::Terminate => Some(Action::Quit),
        }
    }

    pub fn update(&mut self, action: Action) {
        match action {
            Action::Quit => self.running = false,
            Action::SelectTab(tab) => self.set_tab(tab),
            Action::NextTab => self.set_tab(self.tab.next()),
            Action::PrevTab => self.set_tab(self.tab.prev()),
            Action::Resize => self.dirty = true,
        }
    }

    fn set_tab(&mut self, tab: Tab) {
        if self.tab != tab {
            self.tab = tab;
            self.dirty = true;
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
        self.render_body(frame, body);
        self.render_footer(frame, footer);
    }

    fn render_header(&self, frame: &mut Frame, area: Rect) {
        let full: Vec<String> = Tab::ALL
            .iter()
            .enumerate()
            .map(|(i, t)| format!("{} {}", i + 1, t.title()))
            .collect();
        let region = self.region.as_str().to_uppercase();
        let right_long = format!("not signed in · {region} ");
        let right_short = format!("{region} ");

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

    fn render_body(&self, frame: &mut Frame, area: Rect) {
        let block = Block::bordered()
            .border_style(self.theme.border)
            .title(Line::from(format!(" {} ", self.tab.title())).style(self.theme.title));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let (what, phase) = self.tab.placeholder();
        let lines = vec![
            Line::from(self.tab.title()).style(self.theme.title),
            Line::from(""),
            Line::from(what).style(self.theme.text),
            Line::from(format!("Coming in {phase} · see docs/PLAN.md")).style(self.theme.muted),
        ];
        let height = u16::try_from(lines.len()).unwrap_or(u16::MAX);
        let [_, middle, _] = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(height),
            Constraint::Fill(1),
        ])
        .areas(inner);
        frame.render_widget(
            Paragraph::new(lines).centered().wrap(Wrap { trim: true }),
            middle,
        );
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let hints_w = text_width(KEY_HINTS);
        let [left, right] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(hints_w)]).areas(area);
        let status = match &self.notice {
            Some(notice) => Line::from(format!(" {notice}")).style(self.theme.warning),
            None => Line::from(format!(" v{}", env!("CARGO_PKG_VERSION"))).style(self.theme.muted),
        };
        frame.render_widget(status, left);
        frame.render_widget(
            Line::from(KEY_HINTS)
                .style(self.theme.muted)
                .right_aligned(),
            right,
        );
    }

    fn render_too_small(&self, frame: &mut Frame, area: Rect) {
        let lines = vec![
            Line::from("Terminal too small").style(self.theme.warning),
            Line::from(format!(
                "Need {MIN_WIDTH}×{MIN_HEIGHT}, have {}×{}",
                area.width, area.height
            )),
            Line::from("Resize, or press q to quit").style(self.theme.muted),
        ];
        let [_, middle, _] = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(3),
            Constraint::Fill(1),
        ])
        .areas(area);
        frame.render_widget(
            Paragraph::new(lines).centered().wrap(Wrap { trim: true }),
            middle,
        );
    }
}

/// Global key bindings. Screen-specific bindings will be layered on top (Phase 2).
pub fn map_key(key: KeyEvent) -> Option<Action> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return (key.code == KeyCode::Char('c')).then_some(Action::Quit);
    }
    if key.modifiers.contains(KeyModifiers::ALT) {
        return None;
    }
    match key.code {
        KeyCode::Char('q') => Some(Action::Quit),
        KeyCode::Char(c @ '1'..='9') => c
            .to_digit(10)
            .and_then(Tab::from_number)
            .map(Action::SelectTab),
        KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => Some(Action::NextTab),
        KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => Some(Action::PrevTab),
        _ => None,
    }
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

    fn app() -> App {
        App::new(Region::Asia, Theme::plain(), None)
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn draw(app: &App, width: u16, height: u16) -> Terminal<TestBackend> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| app.render(frame)).unwrap();
        terminal
    }

    #[test]
    fn tab_numbers_and_cycling() {
        assert_eq!(Tab::from_number(1), Some(Tab::Dashboard));
        assert_eq!(Tab::from_number(5), Some(Tab::Workouts));
        assert_eq!(Tab::from_number(0), None);
        assert_eq!(Tab::from_number(6), None);
        assert_eq!(Tab::Workouts.next(), Tab::Dashboard);
        assert_eq!(Tab::Dashboard.prev(), Tab::Workouts);
        for tab in Tab::ALL {
            assert_eq!(tab.next().prev(), tab);
            assert_eq!(
                Tab::from_number(u32::try_from(tab.index()).unwrap() + 1),
                Some(tab)
            );
        }
    }

    #[test]
    fn key_mapping() {
        assert_eq!(map_key(key(KeyCode::Char('q'))), Some(Action::Quit));
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(Action::Quit)
        );
        assert_eq!(map_key(key(KeyCode::Char('c'))), None);
        assert_eq!(
            map_key(key(KeyCode::Char('3'))),
            Some(Action::SelectTab(Tab::Analysis))
        );
        assert_eq!(map_key(key(KeyCode::Char('9'))), None);
        assert_eq!(map_key(key(KeyCode::Tab)), Some(Action::NextTab));
        assert_eq!(map_key(key(KeyCode::Char('l'))), Some(Action::NextTab));
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT)),
            Some(Action::PrevTab)
        );
        assert_eq!(map_key(key(KeyCode::Left)), Some(Action::PrevTab));
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::ALT)),
            None
        );
    }

    #[test]
    fn key_release_is_ignored() {
        let release = KeyEvent {
            code: KeyCode::Char('q'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Release,
            state: KeyEventState::NONE,
        };
        assert_eq!(map_key(release), None);
    }

    #[test]
    fn events_map_to_actions() {
        let app = app();
        assert_eq!(app.handle_event(&Event::Tick), None);
        assert_eq!(
            app.handle_event(&Event::Resize(80, 24)),
            Some(Action::Resize)
        );
        assert_eq!(app.handle_event(&Event::Terminate), Some(Action::Quit));
        assert_eq!(
            app.handle_event(&Event::Key(key(KeyCode::Char('2')))),
            Some(Action::SelectTab(Tab::Activities))
        );
    }

    #[test]
    fn update_changes_state_and_marks_dirty_only_on_change() {
        let mut app = app();
        app.dirty = false;

        app.update(Action::SelectTab(Tab::Dashboard));
        assert!(!app.dirty, "selecting the current tab should not redraw");

        app.update(Action::NextTab);
        assert_eq!(app.tab, Tab::Activities);
        assert!(app.dirty);

        app.dirty = false;
        app.update(Action::PrevTab);
        app.update(Action::PrevTab);
        assert_eq!(app.tab, Tab::Workouts);

        app.dirty = false;
        app.update(Action::Resize);
        assert!(app.dirty);

        assert!(app.running);
        app.update(Action::Quit);
        assert!(!app.running);
    }

    #[test]
    fn snapshot_dashboard_100x30() {
        insta::assert_snapshot!(draw(&app(), 100, 30).backend());
    }

    #[test]
    fn snapshot_every_tab_80x24() {
        for tab in Tab::ALL {
            let mut app = app();
            app.update(Action::SelectTab(tab));
            insta::assert_snapshot!(
                format!("tab_{}_80x24", tab.title().to_lowercase()),
                draw(&app, 80, 24).backend()
            );
        }
    }

    #[test]
    fn snapshot_minimum_size_60x15() {
        let mut app = app();
        app.update(Action::SelectTab(Tab::Activities));
        insta::assert_snapshot!(draw(&app, MIN_WIDTH, MIN_HEIGHT).backend());
    }

    #[test]
    fn snapshot_too_small_40x12() {
        insta::assert_snapshot!(draw(&app(), 40, 12).backend());
    }

    #[test]
    fn snapshot_footer_notice() {
        let app = App::new(
            Region::Us,
            Theme::plain(),
            Some("config: unknown key `colour` (ignored)".into()),
        );
        insta::assert_snapshot!(draw(&app, 80, 24).backend());
    }

    #[test]
    fn never_panics_at_any_size() {
        for width in (0..=130).step_by(7) {
            for height in (0..=40).step_by(3) {
                for tab in Tab::ALL {
                    let mut app = app();
                    app.update(Action::SelectTab(tab));
                    draw(&app, width, height);
                }
            }
        }
    }
}
