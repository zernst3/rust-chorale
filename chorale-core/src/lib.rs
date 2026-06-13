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

pub mod clipboard;
mod column;
mod error;
mod labels;
pub mod range;
mod state;
mod theme;
pub mod transitions;
mod types;
pub mod views;
pub mod xlsx;

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

/// Which edge a column is pinned to: `None` (scrollable, the default),
/// `Left`, or `Right`.
///
/// Set via [`ColumnDef::frozen`]. Adapters use [`frozen_left_columns`],
/// [`scrollable_columns`], and [`frozen_right_columns`] to split the column
/// list into rendering zones.
pub use column::FrozenSide;

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

/// The cell that currently holds keyboard focus (row + column), if any.
///
/// Stored in `TableState::active_cell: Option<ActiveCell>`. `None` on mount;
/// set by `set_active_cell`, `move_active_cell`, and related transitions.
pub use types::ActiveCell;

/// Navigation direction for active-cell transitions.
///
/// Passed to `move_active_cell`, `move_active_cell_to_edge`, and
/// `move_active_cell_page`.
pub use types::NavDirection;

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

/// Whether a sort toggle should replace the sort list or append to it.
///
/// Passed to [`toggle_sort`]. `Replace` for plain click (single-column semantics);
/// `Append` for Shift+click (multi-column: adds/flips/removes without clearing others).
pub use types::SortAction;

/// Whether the table renders in discrete pages or accumulates rows via infinite scroll.
///
/// `Pages` (default): the classic pager — `set_page`, `total_pages`, and the
/// pagination bar are active. `InfiniteScroll`: `visible_view` returns the
/// first `loaded_row_count` rows; `load_more_rows` grows that window by
/// `page_size` on each adapter scroll-threshold event.
pub use types::PaginationMode;

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

/// Errors from fallible state transitions.
///
/// One variant per distinct failure mode (ROBUSTNESS-1):
/// `PageOutOfRange`, `PageSizeZero`, `InvalidColumnWidth`,
/// `ColumnNotEditable`, `UnknownColumnId`, `DuplicateColumnId`,
/// `InvalidModeForTransition`.
pub use error::StateError;

// ---- Range selection -------------------------------------------------------

/// A rectangular cell range defined by an anchor and focus corner.
///
/// Stored in `TableState::range_selection: Vec<RangeSelection>`. An empty
/// vec means no range is active. Multi-element vec = disjoint (Ctrl+click)
/// ranges. Resolve to a row/column extent via [`RangeSelection::normalized`].
pub use range::RangeSelection;

/// The resolved min-row / max-row / ordered-columns extent of a `RangeSelection`.
///
/// Returned by [`RangeSelection::normalized`].
pub use range::NormalizedRange;

/// Errors from range operations: `NoRangeSelected`, `MultiRectNotSupportedForThisOperation`,
/// `RangeTooSmallToFill`, `IndexOutOfBounds`.
pub use range::RangeError;

/// Compute fill-handle write targets from a source range and drag endpoint.
///
/// Returns `(visible_row_idx, column_id, CellValue)` for each cell in the
/// extension area. Core detects arithmetic progressions on numeric columns and
/// cycles non-uniform or text sequences. The adapter converts this list into a
/// TSV payload and fires `on_paste`.
pub use range::fill_handle_targets;

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

/// Built-in light + dark `--chorale-*` token stylesheet, injected once per
/// document by the adapters. Scoped to
/// `.chorale-root[data-chorale-theme="light"|"dark"]` so a runtime theme
/// toggle is a single attribute swap.
pub use theme::theme_stylesheet;

/// Attribute name (`data-chorale-theme`) the adapters set on the table root
/// to select a theme block; values come from [`Theme::attribute_value`].
pub use theme::THEME_ATTRIBUTE;

/// Class name (`chorale-root`) the shipped stylesheet scopes its token
/// blocks under; adapters place it on the table root element.
pub use theme::THEME_ROOT_CLASS;

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

/// Cycle sort on `col` using `action` (Replace or Append). Resets page and scroll.
pub use transitions::toggle_sort;

/// Remove a specific column from the active sort list. No-op if not sorted.
pub use transitions::remove_sort;

/// Clear all active sort columns.
pub use transitions::clear_sort;

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

/// Select every row currently on the visible page (excluding detail panels).
pub use transitions::select_all_visible_page;

/// Select every row in the filtered + sorted set (across all pages).
pub use transitions::select_all_filtered;

/// Deselect every row currently on the visible page, leaving other-page selections intact.
pub use transitions::deselect_all_visible_page;

/// Clear the entire selection across all pages.
pub use transitions::deselect_all;

/// Show or hide a column.
pub use transitions::set_column_visibility;

/// Override a column's width in pixels. Returns `Err` if `width_px <= 0`.
pub use transitions::set_column_width;

/// Remove the explicit width override for a column, falling back to `initial_width` or table default.
pub use transitions::reset_column_width;

/// Update the scroll offset of the virtualized scroll container (px).
pub use transitions::set_scroll;

