//! Activity detail: Summary and Laps tabs (docs/UI_DESIGN.md §2.4).
//! Charts, Map and Notes-in-depth come in Phase 3.

use std::cell::Cell;

use chrono::{DateTime, Utc};
use coros_api::{ActivityDetail, ActivitySummary, LapGroup, SportCategory, SportType, StepKind};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Cell as TableCell, HighlightSpacing, Paragraph, Row, Table, TableState, Tabs,
};

use crate::action::{DetailMsg, Move, RequestId, RequestIds, UiError};
use crate::format;
use crate::screens::activities::sport_name;
use crate::screens::detail_views::{
    Metric, XAxis, available_metrics, default_metric, render_charts, render_map, render_notes,
};
use crate::service::{Command, age_label};
use crate::theme::Theme;
use crate::widgets::{bar, message, spinner};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailTab {
    Summary,
    Laps,
    Charts,
    Map,
    Notes,
}

impl DetailTab {
    pub const ALL: [DetailTab; 5] = [
        Self::Summary,
        Self::Laps,
        Self::Charts,
        Self::Map,
        Self::Notes,
    ];

    fn title(self) -> &'static str {
        match self {
            Self::Summary => "Summary",
            Self::Laps => "Laps",
            Self::Charts => "Charts",
            Self::Map => "Map",
            Self::Notes => "Notes",
        }
    }

    fn index(self) -> usize {
        self as usize
    }

    fn step(self, by: isize) -> Self {
        let n = Self::ALL.len() as isize;
        Self::ALL[((self.index() as isize + by).rem_euclid(n)) as usize]
    }
}

pub enum DetailEffect {
    Back,
    Command(Command),
}

#[derive(Debug)]
pub struct DetailScreen {
    /// The list row we came from: enough for the header while loading.
    summary: ActivitySummary,
    req: Option<RequestId>,
    detail: Option<Box<ActivityDetail>>,
    fetched_at: Option<DateTime<Utc>>,
    from_cache: bool,
    error: Option<UiError>,
    tab: DetailTab,
    group: usize,
    lap: usize,
    lap_offset: Cell<usize>,
    viewport: Cell<usize>,
    metric: Metric,
    axis: XAxis,
}

impl DetailScreen {
    pub fn open(summary: ActivitySummary, ids: &mut RequestIds) -> (Self, Command) {
        let sport = summary.sport;
        let mut screen = Self {
            summary,
            req: None,
            detail: None,
            fetched_at: None,
            from_cache: false,
            error: None,
            tab: DetailTab::Summary,
            group: 0,
            lap: 0,
            lap_offset: Cell::new(0),
            viewport: Cell::new(10),
            metric: default_metric(sport),
            axis: XAxis::Distance,
        };
        let command = screen.load(ids, false);
        (screen, command)
    }

    fn load(&mut self, ids: &mut RequestIds, force: bool) -> Command {
        let req = ids.next();
        self.req = Some(req);
        self.error = None;
        Command::LoadDetail {
            req,
            id: self.summary.id.clone(),
            sport: self.summary.sport,
            force,
        }
    }

    pub fn is_loading(&self) -> bool {
        self.req.is_some()
    }

    #[cfg(test)]
    pub fn tab(&self) -> DetailTab {
        self.tab
    }

    pub fn freshness(&self, now: DateTime<Utc>) -> Option<String> {
        let at = self.fetched_at?;
        Some(format!(
            "{} {}",
            if self.from_cache { "cached" } else { "updated" },
            age_label(at, now)
        ))
    }

    pub fn on_loaded(
        &mut self,
        req: RequestId,
        detail: Box<ActivityDetail>,
        fetched_at: DateTime<Utc>,
        from_cache: bool,
    ) -> bool {
        if self.req != Some(req) {
            return false;
        }
        self.req = None;
        // Start on the most useful grouping: workout steps, else the shortest laps.
        self.group = LapGroup::preferred(&detail.lap_groups)
            .and_then(|g| detail.lap_groups.iter().position(|x| std::ptr::eq(x, g)))
            .unwrap_or(0);
        self.lap = 0;
        self.detail = Some(detail);
        self.fetched_at = Some(fetched_at);
        self.from_cache = from_cache;
        true
    }

