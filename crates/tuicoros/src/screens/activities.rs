//! Activity list (docs/UI_DESIGN.md §2.3).

use std::cell::Cell;
use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, Utc};
use coros_api::{ActivityQuery, ActivitySummary, LabelId, Page, SportType};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Cell as TableCell, HighlightSpacing, List, ListItem, ListState, Row, Table, TableState,
};

use crate::action::{ActivitiesMsg, Edit, FilterMsg, Move, RequestId, RequestIds, UiError};
use crate::format;
use crate::service::{Command, age_label};
use crate::theme::Theme;
use crate::widgets::{TextInput, centered, message, popup, spinner};

/// Rows from the end of the list at which the next page is requested.
const PREFETCH_MARGIN: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SportFilter {
    pub label: &'static str,
    pub sports: &'static [SportType],
}

pub const SPORT_FILTERS: [SportFilter; 7] = [
    SportFilter {
        label: "All sports",
        sports: &[],
    },
    SportFilter {
        label: "Runs",
        sports: &SportType::RUNS,
    },
    SportFilter {
        label: "Trail runs",
        sports: &[SportType::TrailRun],
    },
    SportFilter {
        label: "Strength",
        sports: &[SportType::Strength],
    },
    SportFilter {
        label: "Bike",
        sports: &[
            SportType::Bike,
            SportType::IndoorBike,
            SportType::RoadEbike,
            SportType::GravelBike,
            SportType::MountainBike,
            SportType::MountainEbike,
            SportType::HelmetBike,
        ],
    },
    SportFilter {
        label: "Swim",
        sports: &[SportType::PoolSwim, SportType::OpenWater],
    },
    SportFilter {
        label: "Walk & hike",
        sports: &[SportType::Walk, SportType::Hike, SportType::MountainClimb],
    },
];

/// Date ranges offered by the filter; `None` = all time.
pub const RANGES: [Option<u32>; 5] = [Some(30), Some(90), Some(180), Some(365), None];

