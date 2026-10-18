//! Dashboard (docs/UI_DESIGN.md §2.2): today at a glance.

use chrono::{Datelike, NaiveDate};
use coros_api::{
    ActivitySummary, Daily, DailyMetrics, Dashboard, DateRange, Schedule, SportCategory,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Block, Chart, Dataset, GraphType, Paragraph};

use super::{LoadMeta, Slot};
use crate::action::{DashboardMsg, Fetched, Move, RequestId, RequestIds, UiError};
use crate::charts::{labels, padded_bounds};
use crate::format;
use crate::screens::activities::sport_name;
use crate::service::{Command, FetchKind};
use crate::theme::Theme;
use crate::widgets::{bar, spinner};

/// Days of load history on the dashboard chart.
const LOAD_DAYS: u32 = 28;
const RECENT: usize = 5;

pub enum DashEffect {
    Commands(Vec<Command>),
    Open(Box<ActivitySummary>),
}

#[derive(Debug)]
pub struct DashboardScreen {
    today: NaiveDate,
    started: bool,
    dashboard: Slot<Dashboard>,
    daily: Slot<Daily>,
    week: Slot<Schedule>,
    selected: usize,
}

/// Monday of the week containing `day`.
pub fn week_start(day: NaiveDate) -> NaiveDate {
    day - chrono::Duration::days(i64::from(day.weekday().num_days_from_monday()))
}

impl DashboardScreen {
    pub fn new(today: NaiveDate) -> Self {
        Self {
            today,
            started: false,
            dashboard: Slot::default(),
            daily: Slot::default(),
            week: Slot::default(),
            selected: 0,
        }
    }

    pub fn set_today(&mut self, today: NaiveDate) {
        self.today = today;
    }

    fn this_week(&self) -> DateRange {
        let start = week_start(self.today);
        DateRange {
            start,
            end: start + chrono::Duration::days(6),
        }
    }

    /// Load everything (first visit, or `r`).
    pub fn start(&mut self, ids: &mut RequestIds, force: bool) -> Vec<Command> {
        self.started = true;
        let (dash, daily, week) = (ids.next(), ids.next(), ids.next());
        self.dashboard.request(dash);
        self.daily.request(daily);
        self.week.request(week);
        vec![
            Command::Fetch {
                req: dash,
                what: FetchKind::Dashboard,
                force,
            },
            Command::Fetch {
                req: daily,
                what: FetchKind::Daily(DateRange::last_days(self.today, LOAD_DAYS)),
                force,
            },
            Command::Fetch {
                req: week,
                what: FetchKind::Schedule(self.this_week()),
                force,
            },
        ]
    }

    /// Commands for a first visit; nothing if already loaded.
    pub fn on_enter(&mut self, ids: &mut RequestIds) -> Vec<Command> {
        if self.started {
            Vec::new()
        } else {
            self.start(ids, false)
        }
    }

    pub fn is_loading(&self) -> bool {
        self.dashboard.is_loading() || self.daily.is_loading() || self.week.is_loading()
    }

    pub fn freshness(&self, now: chrono::DateTime<chrono::Utc>) -> Option<String> {
        self.dashboard.freshness(now)
    }

    /// Take `data` if one of our requests asked for it; otherwise hand it back.
    pub fn offer(
        &mut self,
        req: RequestId,
        data: Box<Fetched>,
        meta: LoadMeta,
    ) -> Option<Box<Fetched>> {
        match *data {
            Fetched::Dashboard(d) if self.dashboard.owns(req) => {
                self.dashboard.accept(req, d, meta)
            }
            Fetched::Daily(d) if self.daily.owns(req) => self.daily.accept(req, d, meta),
            Fetched::Schedule(s) if self.week.owns(req) => self.week.accept(req, s, meta),
            other => return Some(Box::new(other)),
        };
        None
    }

    pub fn on_failed(&mut self, req: RequestId, error: &UiError) -> bool {
        self.dashboard.fail(req, error.clone())
            || self.daily.fail(req, error.clone())
            || self.week.fail(req, error.clone())
    }

