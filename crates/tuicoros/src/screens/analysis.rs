//! Analysis (docs/UI_DESIGN.md §2.5): EvoLab trends over 4, 12 or 24 weeks.

use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, Utc};
use coros_api::{ActivitySummary, Daily, DailyMetrics, DateRange, SportCategory};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Axis, Bar, BarChart, BarGroup, Block, Chart, Dataset, GraphType, Paragraph,
};

use super::dashboard::{load_chart, load_title, placeholder, week_start};
use super::{LoadMeta, Slot};
use crate::action::{AnalysisMsg, Fetched, RequestId, RequestIds, UiError};
use crate::charts::{labels, padded_bounds};
use crate::format;
use crate::service::{Command, FetchKind};
use crate::theme::Theme;

pub const WINDOWS: [u32; 3] = [4, 12, 24];

#[derive(Debug)]
pub struct AnalysisScreen {
    today: NaiveDate,
    weeks: u32,
    /// Last day of the window (today unless moved back with `[`).
    end: NaiveDate,
    started: bool,
    daily: Slot<Daily>,
    activities: Slot<Vec<ActivitySummary>>,
}

impl AnalysisScreen {
    pub fn new(default_weeks: u32, today: NaiveDate) -> Self {
        Self {
            today,
            weeks: if WINDOWS.contains(&default_weeks) {
                default_weeks
            } else {
                12
            },
            end: today,
            started: false,
            daily: Slot::default(),
            activities: Slot::default(),
        }
    }

    pub fn set_today(&mut self, today: NaiveDate) {
        // Keep following "today" unless the user moved the window back.
        if self.end == self.today {
            self.end = today;
        }
        self.today = today;
    }

    pub fn range(&self) -> DateRange {
        DateRange::last_days(self.end, self.weeks * 7)
    }

    fn start(&mut self, ids: &mut RequestIds, force: bool, keep_values: bool) -> Vec<Command> {
        self.started = true;
        let (daily, acts) = (ids.next(), ids.next());
        if keep_values {
            self.daily.request(daily);
            self.activities.request(acts);
        } else {
            // A different window: don't show the old one's numbers under the new dates.
            self.daily.reset(daily);
            self.activities.reset(acts);
        }
        let range = self.range();
        vec![
            Command::Fetch {
                req: daily,
                what: FetchKind::Daily(range),
                force,
            },
            Command::Fetch {
                req: acts,
                what: FetchKind::Activities(range),
                force,
            },
        ]
    }

    pub fn on_enter(&mut self, ids: &mut RequestIds) -> Vec<Command> {
        if self.started {
            Vec::new()
        } else {
            self.start(ids, false, true)
        }
    }

    pub fn is_loading(&self) -> bool {
        self.daily.is_loading() || self.activities.is_loading()
    }

    pub fn freshness(&self, now: DateTime<Utc>) -> Option<String> {
        self.daily.freshness(now)
    }

    pub fn offer(
        &mut self,
        req: RequestId,
        data: Box<Fetched>,
        meta: LoadMeta,
    ) -> Option<Box<Fetched>> {
        match *data {
            Fetched::Daily(d) if self.daily.owns(req) => self.daily.accept(req, d, meta),
            Fetched::Activities(a) if self.activities.owns(req) => {
                self.activities.accept(req, a, meta)
            }
            other => return Some(Box::new(other)),
        };
        None
    }

    pub fn on_failed(&mut self, req: RequestId, error: &UiError) -> bool {
        self.daily.fail(req, error.clone()) || self.activities.fail(req, error.clone())
    }