fn range_label(days: Option<u32>) -> String {
    match days {
        Some(d) => format!("last {d} days"),
        None => "all time".to_owned(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Date,
    Distance,
    Duration,
    Pace,
    Load,
}

impl SortKey {
    fn next(self) -> Self {
        match self {
            Self::Date => Self::Distance,
            Self::Distance => Self::Duration,
            Self::Duration => Self::Pace,
            Self::Pace => Self::Load,
            Self::Load => Self::Date,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Date => "date",
            Self::Distance => "distance",
            Self::Duration => "time",
            Self::Pace => "pace",
            Self::Load => "load",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FilterPopup {
    /// 0 = sport column, 1 = date column.
    column: u8,
    sport: usize,
    range: usize,
}

/// What the app should do after a message.
#[derive(Debug, Clone)]
pub enum Effect {
    Command(Command),
    Open(Box<ActivitySummary>),
}

#[derive(Debug)]
pub struct ActivitiesScreen {
    page_size: u32,
    today: NaiveDate,
    sport: usize,
    range: usize,
    pages: BTreeMap<u32, Vec<ActivitySummary>>,
    total_pages: u32,
    total_count: u32,
    /// The request in flight and the page it asked for.
    loading: Option<(RequestId, u32)>,
    error: Option<UiError>,
    fetched_at: Option<DateTime<Utc>>,
    from_cache: bool,
    selected: usize,
    selected_id: Option<LabelId>,
    search: Option<TextInput>,
    search_text: String,
    sort: SortKey,
    popup: Option<FilterPopup>,
    // Render-time scroll state (render takes &self).
    offset: Cell<usize>,
    viewport: Cell<usize>,
}

impl ActivitiesScreen {
    pub fn new(page_size: u32, default_range_days: u32, today: NaiveDate) -> Self {
        let range = RANGES
            .iter()
            .position(|r| *r == Some(default_range_days))
            .unwrap_or(1);
        Self {
            page_size: page_size.clamp(1, 200),
            today,
            sport: 0,
            range,
            pages: BTreeMap::new(),
            total_pages: 0,
            total_count: 0,
            loading: None,
            error: None,
            fetched_at: None,
            from_cache: false,
            selected: 0,
            selected_id: None,
            search: None,
            search_text: String::new(),
            sort: SortKey::Date,
            popup: None,
            offset: Cell::new(0),
            viewport: Cell::new(20),
        }
    }

    pub fn set_today(&mut self, today: NaiveDate) {
        self.today = today;
    }

    pub fn is_loading(&self) -> bool {
        self.loading.is_some()
    }

    /// While typing in search or using the filter popup, every key belongs here.
    pub fn captures_input(&self) -> bool {
        self.search.is_some() || self.popup.is_some()
    }

    pub fn is_searching(&self) -> bool {
        self.search.is_some()
    }

    pub fn has_filter_open(&self) -> bool {
        self.popup.is_some()
    }

    pub fn has_loaded(&self) -> bool {
        self.fetched_at.is_some()
    }

    /// Data age for the status bar.
    pub fn freshness(&self, now: DateTime<Utc>) -> Option<String> {
        let at = self.fetched_at?;
        let source = if self.from_cache { "cached" } else { "updated" };
        Some(format!("{source} {}", age_label(at, now)))
    }

    fn query(&self, page: u32) -> ActivityQuery {
        let days = RANGES[self.range];
        ActivityQuery {
            page,
            size: self.page_size,
            from: days.map(|d| coros_api::DateRange::last_days(self.today, d).start),
            to: days.map(|_| self.today),
            sports: SPORT_FILTERS[self.sport].sports.to_vec(),
        }
    }

    /// (Re)load from page 1. Existing rows stay visible until new ones arrive.
    pub fn start(&mut self, ids: &mut RequestIds, force: bool) -> Command {
        let req = ids.next();
        self.loading = Some((req, 1));
        self.error = None;
        Command::LoadActivities {
            req,
            query: self.query(1),
            force,
        }
    }

    fn all(&self) -> impl Iterator<Item = &ActivitySummary> {
        self.pages.values().flatten()
    }

    /// The rows on screen: search-filtered and sorted.
    pub fn visible(&self) -> Vec<&ActivitySummary> {
        let needle = self
            .search
            .as_ref()
            .map_or(self.search_text.as_str(), TextInput::value)
            .to_lowercase();
        let mut rows: Vec<&ActivitySummary> = self
            .all()
            .filter(|a| {
                needle.is_empty()
                    || a.name
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&needle)
                    || a.sport.label().to_lowercase().contains(&needle)
            })
            .collect();
        let desc = |x: Option<f64>| std::cmp::Reverse(x.map(|v| (v * 1000.0) as i64));
        match self.sort {
            SortKey::Date => {}
            SortKey::Distance => rows.sort_by_key(|a| desc(a.distance.map(|d| d.0))),
            SortKey::Duration => {
                rows.sort_by_key(|a| desc(a.moving.or(a.elapsed).map(|d| d.as_secs_f64())))
            }
            SortKey::Pace => rows.sort_by_key(|a| {
                a.avg_pace
                    .map_or(i64::MAX, |p| (p.secs_per_km() * 1000.0) as i64)
            }),
            SortKey::Load => rows.sort_by_key(|a| desc(a.training_load.map(f64::from))),
        }
        rows
    }

    pub fn selected(&self) -> Option<&ActivitySummary> {
        self.visible().get(self.selected).copied()
    }

    fn remember_selection(&mut self) {
        self.selected_id = self.selected().map(|a| a.id.clone());
    }

    /// After the rows change, find the previously selected activity again.
    fn restore_selection(&mut self) {
        let rows = self.visible();
        let index = self
            .selected_id
            .as_ref()
            .and_then(|id| rows.iter().position(|a| &a.id == id))
            .unwrap_or_else(|| self.selected.min(rows.len().saturating_sub(1)));
        drop(rows);
        self.selected = index;
        self.remember_selection();
    }

    pub fn on_loaded(
        &mut self,
        req: RequestId,
        page: Page<ActivitySummary>,
        fetched_at: DateTime<Utc>,
        from_cache: bool,
        refreshing: bool,
    ) -> bool {
        let Some((current, requested_page)) = self.loading else {
            return false;
        };
        if current != req {
            return false; // superseded by a newer request
        }
        if requested_page == 1 {
            self.pages.clear();
        }
        self.pages.insert(requested_page, page.items);
        self.total_pages = page.total_pages;
        self.total_count = page.total_count;
        self.fetched_at = Some(fetched_at);
        self.from_cache = from_cache;
        self.error = None;
        if !refreshing {
            self.loading = None;
        }
        self.restore_selection();
        true
    }

    /// Returns `true` if the failure belonged to this screen.
    pub fn on_failed(&mut self, req: RequestId, error: UiError) -> bool {
        if self.loading.map(|(r, _)| r) != Some(req) {
            return false;
        }
        self.loading = None;
        self.error = Some(error);
        true
    }

    /// Request the next page when the cursor nears the end.
    fn maybe_load_more(&mut self, ids: &mut RequestIds) -> Option<Command> {
        let next = self.pages.keys().max().map_or(1, |p| p + 1);
        let more = next <= self.total_pages;
        let near_end = self.selected + PREFETCH_MARGIN >= self.visible().len();
        if self.loading.is_some() || !more || !near_end || self.error.is_some() {
            return None;
        }
        let req = ids.next();
        self.loading = Some((req, next));
        Some(Command::LoadActivities {
            req,
            query: self.query(next),
            force: false,
        })
    }

    pub fn map_key(&self, key: KeyEvent) -> Option<ActivitiesMsg> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if self.popup.is_some() {
            return Some(ActivitiesMsg::Filter(match key.code {
                KeyCode::Up | KeyCode::Char('k') => FilterMsg::Move(Move::Up),
                KeyCode::Down | KeyCode::Char('j') => FilterMsg::Move(Move::Down),
                KeyCode::Tab
                | KeyCode::BackTab
                | KeyCode::Left
                | KeyCode::Right
                | KeyCode::Char('h' | 'l') => FilterMsg::SwitchColumn,
                KeyCode::Enter => FilterMsg::Apply,
                KeyCode::Esc | KeyCode::Char('q' | 'f') => FilterMsg::Cancel,
                _ => return None,
            }));
        }
        if self.search.is_some() {
            return Some(match key.code {
                KeyCode::Esc => ActivitiesMsg::EndSearch(false),
                KeyCode::Enter => ActivitiesMsg::EndSearch(true),
                KeyCode::Up => ActivitiesMsg::Move(Move::Up),
                KeyCode::Down => ActivitiesMsg::Move(Move::Down),
                KeyCode::Char('u') if ctrl => ActivitiesMsg::Search(Edit::ClearToStart),
                KeyCode::Char(c) if !ctrl => ActivitiesMsg::Search(Edit::Insert(c)),
                KeyCode::Backspace => ActivitiesMsg::Search(Edit::Backspace),
                KeyCode::Delete => ActivitiesMsg::Search(Edit::Delete),
                KeyCode::Left => ActivitiesMsg::Search(Edit::Left),
                KeyCode::Right => ActivitiesMsg::Search(Edit::Right),
                KeyCode::Home => ActivitiesMsg::Search(Edit::Home),
                KeyCode::End => ActivitiesMsg::Search(Edit::End),
                _ => return None,
            });
        }
        Some(match key.code {
            KeyCode::Up | KeyCode::Char('k') => ActivitiesMsg::Move(Move::Up),
            KeyCode::Down | KeyCode::Char('j') => ActivitiesMsg::Move(Move::Down),
            KeyCode::PageUp => ActivitiesMsg::Move(Move::PageUp),
            KeyCode::PageDown => ActivitiesMsg::Move(Move::PageDown),
            KeyCode::Char('u') if ctrl => ActivitiesMsg::Move(Move::PageUp),
            KeyCode::Char('d') if ctrl => ActivitiesMsg::Move(Move::PageDown),
            KeyCode::Home | KeyCode::Char('g') => ActivitiesMsg::Move(Move::Top),
            KeyCode::End | KeyCode::Char('G') => ActivitiesMsg::Move(Move::Bottom),
            KeyCode::Enter => ActivitiesMsg::Open,
            KeyCode::Char('/') => ActivitiesMsg::StartSearch,
            KeyCode::Char('f') => ActivitiesMsg::OpenFilter,
            KeyCode::Char('s') => ActivitiesMsg::CycleSort,
            KeyCode::Char('r') => ActivitiesMsg::Refresh,
            KeyCode::Esc if !self.search_text.is_empty() => ActivitiesMsg::EndSearch(false),
            _ => return None,
        })
    }

    pub fn update(&mut self, msg: ActivitiesMsg, ids: &mut RequestIds) -> Option<Effect> {
        match msg {
            ActivitiesMsg::Move(m) => {
                self.move_selection(m);
                return self.maybe_load_more(ids).map(Effect::Command);
            }
            ActivitiesMsg::Open => {
                return self.selected().cloned().map(|a| Effect::Open(Box::new(a)));
            }
            ActivitiesMsg::Refresh => return Some(Effect::Command(self.start(ids, true))),
            ActivitiesMsg::StartSearch => {
                self.search = Some(TextInput::with_value(&self.search_text))
            }
            ActivitiesMsg::Search(edit) => {
                if let Some(input) = &mut self.search {
                    input.apply(edit);
                }
                self.selected = 0;
                self.remember_selection();
            }
            ActivitiesMsg::EndSearch(keep) => {
                let typed = self.search.take().map(|s| s.value().to_owned());
                self.search_text = if keep {
                    typed.unwrap_or_else(|| self.search_text.clone())
                } else {
                    String::new()
                };
                self.restore_selection();
            }
            ActivitiesMsg::CycleSort => {
                self.sort = self.sort.next();
                self.restore_selection();
            }
            ActivitiesMsg::OpenFilter => {
                self.popup = Some(FilterPopup {
                    column: 0,
                    sport: self.sport,
                    range: self.range,
                });
            }
            ActivitiesMsg::Filter(msg) => return self.update_filter(msg, ids),
        }
        None
    }

    fn update_filter(&mut self, msg: FilterMsg, ids: &mut RequestIds) -> Option<Effect> {
        let popup = self.popup.as_mut()?;
        match msg {
            FilterMsg::SwitchColumn => popup.column = 1 - popup.column,
            FilterMsg::Move(m) => {
                let (index, len) = if popup.column == 0 {
                    (&mut popup.sport, SPORT_FILTERS.len())
                } else {
                    (&mut popup.range, RANGES.len())
                };
                *index = match m {
                    Move::Up => (*index + len - 1) % len,
                    Move::Down => (*index + 1) % len,
                    _ => *index,
                };
            }
            FilterMsg::Cancel => self.popup = None,
            FilterMsg::Apply => {
                let changed = (popup.sport, popup.range) != (self.sport, self.range);
                (self.sport, self.range) = (popup.sport, popup.range);
                self.popup = None;
                if changed {
                    self.pages.clear();
                    self.total_pages = 0;
                    self.total_count = 0;
                    self.selected = 0;
                    self.selected_id = None;
                    self.fetched_at = None;
                    return Some(Effect::Command(self.start(ids, false)));
                }
            }
        }
        None
    }

    fn move_selection(&mut self, m: Move) {
        let len = self.visible().len();
        if len == 0 {
            return;
        }
        let page = self.viewport.get().max(1);
        let last = len - 1;
        self.selected = match m {
            Move::Up => self.selected.saturating_sub(1),
            Move::Down => (self.selected + 1).min(last),
            Move::PageUp => self.selected.saturating_sub(page),
            Move::PageDown => (self.selected + page).min(last),
            Move::Top => 0,
            Move::Bottom => last,
        };
        self.remember_selection();
    }

    fn title(&self, rows: usize) -> String {
        let mut title = format!(
            " Activities · {} · {} ",
            SPORT_FILTERS[self.sport].label,
            range_label(RANGES[self.range])
        );
        if self.total_count > 0 {
            title.push_str(&format!("· {} total ", self.total_count));
        }
        if self.sort != SortKey::Date {
            title.push_str(&format!("· by {} (loaded rows) ", self.sort.label()));
        }
        let needle = self
            .search
            .as_ref()
            .map_or(self.search_text.as_str(), TextInput::value);
        if !needle.is_empty() {
            title.push_str(&format!(
                "· \"{needle}\": {rows} match{} ",
                if rows == 1 { "" } else { "es" }
            ));
        }
        title
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme, tick: u64) {
        let rows = self.visible();
        let block = Block::bordered()
            .border_style(theme.border)
            .title(Line::from(self.title(rows.len())).style(theme.title));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let show_search = self.search.is_some() || !self.search_text.is_empty();
        let [search_area, table_area, status_area] = Layout::vertical([
            Constraint::Length(u16::from(show_search)),
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .areas(inner);

        if show_search {
            let mut spans = vec![Span::styled(" / ", theme.accent)];
            match &self.search {
                Some(input) => spans.extend(
                    input
                        .line(search_area.width.saturating_sub(4), false, true, theme)
                        .spans,
                ),
                None => spans.push(Span::styled(self.search_text.clone(), theme.text)),
            }
            frame.render_widget(Line::from(spans), search_area);
        }

        if rows.is_empty() {
            let lines = if self.loading.is_some() {
                vec![Line::from(Span::styled(
                    format!("{} Loading activities…", spinner(tick)),
                    theme.accent,
                ))]
            } else if let Some(err) = &self.error {
                vec![
                    Line::from(Span::styled(err.message.clone(), theme.error)),
                    Line::from(""),
                    Line::from(Span::styled("Press r to retry.", theme.muted)),
                ]
            } else if show_search && self.has_loaded() {
                vec![Line::from(Span::styled(
                    "No loaded activity matches. Esc clears the search.",
                    theme.muted,
                ))]
            } else if self.has_loaded() {
                vec![
                    Line::from(Span::styled("No activities in this range.", theme.text)),
                    Line::from(Span::styled(
                        "Press f to change the sport or dates.",
                        theme.muted,
                    )),
                ]
            } else {
                vec![]
            };
            message(frame, table_area, lines);
        } else {
            self.render_table(frame, table_area, &rows, theme);
        }

        let loaded = self.all().count();
        let status = match self.loading {
            Some((_, page)) if page > 1 => {
                Span::styled(format!(" {} loading more…", spinner(tick)), theme.accent)
            }
            Some(_) if loaded > 0 => {
                Span::styled(format!(" {} refreshing…", spinner(tick)), theme.accent)
            }
            _ if loaded > 0 => {
                let more = if (self.pages.len() as u32) < self.total_pages {
                    " · scroll for more"
                } else {
                    ""
                };
                Span::styled(
                    format!(
                        " {loaded} of {} loaded{more}",
                        self.total_count.max(loaded as u32)
                    ),
                    theme.muted,
                )
            }
            _ => Span::raw(""),
        };
        frame.render_widget(Line::from(status), status_area);

        if let Some(p) = self.popup {
            self.render_filter(frame, area, p, theme);
        }
    }

    fn render_table(
        &self,
        frame: &mut Frame,
        area: Rect,
        rows: &[&ActivitySummary],
        theme: &Theme,
    ) {
        // Column sets by width (name takes what is left).
        let w = area.width;
        let wide = w >= 96;
        let medium = w >= 76;
        let mut header = vec!["Date"];
        let mut widths = vec![Constraint::Length(10)];
        if medium {
            header.push("Sport");
            widths.push(Constraint::Length(11));
        }
        header.extend(["Name", "Distance", "Time", "Pace"]);
        widths.extend([
            Constraint::Fill(1),
            Constraint::Length(9),
            Constraint::Length(7),
            Constraint::Length(10),
        ]);
        if medium {
            header.push("HR");
            widths.push(Constraint::Length(3));
        }
        if wide {
            header.push("Load");
            widths.push(Constraint::Length(4));
        }

        let body: Vec<Row> = rows
            .iter()
            .map(|a| {
                let date = a.date.or_else(|| a.start.map(|s| s.date_naive()));
                let mut cells = vec![TableCell::from(format::opt(date))];
                if medium {
                    cells.push(TableCell::from(sport_name(a.sport)));
                }
                cells.push(TableCell::from(
                    a.name.clone().unwrap_or_else(|| sport_name(a.sport)),
                ));
                cells.push(right(format::distance(a.distance)));
                cells.push(right(format::duration(a.moving.or(a.elapsed))));
                cells.push(right(format::pace(a.avg_pace, a.sport.category())));
                if medium {
                    cells.push(right(format::opt(a.avg_hr)));
                }
                if wide {
                    cells.push(right(format::opt(a.training_load)));
                }
                Row::new(cells).style(theme.text)
            })
            .collect();

        let header = Row::new(
            header
                .into_iter()
                .map(|h| TableCell::from(h).style(theme.header)),
        );
        let viewport = usize::from(area.height.saturating_sub(1)).max(1);
        self.viewport.set(viewport);
        let mut offset = self.offset.get();
        if self.selected < offset {
            offset = self.selected;
        } else if self.selected >= offset + viewport {
            offset = self.selected + 1 - viewport;
        }
        offset = offset.min(rows.len().saturating_sub(viewport));
        self.offset.set(offset);

        let mut state = TableState::default()
            .with_offset(offset)
            .with_selected(Some(self.selected));
        let table = Table::new(body, widths)
            .header(header)
            .column_spacing(1)
            .row_highlight_style(theme.selected)
            .highlight_symbol("▶ ")
            .highlight_spacing(HighlightSpacing::Always);
        frame.render_stateful_widget(table, area, &mut state);
    }

    fn render_filter(&self, frame: &mut Frame, area: Rect, p: FilterPopup, theme: &Theme) {
        let rect = centered(area, 46, 11);
        let inner = popup(frame, rect, "Filter", theme);
        let [left, right] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Fill(1)]).areas(inner);
        let mut column =
            |title: &str, items: Vec<String>, selected: usize, focused: bool, area: Rect| {
                let block = Block::new().title(Line::from(title.to_owned()).style(if focused {
                    theme.accent
                } else {
                    theme.muted
                }));
                let list = List::new(items.into_iter().map(ListItem::new))
                    .block(block)
                    .highlight_style(if focused { theme.selected } else { theme.title })
                    .highlight_symbol("› ");
                let mut state = ListState::default().with_selected(Some(selected));
                frame.render_stateful_widget(list, area, &mut state);
            };
        column(
            " Sport",
            SPORT_FILTERS.iter().map(|f| f.label.to_owned()).collect(),
            p.sport,
            p.column == 0,
            left,
        );
        column(
            " Dates",
            RANGES.iter().map(|r| range_label(*r)).collect(),
            p.range,
            p.column == 1,
            right,
        );
    }
}

fn right(text: String) -> TableCell<'static> {
    TableCell::from(Line::from(text).right_aligned())
}

pub fn sport_name(sport: SportType) -> String {
    match sport {
        SportType::Unknown(code) => format!("Other ({code})"),
        known => known.label().to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdata;

    fn screen() -> ActivitiesScreen {
        ActivitiesScreen::new(20, 90, testdata::today())
    }

    fn loaded(n: usize, total_pages: u32) -> (ActivitiesScreen, RequestIds) {
        let mut s = screen();
        let mut ids = RequestIds::default();
        let Command::LoadActivities { req, .. } = s.start(&mut ids, false) else {
            panic!()
        };
        s.on_loaded(
            req,
            testdata::page(1, n, total_pages),
            testdata::now(),
            false,
            false,
        );
        (s, ids)
    }

    #[test]
    fn first_load_query_uses_defaults() {
        let mut s = screen();
        let Command::LoadActivities { query, force, .. } =
            s.start(&mut RequestIds::default(), false)
        else {
            panic!()
        };
        assert_eq!(query.page, 1);
        assert_eq!(query.size, 20);
        assert_eq!(query.to, Some(testdata::today()));
        assert_eq!(
            query.from,
            Some(testdata::today() - chrono::Duration::days(89))
        );
        assert!(query.sports.is_empty() && !force);
        assert!(s.is_loading());
    }

    #[test]
    fn stale_answers_are_ignored() {
        let mut s = screen();
        let mut ids = RequestIds::default();
        let Command::LoadActivities { req: old, .. } = s.start(&mut ids, false) else {
            panic!()
        };
        let Command::LoadActivities { req: new, .. } = s.start(&mut ids, true) else {
            panic!()
        };
        assert!(!s.on_loaded(old, testdata::page(1, 3, 1), testdata::now(), false, false));
        assert!(s.visible().is_empty());
        assert!(s.on_loaded(new, testdata::page(1, 3, 1), testdata::now(), false, false));
        assert_eq!(s.visible().len(), 3);
    }

    #[test]
    fn cached_page_then_fresh_page_replaces_not_duplicates() {
        let mut s = screen();
        let mut ids = RequestIds::default();
        let Command::LoadActivities { req, .. } = s.start(&mut ids, false) else {
            panic!()
        };
        s.on_loaded(req, testdata::page(1, 5, 1), testdata::now(), true, true);
        assert!(s.is_loading(), "refresh still coming");
        assert_eq!(
            s.freshness(testdata::now()).as_deref(),
            Some("cached just now")
        );
        s.on_loaded(req, testdata::page(1, 6, 1), testdata::now(), false, false);
        assert_eq!(s.visible().len(), 6);
        assert!(!s.is_loading());
    }

    #[test]
    fn scrolling_near_the_end_loads_the_next_page_once() {
        let (mut s, mut ids) = loaded(20, 3);
        // 20 rows, prefetch margin 10: rows 1–9 don't trigger, row 10 does.
        for _ in 0..9 {
            assert!(
                s.update(ActivitiesMsg::Move(Move::Down), &mut ids)
                    .is_none()
            );
        }
        let Some(Effect::Command(Command::LoadActivities { query, .. })) =
            s.update(ActivitiesMsg::Move(Move::Down), &mut ids)
        else {
            panic!("expected next page");
        };
        assert_eq!(query.page, 2);
        assert!(
            s.update(ActivitiesMsg::Move(Move::Down), &mut ids)
                .is_none(),
            "one request at a time"
        );
        let (req, _) = s.loading.unwrap();
        s.on_loaded(req, testdata::page(2, 20, 3), testdata::now(), false, false);
        assert_eq!(s.visible().len(), 40);
        assert_eq!(s.selected, 11, "selection kept");
    }

    #[test]
    fn no_more_pages_no_more_requests() {
        let (mut s, mut ids) = loaded(5, 1);
        assert!(
            s.update(ActivitiesMsg::Move(Move::Bottom), &mut ids)
                .is_none()
        );
    }

    #[test]
    fn search_filters_loaded_rows_and_keeps_or_clears() {
        let (mut s, mut ids) = loaded(20, 1);
        s.update(ActivitiesMsg::StartSearch, &mut ids);
        assert!(s.captures_input());
        for c in "interval".chars() {
            s.update(ActivitiesMsg::Search(Edit::Insert(c)), &mut ids);
        }
        let n = s.visible().len();
        assert!(n > 0 && n < 20, "{n}");
        assert!(s.visible().iter().all(|a| {
            a.name
                .as_deref()
                .unwrap()
                .to_lowercase()
                .contains("interval")
        }));
        s.update(ActivitiesMsg::EndSearch(true), &mut ids);
        assert_eq!(s.visible().len(), n, "kept after Enter");
        assert!(!s.captures_input());
        assert_eq!(
            s.map_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            Some(ActivitiesMsg::EndSearch(false))
        );
        s.update(ActivitiesMsg::EndSearch(false), &mut ids);
        assert_eq!(s.visible().len(), 20);
    }

    #[test]
    fn sorting_cycles_and_keeps_the_selected_activity() {
        let (mut s, mut ids) = loaded(20, 1);
        s.update(ActivitiesMsg::Move(Move::Down), &mut ids);
        let picked = s.selected().unwrap().id.clone();
        s.update(ActivitiesMsg::CycleSort, &mut ids);
        assert_eq!(s.sort, SortKey::Distance);
        let d: Vec<f64> = s
            .visible()
            .iter()
            .filter_map(|a| a.distance.map(|m| m.0))
            .collect();
        assert!(d.windows(2).all(|w| w[0] >= w[1]), "longest first");
        assert_eq!(s.selected().unwrap().id, picked);
        s.update(ActivitiesMsg::CycleSort, &mut ids);
        s.update(ActivitiesMsg::CycleSort, &mut ids);
        assert_eq!(s.sort, SortKey::Pace);
        let p: Vec<f64> = s
            .visible()
            .iter()
            .filter_map(|a| a.avg_pace.map(|p| p.0))
            .collect();
        assert!(p.windows(2).all(|w| w[0] <= w[1]), "fastest first");
    }

    #[test]
    fn applying_a_filter_reloads_with_the_new_query() {
        let (mut s, mut ids) = loaded(20, 1);
        s.update(ActivitiesMsg::OpenFilter, &mut ids);
        s.update(ActivitiesMsg::Filter(FilterMsg::Move(Move::Down)), &mut ids); // Runs
        s.update(ActivitiesMsg::Filter(FilterMsg::SwitchColumn), &mut ids);
        for _ in 0..3 {
            s.update(ActivitiesMsg::Filter(FilterMsg::Move(Move::Down)), &mut ids); // 90 → 180 → 365 → all
        }
        let Some(Effect::Command(Command::LoadActivities { query, .. })) =
            s.update(ActivitiesMsg::Filter(FilterMsg::Apply), &mut ids)
        else {
            panic!("expected reload");
        };
        assert_eq!(query.sports, SportType::RUNS.to_vec());
        assert_eq!((query.from, query.to), (None, None), "all time");
        assert!(
            s.visible().is_empty(),
            "old rows cleared for a different query"
        );

        s.update(ActivitiesMsg::OpenFilter, &mut ids);
        assert!(
            s.update(ActivitiesMsg::Filter(FilterMsg::Apply), &mut ids)
                .is_none(),
            "unchanged = no reload"
        );
    }

    #[test]
    fn failures_are_shown_and_retryable() {
        let mut s = screen();
        let mut ids = RequestIds::default();
        let Command::LoadActivities { req, .. } = s.start(&mut ids, false) else {
            panic!()
        };
        assert!(s.on_failed(req, UiError::other("boom")));
        assert!(!s.is_loading());
        assert!(!s.on_failed(RequestId(999), UiError::other("not mine")));
        assert!(matches!(
            s.update(ActivitiesMsg::Refresh, &mut ids),
            Some(Effect::Command(Command::LoadActivities { force: true, .. }))
        ));
    }

    #[test]
    fn open_returns_the_selected_activity() {
        let (mut s, mut ids) = loaded(3, 1);
        s.update(ActivitiesMsg::Move(Move::Down), &mut ids);
        let Some(Effect::Open(a)) = s.update(ActivitiesMsg::Open, &mut ids) else {
            panic!()
        };
        assert_eq!(a.id, s.visible()[1].id);
    }
}
