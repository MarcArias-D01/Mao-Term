//! UI logic for Mao-Term: geometry, split layout, focus navigation, fuzzy search.
//!
//! Pure logic, no GPU or window: everything here is unit-testable.
//! See `docs/specs/workspace-layout.md`.

pub mod fuzzy;
mod geom;
pub mod split;

pub use geom::Rect;
pub use split::{Direction, Divider, PaneId, SplitDir, SplitTree};
