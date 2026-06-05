//! `chorale-core`: framework-agnostic headless table state.
//!
//! ## Headless contract
//!
//! `chorale-core` owns everything that does not require a running UI: the
//! complete table state struct, pure immutable state transitions, and derived
//! view functions for sorting, filtering, pagination, and virtualization.
//! Framework adapters (`chorale-dioxus`, `chorale-leptos`) wrap this crate
//! and wire the state into their reactive model.
//!
//! ## Architectural commitments
//!
//! - **[CHORALE-CORE-1]:** zero UI or framework dependencies. `chorale-core`
//!   may depend on `serde`, `thiserror`, `rust_decimal`, `chrono`, `uuid`,
//!   and similar data-layer crates; never on `dioxus`, `leptos`, `yew`, or
//!   any rendering crate.
//! - **[CHORALE-CORE-2]:** all state transitions are pure immutable functions.
//!   Every transition takes `&TableState<TRow>` and returns a fresh
//!   `TableState<TRow>`. No `&mut self`. No async. No signal types. This
//!   gives reactive systems first-class change-detection and makes every
//!   transition unit-testable without a framework runtime.
//!
//! ## Quick start
//!
//! Build a `Vec<ColumnDef<TRow>>` describing your columns, call
//! [`TableState::new`] with your rows, then pass the state to your
//! framework adapter's hook (e.g. `chorale_dioxus::use_table`).
//! See the [repository README] for a complete working example.
//!
//! [CHORALE-CORE-1]: https://github.com/zernst3/rust-chorale/blob/main/docs/CONVENTIONS.md
//! [CHORALE-CORE-2]: https://github.com/zernst3/rust-chorale/blob/main/docs/CONVENTIONS.md
//! [repository README]: https://github.com/zernst3/rust-chorale#quick-start

#![warn(missing_docs)]

mod column;
mod error;
mod labels;
mod state;
mod theme;
pub mod transitions;
mod types;
pub mod views;

// ---- Core state -----------------------------------------------------------

/// The complete, serializable state for one table instance.
///
/// Contains rows, column definitions, active sort, filters, pagination,
/// selection, column visibility/width overrides, and virtualization
/// parameters. All state transitions take `&TableState<TRow>` and return
/// a fresh `TableState<TRow>` (CHORALE-CORE-2).
///
/// Start with [`TableState::new`] for sensible defaults.
pub use state::TableState;

/// Result of [`visible_window`] / [`visible_window_for_state`]: the row-index
/// range to render and the top/bottom spacer heights for the virtual scroll
/// container.
pub use state::VirtualWindow;

// ---- Column definition ----------------------------------------------------

/// Definition for a single column: id, header label, accessor closure,
/// sort/filter/render configuration, and optional width/alignment overrides.
///
/// The `accessor: Arc<dyn Fn(&TRow) -> CellValue>` is the only place in
/// `chorale-core` that knows the row type's internal structure.
pub use column::ColumnDef;

/// What kind of inline editor the adapter should render for an editable column.
///
/// Set via `ColumnDef::editor(kind)`. Default is `None` (column read-only).
/// Variants: `Text`, `Number { min, max, step }`, `Date`, `BoolToggle`, `Custom`.
pub use column::EditorKind;

/// Declares the filter UI and matching strategy for a column.
///
/// Pair with a [`FilterValue`] variant of the same kind. Default is
/// `FilterKind::None` (column not filterable).
pub use column::FilterKind;

/// Declares the default cell rendering style for a column.
///
/// Adapters use this together with the cell's [`CellValue`] to decide how
/// to render a cell (text, number, currency, date, boolean, or badge).
pub use column::RenderKind;

/// A single colored pill variant for [`RenderKind::Badge`] columns:
/// a display label and a CSS color token (e.g. `"green"`, `"red"`).
pub use column::BadgeVariant;

/// Maps cell text values to [`BadgeVariant`]s. Used by [`RenderKind::Badge`].
///
/// Build with `BadgeVariantMap::new().with("Active", ...)`.
pub use column::BadgeVariantMap;

// ---- Types ----------------------------------------------------------------

/// Opaque stable identifier for a row. Backed by a UUID.
///
/// Stable across sort, filter, and pagination so selection and edits survive
/// reordering. Create with [`RowId::new()`].
pub use types::RowId;

