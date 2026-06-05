//! Pure state-transition functions for `TableState<TRow>`.
//!
//! Every function follows the CHORALE-CORE-2 immutable pattern:
//! `fn name(state: &TableState<TRow>, ...) -> TableState<TRow>`.
//! No `&mut self`. No signals. No async. Unit-testable without a framework.

use std::collections::HashMap;

use crate::error::StateError;
use crate::state::TableState;
use crate::types::{ColumnId, EditTarget, FilterValue, PriorEdit, RowId, SortDirection, SortState};

/// Cycle through sort states for `col`: none → ASC → DESC → none.
///
/// If a different column is currently sorted, replaces it with ASC on `col`.
/// Resets `scroll_top` and `page` to 0 so virtualization re-anchors after
/// reorder (recon-2 § 5).
///
/// # Example
///
/// ```rust
/// use chorale_core::{TableState, ColumnId, toggle_sort};
///
/// let state: TableState<String> = TableState::new(vec![], vec![]);
/// // First toggle: no sort → ASC.
/// let s1 = toggle_sort(&state, ColumnId("name"));
/// assert!(!s1.sort.is_empty());
/// // Second toggle: ASC → DESC.
/// let s2 = toggle_sort(&s1, ColumnId("name"));
/// // Third toggle: DESC → none.
/// let s3 = toggle_sort(&s2, ColumnId("name"));
/// assert!(s3.sort.is_empty());
/// ```
#[must_use]
pub fn toggle_sort<TRow: Clone>(state: &TableState<TRow>, col: ColumnId) -> TableState<TRow> {
    let next_sort = match state.sort.first() {
        Some(s) if s.column == col => match s.direction {
            SortDirection::Asc => vec![SortState::new(col, SortDirection::Desc)],
            SortDirection::Desc => vec![],
        },
        _ => vec![SortState::new(col, SortDirection::Asc)],
    };
    // Clear the variable-row-height cache: row indices shift on sort (VIRT-2).
    TableState {
        sort: next_sort,
        scroll_top: 0.0,
        page: 0,
        row_heights: HashMap::new(),
        ..state.clone()
    }
}

/// Set or clear the active filter for `col`.
///
/// `filter = None` removes any existing filter on the column.
/// Resets `scroll_top` and `page` to 0 because the row count may change
/// (recon-2 § 5).
///
/// # Example
///
/// ```rust
/// use chorale_core::{TableState, ColumnId, FilterValue, set_filter};
///
/// let state: TableState<String> = TableState::new(vec![], vec![]);
/// // Apply a text filter.
/// let filtered = set_filter(&state, ColumnId("name"), Some(FilterValue::Text("alice".into())));
/// assert!(filtered.filters.contains_key(&ColumnId("name")));
/// assert_eq!(filtered.page, 0);
/// // Clear it.
/// let cleared = set_filter(&filtered, ColumnId("name"), None);
/// assert!(cleared.filters.is_empty());
/// ```
#[must_use]
pub fn set_filter<TRow: Clone>(
    state: &TableState<TRow>,
    col: ColumnId,
    filter: Option<FilterValue>,
) -> TableState<TRow> {
    let mut filters = state.filters.clone();
    match filter {
        Some(f) => {
            filters.insert(col, f);
        }
        None => {
            filters.remove(&col);
        }
    }
    // Clear the variable-row-height cache: filtered row set (and indices) change (VIRT-2).
    TableState {
        filters,
        scroll_top: 0.0,
        page: 0,
        row_heights: HashMap::new(),
        ..state.clone()
    }
}

/// Set the selection state of a single row.
///
/// `selected = true` adds `row_id` to the selection (idempotent).
/// `selected = false` removes it (idempotent).
#[must_use]
pub fn set_selection<TRow: Clone>(
    state: &TableState<TRow>,
    row_id: RowId,
    selected: bool,
) -> TableState<TRow> {
    let mut selection = state.selection.clone();
    if selected {
        if !selection.contains(&row_id) {
            selection.push(row_id);
        }
    } else {
        selection.retain(|id| *id != row_id);
    }
    TableState {
        selection,
        ..state.clone()
    }
}