    pub fn map_key(&self, key: KeyEvent) -> Option<AnalysisMsg> {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return None;
        }
        Some(match key.code {
            KeyCode::Char('[') => AnalysisMsg::Earlier,
            KeyCode::Char(']') => AnalysisMsg::Later,
            KeyCode::Char('w') => AnalysisMsg::CycleWeeks,
            KeyCode::Char('r') => AnalysisMsg::Refresh,
            _ => return None,
        })
    }

    pub fn update(&mut self, msg: AnalysisMsg, ids: &mut RequestIds) -> Vec<Command> {
        let span = chrono::Duration::days(i64::from(self.weeks) * 7);
        match msg {
            AnalysisMsg::Earlier => self.end -= span,
            AnalysisMsg::Later => {
                let later = (self.end + span).min(self.today);
                if later == self.end {
                    return Vec::new();
                }
                self.end = later;
            }
            AnalysisMsg::CycleWeeks => {
                let i = WINDOWS.iter().position(|w| *w == self.weeks).unwrap_or(0);
                self.weeks = WINDOWS[(i + 1) % WINDOWS.len()];
            }
            AnalysisMsg::Refresh => return self.start(ids, true, true),
        }
        self.start(ids, false, false)
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme, tick: u64) {
        let r = self.range();
        let title = format!(
            " Analysis · {} weeks · {} – {} ",
            self.weeks,
            r.start.format("%d %b"),
            r.end.format("%d %b %Y")
        );
        let block = Block::bordered()
            .border_style(theme.border)
            .title(Line::from(title).style(theme.title));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let [top, middle, bottom] = Layout::vertical([
            Constraint::Fill(5),
            Constraint::Fill(4),
            Constraint::Fill(4),
        ])
        .areas(inner);
        self.render_load(frame, top, theme, tick);
        let [rhr, weekly_load] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Fill(1)]).areas(middle);
        self.render_rhr(frame, rhr, theme, tick);
        self.render_weekly_load(frame, weekly_load, theme, tick);
        let [distance, fitness] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Fill(1)]).areas(bottom);
        self.render_distance(frame, distance, theme, tick);
        self.render_fitness(frame, fitness, theme, tick);
    }

    fn panel(frame: &mut Frame, area: Rect, title: &str, theme: &Theme) -> Rect {
        let block = Block::bordered()
            .border_style(theme.border)
            .title(Line::from(format!(" {title} ")).style(theme.header));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        inner
    }

    fn days(&self) -> Option<&[DailyMetrics]> {
        self.daily.value().map(|d| d.days.as_slice())
    }

    fn render_load(&self, frame: &mut Frame, area: Rect, theme: &Theme, tick: u64) {
        let block = Block::bordered()
            .border_style(theme.border)
            .title(load_title("Training load", theme));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let Some(days) = self.days() else {
            return placeholder(
                frame,
                inner,
                &self.daily,
                theme,
                tick,
                "Loading EvoLab data…",
            );
        };
        let first = self.range().start;
        let x = |d: NaiveDate| (d - first).num_days() as f64;
        let acute: Vec<(f64, f64)> = days
            .iter()
            .filter_map(|d| Some((x(d.date), d.acute_load?)))
            .collect();
        let chronic: Vec<(f64, f64)> = days
            .iter()
            .filter_map(|d| Some((x(d.date), d.chronic_load?)))
            .collect();
        if acute.is_empty() && chronic.is_empty() {
            return frame.render_widget(
                Line::from(Span::styled(" No EvoLab data in this window.", theme.muted)),
                inner,
            );
        }
        load_chart(
            frame,
            inner,
            &acute,
            &chronic,
            first,
            (self.weeks * 7 - 1) as f64,
            theme,
        );
    }

    fn render_rhr(&self, frame: &mut Frame, area: Rect, theme: &Theme, tick: u64) {
        let inner = Self::panel(frame, area, "Resting heart rate (EvoLab daily)", theme);
        let Some(days) = self.days() else {
            return placeholder(frame, inner, &self.daily, theme, tick, "Loading…");
        };
        let first = self.range().start;
        let points: Vec<(f64, f64)> = days
            .iter()
            .filter_map(|d| {
                Some((
                    (d.date - first).num_days() as f64,
                    f64::from(d.test_rhr.or(d.rhr)?),
                ))
            })
            .collect();
        if points.is_empty() {
            return frame.render_widget(
                Line::from(Span::styled(" No resting HR in this window.", theme.muted)),
                inner,
            );
        }
        let values: Vec<f64> = points.iter().map(|p| p.1).collect();
        let avg = values.iter().sum::<f64>() / values.len() as f64;
        let low = values.iter().copied().fold(f64::INFINITY, f64::min);
        let latest = values.last().copied().unwrap_or(avg);
        let [summary, chart_area] =
            Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(inner);
        frame.render_widget(
            Line::from(vec![
                Span::styled(format!(" latest {latest:.0}"), theme.title),
                Span::styled(format!(" · avg {avg:.0} · low {low:.0} bpm"), theme.muted),
            ]),
            summary,
        );
        let (lo, hi) = padded_bounds(values.iter().copied()).unwrap_or((40.0, 80.0));
        let chart = Chart::new(vec![
            Dataset::default()
                .marker(Marker::Braille)
                .graph_type(GraphType::Line)
                .style(theme.zones[5])
                .data(&points),
        ])
        .x_axis(Axis::default().bounds([0.0, (self.weeks * 7 - 1) as f64]))
        .y_axis(
            Axis::default()
                .style(theme.muted)
                .bounds([lo, hi])
                .labels(labels(lo, hi, 2, |v| format!("{v:.0}"))),
        );
        frame.render_widget(chart, chart_area);
    }

    fn render_weekly_load(&self, frame: &mut Frame, area: Rect, theme: &Theme, tick: u64) {
        let inner = Self::panel(frame, area, "Weekly load", theme);
        let Some(daily) = self.daily.value() else {
            return placeholder(frame, inner, &self.daily, theme, tick, "Loading…");
        };
        let r = self.range();
        let weeks: Vec<_> = daily
            .weeks
            .iter()
            .filter(|w| w.week_start >= week_start(r.start) && w.week_start <= r.end)
            .collect();
        if weeks.is_empty() {
            return frame.render_widget(
                Line::from(Span::styled(" No weekly load in this window.", theme.muted)),
                inner,
            );
        }
        let [summary, chart_area] =
            Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(inner);
        if let Some(last) = weeks.last() {
            frame.render_widget(
                Line::from(vec![
                    Span::styled(
                        format!(" this week {}", format::opt_f(last.training_load, 0)),
                        theme.title,
                    ),
                    Span::styled(
                        format!(
                            " · recommended {}–{}",
                            format::opt_f(last.recommended_min, 0),
                            format::opt_f(last.recommended_max, 0)
                        ),
                        theme.muted,
                    ),
                ]),
                summary,
            );
        }
        let width = bar_width(weeks.len(), chart_area.width);
        let bars: Vec<Bar> = weeks
            .iter()
            .map(|w| {
                let load = w.training_load.unwrap_or(0.0);
                let in_range = match (w.recommended_min, w.recommended_max) {
                    (Some(lo), Some(hi)) => (lo..=hi).contains(&load),
                    _ => true,
                };
                week_bar(
                    load,
                    w.week_start,
                    width,
                    if in_range {
                        theme.accent
                    } else {
                        theme.warning
                    },
                )
            })
            .collect();
        frame.render_widget(bar_chart(bars, width, theme), chart_area);
    }

    fn render_distance(&self, frame: &mut Frame, area: Rect, theme: &Theme, tick: u64) {
        let inner = Self::panel(frame, area, "Weekly distance (all sports)", theme);
        let Some(activities) = self.activities.value() else {
            return placeholder(
                frame,
                inner,
                &self.activities,
                theme,
                tick,
                "Loading activities…",
            );
        };
        let r = self.range();
        let mut weeks: BTreeMap<NaiveDate, f64> = BTreeMap::new();
        let mut monday = week_start(r.start);
        while monday <= r.end {
            weeks.insert(monday, 0.0);
            monday += chrono::Duration::days(7);
        }
        let mut runs = 0.0;
        for a in activities {
            if let (Some(date), Some(d)) = (a.date, a.distance) {
                *weeks.entry(week_start(date)).or_default() += d.km();
                if a.sport.category() == SportCategory::Run {
                    runs += d.km();
                }
            }
        }
        let total: f64 = weeks.values().sum();
        let [summary, chart_area] =
            Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(inner);
        frame.render_widget(
            Line::from(vec![
                Span::styled(format!(" {total:.1} km"), theme.title),
                Span::styled(
                    format!(" in {} activities · runs {runs:.1} km", activities.len()),
                    theme.muted,
                ),
            ]),
            summary,
        );
        let width = bar_width(weeks.len(), chart_area.width);
        let bars: Vec<Bar> = weeks
            .iter()
            .map(|(monday, km)| week_bar(*km, *monday, width, theme.zones[2]))
            .collect();
        frame.render_widget(bar_chart(bars, width, theme), chart_area);
    }

    fn render_fitness(&self, frame: &mut Frame, area: Rect, theme: &Theme, tick: u64) {
        let inner = Self::panel(frame, area, "Fitness in this window", theme);
        let Some(days) = self.days() else {
            return placeholder(frame, inner, &self.daily, theme, tick, "Loading…");
        };
        // First and last value of a metric in the window, and the change.
        let change = |label: &str,
                      value: &dyn Fn(&DailyMetrics) -> Option<f64>,
                      fmt: &dyn Fn(f64) -> String,
                      lower_is_better: bool| {
            let first = days.iter().find_map(value);
            let last = days.iter().rev().find_map(value);
            let mut spans = vec![Span::styled(format!(" {label:<16}"), theme.muted)];
            match (first, last) {
                (Some(a), Some(b)) => {
                    spans.push(Span::styled(fmt(b), theme.title));
                    let delta = b - a;
                    if delta.abs() > f64::EPSILON {
                        let better = (delta < 0.0) == lower_is_better;
                        let arrow = if delta > 0.0 { "▲" } else { "▼" };
                        spans.push(Span::styled(
                            format!("  {arrow} from {}", fmt(a)),
                            if better { theme.accent } else { theme.warning },
                        ));
                    }
                }
                _ => spans.push(Span::styled("—", theme.muted)),
            }
            Line::from(spans)
        };
        let lines = vec![
            change("VO2max", &|d| d.vo2max, &|v| format!("{v:.0}"), false),
            change(
                "Threshold HR",
                &|d| d.lthr.map(f64::from),
                &|v| format!("{v:.0} bpm"),
                false,
            ),
            change(
                "Threshold pace",
                &|d| d.lt_pace.map(|p| p.0),
                &|v| format::pace(Some(coros_api::Pace(v)), SportCategory::Run),
                true,
            ),
            change(
                "Stamina",
                &|d| d.stamina_level,
                &|v| format!("{v:.0}"),
                false,
            ),
            change(
                "Fitness (CTI)",
                &|d| d.chronic_load,
                &|v| format!("{v:.0}"),
                false,
            ),
        ];
        frame.render_widget(Paragraph::new(lines), inner);
    }
}