/// Replace a row's data in-place by `RowId` (the cell-editing escape valve).
pub use transitions::update_row;

/// Replace the entire row set (streaming full-refresh).
pub use transitions::set_rows;

/// Insert a single row at a position (caller supplies the `RowId`).
pub use transitions::insert_row;

/// Append rows to the end of the row set (streaming new records).
pub use transitions::append_rows;

/// Remove a single row by `RowId` (no-op if absent).
pub use transitions::remove_row;

/// Remove multiple rows by `RowId` in one transition.
pub use transitions::remove_rows;

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

/// Set an explicit column render order. Validates that every id exists and has
/// no duplicates; returns `Err` otherwise.
pub use transitions::set_column_order;

/// Move a column to a new index in the render order. Clamps out-of-bounds
/// `to_index`. Returns `Err(UnknownColumnId)` if `column_id` is not found.
pub use transitions::move_column;

/// Reset to definition order by clearing `column_order`.
pub use transitions::reset_column_order;

/// Switch the table between `PaginationMode::Pages` and `PaginationMode::InfiniteScroll`.
///
/// Switching to `InfiniteScroll` initialises `loaded_row_count = page_size` and resets
/// page/scroll. Switching to `Pages` clears `loaded_row_count` and resets page/scroll.
pub use transitions::set_pagination_mode;

/// Increase `loaded_row_count` by `page_size` (capped at filtered row count).
///
/// Only valid in `PaginationMode::InfiniteScroll`; returns
/// `Err(InvalidModeForTransition)` in `Pages` mode. Adapters call this from
/// a scroll-threshold handler to implement infinite-scroll load-more.
pub use transitions::load_more_rows;

/// Set the columns to group by (outermost-first). Clears collapsed state.
///
/// Pass an empty vec to remove all grouping. Resets page and scroll.
pub use transitions::set_grouping;

/// Toggle the collapsed/expanded state of a group identified by `key`.
///
/// Collapsed groups show their header row but hide data children.
/// Obtain `key` values from [`GroupedRow::Header::key`] in the output of
/// [`visible_grouped_view`].
pub use transitions::toggle_group;

/// Expand all groups (clear `collapsed_groups`).
pub use transitions::expand_all_groups;

/// Collapse all groups by inserting every group header key into `collapsed_groups`.
///
/// No-op when `state.grouping` is empty.
pub use transitions::collapse_all_groups;

/// Toggle whether a row is expanded (showing a detail panel below it).
///
/// Mirrors `toggle_group`: if `row_id` is in `expanded_rows`, it is removed
/// (row collapses). Otherwise it is inserted (row expands).
pub use transitions::toggle_row_expansion;

/// Collapse all expanded rows (clear `expanded_rows`).
pub use transitions::collapse_all_rows;

// ---- Item 15: Active-cell transitions -------------------------------------

/// Set the active cell to a specific visible-row index and column.
///
/// Returns `Err(StateError::RowIndexOutOfBounds)` or `Err(StateError::ColumnNotFound)`
/// on invalid input.
pub use transitions::set_active_cell;

/// Move the active cell one step in `direction`. Clamps at boundaries.
/// If `active_cell` is `None`, moves to the first or last cell depending on direction.
pub use transitions::move_active_cell;

/// Move the active cell to the data edge in `direction` (Ctrl+Arrow behavior).
pub use transitions::move_active_cell_to_edge;

/// Move the active cell by `page_size` rows Up or Down (Page Up/Down behavior).
pub use transitions::move_active_cell_page;

/// Move the active cell to the first column of the current row (Home key).
pub use transitions::move_active_cell_home;

/// Move the active cell to the last column of the current row (End key).
pub use transitions::move_active_cell_end;

/// Move the active cell to the absolute first visible cell (Ctrl+Home).
pub use transitions::move_active_cell_first;

/// Move the active cell to the absolute last visible cell (Ctrl+End).
pub use transitions::move_active_cell_last;

/// Clear the active cell (returns state with `active_cell: None`). Idempotent.
pub use transitions::clear_active_cell;

// ---- Item 16: Range selection transitions ---------------------------------

/// Begin a new range anchored at the given cell (replaces existing range).
/// Also sets `active_cell` to the anchor. Row index is clamped to bounds.
pub use transitions::start_range_selection;

/// Extend the active (last) range so its focus moves to the given cell.
/// If no range exists, behaves like `start_range_selection`.
pub use transitions::extend_range_to;

/// Add a disjoint range (Ctrl+click). Subsequent `extend_range_to` extends the new range.
pub use transitions::add_disjoint_range;

/// Select all visible rows × all visible columns (Ctrl+A). Idempotent.
pub use transitions::select_all;

/// Clear all ranges (Escape when not editing). Idempotent.
pub use transitions::clear_range_selection;

// ---- Item 17: Clipboard ---------------------------------------------------

/// Errors from clipboard operations: `MultiRectCopyNotSupported`, `NoRangeSelected`,
/// `MultiRectPasteNotSupported`.
pub use clipboard::ClipboardError;

