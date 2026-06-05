//! Derived views over `TableState<TRow>`.
//!
//! Unlike transitions these functions do not return a new state; they
//! compute read-only projections for the adapter to render.

use std::collections::HashMap;

use crate::state::{TableState, VirtualWindow};
use crate::types::RowId;

/// Compute the fixed-height virtual window for a scroll offset.
///
/// Returns a `VirtualWindow` with `start_index` / `end_index` (inclusive)
/// within the range `0..total_rows`, plus spacer pixel heights.
///
/// Per VIRT-1: v0.1 supports fixed row height only. The math is O(1) —
/// two integer divisions, no binary search (recon-2 § 2).
///
/// Buffer rows (overscan) default to 3 per session recon-2 § 2.
#[must_use]
pub fn visible_window(
    scroll_top: f64,
    viewport_height: f64,
    row_height: f64,
    total_rows: usize,
    buffer_rows: usize,
) -> VirtualWindow {
    if total_rows == 0 || row_height <= 0.0 {
        return VirtualWindow {
            start_index: 0,
            end_index: 0,
            top_pad_px: 0.0,
            bottom_pad_px: 0.0,
        };
    }

    // floor/ceil ensure non-negative integer results before casting to usize.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let raw_start = (scroll_top / row_height).floor() as usize;
    // ceil((scroll_top + viewport_height) / row_height) - 1
    // A partially-visible row must be rendered (recon-2 § 2 "Why ceil - 1").
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let raw_end = ((scroll_top + viewport_height) / row_height).ceil() as usize;
    let raw_end = raw_end.saturating_sub(1);

    let raw_start = raw_start.min(total_rows - 1);
    let raw_end = raw_end.min(total_rows - 1);

    let start_index = raw_start.saturating_sub(buffer_rows);
    let end_index = (raw_end + buffer_rows).min(total_rows - 1);

    #[allow(clippy::cast_precision_loss)]
    let top_pad_px = start_index as f64 * row_height;
    #[allow(clippy::cast_precision_loss)]
    let bottom_pad_px = (total_rows - 1 - end_index) as f64 * row_height;

    VirtualWindow {
        start_index,
        end_index,
        top_pad_px,
        bottom_pad_px,
    }
}

/// Compute the variable-height virtual window for a scroll offset.
///
/// Unlike [`visible_window`], this function reads per-row heights from
/// `row_heights` (keyed by row index within the current page view). Rows
/// not yet in the cache fall back to `default_row_height`. The window math
/// uses a prefix-sum array built on-the-fly from the cached heights and
/// binary search to locate the first and last visible rows.
///
/// The complexity is O(n) over `total_rows` for the prefix-sum build.
/// If profiling later shows cost above 2 ms at scale, the prefix-sum can
/// be cached in `TableState` as a subsequent non-breaking field addition.
///
/// Per VIRT-2: introduced in v0.2.0 (Item 6 — variable-row-height
/// virtualization, signed off 2026-06-04).
#[must_use]
pub fn visible_window_variable<S: std::hash::BuildHasher>(
    row_heights: &HashMap<usize, f64, S>,
    scroll_top: f64,
    viewport_height: f64,
    default_row_height: f64,
    total_rows: usize,
    buffer_rows: usize,
) -> VirtualWindow {
    if total_rows == 0 || default_row_height <= 0.0 {
        return VirtualWindow {
            start_index: 0,
            end_index: 0,
            top_pad_px: 0.0,
            bottom_pad_px: 0.0,
        };
    }

    // prefix[i] = y-offset of row i's top edge.
    // prefix[i+1] = y-offset of row i's bottom edge (= prefix[i] + height(i)).
    let mut prefix: Vec<f64> = Vec::with_capacity(total_rows + 1);
    prefix.push(0.0_f64);
    for i in 0..total_rows {
        let h = row_heights
            .get(&i)
            .copied()
            .unwrap_or(default_row_height)
            .max(0.0);
        prefix.push(prefix[i] + h);
    }

    let bottom = scroll_top + viewport_height;

    // Number of rows whose bottom edge is ≤ scroll_top (rows fully above viewport).
    // prefix[i+1] is the bottom of row i; prefix[1..] partition_point gives this count.
    let raw_start = prefix[1..]
        .partition_point(|&x| x <= scroll_top)
        .min(total_rows - 1);

    // Last row whose top edge is strictly below the viewport bottom.
    // prefix.partition_point(< bottom) gives count of prefix entries < bottom;
    // subtract 1 to get the row index of the last visible row's top.
    let raw_end = {
        let idx = prefix.partition_point(|&x| x < bottom);
        idx.saturating_sub(1).min(total_rows - 1)
    };

    // Ensure end >= start (degenerate case: viewport height < smallest row height).
    let raw_end = raw_end.max(raw_start);

    // Apply buffer (overscan rows rendered beyond the visible edge).
    let start_index = raw_start.saturating_sub(buffer_rows);
    let end_index = (raw_end + buffer_rows).min(total_rows - 1);

    #[allow(clippy::cast_precision_loss)]
    let top_pad_px = prefix[start_index];
    let bottom_pad_px = (prefix[total_rows] - prefix[end_index + 1]).max(0.0);

    VirtualWindow {
        start_index,
        end_index,
        top_pad_px,
        bottom_pad_px,
    }
}