    pub fn map_key(&self, key: KeyEvent) -> Option<DashboardMsg> {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return None;
        }
        Some(match key.code {
            KeyCode::Up | KeyCode::Char('k') => DashboardMsg::Move(Move::Up),
            KeyCode::Down | KeyCode::Char('j') => DashboardMsg::Move(Move::Down),
            KeyCode::Enter => DashboardMsg::Open,
            KeyCode::Char('r') => DashboardMsg::Refresh,
            _ => return None,
        })
    }

    pub fn update(
        &mut self,
        msg: DashboardMsg,
        recent: &[&ActivitySummary],
        ids: &mut RequestIds,
    ) -> Option<DashEffect> {
        match msg {
            DashboardMsg::Move(m) => {
                let last = recent.len().min(RECENT).saturating_sub(1);
                self.selected = match m {
                    Move::Up => self.selected.saturating_sub(1),
                    Move::Down => (self.selected + 1).min(last),
                    _ => self.selected,
                };
                None
            }
            DashboardMsg::Open => recent
                .get(self.selected)
                .map(|a| DashEffect::Open(Box::new((*a).clone()))),
            DashboardMsg::Refresh => Some(DashEffect::Commands(self.start(ids, true))),
        }
    }

    /// The newest day with EvoLab values.
    fn latest(&self) -> Option<&DailyMetrics> {
        self.daily
            .value()?
            .days
            .iter()
            .rev()
            .find(|d| d.acute_load.is_some() || d.vo2max.is_some())
    }

    pub fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        theme: &Theme,
        tick: u64,
        recent: &[&ActivitySummary],
    ) {
        let title = format!(" Dashboard · {} ", self.today.format("%a %d %b %Y"));
        let block = Block::bordered()
            .border_style(theme.border)
            .title(Line::from(title).style(theme.title));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let [tiles, middle, bottom] = Layout::vertical([
            Constraint::Length(4),
            Constraint::Fill(3),
            Constraint::Fill(2),
        ])
        .areas(inner);
        self.render_tiles(frame, tiles, theme, tick);

        let side = match inner.width {
            90.. => 36,
            76.. => 34,
            _ => 30,
        };
        let [load, week] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(side)]).areas(middle);
        self.render_load(frame, load, theme, tick);
        self.render_week(frame, week, theme, tick);
        let [recent_area, abilities] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(side)]).areas(bottom);
        self.render_recent(frame, recent_area, theme, recent);
        self.render_abilities(frame, abilities, theme, tick);
    }

    fn render_tiles(&self, frame: &mut Frame, area: Rect, theme: &Theme, tick: u64) {
        let d = self.dashboard.value();
        let day = self.latest();
        if d.is_none() && day.is_none() {
            let text = match (self.dashboard.error(), self.is_loading()) {
                (Some(e), _) => Span::styled(format!(" {}", e.message), theme.error),
                (None, true) => Span::styled(
                    format!(" {} Loading today's numbers…", spinner(tick)),
                    theme.accent,
                ),
                _ => Span::raw(""),
            };
            frame.render_widget(Line::from(text), area);
            return;
        }
        let meter = |zone: Option<i64>| match zone {
            Some(z @ 1..=5) => format!(
                "{}{} zone {z}/5",
                "■".repeat(z as usize),
                "□".repeat(5 - z as usize)
            ),
            _ => String::new(),
        };
        let hrv = d.and_then(|d| d.hrv.as_ref());
        let (hrv_value, hrv_sub) = match hrv {
            Some(h) if h.latest.is_some() => (
                format!("{:.0} ms", h.latest.as_ref().map_or(0.0, |n| n.avg)),
                format!("base {}", format::opt_f(h.baseline, 0)),
            ),
            Some(h) if h.remaining_wear_days.is_some() => {
                let n = h.remaining_wear_days.unwrap_or(0);
                (
                    "—".to_owned(),
                    format!("{n} more night{}", if n == 1 { "" } else { "s" }),
                )
            }
            _ => ("—".to_owned(), String::new()),
        };
        // Most important first: narrow terminals drop tiles from the end.
        let tiles: Vec<(&str, String, String)> = vec![
            (
                "Recovery",
                format!("{}%", format::opt_f(d.and_then(|d| d.recovery_pct), 0)),
                d.and_then(|d| d.full_recovery_hours)
                    .map_or_else(String::new, |h| format!("full in {h:.0} h")),
            ),
            (
                "Fatigue",
                format::opt_f(day.and_then(|d| d.fatigue), 0),
                meter(day.and_then(|d| d.fatigue_state)),
            ),
            (
                "Load ratio",
                format::opt_f(day.and_then(|d| d.load_ratio), 2),
                meter(day.and_then(|d| d.load_ratio_state)),
            ),
            (
                "Resting HR",
                format!("{} bpm", format::opt(d.and_then(|d| d.resting_hr))),
                String::new(),
            ),
            ("HRV", hrv_value, hrv_sub),
            (
                "VO2max",
                format::opt_f(day.and_then(|d| d.vo2max), 0),
                String::new(),
            ),
            (
                "Threshold",
                format!("{} bpm", format::opt(d.and_then(|d| d.lthr))),
                format::pace(d.and_then(|d| d.lt_pace), SportCategory::Run),
            ),
        ];
        let count = usize::from(area.width / 15).clamp(1, tiles.len());
        let areas = Layout::horizontal(vec![Constraint::Fill(1); count]).split(area);
        for ((label, value, sub), rect) in tiles.into_iter().zip(areas.iter()) {
            let lines = vec![
                Line::from(Span::styled(format!(" {label}"), theme.muted)),
                Line::from(Span::styled(format!(" {value}"), theme.title)),
                Line::from(Span::styled(format!(" {sub}"), theme.muted)),
            ];
            frame.render_widget(Paragraph::new(lines), *rect);
        }
    }

    fn render_load(&self, frame: &mut Frame, area: Rect, theme: &Theme, tick: u64) {
        let block = Block::bordered()
            .border_style(theme.border)
            .title(load_title("Training load · 28 days", theme));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let Some(daily) = self.daily.value() else {
            return placeholder(
                frame,
                inner,
                &self.daily,
                theme,
                tick,
                "Loading training load…",
            );
        };
        let first = DateRange::last_days(self.today, LOAD_DAYS).start;
        let x = |d: NaiveDate| (d - first).num_days() as f64;
        let acute: Vec<(f64, f64)> = daily
            .days
            .iter()
            .filter_map(|d| Some((x(d.date), d.acute_load?)))
            .collect();
        let chronic: Vec<(f64, f64)> = daily
            .days
            .iter()
            .filter_map(|d| Some((x(d.date), d.chronic_load?)))
            .collect();
        if acute.is_empty() && chronic.is_empty() {
            return frame.render_widget(
                Line::from(Span::styled(
                    " No EvoLab data yet: keep training with the watch.",
                    theme.muted,
                )),
                inner,
            );
        }
        load_chart(
            frame,
            inner,
            &acute,
            &chronic,
            first,
            f64::from(LOAD_DAYS - 1),
            theme,
        );
    }

    fn render_week(&self, frame: &mut Frame, area: Rect, theme: &Theme, tick: u64) {
        let block = Block::bordered()
            .border_style(theme.border)
            .title(Line::from(" This week ").style(theme.header));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let Some(schedule) = self.week.value() else {
            return placeholder(frame, inner, &self.week, theme, tick, "Loading this week…");
        };
        let start = week_start(self.today);
        let Some(week) = schedule.weeks.iter().find(|w| w.week_start == start) else {
            return frame.render_widget(
                Line::from(Span::styled(" No training plan this week.", theme.muted)),
                inner,
            );
        };
        let bar_w = inner.width.saturating_sub(24).max(4);
        let row = |label: &str,
                   plan: Option<f64>,
                   done: Option<f64>,
                   fmt: &dyn Fn(Option<f64>) -> String| {
            let fraction = match (plan, done) {
                (Some(p), Some(d)) if p > 0.0 => d / p,
                _ => 0.0,
            };
            vec![
                Line::from(vec![
                    Span::styled(format!(" {label:<9}"), theme.muted),
                    Span::styled(fmt(done), theme.title),
                    Span::styled(format!(" / {}", fmt(plan)), theme.muted),
                ]),
                Line::from(vec![
                    Span::raw(" "),
                    bar(fraction.min(1.0), bar_w, theme.accent),
                    Span::styled(
                        if plan.is_some_and(|p| p > 0.0) {
                            format!(" {:.0}%", fraction * 100.0)
                        } else {
                            String::new()
                        },
                        theme.muted,
                    ),
                ]),
            ]
        };
        let km = |m: Option<f64>| format::distance(m.map(coros_api::Meters));
        let dur = |s: Option<f64>| format::duration(s.map(std::time::Duration::from_secs_f64));
        let load = |v: Option<f64>| format::opt_f(v, 0);
        let mut lines = Vec::new();
        lines.extend(row(
            "Distance",
            week.plan_distance.map(|m| m.0),
            week.actual_distance.map(|m| m.0),
            &km,
        ));
        lines.extend(row(
            "Time",
            week.plan_duration.map(|d| d.as_secs_f64()),
            week.actual_duration.map(|d| d.as_secs_f64()),
            &dur,
        ));
        lines.extend(row("Load", week.plan_load, week.actual_load, &load));
        frame.render_widget(Paragraph::new(lines), inner);
    }

    fn render_recent(
        &self,
        frame: &mut Frame,
        area: Rect,
        theme: &Theme,
        recent: &[&ActivitySummary],
    ) {
        let block = Block::bordered()
            .border_style(theme.border)
            .title(Line::from(" Recent activities · Enter opens ").style(theme.header));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if recent.is_empty() {
            return frame.render_widget(
                Line::from(Span::styled(" No activities loaded yet.", theme.muted)),
                inner,
            );
        }
        // Fixed columns: marker, date, distance, time, pace and their gaps = 44 cells.
        let name_w = usize::from(inner.width.saturating_sub(44)).max(6);
        let lines: Vec<Line> = recent
            .iter()
            .take(RECENT)
            .enumerate()
            .map(|(i, a)| {
                let text = format!(
                    "{} {:<10} {} {:>9} {:>8} {:>10}",
                    if i == self.selected { "▶" } else { " " },
                    a.date
                        .map_or_else(String::new, |d| d.format("%a %d %b").to_string()),
                    format::fit(
                        &a.name.clone().unwrap_or_else(|| sport_name(a.sport)),
                        name_w
                    ),
                    format::distance(a.distance),
                    format::duration(a.moving.or(a.elapsed)),
                    format::pace(a.avg_pace, a.sport.category()),
                );
                Line::from(Span::styled(
                    text,
                    if i == self.selected {
                        theme.selected
                    } else {
                        theme.text
                    },
                ))
            })
            .collect();
        frame.render_widget(Paragraph::new(lines), inner);
    }

    fn render_abilities(&self, frame: &mut Frame, area: Rect, theme: &Theme, tick: u64) {
        let block = Block::bordered()
            .border_style(theme.border)
            .title(Line::from(" Running abilities ").style(theme.header));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let Some(d) = self.dashboard.value() else {
            return placeholder(frame, inner, &self.dashboard, theme, tick, "Loading…");
        };
        let bar_w = inner.width.saturating_sub(24).max(4);
        let s = &d.scores;
        let rows = [
            ("Aerobic end.", s.aerobic_endurance),
            ("Threshold", s.lactate_threshold),
            ("Anaerobic end.", s.anaerobic_endurance),
            ("Anaerobic cap.", s.anaerobic_capacity),
            ("Stamina", d.stamina_level),
        ];
        let lines: Vec<Line> = rows
            .iter()
            .map(|(label, value)| {
                Line::from(vec![
                    Span::styled(format!(" {label:<15}"), theme.muted),
                    Span::styled(format!("{:>5} ", format::opt_f(*value, 1)), theme.text),
                    bar(value.unwrap_or(0.0) / 100.0, bar_w, theme.accent),
                ])
            })
            .collect();
        frame.render_widget(Paragraph::new(lines), inner);
    }
}

