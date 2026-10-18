//! Everything that can change app state (docs/ARCHITECTURE.md §3).
//!
//! Key presses become screen messages; finished background work comes back as
//! the `*Loaded` / `*Failed` variants. `App::update` is the only consumer.

use chrono::{DateTime, Utc};
use coros_api::{
    ActivityDetail, ActivitySummary, CorosError, Daily, Dashboard, Page, Region, Schedule,
};
use serde::{Deserialize, Serialize};

use crate::app::Tab;

/// Identifies one request so late answers to superseded requests are dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct RequestId(pub u64);

/// Hands out increasing request ids.
#[derive(Debug, Default)]
pub struct RequestIds(u64);

impl RequestIds {
    pub fn next(&mut self) -> RequestId {
        self.0 += 1;
        RequestId(self.0)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Quit,
    /// Periodic tick (spinners, toast expiry).
    Tick,
    Resize,
    ToggleHelp,
    SelectTab(Tab),
    NextTab,
    PrevTab,

    Login(LoginMsg),
    Activities(ActivitiesMsg),
    Detail(DetailMsg),
    Dashboard(DashboardMsg),
    Analysis(AnalysisMsg),

    // Results of background work.
    ActivitiesLoaded {
        req: RequestId,
        page: Page<ActivitySummary>,
        fetched_at: DateTime<Utc>,
        from_cache: bool,
        /// A network refresh for the same request follows (cached data was stale).
        refreshing: bool,
    },
    DetailLoaded {
        req: RequestId,
        detail: Box<ActivityDetail>,
        fetched_at: DateTime<Utc>,
        from_cache: bool,
    },
    /// Result of a `Command::Fetch` (dashboard, daily metrics, schedule, …).
    Fetched {
        req: RequestId,
        data: Box<Fetched>,
        fetched_at: DateTime<Utc>,
        from_cache: bool,
        /// A network refresh for the same request follows (cached data was stale).
        refreshing: bool,
    },
    RequestFailed {
        req: RequestId,
        error: UiError,
    },
    LoginSucceeded {
        who: String,
        region: Region,
    },
    LoginFailed {
        message: String,
    },
}

/// Data delivered by `Command::Fetch`. Also the on-disk cache format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Fetched {
    Dashboard(Dashboard),
    Daily(Daily),
    Schedule(Schedule),
    /// Every activity in a date range (for weekly totals).
    Activities(Vec<ActivitySummary>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashboardMsg {
    Move(Move),
    Open,
    Refresh,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisMsg {
    /// `[`: move the window back by its length.
    Earlier,
    /// `]`: move it forward (not past today).
    Later,
    /// `w`: 4 → 12 → 24 weeks.
    CycleWeeks,
    Refresh,
}

/// Text-field editing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    Insert(char),
    Backspace,
    Delete,
    Left,
    Right,
    Home,
    End,
    /// Ctrl+U: delete everything before the cursor.
    ClearToStart,
}

/// Cursor movement in lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    Up,
    Down,
    PageUp,
    PageDown,
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginMsg {
    Edit(Edit),
    NextField,
    PrevField,
    /// ←/→ on the region selector.
    CycleRegion(i8),
    /// Space on the remember-me box.
    Toggle,
    Submit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivitiesMsg {
    Move(Move),
    Open,
    Refresh,
    StartSearch,
    Search(Edit),
    /// Leave the search box, keeping (`true`) or clearing the filter.
    EndSearch(bool),
    CycleSort,
    OpenFilter,
    Filter(FilterMsg),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterMsg {
    Move(Move),
    SwitchColumn,
    Apply,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailMsg {
    Back,
    NextTab,
    PrevTab,
    Move(Move),
    CycleLapGroup,
    /// Charts: next metric (HR, pace, elevation, cadence, power).
    CycleMetric,
    /// Charts: distance ↔ time on the x axis.
    ToggleAxis,
    Refresh,
}

/// An error, reduced to what the UI needs. Details go to the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiError {
    pub kind: UiErrorKind,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiErrorKind {
    /// The token was rejected and could not be renewed: show the login screen.
    SessionExpired,
    RateLimited,
    Network,
    Other,
}

impl UiError {
    #[cfg(test)]
    pub fn other(message: impl Into<String>) -> Self {
        Self {
            kind: UiErrorKind::Other,
            message: message.into(),
        }
    }
}

impl From<&CorosError> for UiError {
    fn from(err: &CorosError) -> Self {
        let (kind, message) = match err {
            CorosError::TokenInvalid { .. } | CorosError::NotAuthenticated => (
                UiErrorKind::SessionExpired,
                "Your session expired. Sign in again.".to_owned(),
            ),
            CorosError::RateLimited => (
                UiErrorKind::RateLimited,
                "COROS is rate limiting requests. Try again in a minute.".to_owned(),
            ),
            CorosError::Network { source, .. } if source.is_timeout() => (
                UiErrorKind::Network,
                "COROS didn't answer in time. Check your connection and retry.".to_owned(),
            ),
            CorosError::Network { .. } => (
                UiErrorKind::Network,
                "Can't reach COROS. Check your connection and retry.".to_owned(),
            ),
            CorosError::Api {
                result, message, ..
            } => (
                UiErrorKind::Other,
                format!("COROS error {result}: {message}"),
            ),
            CorosError::Http { status, .. } => (
                UiErrorKind::Other,
                format!("COROS answered HTTP {status}. Try again later."),
            ),
            CorosError::Unexpected { .. } => (
                UiErrorKind::Other,
                "COROS sent something unexpected (details in the log).".to_owned(),
            ),
            CorosError::InvalidInput(msg) => (UiErrorKind::Other, msg.clone()),
        };
        Self { kind, message }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_ids_increase() {
        let mut ids = RequestIds::default();
        let (a, b) = (ids.next(), ids.next());
        assert!(b > a);
    }

    #[test]
    fn errors_map_to_ui_kinds() {
        let expired = UiError::from(&CorosError::TokenInvalid { region: Region::Us });
        assert_eq!(expired.kind, UiErrorKind::SessionExpired);
        assert_eq!(
            UiError::from(&CorosError::RateLimited).kind,
            UiErrorKind::RateLimited
        );
        let api = UiError::from(&CorosError::Api {
            endpoint: "x".into(),
            result: "1001".into(),
            message: "Service exceptions".into(),
            tlog_id: None,
        });
        assert_eq!(api.message, "COROS error 1001: Service exceptions");
    }
}