/// Opaque identifier for a column. Backed by a `&'static str`.
///
/// Zero-cost to copy and usable as a `HashMap` key. Construct inline:
/// `ColumnId("my_column")`.
pub use types::ColumnId;

/// The typed value returned by a column accessor.
///
/// Used for sort comparisons, filter matching, CSV serialization, and
/// adapter rendering. Variants: `Text`, `Integer`, `Float`, `Boolean`,
/// `Date`, `DateTime`, `Empty`.
pub use types::CellValue;

/// Identifies the cell currently open for in-cell editing.
///
/// Stored in `TableState::editing: Option<EditTarget>`.
/// `start_edit` sets it; `commit_edit` and `cancel_edit` clear it.
pub use types::EditTarget;

/// Snapshot of a cell's prior state for optimistic-edit rollback.
///
/// Returned in [`CommittedEdit::prior`]. Pass back to [`revert_edit`] from an
/// async persistence-failure path to undo the committed change.
pub use types::PriorEdit;

/// Payload delivered to the adapter's `on_commit_edit` callback.
///
/// Contains the raw string value the user typed and a [`PriorEdit`] snapshot
/// for optional rollback via [`revert_edit`].
pub use types::CommittedEdit;

/// Current filter value for a column, paired with a [`FilterKind`].
///
/// Variants: `Text`, `NumericRange`, `DateRange`, `MultiSelect`, `Boolean`.
/// Pass to [`set_filter`] to apply.
pub use types::FilterValue;

/// Sort direction for a column: `Asc` or `Desc`.
pub use types::SortDirection;

/// Active sort on a single column: `column: ColumnId` + `direction: SortDirection`.
pub use types::SortState;

/// Horizontal text alignment for a column: `Left` (default), `Center`, `Right`.
pub use types::Alignment;

/// ISO 4217 currency code used by [`RenderKind::Currency`].
///
/// Predefined constants: `CurrencyCode::USD`, `CurrencyCode::EUR`,
/// `CurrencyCode::GBP`.
pub use types::CurrencyCode;

// ---- Errors ---------------------------------------------------------------

/// Errors from fallible state transitions (`set_page`, `set_page_size`,
/// `set_column_width`).
///
/// One variant per distinct failure mode (ROBUSTNESS-1):
/// `PageOutOfRange`, `PageSizeZero`, `InvalidColumnWidth`.
pub use error::StateError;

// ---- Labels ---------------------------------------------------------------

/// All user-visible strings the table renders. Override any field to customise.
///
/// Construct with [`Labels::default`] and mutate the fields you want to change.
/// `Labels` is `#[non_exhaustive]` so future minor releases can add new fields
/// without breaking existing callsites.
pub use labels::Labels;

// ---- Theming --------------------------------------------------------------

/// Visual theme applied by the adapter: `Light` (default), `Dark`, or `Custom`.
///
/// `Custom` suppresses the injected stylesheet; the host app supplies its
/// own CSS targeting the structural class names (`chorale-row`, `chorale-cell`, etc.).
pub use theme::Theme;

/// Row metadata passed to a [`RowClassFn`] to compute per-row CSS classes.
pub use theme::Row;

/// Cell metadata passed to a [`CellClassFn`] to compute per-cell CSS classes.
pub use theme::CellInfo;

/// Closure type for per-row dynamic CSS class resolution.
///
/// `Arc<dyn Fn(&Row<TRow>) -> String + Send + Sync>`.
pub use theme::RowClassFn;

/// Closure type for per-cell dynamic CSS class resolution.
///
/// `Arc<dyn Fn(&CellInfo<TRow>) -> String + Send + Sync>`.
pub use theme::CellClassFn;

// ---- Transitions ----------------------------------------------------------

/// Cycle sort on `col`: none → ASC → DESC → none. Resets page and scroll.
pub use transitions::toggle_sort;

/// Set or clear the filter on `col`. Resets page and scroll.
pub use transitions::set_filter;

/// Jump to page `page` (zero-based). Returns `Err(PageOutOfRange)` if out of range.
pub use transitions::set_page;

/// Change rows per page. Returns `Err(PageSizeZero)` if `size == 0`.
pub use transitions::set_page_size;