/// Returns the `RowId`s of rows on the current page (post-sort/post-filter).
///
/// Used by `toggle_select_all`.
#[must_use]
pub fn visible_row_ids<TRow: Clone>(state: &TableState<TRow>) -> Vec<RowId> {
    let filtered_sorted = filtered_sorted_pairs(state);
    let start = state.page * state.page_size;
    let end = (start + state.page_size).min(filtered_sorted.len());
    filtered_sorted[start..end]
        .iter()
        .map(|(id, _)| *id)
        .collect()
}

/// Returns the rows on the current page (post-sort/post-filter).
///
/// Data pipeline per the work-queue spec and recon-2 § 5:
///   raw rows → filter → sort → paginate
#[must_use]
pub fn visible_rows<TRow: Clone>(state: &TableState<TRow>) -> Vec<TRow> {
    let filtered_sorted = filtered_sorted_pairs(state);
    let start = state.page * state.page_size;
    let end = (start + state.page_size).min(filtered_sorted.len());
    filtered_sorted[start..end]
        .iter()
        .map(|(_, row)| row.clone())
        .collect()
}

/// Post-sort/post-filter slice of all rows — NO pagination.
///
/// Used by `to_csv()` which must export the full filtered+sorted dataset,
/// not just the current page (work-queue spec v0.1-core § 3).
#[must_use]
pub fn filtered_sorted_rows<TRow: Clone>(state: &TableState<TRow>) -> Vec<TRow> {
    filtered_sorted_pairs(state)
        .into_iter()
        .map(|(_, row)| row)
        .collect()
}

/// Post-sort / post-filter / post-pagination `(RowId, TRow)` pairs in one pass.
///
/// This is the **single-source-of-truth view** for adapters that need both
/// the rendered rows AND their IDs (e.g. for selection-state lookups).
/// Calling `visible_rows()` and `visible_row_ids()` separately runs the
/// underlying filter-sort-paginate pipeline twice; calling `visible_view()`
/// runs it once and lets the caller derive both shapes from the same Vec.
///
/// Adapters should pair this with their framework's memoization primitive
/// (Dioxus `use_memo`, Leptos `create_memo`, etc.) keyed on the table-state
/// signal so scroll-only state changes don't reinvoke the pipeline.
///
/// # Example
///
/// ```rust
/// use chorale_core::{TableState, visible_view};
///
/// let state: TableState<String> = TableState::new(vec![], vec![]);
/// let view = visible_view(&state);
/// // Empty dataset → empty view.
/// assert!(view.is_empty());
/// ```
#[must_use]
pub fn visible_view<TRow: Clone>(state: &TableState<TRow>) -> Vec<(RowId, TRow)> {
    let filtered_sorted = filtered_sorted_pairs(state);
    let start = state.page * state.page_size;
    let end = (start + state.page_size).min(filtered_sorted.len());
    filtered_sorted[start..end].to_vec()
}