/// Width of each bar so `n` bars fill `width` (1-cell gaps).
fn bar_width(n: usize, width: u16) -> u16 {
    let n = u16::try_from(n).unwrap_or(u16::MAX).max(1);
    (width.saturating_sub(n) / n).clamp(1, 7)
}

/// One bar: the value centred on it, and a date label that fits.
fn week_bar(
    value: f64,
    monday: NaiveDate,
    width: u16,
    style: ratatui::style::Style,
) -> Bar<'static> {
    let w = usize::from(width);
    let label = if width >= 5 {
        monday.format("%d/%m").to_string()
    } else {
        monday.format("%d").to_string()
    };
    let v = value.max(0.0).round() as u64;
    Bar::default()
        .value(v)
        .text_value(format!("{v:^w$}"))
        .label(Line::from(label))
        .style(style)
}

/// A bar chart of week bars, `bar_width` wide each.
fn bar_chart<'a>(bars: Vec<Bar<'a>>, width: u16, theme: &Theme) -> BarChart<'a> {
    BarChart::default()
        .data(BarGroup::default().bars(&bars))
        .bar_width(width)
        .bar_gap(1)
        .value_style(theme.header)
        .label_style(theme.muted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdata;

    fn fetches(commands: &[Command]) -> Vec<FetchKind> {
        commands
            .iter()
            .filter_map(|c| match c {
                Command::Fetch { what, .. } => Some(*what),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn window_navigation() {
        let mut s = AnalysisScreen::new(12, testdata::today());
        let mut ids = RequestIds::default();
        let first = fetches(&s.on_enter(&mut ids));
        let r = s.range();
        assert_eq!((r.end, r.days()), (testdata::today(), 84));
        assert_eq!(first, vec![FetchKind::Daily(r), FetchKind::Activities(r)]);

        assert!(
            s.update(AnalysisMsg::Later, &mut ids).is_empty(),
            "can't go past today"
        );
        s.update(AnalysisMsg::Earlier, &mut ids);
        assert_eq!(
            s.range().end,
            testdata::today() - chrono::Duration::days(84)
        );
        assert!(
            s.daily.value().is_none() && s.is_loading(),
            "old window cleared, new one loading"
        );
        s.update(AnalysisMsg::Later, &mut ids);
        assert_eq!(s.range().end, testdata::today());

        s.update(AnalysisMsg::CycleWeeks, &mut ids);
        assert_eq!(s.range().days(), 24 * 7);
        s.update(AnalysisMsg::CycleWeeks, &mut ids);
        assert_eq!(s.range().days(), 4 * 7);
    }

    #[test]
    fn unknown_default_window_falls_back_to_12_weeks() {
        assert_eq!(AnalysisScreen::new(7, testdata::today()).range().days(), 84);
    }

    #[test]
    fn data_routing() {
        let mut s = AnalysisScreen::new(4, testdata::today());
        let mut ids = RequestIds::default();
        let commands = s.on_enter(&mut ids);
        let Command::Fetch { req, .. } = commands[0] else {
            panic!()
        };
        let meta = LoadMeta {
            fetched_at: testdata::now(),
            from_cache: false,
            refreshing: false,
        };
        assert!(
            s.offer(req, Box::new(Fetched::Daily(testdata::daily(28))), meta)
                .is_none()
        );
        assert!(s.daily.value().is_some());
        assert!(s.on_failed(
            match commands[1] {
                Command::Fetch { req, .. } => req,
                _ => panic!(),
            },
            &UiError::other("x")
        ));
    }
}