    pub fn on_failed(&mut self, req: RequestId, error: UiError) -> bool {
        if self.req != Some(req) {
            return false;
        }
        self.req = None;
        self.error = Some(error);
        true
    }

    fn sport(&self) -> SportType {
        self.detail
            .as_ref()
            .and_then(|d| d.summary.sport)
            .unwrap_or(self.summary.sport)
    }

    /// Lap groups worth showing (non-empty), in API order.
    fn groups(&self) -> Vec<&LapGroup> {
        self.detail
            .as_ref()
            .map(|d| d.lap_groups.iter().filter(|g| !g.laps.is_empty()).collect())
            .unwrap_or_default()
    }

    fn current_group(&self) -> Option<&LapGroup> {
        let d = self.detail.as_ref()?;
        d.lap_groups
            .get(self.group)
            .filter(|g| !g.laps.is_empty())
            .or_else(|| self.groups().first().copied())
    }

    pub fn map_key(&self, key: KeyEvent) -> Option<DetailMsg> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        Some(match key.code {
            KeyCode::Esc | KeyCode::Backspace | KeyCode::Char('q') => DetailMsg::Back,
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => DetailMsg::NextTab,
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => DetailMsg::PrevTab,
            KeyCode::Up | KeyCode::Char('k') => DetailMsg::Move(Move::Up),
            KeyCode::Down | KeyCode::Char('j') => DetailMsg::Move(Move::Down),
            KeyCode::PageUp => DetailMsg::Move(Move::PageUp),
            KeyCode::PageDown => DetailMsg::Move(Move::PageDown),
            KeyCode::Char('u') if ctrl => DetailMsg::Move(Move::PageUp),
            KeyCode::Char('d') if ctrl => DetailMsg::Move(Move::PageDown),
            KeyCode::Home | KeyCode::Char('g') => DetailMsg::Move(Move::Top),
            KeyCode::End | KeyCode::Char('G') => DetailMsg::Move(Move::Bottom),
            KeyCode::Char('c') => DetailMsg::CycleLapGroup,
            KeyCode::Char('m') => DetailMsg::CycleMetric,
            KeyCode::Char('x') => DetailMsg::ToggleAxis,
            KeyCode::Char('r') => DetailMsg::Refresh,
            _ => return None,
        })
    }

    pub fn update(&mut self, msg: DetailMsg, ids: &mut RequestIds) -> Option<DetailEffect> {
        match msg {
            DetailMsg::Back => return Some(DetailEffect::Back),
            DetailMsg::NextTab => self.tab = self.tab.step(1),
            DetailMsg::PrevTab => self.tab = self.tab.step(-1),
            DetailMsg::CycleMetric => {
                // First press just opens the Charts tab; later presses cycle.
                if self.tab != DetailTab::Charts {
                    self.tab = DetailTab::Charts;
                } else if let Some(d) = &self.detail {
                    let metrics = available_metrics(d);
                    let i = metrics.iter().position(|m| *m == self.metric);
                    if let Some(next) =
                        i.map_or(metrics.first(), |i| metrics.get((i + 1) % metrics.len()))
                    {
                        self.metric = *next;
                    }
                }
            }
            DetailMsg::ToggleAxis => {
                if self.tab != DetailTab::Charts {
                    self.tab = DetailTab::Charts;
                } else {
                    self.axis = match self.axis {
                        XAxis::Distance => XAxis::Time,
                        XAxis::Time => XAxis::Distance,
                    };
                }
            }
            DetailMsg::Move(m) => {
                match self.tab {
                    // Moving only makes sense in the lap table.
                    DetailTab::Summary => self.tab = DetailTab::Laps,
                    DetailTab::Laps => {}
                    DetailTab::Charts | DetailTab::Map | DetailTab::Notes => return None,
                }
                let len = self.current_group().map_or(0, |g| g.laps.len());
                if len > 0 {
                    let page = self.viewport.get().max(1);
                    self.lap = match m {
                        Move::Up => self.lap.saturating_sub(1),
                        Move::Down => (self.lap + 1).min(len - 1),
                        Move::PageUp => self.lap.saturating_sub(page),
                        Move::PageDown => (self.lap + page).min(len - 1),
                        Move::Top => 0,
                        Move::Bottom => len - 1,
                    };
                }
            }
            DetailMsg::CycleLapGroup => {
                if let Some(d) = &self.detail {
                    let usable: Vec<usize> = d
                        .lap_groups
                        .iter()
                        .enumerate()
                        .filter(|(_, g)| !g.laps.is_empty())
                        .map(|(i, _)| i)
                        .collect();
                    if let Some(pos) = usable.iter().position(|i| *i == self.group) {
                        self.group = usable[(pos + 1) % usable.len()];
                    } else if let Some(first) = usable.first() {
                        self.group = *first;
                    }
                    self.lap = 0;
                    self.tab = DetailTab::Laps;
                }
            }
            DetailMsg::Refresh => return Some(DetailEffect::Command(self.load(ids, true))),
        }
        None
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme, tick: u64) {
        let sport = self.sport();
        let name = self
            .detail
            .as_ref()
            .and_then(|d| d.summary.name.clone())
            .or_else(|| self.summary.name.clone());
        let start = self
            .detail
            .as_ref()
            .and_then(|d| d.summary.start)
            .or(self.summary.start);
        let mut title = format!(
            " {} · {}",
            name.unwrap_or_else(|| sport_name(sport)),
            sport_name(sport)
        );
        if let Some(start) = start {
            title.push_str(&start.format(" · %a %d %b %Y %H:%M (UTC%:z)").to_string());
        }
        title.push(' ');
        let block = Block::bordered()
            .border_style(theme.border)
            .title(Line::from(title).style(theme.title));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let [tabs_area, body] =
            Layout::vertical([Constraint::Length(2), Constraint::Fill(1)]).areas(inner);
        let tabs = Tabs::new(DetailTab::ALL.map(DetailTab::title))
            .select(self.tab.index())
            .style(theme.tab)
            .highlight_style(theme.tab_active)
            .divider("│");
        frame.render_widget(tabs, tabs_area);

        let Some(detail) = &self.detail else {
            let lines = match &self.error {
                Some(err) => vec![
                    Line::from(Span::styled(err.message.clone(), theme.error)),
                    Line::from(""),
                    Line::from(Span::styled(
                        "Press r to retry, Esc to go back.",
                        theme.muted,
                    )),
                ],
                None => vec![Line::from(Span::styled(
                    format!("{} Loading activity…", spinner(tick)),
                    theme.accent,
                ))],
            };
            message(frame, body, lines);
            return;
        };
        match self.tab {
            DetailTab::Summary => self.render_summary(frame, body, detail, sport, theme),
            DetailTab::Laps => self.render_laps(frame, body, sport, theme),
            DetailTab::Charts => render_charts(frame, body, detail, self.metric, self.axis, theme),
            DetailTab::Map => render_map(frame, body, detail, theme),
            DetailTab::Notes => render_notes(frame, body, detail, &self.summary, theme),
        }
    }

    fn render_summary(
        &self,
        frame: &mut Frame,
        area: Rect,
        d: &ActivityDetail,
        sport: SportType,
        theme: &Theme,
    ) {
        let s = &d.summary;
        let cat = sport.category();
        let runlike = matches!(cat, SportCategory::Run | SportCategory::Walk);
        let mut stats: Vec<(&str, String)> = vec![
            ("Distance", format::distance(s.distance)),
            ("Moving time", format::duration(s.moving)),
            ("Elapsed", format::duration(s.elapsed)),
        ];
        if s.avg_pace.is_some() {
            stats.push(("Avg pace", format::pace(s.avg_pace, cat)));
        }
        if runlike && s.adjusted_pace.is_some() {
            stats.push(("Grade-adj. pace", format::pace(s.adjusted_pace, cat)));
        }
        if runlike && s.best_km.is_some() {
            stats.push(("Best km", format::pace(s.best_km, cat)));
        }
        stats.push((
            "Avg / max HR",
            format!("{} / {} bpm", format::opt(s.avg_hr), format::opt(s.max_hr)),
        ));
        if s.avg_hr.is_some() {
            // Where the heart rate came from: a paired HR monitor or the watch's
            // wrist sensor. The Notes tab names the model; values here get 17 cells.
            let source = if d.heart_rate_sensor().is_some() {
                "HR monitor"
            } else {
                "wrist (watch)"
            };
            stats.push(("Heart rate from", source.to_owned()));
        }
        if s.avg_cadence.is_some() {
            stats.push((
                "Cadence",
                format!(
                    "{} / {} spm",
                    format::opt(s.avg_cadence),
                    format::opt(s.max_cadence)
                ),
            ));
        }
        if s.avg_stride_length_cm.is_some() {
            stats.push((
                "Stride",
                format!("{} cm", format::opt(s.avg_stride_length_cm)),
            ));
        }
        if s.avg_power.is_some() {
            stats.push((
                "Power",
                format!(
                    "{} / {} W",
                    format::opt(s.avg_power),
                    format::opt(s.max_power)
                ),
            ));
        }
        if s.elevation_gain.is_some() || s.elevation_loss.is_some() {
            stats.push((
                "Elevation",
                format!(
                    "+{} / -{} m",
                    format::opt_f(s.elevation_gain.map(|m| m.0), 0),
                    format::opt_f(s.elevation_loss.map(|m| m.0), 0)
                ),
            ));
        }
        stats.push(("Training load", format::opt(s.training_load)));
        if s.aerobic_effect.is_some() || s.anaerobic_effect.is_some() {
            stats.push((
                "Training effect",
                format!(
                    "{} aer. / {} anaer.",
                    format::opt_f(s.aerobic_effect, 1),
                    format::opt_f(s.anaerobic_effect, 1)
                ),
            ));
        }
        stats.push((
            "Calories",
            format!("{} kcal", format::opt_f(s.calories_kcal, 0)),
        ));
        if s.vo2max.is_some() {
            stats.push(("VO2max", format::opt_f(s.vo2max, 0)));
        }

        // Stats in as many 34-cell columns as fit.
        let cols = usize::from((area.width / 34).clamp(1, 3));
        let rows_needed = stats.len().div_ceil(cols);
        let mut lines: Vec<Line> = (0..rows_needed)
            .map(|r| {
                let mut spans = Vec::new();
                for c in 0..cols {
                    if let Some((label, value)) = stats.get(c * rows_needed + r) {
                        spans.push(Span::styled(format!(" {label:<16}"), theme.muted));
                        spans.push(Span::styled(format!("{value:<17}"), theme.text));
                    }
                }
                Line::from(spans)
            })
            .collect();

        if !d.hr_zones.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(" Heart-rate zones", theme.header)));
            let bar_w = area.width.saturating_sub(34).clamp(5, 40);
            for z in &d.hr_zones {
                let bounds = if z.zone == 0 {
                    format!("<{}", format::opt(z.low_bpm))
                } else {
                    format!("{}–{}", format::opt(z.low_bpm), format::opt(z.high_bpm))
                };
                let style = theme.zones[(z.zone as usize).min(5)];
                lines.push(Line::from(vec![
                    Span::styled(format!(" Z{} {bounds:<8}", z.zone), theme.text),
                    Span::styled(format!("{:>7} ", format::duration(z.time)), theme.muted),
                    Span::styled(format!("{:>3}% ", format::opt_f(z.percent, 0)), theme.text),
                    bar(z.percent.unwrap_or(0.0) / 100.0, bar_w, style),
                ]));
            }
        }

        if d.feel.perceived_exertion.is_some() || d.feel.note.is_some() {
            lines.push(Line::from(""));
            if let Some(rpe) = d.feel.perceived_exertion {
                lines.push(Line::from(vec![
                    Span::styled(format!(" {:<16}", "Effort"), theme.muted),
                    Span::styled(rpe.label(), theme.text),
                ]));
            }
            if let Some(note) = &d.feel.note {
                lines.push(Line::from(vec![
                    Span::styled(format!(" {:<16}", "Note"), theme.muted),
                    Span::styled(note.clone(), theme.text),
                ]));
            }
        }
        frame.render_widget(Paragraph::new(lines), area);
    }

    fn render_laps(&self, frame: &mut Frame, area: Rect, sport: SportType, theme: &Theme) {
        let Some(group) = self.current_group() else {
            message(
                frame,
                area,
                vec![Line::from(Span::styled(
                    "This activity has no laps.",
                    theme.muted,
                ))],
            );
            return;
        };
        let groups = self.groups();
        let position = groups
            .iter()
            .position(|g| std::ptr::eq(*g, group))
            .map_or(1, |p| p + 1);
        let [info, table_area] =
            Layout::vertical([Constraint::Length(2), Constraint::Fill(1)]).areas(area);
        let hint = if groups.len() > 1 {
            format!("  ({position} of {} · c to switch)", groups.len())
        } else {
            String::new()
        };
        frame.render_widget(
            Line::from(vec![
                Span::styled(format!(" {}", group.label()), theme.header),
                Span::styled(hint, theme.muted),
            ]),
            info,
        );

        let cat = sport.category();
        let has_steps = group.laps.iter().any(|l| l.step_kind.is_some());
        let mut header = vec!["#"];
        let mut widths = vec![Constraint::Length(3)];
        if has_steps {
            header.push("Step");
            widths.push(Constraint::Length(9));
        }
        header.extend(["Distance", "Time", "Pace", "HR", "Max", "Cad", "Pwr", ""]);
        widths.extend([
            Constraint::Length(9),
            Constraint::Length(8),
            Constraint::Length(10),
            Constraint::Length(4),
            Constraint::Length(4),
            Constraint::Length(4),
            Constraint::Length(4),
            Constraint::Length(1),
        ]);

        let rows: Vec<Row> = group
            .laps
            .iter()
            .enumerate()
            .map(|(i, lap)| {
                let fastest = group.fastest.contains(&(i as u32));
                let mut cells = vec![right(lap.index.to_string())];
                if has_steps {
                    let step = match lap.step_kind {
                        Some(StepKind::Training) if matches!(cat, SportCategory::Run) => "Run",
                        Some(k) => k.label(),
                        None => "",
                    };
                    cells.push(TableCell::from(step));
                }
                cells.extend([
                    right(format::distance(lap.distance)),
                    right(format::duration(lap.duration)),
                    right(format::pace(lap.pace, cat)),
                    right(format::opt(lap.avg_hr)),
                    right(format::opt(lap.max_hr)),
                    right(format::opt(lap.avg_cadence)),
                    right(format::opt(lap.avg_power)),
                    TableCell::from(if fastest { "*" } else { "" }),
                ]);
                Row::new(cells).style(if fastest { theme.fastest } else { theme.text })
            })
            .collect();

        let viewport = usize::from(table_area.height.saturating_sub(1)).max(1);
        self.viewport.set(viewport);
        let mut offset = self.lap_offset.get();
        if self.lap < offset {
            offset = self.lap;
        } else if self.lap >= offset + viewport {
            offset = self.lap + 1 - viewport;
        }
        offset = offset.min(group.laps.len().saturating_sub(viewport));
        self.lap_offset.set(offset);
        let mut state = TableState::default()
            .with_offset(offset)
            .with_selected(Some(self.lap));
        let table = Table::new(rows, widths)
            .header(Row::new(
                header
                    .into_iter()
                    .map(|h| TableCell::from(h).style(theme.header)),
            ))
            .column_spacing(1)
            .row_highlight_style(theme.selected)
            .highlight_symbol("▶ ")
            .highlight_spacing(HighlightSpacing::Always);
        frame.render_stateful_widget(table, table_area, &mut state);
    }
}