/// Compute the `VirtualWindow` for the current state and return the
/// corresponding row slice (from `visible_rows()`).
///
/// Defined in recon-2 § 5.
#[must_use]
pub fn visible_window_for_state<TRow: Clone>(
    state: &TableState<TRow>,
) -> (VirtualWindow, Vec<TRow>) {
    let rows = visible_rows(state);
    let win = visible_window(
        state.scroll_top,
        state.viewport_height,
        state.row_height,
        rows.len(),
        state.buffer_rows,
    );
    let end = win.end_index.min(rows.len().saturating_sub(1));
    let slice = if rows.is_empty() {
        vec![]
    } else {
        rows[win.start_index..=end].to_vec()
    };
    (win, slice)
}

/// Serialize the post-sort / post-filter view (all pages) to a CSV string.
///
/// Only visible columns (per `column_visibility`) are included.
/// The CSV uses RFC 4180 quoting (double-quote fields that contain commas,
/// double-quotes, or newlines).
///
/// Per work-queue spec v0.1-core § 3: "NOT just the current page."
#[must_use]
pub fn to_csv<TRow: Clone>(state: &TableState<TRow>) -> String {
    let visible_cols: Vec<&crate::column::ColumnDef<TRow>> = state
        .columns
        .iter()
        .filter(|c| state.is_column_visible(c.id))
        .collect();

    let mut out = String::new();

    // Header row
    let header_line = visible_cols
        .iter()
        .map(|c| csv_quote(&c.header))
        .collect::<Vec<_>>()
        .join(",");
    out.push_str(&header_line);
    out.push('\n');

    // All post-sort/post-filter rows (no pagination)
    let rows = filtered_sorted_rows(state);
    for row in &rows {
        let line = visible_cols
            .iter()
            .map(|c| {
                let val = (c.accessor)(row);
                csv_quote(&val.to_csv_string())
            })
            .collect::<Vec<_>>()
            .join(",");
        out.push_str(&line);
        out.push('\n');
    }

    out
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Returns filtered + sorted `(RowId, TRow)` pairs (no pagination).
fn filtered_sorted_pairs<TRow: Clone>(state: &TableState<TRow>) -> Vec<(RowId, TRow)> {
    let mut pairs: Vec<(RowId, TRow)> = state
        .rows
        .iter()
        .filter(|(_, row)| state.row_passes_filters(row))
        .cloned()
        .collect();

    if let Some(sort) = state.sort.first() {
        if let Some(col) = state.columns.iter().find(|c| c.id == sort.column) {
            let direction = sort.direction;
            pairs.sort_by(|(_, a), (_, b)| {
                let a_val = (col.accessor)(a);
                let b_val = (col.accessor)(b);
                let ord = a_val.cmp_for_sort(&b_val);
                match direction {
                    crate::types::SortDirection::Asc => ord,
                    crate::types::SortDirection::Desc => ord.reverse(),
                }
            });
        }
    }

    pairs
}

/// RFC 4180 CSV field quoting.
fn csv_quote(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        let escaped = s.replace('"', "\"\"");
        format!("\"{escaped}\"")
    } else {
        s.to_owned()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::float_cmp, clippy::cast_precision_loss)]
mod tests {
    use std::collections::HashMap;

    use crate::column::ColumnDef;
    use crate::state::TableState;
    use crate::types::{
        Alignment, CellValue, ColumnId, FilterValue, RowId, SortDirection, SortState,
    };

    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    struct R {
        name: String,
        score: i64,
    }

