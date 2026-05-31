//! `chorale-core`: framework-agnostic table state and logic.
//!
//! `chorale-core` is a pure-logic crate with no UI or framework dependency
//! (CHORALE-CORE-1). It provides:
//!
//! - **`TableState<TRow>`** — the complete, serializable table state struct.
//! - **Transition functions** — immutable-returns-new state transitions
//!   (CHORALE-CORE-2), e.g. `toggle_sort`, `set_filter`, `set_page`.
//! - **Derived views** — `visible_rows`, `visible_window`, `to_csv`.
//!
//! Adapters (`chorale-dioxus`, future `chorale-leptos`) consume this crate
//! and wire the state into their framework's reactive model.

mod column;
mod error;
mod state;
mod theme;
pub mod transitions;
mod types;
pub mod views;

// ---- public re-exports ----------------------------------------------------

pub use column::{BadgeVariant, BadgeVariantMap, ColumnDef, FilterKind, RenderKind};
pub use error::StateError;
pub use state::{TableState, VirtualWindow};
pub use theme::{CellClassFn, CellInfo, Row, RowClassFn, Theme};
pub use transitions::{
    set_column_visibility, set_column_width, set_filter, set_page, set_page_size, set_scroll,
    set_selection, toggle_select_all, toggle_sort, update_row,
};
pub use types::{
    Alignment, CellValue, ColumnId, CurrencyCode, FilterValue, RowId, SortDirection, SortState,
};

// Re-export third-party types used in `chorale-core`'s public surface so
// adapter crates don't need to add `chrono` or `uuid` to their Cargo.toml.
pub use chrono::NaiveDate;
pub use views::{
    filtered_sorted_rows, to_csv, visible_row_ids, visible_rows, visible_view, visible_window,
    visible_window_for_state,
};
