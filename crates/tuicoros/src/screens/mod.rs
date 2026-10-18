//! Full-screen views. Each has `map_key` (key → message), `update`
//! (message → state change + effects) and `render`.

pub mod activities;
pub mod analysis;
pub mod dashboard;
pub mod detail;
pub mod detail_views;
pub mod login;

use chrono::{DateTime, Utc};

use crate::action::{RequestId, UiError};
use crate::service::age_label;

/// How a fetched value arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoadMeta {
    pub fetched_at: DateTime<Utc>,
    pub from_cache: bool,
    /// A network refresh for the same request follows.
    pub refreshing: bool,
}

/// One remote value: the data (kept while refreshing), the request in
/// flight, the last error, and when it was fetched.
#[derive(Debug)]
pub struct Slot<T> {
    value: Option<T>,
    req: Option<RequestId>,
    meta: Option<LoadMeta>,
    error: Option<UiError>,
}

impl<T> Default for Slot<T> {
    fn default() -> Self {
        Self {
            value: None,
            req: None,
            meta: None,
            error: None,
        }
    }
}

impl<T> Slot<T> {
    pub fn value(&self) -> Option<&T> {
        self.value.as_ref()
    }

    pub fn is_loading(&self) -> bool {
        self.req.is_some()
    }

    pub fn error(&self) -> Option<&UiError> {
        self.error.as_ref()
    }

    pub fn owns(&self, req: RequestId) -> bool {
        self.req == Some(req)
    }

    /// A new request starts; the old value stays visible until it's replaced.
    pub fn request(&mut self, req: RequestId) {
        self.req = Some(req);
        self.error = None;
    }

    /// Forget the value too (the question changed, e.g. a different date range).
    pub fn reset(&mut self, req: RequestId) {
        self.value = None;
        self.meta = None;
        self.request(req);
    }

    pub fn accept(&mut self, req: RequestId, value: T, meta: LoadMeta) -> bool {
        if !self.owns(req) {
            return false;
        }
        self.value = Some(value);
        self.meta = Some(meta);
        self.error = None;
        if !meta.refreshing {
            self.req = None;
        }
        true
    }

    pub fn fail(&mut self, req: RequestId, error: UiError) -> bool {
        if !self.owns(req) {
            return false;
        }
        self.req = None;
        self.error = Some(error);
        true
    }

    /// "cached 3 min ago" / "updated just now".
    pub fn freshness(&self, now: DateTime<Utc>) -> Option<String> {
        let meta = self.meta?;
        let source = if meta.from_cache { "cached" } else { "updated" };
        Some(format!("{source} {}", age_label(meta.fetched_at, now)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdata;

    fn meta(refreshing: bool) -> LoadMeta {
        LoadMeta {
            fetched_at: testdata::now(),
            from_cache: true,
            refreshing,
        }
    }

    #[test]
    fn slot_lifecycle() {
        let mut slot: Slot<u32> = Slot::default();
        slot.request(RequestId(1));
        assert!(slot.is_loading());
        assert!(!slot.accept(RequestId(2), 5, meta(false)), "not mine");
        assert!(slot.accept(RequestId(1), 5, meta(true)));
        assert!(slot.is_loading(), "refresh still coming");
        assert!(slot.accept(RequestId(1), 6, meta(false)));
        assert_eq!((slot.value(), slot.is_loading()), (Some(&6), false));
        assert_eq!(
            slot.freshness(testdata::now()).as_deref(),
            Some("cached just now")
        );

        slot.request(RequestId(3));
        assert!(slot.fail(RequestId(3), UiError::other("x")));
        assert_eq!(slot.value(), Some(&6), "old value kept on error");
        slot.reset(RequestId(4));
        assert_eq!(slot.value(), None);
    }
}