fn right(text: String) -> TableCell<'static> {
    TableCell::from(Line::from(text).right_aligned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdata;

    fn opened() -> (DetailScreen, RequestIds, RequestId) {
        let mut ids = RequestIds::default();
        let (screen, cmd) = DetailScreen::open(testdata::activity(1), &mut ids);
        let Command::LoadDetail {
            req,
            id,
            sport,
            force,
        } = cmd
        else {
            panic!()
        };
        assert_eq!(
            (id, sport, force),
            (testdata::activity(1).id, SportType::Run, false)
        );
        (screen, ids, req)
    }

    #[test]
    fn loads_and_starts_on_the_workout_steps() {
        let (mut s, _, req) = opened();
        assert!(s.is_loading());
        assert!(s.on_loaded(req, Box::new(testdata::detail()), testdata::now(), true));
        assert!(!s.is_loading());
        assert_eq!(s.current_group().unwrap().label(), "Workout steps");
        assert_eq!(
            s.freshness(testdata::now()).as_deref(),
            Some("cached just now")
        );
    }

    #[test]
    fn ignores_stale_answers_and_handles_failures() {
        let (mut s, mut ids, req) = opened();
        assert!(!s.on_loaded(
            RequestId(999),
            Box::new(testdata::detail()),
            testdata::now(),
            false
        ));
        assert!(s.on_failed(req, UiError::other("boom")));
        assert!(s.error.is_some());
        let Some(DetailEffect::Command(Command::LoadDetail { force: true, .. })) =
            s.update(DetailMsg::Refresh, &mut ids)
        else {
            panic!("retry reloads, skipping the cache");
        };
        assert!(s.error.is_none());
    }

    #[test]
    fn first_m_or_x_opens_charts_without_changing_them() {
        let (mut s, mut ids, req) = opened();
        s.on_loaded(req, Box::new(testdata::detail()), testdata::now(), false);
        let (metric, axis) = (s.metric, s.axis);
        assert_eq!(metric, Metric::Pace, "runs start on pace");
        s.update(DetailMsg::CycleMetric, &mut ids);
        assert_eq!((s.tab, s.metric), (DetailTab::Charts, metric));
        s.update(DetailMsg::CycleMetric, &mut ids);
        assert_ne!(s.metric, metric);
        s.tab = DetailTab::Map;
        s.update(DetailMsg::ToggleAxis, &mut ids);
        assert_eq!((s.tab, s.axis), (DetailTab::Charts, axis));
        s.update(DetailMsg::ToggleAxis, &mut ids);
        assert_eq!(s.axis, XAxis::Time);
        s.update(DetailMsg::Move(Move::Down), &mut ids);
        assert_eq!(s.tab, DetailTab::Charts, "arrows don't leave the chart");
    }

    #[test]
    fn tabs_groups_and_lap_selection() {
        let (mut s, mut ids, req) = opened();
        s.on_loaded(req, Box::new(testdata::detail()), testdata::now(), false);
        s.update(DetailMsg::NextTab, &mut ids);
        assert_eq!(s.tab(), DetailTab::Laps);
        s.update(DetailMsg::Move(Move::Bottom), &mut ids);
        assert_eq!(s.lap, 7);
        s.update(DetailMsg::CycleLapGroup, &mut ids);
        assert_eq!(s.current_group().unwrap().label(), "Whole activity");
        assert_eq!(s.lap, 0, "selection resets per group");
        s.update(DetailMsg::CycleLapGroup, &mut ids);
        assert_eq!(
            s.current_group().unwrap().label(),
            "1 km laps",
            "wraps around"
        );
        assert!(matches!(
            s.update(DetailMsg::Back, &mut ids),
            Some(DetailEffect::Back)
        ));
    }
}