/// Payload for the adapter's `on_copy` callback: the TSV string and the copied range.
pub use clipboard::ClipboardCopyEvent;

/// Payload for the adapter's `on_paste` callback: the TSV string and the effective target range.
pub use clipboard::ClipboardPasteEvent;

/// Serialize the active single-rect range selection to a tab-separated (TSV) string.
///
/// Returns `Ok("")` when no range is selected. Returns
/// `Err(ClipboardError::MultiRectCopyNotSupported)` for disjoint (Ctrl+click) selections.
/// Cells containing a tab are wrapped in double-quotes; cells with embedded newlines
/// have the newline replaced with a space (minimal Excel-compatible escaping).
pub use clipboard::to_clipboard_tsv;

/// Parse a TSV string and adjust the active range to match the payload size.
///
/// Returns a new `TableState` with the `range_selection` expanded when the payload
/// is larger than the original selection, or unchanged when the payload fits within
/// the range (tile behavior is the host's responsibility via the `on_paste` callback).
/// Row data is **not** mutated in core — the host applies per-cell writes.
///
/// Empty or all-whitespace `tsv` is a no-op.
pub use clipboard::paste_tsv_into_range;

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

/// Post-filter / post-sort `(RowId, TRow)` pairs for ALL pages (no pagination).
///
/// Used by bulk-selection transitions and XLSX export.
pub use views::filtered_sorted_pairs;

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
pub use views::visible_window_variable;

/// Serialize the post-filter / post-sort view (all pages) to an RFC 4180 CSV string.
pub use views::to_csv;

/// Columns pinned to the left edge, in effective column order. Excludes hidden columns.
///
/// Combine with [`scrollable_columns`] and [`frozen_right_columns`] to produce
/// the full left-to-right render order for adapters.
pub use views::frozen_left_columns;

/// Columns pinned to the right edge, in effective column order. Excludes hidden columns.
pub use views::frozen_right_columns;

/// Scrollable (non-frozen) columns, in effective column order. Excludes hidden columns.
///
/// The union of `frozen_left_columns`, `scrollable_columns`, and
/// `frozen_right_columns` equals all visible columns.
pub use views::scrollable_columns;

/// Interleaved group-header + data rows for the current grouping state.
///
/// When `state.grouping` is empty, every item is `GroupedRow::Data` and
/// this degrades gracefully to the flat view. Adapters match on
/// `GroupedRow::Header` / `GroupedRow::Data` to render group separators.
///
/// Pagination applies to data rows only in `DataRowsOnly` mode (the default).
/// In `Virtualized` mode the full tree is returned without slicing.
pub use views::visible_grouped_view;

/// A row in the grouped view: either a `Header` (group separator) or `Data` row.
///
/// Returned by [`visible_grouped_view`]. `#[non_exhaustive]`.
pub use views::GroupedRow;

/// What the renderer should draw for one virtualized row slot (`Data` or `DetailPanel`).
///
/// Returned by [`visible_view`] to support master/detail layouts.
pub use views::RenderRow;

/// How to aggregate rows within a group for a column (Sum/Average/Count/Min/Max/Custom).
///
/// Set via [`ColumnDef::aggregator`]. Appears in [`GroupedRow::Header::aggregates`].
pub use column::AggregatorKind;

/// How pagination interacts with grouped rows: `DataRowsOnly` (default) or `Virtualized`.
///
/// Set via `TableState::grouped_pagination`.
pub use types::GroupedPaginationMode;

/// Opaque group identifier, built from the concatenated values of the group-by columns.
///
/// Obtain from [`GroupedRow::Header::key`]; pass to [`toggle_group`] to
/// collapse/expand that group.
pub use types::GroupKey;

// Re-export third-party types used in the public surface so adapter crates
// don't need to add `chrono` to their Cargo.toml.
/// `chrono::NaiveDate` re-exported for use in [`CellValue::Date`] and filter values
/// without requiring adapter crates to declare a `chrono` dependency.
pub use chrono::NaiveDate;

// ---- Item 18: XLSX export (feature = "xlsx") ---------------------------------

/// XLSX export error: `SerializationError(String)`.
///
/// Only available when the `xlsx` feature is enabled.
#[cfg(feature = "xlsx")]
pub use xlsx::XlsxError;

/// Options for XLSX export: `sheet_name` and `bold_headers`.
///
/// `#[non_exhaustive]` with `Default` impl (`Sheet1`, bold = `true`).
/// Only available when the `xlsx` feature is enabled.
#[cfg(feature = "xlsx")]
pub use xlsx::XlsxOptions;

/// Export the filtered+sorted table view to raw XLSX bytes.
///
/// Respects filter, sort, column visibility, and column order (same semantics
/// as `to_csv`). Auto-inferred styling: bold headers, number/date/currency
/// formats, column widths, and freeze panes from `ColumnDef.frozen`.
///
/// # Errors
///
/// Returns [`XlsxError::SerializationError`] if `rust_xlsxwriter` fails.
///
/// Only available when the `xlsx` feature is enabled.
#[cfg(feature = "xlsx")]
pub use xlsx::to_xlsx;