/// Set or clear the selection state of a single row (idempotent).
pub use transitions::set_selection;

/// Toggle between "select all visible rows" and "select none".
pub use transitions::toggle_select_all;

/// Show or hide a column.
pub use transitions::set_column_visibility;

/// Override a column's width in pixels. Returns `Err` if `width_px <= 0`.
pub use transitions::set_column_width;

/// Update the scroll offset of the virtualized scroll container (px).
pub use transitions::set_scroll;

/// Replace a row's data in-place by `RowId` (the cell-editing escape valve).
pub use transitions::update_row;

/// Record a measured row height (px) for the variable-row-height cache.
///
/// `index` is the row's zero-based position in the current page's
/// `visible_view` output. The cache is invalidated automatically by
/// [`toggle_sort`], [`set_filter`], and [`set_page`].
pub use transitions::record_row_height;

/// Open an editor for a cell. Returns `Err(ColumnNotEditable)` if the column
/// has no `EditorKind`. Opening a second cell cancels the first implicitly.
pub use transitions::start_edit;

/// Close the editor after a commit. Clears `editing`; does not update row data
/// (the host's `on_commit_edit` callback is responsible for persistence).
pub use transitions::commit_edit;

/// Cancel the editor without persisting. Clears `editing`. No-op if no edit
/// is in progress.
pub use transitions::cancel_edit;

/// Roll back a previously-committed edit using the [`PriorEdit`] snapshot.
/// No-op if the row was deleted between commit and the persistence callback.
pub use transitions::revert_edit;

/// Move the edit cursor to the next editable column in the same row (Tab).
/// Wraps to the first after the last. No-op if no edit is in progress.
pub use transitions::next_editable_cell;

/// Move the edit cursor to the previous editable column in the same row
/// (Shift+Tab). Wraps to the last before the first. No-op if no edit in progress.
pub use transitions::prev_editable_cell;

/// Merge a batch of measured row heights into the cache in one transition.
///
/// Equivalent to calling [`record_row_height`] for every entry but produces
/// only one `TableState` clone. The adapter measurement loop uses this to
/// avoid N signal writes for an N-row virtual window.
pub use transitions::batch_record_row_heights;

/// Clear the variable-row-height cache (e.g. on data reload).
///
/// [`toggle_sort`], [`set_filter`], and [`set_page`] call this implicitly.
pub use transitions::clear_row_height_cache;

// ---- Views ----------------------------------------------------------------

/// Post-filter / post-sort / post-pagination `(RowId, TRow)` pairs for the
/// current page. The **preferred** view for adapters — runs the pipeline
/// once and provides both IDs and rows.
pub use views::visible_view;

/// Post-filter / post-sort / post-pagination row data for the current page.
/// Prefer [`visible_view`] when row IDs are also needed.
pub use views::visible_rows;

/// `RowId`s for the current page. Prefer [`visible_view`] when row data is
/// also needed.
pub use views::visible_row_ids;

/// Post-filter / post-sort rows for ALL pages (no pagination). Used by `to_csv`.
pub use views::filtered_sorted_rows;

/// Compute the fixed-height virtual window (row-index range + spacer heights)
/// for a given `scroll_top`, `viewport_height`, `row_height`, and `total_rows`.
pub use views::visible_window;

/// Convenience: compute the virtual window AND the row slice for the current state.
pub use views::visible_window_for_state;

/// Compute the variable-height virtual window using a per-row height cache.
///
/// Drop-in companion to [`visible_window`] for tables where rows may have
/// different heights. Pass `&state.row_heights` as the first argument.
/// Rows not yet measured fall back to `default_row_height`. The window
/// geometry uses a prefix-sum array (O(n)) and binary search.
///
/// Per VIRT-2 (v0.2.0 Item 6, signed off 2026-06-04).
pub use views::visible_window_variable;

/// Serialize the post-filter / post-sort view (all pages) to an RFC 4180 CSV string.
pub use views::to_csv;

// Re-export third-party types used in the public surface so adapter crates
// don't need to add `chrono` to their Cargo.toml.
/// `chrono::NaiveDate` re-exported for use in [`CellValue::Date`] and filter values
/// without requiring adapter crates to declare a `chrono` dependency.
pub use chrono::NaiveDate;