/// Toggle between "select all visible rows" and "select none".
///
/// If all currently visible row IDs (post-sort/post-filter/post-pagination)
/// are already selected, this deselects all. Otherwise it selects all
/// visible rows.
#[must_use]
pub fn toggle_select_all<TRow: Clone>(state: &TableState<TRow>) -> TableState<TRow> {
    let visible_ids: Vec<RowId> = crate::views::visible_row_ids(state);
    let all_selected =
        !visible_ids.is_empty() && visible_ids.iter().all(|id| state.selection.contains(id));

    let selection = if all_selected {
        // Deselect all visible rows; keep any out-of-page selections
        state
            .selection
            .iter()
            .copied()
            .filter(|id| !visible_ids.contains(id))
            .collect()
    } else {
        // Add all visible IDs that aren't already selected
        let mut sel = state.selection.clone();
        for id in &visible_ids {
            if !sel.contains(id) {
                sel.push(*id);
            }
        }
        sel
    };

    TableState {
        selection,
        ..state.clone()
    }
}

/// Jump to page `page` (zero-based).
///
/// # Errors
///
/// Returns `Err(StateError::PageOutOfRange)` if `page >= total_pages()`.
pub fn set_page<TRow: Clone>(
    state: &TableState<TRow>,
    page: usize,
) -> Result<TableState<TRow>, StateError> {
    let total = state.total_pages();
    if page >= total {
        return Err(StateError::PageOutOfRange(page));
    }
    // Clear the variable-row-height cache: page boundary shifts all view indices (VIRT-2).
    Ok(TableState {
        page,
        scroll_top: 0.0,
        row_heights: HashMap::new(),
        ..state.clone()
    })
}

/// Change the number of rows per page.
///
/// Resets to page 0 because the new page boundaries are different.
///
/// # Errors
///
/// Returns `Err(StateError::PageSizeZero)` if `size == 0`.
pub fn set_page_size<TRow: Clone>(
    state: &TableState<TRow>,
    size: usize,
) -> Result<TableState<TRow>, StateError> {
    if size == 0 {
        return Err(StateError::PageSizeZero);
    }
    Ok(TableState {
        page_size: size,
        page: 0,
        scroll_top: 0.0,
        ..state.clone()
    })
}

/// Show or hide a column.
#[must_use]
pub fn set_column_visibility<TRow: Clone>(
    state: &TableState<TRow>,
    col: ColumnId,
    visible: bool,
) -> TableState<TRow> {
    let mut column_visibility = state.column_visibility.clone();
    column_visibility.insert(col, visible);
    TableState {
        column_visibility,
        ..state.clone()
    }
}

/// Override the width of a column in pixels (for resize handles).
///
/// # Errors
///
/// Returns `Err(StateError::InvalidColumnWidth)` if `width_px <= 0`.
pub fn set_column_width<TRow: Clone>(
    state: &TableState<TRow>,
    col: ColumnId,
    width_px: f64,
) -> Result<TableState<TRow>, StateError> {
    if width_px <= 0.0 {
        return Err(StateError::InvalidColumnWidth(width_px.to_string()));
    }
    let mut column_widths = state.column_widths.clone();
    column_widths.insert(col, width_px);
    Ok(TableState {
        column_widths,
        ..state.clone()
    })
}

/// Update the scroll position of the virtualized scroll container (px).
///
/// Called by the adapter's `onscroll` handler. Pure: the adapter owns the
/// DOM listener; this function just updates state (recon-2 § 3).
#[must_use]
pub fn set_scroll<TRow: Clone>(state: &TableState<TRow>, scroll_top: f64) -> TableState<TRow> {
    TableState {
        scroll_top,
        ..state.clone()
    }
}

/// Replace a row's data in-place, identified by `row_id`.
///
/// If `row_id` is not found, the state is returned unchanged.
/// Defined in recon-2 § 7d as the cell-editing escape valve.
#[must_use]
pub fn update_row<TRow: Clone>(
    state: &TableState<TRow>,
    row_id: RowId,
    new_row: TRow,
) -> TableState<TRow> {
    let mut rows = state.rows.clone();
    if let Some(slot) = rows.iter_mut().find(|(id, _)| *id == row_id) {
        slot.1 = new_row;
    }
    TableState {
        rows,
        ..state.clone()
    }
}

/// Record the measured height (px) for row at `index` in the current page view.
///
/// The adapter calls this after DOM measurement (VIRT-2). `index` is the row's
/// zero-based position in the post-filter/sort/paginated `visible_view` output
/// for the current page. The cache is invalidated automatically by
/// [`toggle_sort`], [`set_filter`], and [`set_page`].
#[must_use]
pub fn record_row_height<TRow: Clone>(
    state: &TableState<TRow>,
    index: usize,
    height: f64,
) -> TableState<TRow> {
    let mut row_heights = state.row_heights.clone();
    row_heights.insert(index, height);
    TableState {
        row_heights,
        ..state.clone()
    }
}