    fn make_state() -> TableState<R> {
        let rows = vec![
            (
                RowId::new(),
                R {
                    name: "Alice".into(),
                    score: 90,
                },
            ),
            (
                RowId::new(),
                R {
                    name: "Bob".into(),
                    score: 75,
                },
            ),
            (
                RowId::new(),
                R {
                    name: "Charlie".into(),
                    score: 85,
                },
            ),
        ];
        let columns = vec![
            ColumnDef::new(ColumnId("name"), "Name", |r: &R| {
                CellValue::Text(r.name.clone())
            })
            .sortable()
            .filter(crate::column::FilterKind::Text),
            ColumnDef::new(ColumnId("score"), "Score", |r: &R| {
                CellValue::Integer(r.score)
            })
            .sortable()
            .filter(crate::column::FilterKind::Text)
            .alignment(Alignment::Right)
            .render_kind(crate::column::RenderKind::Number),
        ];
        TableState {
            rows,
            columns,
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

    // ---- visible_rows ------------------------------------------------------

    #[test]
    fn visible_rows_returns_all_when_no_filter_or_sort() {
        let s = make_state();
        let rows = visible_rows(&s);
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn visible_rows_filters_by_text() {
        let mut s = make_state();
        s.filters
            .insert(ColumnId("name"), FilterValue::Text("ali".into()));
        let rows = visible_rows(&s);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Alice");
    }

    #[test]
    fn visible_rows_sorts_asc() {
        let mut s = make_state();
        s.sort = vec![SortState::new(ColumnId("name"), SortDirection::Asc)];
        let rows = visible_rows(&s);
        assert_eq!(rows[0].name, "Alice");
        assert_eq!(rows[1].name, "Bob");
        assert_eq!(rows[2].name, "Charlie");
    }

    #[test]
    fn visible_rows_sorts_desc() {
        let mut s = make_state();
        s.sort = vec![SortState::new(ColumnId("name"), SortDirection::Desc)];
        let rows = visible_rows(&s);
        assert_eq!(rows[0].name, "Charlie");
        assert_eq!(rows[2].name, "Alice");
    }

    #[test]
    fn visible_rows_paginates() {
        let mut s = make_state();
        s.page_size = 2;
        let page0 = visible_rows(&s);
        assert_eq!(page0.len(), 2);
        s.page = 1;
        let page1 = visible_rows(&s);
        assert_eq!(page1.len(), 1);
    }

    // ---- to_csv ------------------------------------------------------------

    #[test]
    fn to_csv_has_header_and_rows() {
        let s = make_state();
        let csv = to_csv(&s);
        let lines: Vec<&str> = csv.lines().collect();
        // 1 header + 3 data rows
        assert_eq!(lines.len(), 4);
        assert!(lines[0].contains("Name"));
        assert!(lines[0].contains("Score"));
    }

    #[test]
    fn to_csv_respects_filter_but_not_pagination() {
        let mut s = make_state();
        s.page_size = 1; // only 1 row per page
        s.filters
            .insert(ColumnId("name"), FilterValue::Text("li".into())); // matches Alice + Charlie
        let csv = to_csv(&s);
        let lines: Vec<&str> = csv.lines().collect();
        // 1 header + 2 matching rows (Alice + Charlie), NOT limited to page 1
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn to_csv_quotes_commas_in_values() {
        let mut s = make_state();
        // Inject a row with a comma in the name
        s.rows.push((
            RowId::new(),
            R {
                name: "Smith, Jr.".into(),
                score: 50,
            },
        ));
        let csv = to_csv(&s);
        assert!(csv.contains("\"Smith, Jr.\""));
    }

    // ---- visible_view (single-pass dedupe) ---------------------------------

    #[test]
    fn visible_view_pairs_match_visible_rows_and_visible_row_ids() {
        // The dedupe contract: visible_view must return the SAME (RowId, TRow)
        // pairs in the SAME order that visible_rows() + visible_row_ids() would
        // produce when called independently. If this drifts, adapters that
        // migrate from two separate calls to one visible_view call could
        // silently render different rows or mis-attribute selection state.
        let mut s = make_state();
        s.sort = vec![SortState::new(ColumnId("score"), SortDirection::Desc)];
        s.filters
            .insert(ColumnId("name"), FilterValue::Text("i".into())); // Alice + Charlie

        let view = visible_view(&s);
        let rows = visible_rows(&s);
        let ids = visible_row_ids(&s);

        assert_eq!(view.len(), rows.len());
        assert_eq!(view.len(), ids.len());
        for (i, (id, row)) in view.iter().enumerate() {
            assert_eq!(*id, ids[i], "row {i} id mismatch");
            assert_eq!(*row, rows[i], "row {i} data mismatch");
        }
    }

    #[test]
    fn visible_view_is_paginated() {
        let mut s = make_state();
        s.page_size = 2;

        let page0 = visible_view(&s);
        assert_eq!(page0.len(), 2);

        s.page = 1;
        let page1 = visible_view(&s);
        assert_eq!(page1.len(), 1);
    }

    #[test]
    fn visible_view_respects_filter_and_sort() {
        let mut s = make_state();
        s.filters
            .insert(ColumnId("name"), FilterValue::Text("i".into())); // Alice + Charlie
        s.sort = vec![SortState::new(ColumnId("score"), SortDirection::Desc)];
        let view = visible_view(&s);
        // Alice (90) + Charlie (85), sorted desc by score → Alice first.
        assert_eq!(view.len(), 2);
        assert_eq!(view[0].1.name, "Alice");
        assert_eq!(view[1].1.name, "Charlie");
    }

    #[test]
    fn visible_view_is_deterministic_for_same_state() {
        // A wiring bug that mutates state between calls would surface here.
        let s = make_state();
        let v1 = visible_view(&s);
        let v2 = visible_view(&s);
        assert_eq!(v1, v2);
    }

    // ---- visible_window ----------------------------------------------------

    #[test]
    fn visible_window_full_list_fits_in_viewport() {
        // 10 rows * 40px = 400px total; viewport = 500px → all visible
        let win = visible_window(0.0, 500.0, 40.0, 10, 0);
        assert_eq!(win.start_index, 0);
        assert_eq!(win.end_index, 9);
        assert!((win.top_pad_px - 0.0).abs() < f64::EPSILON);
        assert!((win.bottom_pad_px - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn visible_window_scrolled_past_first_row() {
        // scroll_top = 50px, row_height = 40px → raw_start = floor(50/40) = 1
        // viewport = 100px → raw_end = ceil(150/40) - 1 = ceil(3.75) - 1 = 4 - 1 = 3
        // with buffer 0: start=1, end=3
        let win = visible_window(50.0, 100.0, 40.0, 20, 0);
        assert_eq!(win.start_index, 1);
        assert_eq!(win.end_index, 3);
    }

    #[test]
    fn visible_window_buffer_rows_expand_range() {
        let win = visible_window(50.0, 100.0, 40.0, 20, 2);
        // Without buffer: start=1, end=3. With buffer=2: start=0 (clamped), end=5
        assert_eq!(win.start_index, 0);
        assert_eq!(win.end_index, 5);
    }

    #[test]
    fn visible_window_empty_list_returns_zero_window() {
        let win = visible_window(0.0, 500.0, 40.0, 0, 3);
        assert_eq!(win.start_index, 0);
        assert_eq!(win.end_index, 0);
        assert!((win.top_pad_px - 0.0).abs() < f64::EPSILON);
        assert!((win.bottom_pad_px - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn visible_window_pad_heights_sum_to_total_minus_rendered() {
        // 100 rows * 40px = 4000px. Scroll to row 50 (2000px).
        // viewport = 200px → raw rows 50..54 visible. buffer=0.
        let win = visible_window(2000.0, 200.0, 40.0, 100, 0);
        let rendered_rows = win.end_index - win.start_index + 1;
        let total_height = 100.0 * 40.0;
        let rendered_height = rendered_rows as f64 * 40.0;
        let pad_sum = win.top_pad_px + win.bottom_pad_px;
        assert!((pad_sum - (total_height - rendered_height)).abs() < f64::EPSILON);
    }

    // ---- visible_row_ids --------------------------------------------------

    #[test]
    fn visible_row_ids_matches_visible_view_ids() {
        let s = make_state();
        let ids = visible_row_ids(&s);
        let view = visible_view(&s);
        assert_eq!(ids.len(), view.len());
        for (id, (view_id, _)) in ids.iter().zip(view.iter()) {
            assert_eq!(id, view_id);
        }
    }

    #[test]
    fn visible_row_ids_is_paginated() {
        let mut s = make_state();
        s.page_size = 2;
        let page0_ids = visible_row_ids(&s);
        assert_eq!(page0_ids.len(), 2);
        s.page = 1;
        let page1_ids = visible_row_ids(&s);
        assert_eq!(page1_ids.len(), 1);
        // No overlap between pages
        for id in &page0_ids {
            assert!(!page1_ids.contains(id));
        }
    }

    // ---- filtered_sorted_rows ---------------------------------------------

    #[test]
    fn filtered_sorted_rows_ignores_pagination() {
        let mut s = make_state();
        s.page_size = 1; // only 1 row per page
        let all = filtered_sorted_rows(&s);
        // Despite page_size=1, all 3 rows are returned.
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn filtered_sorted_rows_respects_filter() {
        let mut s = make_state();
        s.filters
            .insert(ColumnId("name"), FilterValue::Text("Ali".into()));
        let rows = filtered_sorted_rows(&s);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Alice");
    }

    #[test]
    fn filtered_sorted_rows_respects_sort() {
        let mut s = make_state();
        s.sort = vec![SortState::new(ColumnId("score"), SortDirection::Desc)];
        let rows = filtered_sorted_rows(&s);
        // Alice=90, Charlie=85, Bob=75 → desc
        assert_eq!(rows[0].name, "Alice");
        assert_eq!(rows[2].name, "Bob");
    }

    // ---- visible_window_for_state -----------------------------------------

    #[test]
    fn visible_window_for_state_returns_window_and_slice() {
        let state = make_state_with_scroll(0.0, 40.0, 120.0);
        let (win, rows) = visible_window_for_state(&state);
        // 3 rows at 40px each = 120px total; viewport = 120px → all rows visible.
        assert_eq!(win.start_index, 0);
        assert!(rows.len() <= 3);
    }

    #[test]
    fn visible_window_for_state_slices_correctly() {
        // 3 rows, row_height=40, viewport=40 (shows 1 row), buffer=0.
        let mut state = make_state_with_scroll(40.0, 40.0, 40.0);
        state.buffer_rows = 0;
        let (win, rows) = visible_window_for_state(&state);
        // scroll_top=40 → start_index=1 (second row), end_index=1.
        assert_eq!(win.start_index, 1);
        assert_eq!(win.end_index, 1);
        assert_eq!(rows.len(), 1);
        // The sliced row should match what visible_rows returns for the same state.
        let all = visible_rows(&state);
        assert_eq!(rows[0], all[win.start_index]);
    }

    fn make_state_with_scroll(scroll_top: f64, row_height: f64, viewport: f64) -> TableState<R> {
        let mut s = make_state();
        s.scroll_top = scroll_top;
        s.row_height = row_height;
        s.viewport_height = viewport;
        s
    }

    // ---- visible_window_variable -------------------------------------------

    /// Helper: build a heights map from a slice of (index, height) pairs.
    fn heights(pairs: &[(usize, f64)]) -> HashMap<usize, f64> {
        pairs.iter().copied().collect()
    }

    #[test]
    fn visible_window_variable_zero_rows_returns_empty() {
        let win = visible_window_variable(&HashMap::new(), 0.0, 500.0, 40.0, 0, 0);
        assert_eq!(win.start_index, 0);
        assert_eq!(win.end_index, 0);
        assert!((win.top_pad_px - 0.0).abs() < f64::EPSILON);
        assert!((win.bottom_pad_px - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn visible_window_variable_all_default_height_matches_fixed() {
        // When all rows use the default height, variable-height must equal
        // the fixed-height result for the same inputs.
        let total = 100usize;
        let row_h = 40.0_f64;
        let scroll = 800.0_f64;
        let vp = 200.0_f64;
        let buf = 3usize;
        let fixed = super::visible_window(scroll, vp, row_h, total, buf);
        let variable = visible_window_variable(&HashMap::new(), scroll, vp, row_h, total, buf);
        assert_eq!(fixed.start_index, variable.start_index, "start mismatch");
        assert_eq!(fixed.end_index, variable.end_index, "end mismatch");
        assert!(
            (fixed.top_pad_px - variable.top_pad_px).abs() < f64::EPSILON,
            "top_pad mismatch: fixed={} variable={}",
            fixed.top_pad_px,
            variable.top_pad_px
        );
        assert!(
            (fixed.bottom_pad_px - variable.bottom_pad_px).abs() < f64::EPSILON,
            "bottom_pad mismatch"
        );
    }

    #[test]
    fn visible_window_variable_all_measured_precise_window() {
        // 5 rows with heights [40, 80, 40, 60, 40]. Total = 260px.
        // prefix = [0, 40, 120, 160, 220, 260].
        // scroll_top=40, viewport=80: visible range [40, 120).
        // Row 1 spans [40, 120): its top=40 is exactly scroll_top.
        // Only row 1 is visible (top 40, bottom 120; viewport bottom 120 exclusive).
        let h = heights(&[(0, 40.0), (1, 80.0), (2, 40.0), (3, 60.0), (4, 40.0)]);
        let win = visible_window_variable(&h, 40.0, 80.0, 40.0, 5, 0);
        assert_eq!(win.start_index, 1);
        assert_eq!(win.end_index, 1);
        assert!(
            (win.top_pad_px - 40.0).abs() < f64::EPSILON,
            "top_pad={}",
            win.top_pad_px
        );
        // bottom_pad = 260 - 120 = 140
        assert!(
            (win.bottom_pad_px - 140.0).abs() < f64::EPSILON,
            "bot_pad={}",
            win.bottom_pad_px
        );
    }

    #[test]
    fn visible_window_variable_partial_measurement_uses_fallback() {
        // 5 rows. Rows 1 and 3 measured at 80px; rows 0,2,4 use default 40px.
        // prefix = [0, 40, 120, 160, 240, 280].
        let h = heights(&[(1, 80.0), (3, 80.0)]);
        // scroll_top=0, viewport=200: rows 0-3 visible (tops 0,40,120,160 all < 200).
        let win = visible_window_variable(&h, 0.0, 200.0, 40.0, 5, 0);
        assert_eq!(win.start_index, 0);
        assert_eq!(win.end_index, 3);
        assert!((win.top_pad_px - 0.0).abs() < f64::EPSILON);
        // bottom_pad = 280 - prefix[4] = 280 - 240 = 40
        assert!(
            (win.bottom_pad_px - 40.0).abs() < f64::EPSILON,
            "bot_pad={}",
            win.bottom_pad_px
        );
    }

    #[test]
    fn visible_window_variable_buffer_expands_range() {
        // 10 uniform rows of 40px. scroll_top=80, viewport=40 → raw [2, 2].
        // With buffer=2: start=0, end=4.
        let win = visible_window_variable(&HashMap::new(), 80.0, 40.0, 40.0, 10, 2);
        assert_eq!(win.start_index, 0);
        assert_eq!(win.end_index, 4);
    }

    #[test]
    fn visible_window_variable_pad_sums_equal_unrendered_height() {
        // All 20 uniform rows of 40px = 800px total.
        // scroll_top=0, viewport=100, buffer=0 → rows 0..2 rendered.
        let win = visible_window_variable(&HashMap::new(), 0.0, 100.0, 40.0, 20, 0);
        let rendered = win.end_index - win.start_index + 1;
        let total_h = 20.0 * 40.0;
        let rendered_h = rendered as f64 * 40.0;
        assert!(
            (win.top_pad_px + win.bottom_pad_px - (total_h - rendered_h)).abs() < f64::EPSILON,
            "pad_sum={} expected={}",
            win.top_pad_px + win.bottom_pad_px,
            total_h - rendered_h
        );
    }

    #[test]
    fn visible_window_variable_scroll_past_content_clamps_to_last_row() {
        // scroll_top way beyond total content height → last valid window.
        let win = visible_window_variable(&HashMap::new(), 100_000.0, 500.0, 40.0, 10, 0);
        assert_eq!(win.end_index, 9);
        assert!(win.bottom_pad_px >= 0.0);
    }
}
