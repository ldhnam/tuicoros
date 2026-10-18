//! Unofficial client for the private COROS Training Hub API (the backend behind
//! <https://t.coros.com>).
//!
//! This crate must stay free of terminal/UI dependencies. See
//! `docs/COROS_API.md` for what is known about the API and
//! `docs/ARCHITECTURE.md` for how this crate is meant to grow (Phase 1).

mod region;

pub use region::{ParseRegionError, Region};