/// Merge a batch of measured row heights into the cache in a single transition.
///
/// Equivalent to calling [`record_row_height`] for every entry in `heights`
/// but produces only one clone of `TableState` instead of one per entry.
/// The adapter measurement loop uses this to avoid N signal writes for an
/// N-row virtual window.
#[must_use]
pub fn batch_record_row_heights<TRow: Clone, S: std::hash::BuildHasher>(
    state: &TableState<TRow>,
    heights: &HashMap<usize, f64, S>,
) -> TableState<TRow> {
    if heights.is_empty() {
        return state.clone();
    }
    let mut row_heights = state.row_heights.clone();
    row_heights.extend(heights.iter().map(|(k, v)| (*k, *v)));
    TableState {
        row_heights,
        ..state.clone()
    }
}

/// Clear the variable-row-height cache.
///
/// Called when the row set changes in a way that invalidates cached heights
/// (e.g., after a data reload or a transition not already covered by
/// [`toggle_sort`] / [`set_filter`] / [`set_page`]).
#[must_use]
pub fn clear_row_height_cache<TRow: Clone>(state: &TableState<TRow>) -> TableState<TRow> {
    TableState {
        row_heights: HashMap::new(),
        ..state.clone()
    }
}

// ---------------------------------------------------------------------------
// In-cell editing transitions (v0.2.0 Item 7)
// ---------------------------------------------------------------------------

/// Open an editor for the cell at (`row_id`, `column_id`).
///
/// Opening a second cell while one is already open implicitly cancels the
/// first (no orphaned edit lock).
///
/// # Errors
///
/// Returns `Err(StateError::ColumnNotEditable)` if the target column has no
/// `EditorKind` configured.
#[must_use = "returns a new state; the original is unchanged"]
pub fn start_edit<TRow: Clone>(
    state: &TableState<TRow>,
    row_id: RowId,
    column_id: ColumnId,
) -> Result<TableState<TRow>, StateError> {
    let has_editor = state
        .columns
        .iter()
        .any(|c| c.id == column_id && c.editor.is_some());
    if !has_editor {
        return Err(StateError::ColumnNotEditable(column_id));
    }
    Ok(TableState {
        editing: Some(EditTarget { row_id, column_id }),
        ..state.clone()
    })
}

/// Close the editor after a successful commit. Returns a state with
/// `editing: None`. Does **not** update row data; the caller is responsible
/// for calling `update_row` (or letting the host's `on_commit_edit` callback
/// handle persistence).
#[must_use]
pub fn commit_edit<TRow: Clone>(state: &TableState<TRow>) -> TableState<TRow> {
    TableState {
        editing: None,
        ..state.clone()
    }
}

/// Cancel the editor without persisting. Returns a state with `editing: None`.
/// No-op (returns a clone) if no edit is in progress.
#[must_use]
pub fn cancel_edit<TRow: Clone>(state: &TableState<TRow>) -> TableState<TRow> {
    if state.editing.is_none() {
        return state.clone();
    }
    TableState {
        editing: None,
        ..state.clone()
    }
}

/// Roll back a previously-committed edit. Restores the full prior row so the
/// host can call this from an async persistence-failure path.
///
/// No-op (returns a clone) if the row is no longer present (it was deleted
/// between commit and the persistence callback firing).
#[must_use]
pub fn revert_edit<TRow: Clone>(
    state: &TableState<TRow>,
    prior: &PriorEdit<TRow>,
) -> TableState<TRow> {
    if !state.rows.iter().any(|(id, _)| *id == prior.row_id) {
        return state.clone();
    }
    update_row(state, prior.row_id, prior.prior_row.clone())
}

