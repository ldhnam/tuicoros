//! Everything that can change app state. See `docs/ARCHITECTURE.md` §3.

use crate::app::Tab;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    SelectTab(Tab),
    NextTab,
    PrevTab,
    /// The terminal was resized; redraw.
    Resize,
}
