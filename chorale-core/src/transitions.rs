//! Pure state-transition functions for `TableState<TRow>`.
//!
//! Every function follows the CHORALE-CORE-2 immutable pattern:
//! `fn name(state: &TableState<TRow>, ...) -> TableState<TRow>`.
//! No `&mut self`. No signals. No async. Unit-testable without a framework.

use crate::error::StateError;
use crate::state::TableState;
use crate::types::{ColumnId, FilterValue, RowId, SortDirection, SortState};

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
    TableState {
        sort: next_sort,
        scroll_top: 0.0,
        page: 0,
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
    TableState {
        filters,
        scroll_top: 0.0,
        page: 0,
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
    Ok(TableState {
        page,
        scroll_top: 0.0,
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
    use std::sync::Arc;

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
            ColumnDef {
                id: col_name(),
                header: "Name".into(),
                accessor: Arc::new(|r: &TestRow| CellValue::Text(r.name.clone())),
                sortable: true,
                filter: crate::column::FilterKind::Text,
                initial_width: None,
                alignment: Alignment::Left,
                render_kind: crate::column::RenderKind::Text,
                header_class: None,
                cell_class: None,
            },
            ColumnDef {
                id: col_score(),
                header: "Score".into(),
                accessor: Arc::new(|r: &TestRow| CellValue::Float(r.score)),
                sortable: true,
                filter: crate::column::FilterKind::Text,
                initial_width: None,
                alignment: Alignment::Right,
                render_kind: crate::column::RenderKind::Number,
                header_class: None,
                cell_class: None,
            },
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
            scroll_top: 0.0,
            viewport_height: 500.0,
            row_height: 40.0,
            buffer_rows: 3,
        }
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
}