/// Move the edit cursor to the next editable column in the same row (Tab).
///
/// Cycles within the row's editable columns: after the last editable column,
/// wraps to the first. No-op if no edit is currently in progress or if there
/// are no editable columns.
#[must_use]
pub fn next_editable_cell<TRow: Clone>(state: &TableState<TRow>) -> TableState<TRow> {
    let Some(current) = state.editing else {
        return state.clone();
    };
    let editable: Vec<ColumnId> = state
        .columns
        .iter()
        .filter(|c| c.editor.is_some())
        .map(|c| c.id)
        .collect();
    if editable.is_empty() {
        return state.clone();
    }
    let next = match editable.iter().position(|&c| c == current.column_id) {
        Some(i) if i + 1 < editable.len() => editable[i + 1],
        _ => editable[0],
    };
    TableState {
        editing: Some(EditTarget {
            row_id: current.row_id,
            column_id: next,
        }),
        ..state.clone()
    }
}

/// Move the edit cursor to the previous editable column in the same row
/// (Shift+Tab).
///
/// Cycles within the row's editable columns: before the first editable column,
/// wraps to the last. No-op if no edit is currently in progress or if there
/// are no editable columns.
#[must_use]
pub fn prev_editable_cell<TRow: Clone>(state: &TableState<TRow>) -> TableState<TRow> {
    let Some(current) = state.editing else {
        return state.clone();
    };
    let editable: Vec<ColumnId> = state
        .columns
        .iter()
        .filter(|c| c.editor.is_some())
        .map(|c| c.id)
        .collect();
    if editable.is_empty() {
        return state.clone();
    }
    let prev = match editable.iter().position(|&c| c == current.column_id) {
        Some(0) | None => editable[editable.len() - 1],
        Some(i) => editable[i - 1],
    };
    TableState {
        editing: Some(EditTarget {
            row_id: current.row_id,
            column_id: prev,
        }),
        ..state.clone()
    }
}