/// Loading / error / nothing, for a panel whose slot has no value yet.
pub fn placeholder<T>(
    frame: &mut Frame,
    area: Rect,
    slot: &Slot<T>,
    theme: &Theme,
    tick: u64,
    loading: &str,
) {
    let line = match (slot.error(), slot.is_loading()) {
        (Some(e), _) => Line::from(Span::styled(format!(" {}", e.message), theme.error)),
        (None, true) => Line::from(Span::styled(
            format!(" {} {loading}", spinner(tick)),
            theme.accent,
        )),
        _ => Line::from(""),
    };
    frame.render_widget(
        Paragraph::new(line).wrap(ratatui::widgets::Wrap { trim: true }),
        area,
    );
}

/// Panel title with the load chart's legend, coloured like its lines.
pub fn load_title(title: &str, theme: &Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!(" {title} · "), theme.header),
        Span::styled("━ acute", theme.accent),
        Span::styled(" · ", theme.header),
        Span::styled("━ chronic (fitness) ", theme.zones[2]),
    ])
}

/// Acute vs chronic training load over days since `first`.
pub fn load_chart(
    frame: &mut Frame,
    area: Rect,
    acute: &[(f64, f64)],
    chronic: &[(f64, f64)],
    first: NaiveDate,
    last_x: f64,
    theme: &Theme,
) {
    let (lo, hi) = padded_bounds(acute.iter().chain(chronic).map(|p| p.1)).unwrap_or((0.0, 1.0));
    let (lo, hi) = (lo.max(0.0), hi);
    let date = |x: f64| {
        (first + chrono::Duration::days(x.round() as i64))
            .format("%d %b")
            .to_string()
    };
    let datasets = vec![
        Dataset::default()
            .name("acute")
            .marker(Marker::Braille)
            .graph_type(GraphType::Line)
            .style(theme.accent)
            .data(acute),
        Dataset::default()
            .name("chronic (fitness)")
            .marker(Marker::Braille)
            .graph_type(GraphType::Line)
            .style(theme.zones[2].patch(Style::new()))
            .data(chronic),
    ];
    let chart = Chart::new(datasets)
        .x_axis(
            Axis::default()
                .style(theme.muted)
                .bounds([0.0, last_x.max(1.0)])
                .labels(labels(0.0, last_x, 3, date)),
        )
        .y_axis(
            Axis::default()
                .style(theme.muted)
                .bounds([lo, hi])
                .labels(labels(lo, hi, 3, |v| format!("{v:.0}"))),
        )
        // The legend lives in the panel title (`load_title`); inside it hides data.
        .legend_position(None);
    frame.render_widget(chart, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdata;

    #[test]
    fn first_visit_loads_three_things_once() {
        let mut s = DashboardScreen::new(testdata::today());
        let mut ids = RequestIds::default();
        let commands = s.on_enter(&mut ids);
        assert_eq!(commands.len(), 3);
        assert!(commands.iter().any(|c| matches!(
            c,
            Command::Fetch {
                what: FetchKind::Dashboard,
                ..
            }
        )));
        let week = commands.iter().find_map(|c| match c {
            Command::Fetch {
                what: FetchKind::Schedule(r),
                ..
            } => Some(*r),
            _ => None,
        });
        let week = week.unwrap();
        assert_eq!(
            (week.start, week.end),
            (testdata::date(2026, 10, 12), testdata::date(2026, 10, 18)),
            "Monday–Sunday"
        );
        assert!(s.on_enter(&mut ids).is_empty(), "already started");
        assert!(s.is_loading());
    }

    #[test]
    fn offered_data_goes_to_the_right_slot_or_back() {
        let mut s = DashboardScreen::new(testdata::today());
        let mut ids = RequestIds::default();
        let commands = s.on_enter(&mut ids);
        let req = |kind: fn(&FetchKind) -> bool| {
            commands.iter().find_map(|c| match c {
                Command::Fetch { req, what, .. } if kind(what) => Some(*req),
                _ => None,
            })
        };
        let meta = LoadMeta {
            fetched_at: testdata::now(),
            from_cache: false,
            refreshing: false,
        };
        let dash = req(|k| matches!(k, FetchKind::Dashboard)).unwrap();
        assert!(
            s.offer(
                dash,
                Box::new(Fetched::Dashboard(testdata::dashboard())),
                meta
            )
            .is_none()
        );
        assert!(s.dashboard.value().is_some());
        let back = s.offer(
            RequestId(999),
            Box::new(Fetched::Dashboard(testdata::dashboard())),
            meta,
        );
        assert!(back.is_some(), "not ours: handed back");
    }

    #[test]
    fn recent_selection_and_open() {
        let mut s = DashboardScreen::new(testdata::today());
        let mut ids = RequestIds::default();
        let acts: Vec<ActivitySummary> = (0..8).map(testdata::activity).collect();
        let recent: Vec<&ActivitySummary> = acts.iter().collect();
        for _ in 0..10 {
            s.update(DashboardMsg::Move(Move::Down), &recent, &mut ids);
        }
        assert_eq!(s.selected, RECENT - 1, "only the visible five");
        let Some(DashEffect::Open(a)) = s.update(DashboardMsg::Open, &recent, &mut ids) else {
            panic!()
        };
        assert_eq!(a.id, acts[RECENT - 1].id);
        assert!(
            matches!(s.update(DashboardMsg::Refresh, &recent, &mut ids), Some(DashEffect::Commands(c)) if c.len() == 3)
        );
    }

    #[test]
    fn weeks_start_on_monday() {
        assert_eq!(
            week_start(testdata::date(2026, 10, 18)),
            testdata::date(2026, 10, 12)
        );
        assert_eq!(
            week_start(testdata::date(2026, 10, 12)),
            testdata::date(2026, 10, 12)
        );
    }
}