// ---------------------------------------------------------------------------
// Tests (TESTS-1: every transition has a unit test asserting the result)
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_precision_loss
)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::column::ColumnDef;
    use crate::state::TableState;
    use crate::types::{Alignment, CellValue, ColumnId, FilterValue, RowId, SortDirection};

    // ---- helpers -----------------------------------------------------------

    #[derive(Clone, Debug, PartialEq)]
    struct TestRow {
        name: String,
        score: f64,
    }

    fn col_name() -> ColumnId {
        ColumnId("name")
    }
    fn col_score() -> ColumnId {
        ColumnId("score")
    }

    fn make_columns() -> Vec<ColumnDef<TestRow>> {
        vec![
            ColumnDef::new(col_name(), "Name", |r: &TestRow| {
                CellValue::Text(r.name.clone())
            })
            .sortable()
            .filter(crate::column::FilterKind::Text),
            ColumnDef::new(col_score(), "Score", |r: &TestRow| {
                CellValue::Float(r.score)
            })
            .sortable()
            .filter(crate::column::FilterKind::Text)
            .alignment(Alignment::Right)
            .render_kind(crate::column::RenderKind::Number),
        ]
    }

    fn make_state() -> TableState<TestRow> {
        let rows = vec![
            (
                RowId::new(),
                TestRow {
                    name: "Alice".into(),
                    score: 90.0,
                },
            ),
            (
                RowId::new(),
                TestRow {
                    name: "Bob".into(),
                    score: 75.0,
                },
            ),
            (
                RowId::new(),
                TestRow {
                    name: "Charlie".into(),
                    score: 85.0,
                },
            ),
        ];
        TableState {
            rows,
            columns: make_columns(),
            sort: vec![],
            filters: HashMap::new(),
            selection: vec![],
            page: 0,
            page_size: 10,
            column_visibility: HashMap::new(),
            column_widths: HashMap::new(),
            editing: None,
            row_heights: HashMap::new(),
            scroll_top: 0.0,
            viewport_height: 500.0,
            row_height: 40.0,
            buffer_rows: 3,
        }
    }

    fn make_editable_columns() -> Vec<ColumnDef<TestRow>> {
        vec![
            ColumnDef::new(col_name(), "Name", |r: &TestRow| {
                CellValue::Text(r.name.clone())
            })
            .editor(crate::column::EditorKind::Text),
            ColumnDef::new(col_score(), "Score", |r: &TestRow| {
                CellValue::Float(r.score)
            })
            .editor(crate::column::EditorKind::Number {
                min: Some(0.0),
                max: Some(100.0),
                step: Some(1.0),
            }),
        ]
    }

    fn make_editable_state() -> TableState<TestRow> {
        let mut s = make_state();
        s.columns = make_editable_columns();
        s
    }

    // ---- toggle_sort -------------------------------------------------------

    #[test]
    fn toggle_sort_none_to_asc() {
        let s = make_state();
        let s2 = toggle_sort(&s, col_name());
        assert_eq!(
            s2.sort,
            vec![SortState::new(col_name(), SortDirection::Asc)]
        );
        assert_eq!(s2.scroll_top, 0.0);
        assert_eq!(s2.page, 0);
    }

    #[test]
    fn toggle_sort_asc_to_desc() {
        let s = make_state();
        let s = toggle_sort(&s, col_name());
        let s2 = toggle_sort(&s, col_name());
        assert_eq!(
            s2.sort,
            vec![SortState::new(col_name(), SortDirection::Desc)]
        );
    }

    #[test]
    fn toggle_sort_desc_to_none() {
        let s = make_state();
        let s = toggle_sort(&s, col_name());
        let s = toggle_sort(&s, col_name());
        let s2 = toggle_sort(&s, col_name());
        assert!(s2.sort.is_empty());
    }

    #[test]
    fn toggle_sort_different_column_resets_to_asc() {
        let s = make_state();
        let s = toggle_sort(&s, col_name());
        let s2 = toggle_sort(&s, col_score());
        assert_eq!(
            s2.sort,
            vec![SortState::new(col_score(), SortDirection::Asc)]
        );
    }

    // ---- set_filter --------------------------------------------------------

    #[test]
    fn set_filter_adds_filter() {
        let s = make_state();
        let s2 = set_filter(&s, col_name(), Some(FilterValue::Text("ali".into())));
        assert_eq!(
            s2.filters.get(&col_name()),
            Some(&FilterValue::Text("ali".into()))
        );
        assert_eq!(s2.page, 0);
        assert_eq!(s2.scroll_top, 0.0);
    }

    #[test]
    fn set_filter_removes_filter() {
        let s = make_state();
        let s = set_filter(&s, col_name(), Some(FilterValue::Text("ali".into())));
        let s2 = set_filter(&s, col_name(), None);
        assert!(!s2.filters.contains_key(&col_name()));
    }

    // ---- set_selection -----------------------------------------------------

    #[test]
    fn set_selection_adds_and_removes() {
        let s = make_state();
        let id = s.rows[0].0;
        let s2 = set_selection(&s, id, true);
        assert!(s2.selection.contains(&id));
        let s3 = set_selection(&s2, id, false);
        assert!(!s3.selection.contains(&id));
    }

    #[test]
    fn set_selection_idempotent_add() {
        let s = make_state();
        let id = s.rows[0].0;
        let s = set_selection(&s, id, true);
        let s2 = set_selection(&s, id, true);
        assert_eq!(s2.selection.iter().filter(|&&i| i == id).count(), 1);
    }

    // ---- toggle_select_all -------------------------------------------------

    #[test]
    fn toggle_select_all_selects_all_when_none_selected() {
        let s = make_state();
        let s2 = toggle_select_all(&s);
        assert_eq!(s2.selection.len(), s.rows.len());
    }

    #[test]
    fn toggle_select_all_deselects_all_when_all_selected() {
        let s = make_state();
        let s = toggle_select_all(&s); // select all
        let s2 = toggle_select_all(&s); // deselect all
        assert!(s2.selection.is_empty());
    }

    // ---- set_page ----------------------------------------------------------

    #[test]
    fn set_page_valid() {
        let mut s = make_state();
        s.page_size = 2; // 3 rows → 2 pages (0, 1)
        let s2 = set_page(&s, 1).expect("page 1 should be valid");
        assert_eq!(s2.page, 1);
        assert_eq!(s2.scroll_top, 0.0);
    }

    #[test]
    fn set_page_out_of_range() {
        let s = make_state();
        let err = set_page(&s, 99).unwrap_err();
        assert_eq!(err, StateError::PageOutOfRange(99));
    }

    // ---- set_page_size -----------------------------------------------------

    #[test]
    fn set_page_size_valid() {
        let s = make_state();
        let s2 = set_page_size(&s, 5).expect("size 5 is valid");
        assert_eq!(s2.page_size, 5);
        assert_eq!(s2.page, 0);
    }

    #[test]
    fn set_page_size_zero_is_error() {
        let s = make_state();
        let err = set_page_size(&s, 0).unwrap_err();
        assert_eq!(err, StateError::PageSizeZero);
    }

    // ---- set_column_visibility ---------------------------------------------

    #[test]
    fn set_column_visibility_hides_and_shows() {
        let s = make_state();
        assert!(s.is_column_visible(col_name()));
        let s2 = set_column_visibility(&s, col_name(), false);
        assert!(!s2.is_column_visible(col_name()));
        let s3 = set_column_visibility(&s2, col_name(), true);
        assert!(s3.is_column_visible(col_name()));
    }

    // ---- set_column_width --------------------------------------------------

    #[test]
    fn set_column_width_valid() {
        let s = make_state();
        let s2 = set_column_width(&s, col_name(), 200.0).expect("valid width");
        assert_eq!(s2.column_widths.get(&col_name()), Some(&200.0));
    }

    #[test]
    fn set_column_width_zero_is_error() {
        let s = make_state();
        assert!(set_column_width(&s, col_name(), 0.0).is_err());
    }

    #[test]
    fn set_column_width_negative_is_error() {
        let s = make_state();
        assert!(set_column_width(&s, col_name(), -10.0).is_err());
    }

    // ---- set_scroll --------------------------------------------------------

    #[test]
    fn set_scroll_updates_scroll_top() {
        let s = make_state();
        let s2 = set_scroll(&s, 120.5);
        assert!((s2.scroll_top - 120.5).abs() < f64::EPSILON);
    }

    // ---- update_row --------------------------------------------------------

    #[test]
    fn update_row_replaces_matching_row() {
        let s = make_state();
        let id = s.rows[0].0;
        let new_row = TestRow {
            name: "Alice (updated)".into(),
            score: 99.0,
        };
        let s2 = update_row(&s, id, new_row.clone());
        assert_eq!(s2.rows[0].1, new_row);
        // Other rows unchanged
        assert_eq!(s2.rows[1].1, s.rows[1].1);
    }

    #[test]
    fn update_row_unknown_id_leaves_state_unchanged() {
        let s = make_state();
        let unknown = RowId::new();
        let new_row = TestRow {
            name: "Ghost".into(),
            score: 0.0,
        };
        let s2 = update_row(&s, unknown, new_row);
        assert_eq!(s2.rows[0].1, s.rows[0].1);
    }

    // ---- toggle_select_all with active filter ----------------------------

    #[test]
    fn toggle_select_all_only_selects_visible_page_rows() {
        // 3 rows, page_size=2 → page 0 has rows[0] and rows[1], page 1 has rows[2].
        let s = make_state();
        let mut s2 = s.clone();
        s2.page_size = 2;
        let s3 = toggle_select_all(&s2);
        // Only the first 2 rows (the visible page) should be selected.
        assert_eq!(s3.selection.len(), 2);
        assert!(!s3.selection.contains(&s2.rows[2].0));
    }

    #[test]
    fn set_scroll_is_idempotent_at_same_position() {
        // set_scroll with the same value should return a state equal to
        // calling it once (the transition is deterministic and pure).
        let s = make_state();
        let s1 = set_scroll(&s, 100.0);
        let s2 = set_scroll(&s1, 100.0);
        assert!((s2.scroll_top - 100.0).abs() < f64::EPSILON);
    }

    // ---- record_row_height ------------------------------------------------

    #[test]
    fn record_row_height_inserts_entry() {
        let s = make_state();
        let s2 = record_row_height(&s, 3, 55.5);
        assert_eq!(s2.row_heights.get(&3), Some(&55.5));
        // All other fields unchanged.
        assert_eq!(s2.rows.len(), s.rows.len());
        assert!(s2.sort.is_empty());
    }

    #[test]
    fn record_row_height_overwrites_existing_entry() {
        let s = make_state();
        let s = record_row_height(&s, 0, 40.0);
        let s2 = record_row_height(&s, 0, 60.0);
        assert_eq!(s2.row_heights.get(&0), Some(&60.0));
    }

    #[test]
    fn record_row_height_does_not_mutate_input() {
        let s = make_state();
        let _ = record_row_height(&s, 0, 50.0);
        assert!(s.row_heights.is_empty());
    }

    // ---- clear_row_height_cache -------------------------------------------

    #[test]
    fn clear_row_height_cache_empties_map() {
        let s = make_state();
        let s = record_row_height(&s, 0, 40.0);
        let s = record_row_height(&s, 1, 80.0);
        assert_eq!(s.row_heights.len(), 2);
        let s2 = clear_row_height_cache(&s);
        assert!(s2.row_heights.is_empty());
    }

    #[test]
    fn clear_row_height_cache_idempotent_on_empty() {
        let s = make_state();
        assert!(s.row_heights.is_empty());
        let s2 = clear_row_height_cache(&s);
        assert!(s2.row_heights.is_empty());
    }

    #[test]
    fn clear_row_height_cache_preserves_other_fields() {
        let s = make_state();
        let s = record_row_height(&s, 0, 50.0);
        let s2 = clear_row_height_cache(&s);
        assert_eq!(s2.rows.len(), s.rows.len());
        assert_eq!(s2.page, s.page);
    }

    // ---- cache-invalidation on sort / filter / page -----------------------

    #[test]
    fn toggle_sort_clears_row_height_cache() {
        let s = make_state();
        let s = record_row_height(&s, 0, 60.0);
        let s2 = toggle_sort(&s, col_name());
        assert!(s2.row_heights.is_empty());
    }

    #[test]
    fn set_filter_clears_row_height_cache() {
        let s = make_state();
        let s = record_row_height(&s, 0, 60.0);
        let s2 = set_filter(&s, col_name(), Some(FilterValue::Text("ali".into())));
        assert!(s2.row_heights.is_empty());
    }

    #[test]
    fn set_page_clears_row_height_cache() {
        let mut s = make_state();
        s.page_size = 2; // 3 rows → 2 pages
        let s = record_row_height(&s, 0, 60.0);
        let s2 = set_page(&s, 1).expect("page 1 valid");
        assert!(s2.row_heights.is_empty());
    }

    #[test]
    fn batch_record_row_heights_merges_entries() {
        let s = make_state();
        let heights: HashMap<usize, f64> = [(0, 40.0), (1, 60.0), (2, 50.0)].into();
        let s2 = batch_record_row_heights(&s, &heights);
        assert_eq!(s2.row_heights.get(&0), Some(&40.0));
        assert_eq!(s2.row_heights.get(&1), Some(&60.0));
        assert_eq!(s2.row_heights.get(&2), Some(&50.0));
    }

    #[test]
    fn batch_record_row_heights_empty_batch_is_no_op() {
        let s = record_row_height(&make_state(), 0, 55.0);
        let s2 = batch_record_row_heights(&s, &HashMap::new());
        assert_eq!(s2.row_heights, s.row_heights);
    }

    #[test]
    fn batch_record_row_heights_overwrites_existing_entries() {
        let s = record_row_height(&make_state(), 0, 40.0);
        let heights: HashMap<usize, f64> = [(0, 80.0)].into();
        let s2 = batch_record_row_heights(&s, &heights);
        assert_eq!(s2.row_heights.get(&0), Some(&80.0));
    }

    #[test]
    fn batch_record_row_heights_does_not_mutate_input() {
        let s = make_state();
        let heights: HashMap<usize, f64> = [(0, 40.0)].into();
        let _ = batch_record_row_heights(&s, &heights);
        assert!(s.row_heights.is_empty());
    }

    // ---- in-cell editing transitions (Item 7) -----------------------------

    #[test]
    fn start_edit_happy_path() {
        let s = make_editable_state();
        let row_id = s.rows[0].0;
        let s2 = start_edit(&s, row_id, col_name()).expect("col_name is editable");
        assert_eq!(
            s2.editing,
            Some(EditTarget {
                row_id,
                column_id: col_name()
            })
        );
    }

    #[test]
    fn start_edit_non_editable_column_returns_err() {
        let s = make_state(); // columns have no editor
        let row_id = s.rows[0].0;
        let err = start_edit(&s, row_id, col_name()).unwrap_err();
        assert_eq!(err, StateError::ColumnNotEditable(col_name()));
    }

    #[test]
    fn start_edit_replaces_prior_edit_without_orphan() {
        let s = make_editable_state();
        let row_id = s.rows[0].0;
        let s2 = start_edit(&s, row_id, col_name()).unwrap();
        let s3 = start_edit(&s2, row_id, col_score()).unwrap();
        assert_eq!(
            s3.editing,
            Some(EditTarget {
                row_id,
                column_id: col_score()
            })
        );
    }

    #[test]
    fn commit_edit_clears_editing() {
        let s = make_editable_state();
        let row_id = s.rows[0].0;
        let s2 = start_edit(&s, row_id, col_name()).unwrap();
        let s3 = commit_edit(&s2);
        assert_eq!(s3.editing, None);
        assert_eq!(s3.rows.len(), s.rows.len()); // other fields unchanged
    }

    #[test]
    fn cancel_edit_clears_editing() {
        let s = make_editable_state();
        let row_id = s.rows[0].0;
        let s2 = start_edit(&s, row_id, col_name()).unwrap();
        let s3 = cancel_edit(&s2);
        assert_eq!(s3.editing, None);
    }

    #[test]
    fn cancel_edit_when_no_edit_in_progress_is_noop() {
        let s = make_state();
        assert!(s.editing.is_none());
        let s2 = cancel_edit(&s);
        assert!(s2.editing.is_none());
        assert_eq!(s2.rows.len(), s.rows.len());
    }

    #[test]
    fn commit_cancel_idempotent() {
        let s = make_editable_state();
        let row_id = s.rows[0].0;
        let s2 = start_edit(&s, row_id, col_name()).unwrap();
        let cancelled = cancel_edit(&s2);
        let committed_then_cancelled = commit_edit(&cancel_edit(&s2));
        assert_eq!(cancelled.editing, committed_then_cancelled.editing);
    }

    #[test]
    fn revert_edit_restores_prior_row() {
        let s = make_editable_state();
        let row_id = s.rows[0].0;
        let prior_row = s.rows[0].1.clone();
        // Simulate: the host calls update_row with new data, then revert_edit to undo
        let modified = update_row(
            &s,
            row_id,
            TestRow {
                name: "Alice Updated".into(),
                score: 99.0,
            },
        );
        let prior = PriorEdit {
            row_id,
            column_id: col_name(),
            prior_row: prior_row.clone(),
        };
        let reverted = revert_edit(&modified, &prior);
        assert_eq!(reverted.rows[0].1, prior_row);
    }

    #[test]
    fn revert_edit_noop_when_row_missing() {
        let s = make_editable_state();
        let ghost_id = RowId::new(); // not in the table
        let prior = PriorEdit {
            row_id: ghost_id,
            column_id: col_name(),
            prior_row: TestRow {
                name: "Ghost".into(),
                score: 0.0,
            },
        };
        let s2 = revert_edit(&s, &prior);
        assert_eq!(s2.rows.len(), s.rows.len());
    }

    #[test]
    fn column_def_editor_builder() {
        use crate::column::EditorKind;
        let col = ColumnDef::new(col_name(), "Name", |r: &TestRow| {
            CellValue::Text(r.name.clone())
        })
        .editor(EditorKind::Text);
        assert!(matches!(col.editor, Some(EditorKind::Text)));
    }

    #[test]
    fn next_editable_cell_advances_within_row() {
        let s = make_editable_state();
        let row_id = s.rows[0].0;
        let s2 = start_edit(&s, row_id, col_name()).unwrap();
        let s3 = next_editable_cell(&s2);
        assert_eq!(
            s3.editing,
            Some(EditTarget {
                row_id,
                column_id: col_score()
            })
        );
    }

    #[test]
    fn next_editable_cell_wraps_to_first_after_last() {
        let s = make_editable_state();
        let row_id = s.rows[0].0;
        let s2 = start_edit(&s, row_id, col_score()).unwrap(); // last editable
        let s3 = next_editable_cell(&s2);
        assert_eq!(
            s3.editing,
            Some(EditTarget {
                row_id,
                column_id: col_name()
            })
        );
    }

    #[test]
    fn prev_editable_cell_moves_backwards() {
        let s = make_editable_state();
        let row_id = s.rows[0].0;
        let s2 = start_edit(&s, row_id, col_score()).unwrap();
        let s3 = prev_editable_cell(&s2);
        assert_eq!(
            s3.editing,
            Some(EditTarget {
                row_id,
                column_id: col_name()
            })
        );
    }

    #[test]
    fn prev_editable_cell_wraps_to_last_before_first() {
        let s = make_editable_state();
        let row_id = s.rows[0].0;
        let s2 = start_edit(&s, row_id, col_name()).unwrap(); // first editable
        let s3 = prev_editable_cell(&s2);
        assert_eq!(
            s3.editing,
            Some(EditTarget {
                row_id,
                column_id: col_score()
            })
        );
    }

    #[test]
    fn next_editable_cell_noop_when_no_edit_in_progress() {
        let s = make_editable_state();
        assert!(s.editing.is_none());
        let s2 = next_editable_cell(&s);
        assert!(s2.editing.is_none());
    }
}
