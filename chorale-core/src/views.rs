//! Derived views over `TableState<TRow>`.
//!
//! Unlike transitions these functions do not return a new state; they
//! compute read-only projections for the adapter to render.

use std::collections::HashMap;

use crate::column::AggregatorKind;
use crate::state::{TableState, VirtualWindow};
use crate::types::{CellValue, ColumnId, GroupKey, GroupedPaginationMode, PaginationMode, RowId};

/// A row in the grouped view: either a group header or a data row.
///
/// Returned by [`visible_grouped_view`]. Adapters match on this enum and render
/// `Header` rows as group-header `<tr>` elements and `Data` rows as ordinary cells.
///
/// `GroupedRow` is `#[non_exhaustive]` so additional variants can be added in
/// future minor releases without breaking existing match arms.
#[non_exhaustive]
pub enum GroupedRow<TRow> {
    /// A group header row.
    Header {
        /// The opaque key for this group. Pass to [`crate::transitions::toggle_group`]
        /// to collapse or expand.
        key: GroupKey,
        /// The display label for this group (the cell value of the group-by column).
        label: String,
        /// Nesting depth (0 = outermost group).
        depth: usize,
        /// Total number of data rows in this group (before collapse, after filter).
        row_count: usize,
        /// Whether this group is currently collapsed.
        is_collapsed: bool,
        /// Aggregate value per column in effective column order (`None` when the
        /// column has no aggregator). Index `i` corresponds to the column at
        /// position `i` in `effective_column_order`.
        aggregates: Vec<Option<CellValue>>,
    },
    /// A data row.
    Data(RowId, TRow),
}

impl<TRow: Clone> Clone for GroupedRow<TRow> {
    fn clone(&self) -> Self {
        match self {
            Self::Header {
                key,
                label,
                depth,
                row_count,
                is_collapsed,
                aggregates,
            } => Self::Header {
                key: key.clone(),
                label: label.clone(),
                depth: *depth,
                row_count: *row_count,
                is_collapsed: *is_collapsed,
                aggregates: aggregates.clone(),
            },
            Self::Data(id, row) => Self::Data(*id, row.clone()),
        }
    }
}

impl<TRow: PartialEq> PartialEq for GroupedRow<TRow> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::Header {
                    key: k1,
                    label: l1,
                    depth: d1,
                    row_count: r1,
                    is_collapsed: c1,
                    aggregates: a1,
                },
                Self::Header {
                    key: k2,
                    label: l2,
                    depth: d2,
                    row_count: r2,
                    is_collapsed: c2,
                    aggregates: a2,
                },
            ) => k1 == k2 && l1 == l2 && d1 == d2 && r1 == r2 && c1 == c2 && a1 == a2,
            (Self::Data(id1, r1), Self::Data(id2, r2)) => id1 == id2 && r1 == r2,
            _ => false,
        }
    }
}

/// Returns the interleaved group-header + data rows for the current state.
///
/// When `state.grouping` is empty, every item is `GroupedRow::Data`
/// (callers may use the simpler [`visible_view`] instead).
///
/// In `GroupedPaginationMode::DataRowsOnly` (the default), pagination applies
/// to data rows only; group headers repeat on each page where their rows appear.
/// In `GroupedPaginationMode::Virtualized`, the full grouped tree is returned
/// without pagination.
///
/// Collapsed groups (keys in `state.collapsed_groups`) emit a `Header` row
/// with `is_collapsed: true` and no `Data` children.
#[must_use]
pub fn visible_grouped_view<TRow: Clone>(state: &TableState<TRow>) -> Vec<GroupedRow<TRow>> {
    let filtered_sorted = filtered_sorted_pairs(state);

    if state.grouping.is_empty() {
        return filtered_sorted
            .into_iter()
            .map(|(id, row)| GroupedRow::Data(id, row))
            .collect();
    }

    let flat = build_group_rows(&filtered_sorted, state, &state.grouping, &[], 0);

    match state.grouped_pagination {
        GroupedPaginationMode::DataRowsOnly => paginate_grouped(flat, state.page, state.page_size),
        GroupedPaginationMode::Virtualized => flat,
    }
}

// ---------------------------------------------------------------------------
// Grouping internal helpers
// ---------------------------------------------------------------------------

/// Recursively build the flat interleaved list of headers and data rows.
fn build_group_rows<TRow: Clone>(
    pairs: &[(RowId, TRow)],
    state: &TableState<TRow>,
    grouping: &[ColumnId],
    parent_values: &[String],
    depth: usize,
) -> Vec<GroupedRow<TRow>> {
    if grouping.is_empty() {
        return pairs
            .iter()
            .map(|(id, row)| GroupedRow::Data(*id, row.clone()))
            .collect();
    }

    let group_col_id = grouping[0];
    let group_col = state.columns.iter().find(|c| c.id == group_col_id);

    // Partition into groups preserving first-occurrence order.
    let mut group_order: Vec<String> = Vec::new();
    let mut group_map: HashMap<String, Vec<(RowId, TRow)>> = HashMap::new();
    for (id, row) in pairs {
        let val = group_col.map_or_else(String::new, |c| (c.accessor)(row).to_csv_string());
        if !group_map.contains_key(&val) {
            group_order.push(val.clone());
        }
        group_map.entry(val).or_default().push((*id, row.clone()));
    }

    let cols_in_order = effective_column_order(state);
    let mut result = Vec::new();

    for val in &group_order {
        let group_pairs = &group_map[val];
        let mut current_values = parent_values.to_vec();
        current_values.push(val.clone());
        let key = GroupKey::from_values(&current_values);

        let is_collapsed = state.collapsed_groups.contains(&key);
        let aggregates = compute_aggregates(group_pairs, &cols_in_order);

        result.push(GroupedRow::Header {
            key: key.clone(),
            label: val.clone(),
            depth,
            row_count: group_pairs.len(),
            is_collapsed,
            aggregates,
        });

        if !is_collapsed {
            let children = build_group_rows(
                group_pairs,
                state,
                &grouping[1..],
                &current_values,
                depth + 1,
            );
            result.extend(children);
        }
    }
    result
}

/// Compute aggregate values for a group's rows, one per column in effective order.
fn compute_aggregates<TRow: Clone>(
    pairs: &[(RowId, TRow)],
    cols: &[&crate::column::ColumnDef<TRow>],
) -> Vec<Option<CellValue>> {
    cols.iter()
        .map(|col| {
            let agg = col.aggregator.as_ref()?;
            let values: Vec<CellValue> = pairs.iter().map(|(_, row)| (col.accessor)(row)).collect();
            let row_refs: Vec<&TRow> = pairs.iter().map(|(_, row)| row).collect();
            Some(match agg {
                AggregatorKind::Sum => {
                    let sum: f64 = values.iter().filter_map(cell_to_f64).sum();
                    CellValue::Float(sum)
                }
                AggregatorKind::Average => {
                    let nums: Vec<f64> = values.iter().filter_map(cell_to_f64).collect();
                    if nums.is_empty() {
                        CellValue::Text("\u{2014}".into())
                    } else {
                        #[allow(clippy::cast_precision_loss)]
                        CellValue::Float(nums.iter().sum::<f64>() / nums.len() as f64)
                    }
                }
                AggregatorKind::Count =>
                {
                    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
                    CellValue::Integer(pairs.len() as i64)
                }
                AggregatorKind::Min => values
                    .iter()
                    .cloned()
                    .reduce(|a, b| {
                        if b.cmp_for_sort(&a) == std::cmp::Ordering::Less {
                            b
                        } else {
                            a
                        }
                    })
                    .unwrap_or(CellValue::Empty),
                AggregatorKind::Max => values
                    .iter()
                    .cloned()
                    .reduce(|a, b| {
                        if b.cmp_for_sort(&a) == std::cmp::Ordering::Greater {
                            b
                        } else {
                            a
                        }
                    })
                    .unwrap_or(CellValue::Empty),
                AggregatorKind::Custom(f) => f(&row_refs),
            })
        })
        .collect()
}

/// Extract a numeric `f64` from a `CellValue` for Sum/Average aggregation.
fn cell_to_f64(v: &CellValue) -> Option<f64> {
    match v {
        CellValue::Integer(n) =>
        {
            #[allow(clippy::cast_precision_loss)]
            Some(*n as f64)
        }
        CellValue::Float(f) => Some(*f),
        _ => None,
    }
}

/// Apply `DataRowsOnly` pagination to a flat grouped list.
///
/// Headers are included on every page that contains at least one of their data
/// rows. Data rows are sliced to `[page * page_size .. (page+1) * page_size)`.
fn paginate_grouped<TRow: Clone>(
    flat: Vec<GroupedRow<TRow>>,
    page: usize,
    page_size: usize,
) -> Vec<GroupedRow<TRow>> {
    if page_size == 0 {
        return flat;
    }

    // Collect flat indices of data rows.
    let data_flat_indices: Vec<usize> = flat
        .iter()
        .enumerate()
        .filter_map(|(i, row)| {
            if matches!(row, GroupedRow::Data(..)) {
                Some(i)
            } else {
                None
            }
        })
        .collect();

    let start = page * page_size;
    if start >= data_flat_indices.len() {
        return vec![];
    }
    let end = ((page + 1) * page_size).min(data_flat_indices.len());

    let page_flat_indices: std::collections::HashSet<usize> =
        data_flat_indices[start..end].iter().copied().collect();

    let n = flat.len();
    let mut result = Vec::new();
    let mut i = 0;

    while i < n {
        match &flat[i] {
            GroupedRow::Header {
                depth,
                is_collapsed,
                ..
            } => {
                let d = *depth;
                // Collapsed headers always appear (user needs them to expand).
                let include = *is_collapsed || {
                    // Include expanded header when any data row in its subtree is on the page.
                    let mut j = i + 1;
                    let mut found = false;
                    while j < n {
                        match &flat[j] {
                            // Subtree ends at any header with depth <= d.
                            GroupedRow::Header { depth: jd, .. } if *jd <= d => break,
                            GroupedRow::Data(..) if page_flat_indices.contains(&j) => {
                                found = true;
                                break;
                            }
                            _ => {}
                        }
                        j += 1;
                    }
                    found
                };
                if include {
                    result.push(flat[i].clone());
                }
                i += 1;
            }
            GroupedRow::Data(..) => {
                if page_flat_indices.contains(&i) {
                    result.push(flat[i].clone());
                }
                i += 1;
            }
        }
    }
    result
}

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
/// In `PaginationMode::InfiniteScroll`, returns the first `loaded_row_count` rows.
/// Used by `toggle_select_all`.
#[must_use]
pub fn visible_row_ids<TRow: Clone>(state: &TableState<TRow>) -> Vec<RowId> {
    visible_view(state).into_iter().map(|(id, _)| id).collect()
}

/// Returns the rows on the current page (post-sort/post-filter).
///
/// In `PaginationMode::InfiniteScroll`, returns the first `loaded_row_count` rows.
///
/// Data pipeline per the work-queue spec and recon-2 § 5:
///   raw rows → filter → sort → paginate
#[must_use]
pub fn visible_rows<TRow: Clone>(state: &TableState<TRow>) -> Vec<TRow> {
    visible_view(state)
        .into_iter()
        .map(|(_, row)| row)
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
/// In `PaginationMode::InfiniteScroll`, returns the first `loaded_row_count`
/// rows instead of a page slice. The adapter grows `loaded_row_count` by
/// calling [`load_more_rows`](crate::load_more_rows) when the user scrolls
/// near the bottom.
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
    match state.pagination_mode {
        PaginationMode::Pages => {
            let start = state.page * state.page_size;
            let end = (start + state.page_size).min(filtered_sorted.len());
            filtered_sorted[start..end].to_vec()
        }
        PaginationMode::InfiniteScroll => {
            let end = state.loaded_row_count.min(filtered_sorted.len());
            filtered_sorted[..end].to_vec()
        }
    }
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

/// Returns columns in their effective render order (respects `state.column_order`).
///
/// If `column_order` is empty, returns columns in definition order.
/// Otherwise columns appear in `column_order` sequence; any column not listed
/// in `column_order` is appended at the end in definition order.
///
/// Visibility filtering is NOT applied — this helper operates on all columns.
/// Callers that need only visible columns should filter the result.
pub(crate) fn effective_column_order<TRow: Clone>(
    state: &TableState<TRow>,
) -> Vec<&crate::column::ColumnDef<TRow>> {
    if state.column_order.is_empty() {
        return state.columns.iter().collect();
    }
    let mut result: Vec<&crate::column::ColumnDef<TRow>> = state
        .column_order
        .iter()
        .filter_map(|id| state.columns.iter().find(|c| c.id == *id))
        .collect();
    for col in &state.columns {
        if !state.column_order.contains(&col.id) {
            result.push(col);
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Frozen-column view helpers (Item 10)
// ---------------------------------------------------------------------------

/// Returns the columns pinned to the left edge, in effective column order.
///
/// Excludes hidden columns. An empty vec is returned when no columns have
/// `frozen == FrozenSide::Left` or when all such columns are hidden.
///
/// For adapter rendering, combine the three zone helpers as:
/// `frozen_left_columns` + `scrollable_columns` + `frozen_right_columns`.
#[must_use]
pub fn frozen_left_columns<TRow: Clone>(
    state: &TableState<TRow>,
) -> Vec<&crate::column::ColumnDef<TRow>> {
    effective_column_order(state)
        .into_iter()
        .filter(|c| c.frozen == crate::column::FrozenSide::Left && state.is_column_visible(c.id))
        .collect()
}

/// Returns the columns pinned to the right edge, in effective column order.
///
/// Excludes hidden columns. An empty vec is returned when no columns have
/// `frozen == FrozenSide::Right` or when all such columns are hidden.
#[must_use]
pub fn frozen_right_columns<TRow: Clone>(
    state: &TableState<TRow>,
) -> Vec<&crate::column::ColumnDef<TRow>> {
    effective_column_order(state)
        .into_iter()
        .filter(|c| c.frozen == crate::column::FrozenSide::Right && state.is_column_visible(c.id))
        .collect()
}

/// Returns the scrollable (non-frozen) columns, in effective column order.
///
/// Excludes hidden columns. The union of `frozen_left_columns`,
/// `scrollable_columns`, and `frozen_right_columns` equals all visible columns
/// in definition/effective order.
#[must_use]
pub fn scrollable_columns<TRow: Clone>(
    state: &TableState<TRow>,
) -> Vec<&crate::column::ColumnDef<TRow>> {
    effective_column_order(state)
        .into_iter()
        .filter(|c| c.frozen == crate::column::FrozenSide::None && state.is_column_visible(c.id))
        .collect()
}

/// Serialize the post-sort / post-filter view (all pages) to a CSV string.
///
/// Columns are serialized in their effective render order (respecting
/// `state.column_order`). Only visible columns (per `column_visibility`) are
/// included. The CSV uses RFC 4180 quoting (double-quote fields that contain
/// commas, double-quotes, or newlines).
///
/// Per work-queue spec v0.1-core § 3: "NOT just the current page."
#[must_use]
pub fn to_csv<TRow: Clone>(state: &TableState<TRow>) -> String {
    let visible_cols: Vec<&crate::column::ColumnDef<TRow>> = effective_column_order(state)
        .into_iter()
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

    if !state.sort.is_empty() {
        // Build a list of (accessor_ref, direction) pairs in priority order.
        // Columns not found in state.columns are silently skipped.
        let sort_keys: Vec<(&crate::column::ColumnDef<TRow>, crate::types::SortDirection)> = state
            .sort
            .iter()
            .filter_map(|s| {
                state
                    .columns
                    .iter()
                    .find(|c| c.id == s.column)
                    .map(|c| (c, s.direction))
            })
            .collect();

        pairs.sort_by(|(_, a), (_, b)| {
            for (col, direction) in &sort_keys {
                let a_val = (col.accessor)(a);
                let b_val = (col.accessor)(b);
                let ord = a_val.cmp_for_sort(&b_val);
                let ord = match direction {
                    crate::types::SortDirection::Asc => ord,
                    crate::types::SortDirection::Desc => ord.reverse(),
                };
                if ord != std::cmp::Ordering::Equal {
                    return ord;
                }
            }
            std::cmp::Ordering::Equal
        });
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
#[allow(clippy::float_cmp, clippy::cast_precision_loss, clippy::unwrap_used)]
mod tests {
    use std::collections::HashMap;

    use crate::column::ColumnDef;
    use crate::state::TableState;
    use crate::types::{
        Alignment, CellValue, ColumnId, FilterValue, GroupKey, GroupedPaginationMode,
        PaginationMode, RowId, SortAction, SortDirection, SortState,
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
            column_order: vec![],
            editing: None,
            row_heights: HashMap::new(),
            scroll_top: 0.0,
            viewport_height: 500.0,
            row_height: 40.0,
            buffer_rows: 3,
            pagination_mode: PaginationMode::Pages,
            loaded_row_count: 0,
            grouping: vec![],
            collapsed_groups: std::collections::HashSet::new(),
            grouped_pagination: GroupedPaginationMode::DataRowsOnly,
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

    // ---- effective_column_order -------------------------------------------

    #[test]
    fn effective_column_order_empty_order_returns_definition_order() {
        let s = make_state();
        let order = effective_column_order(&s);
        assert_eq!(order[0].id, ColumnId("name"));
        assert_eq!(order[1].id, ColumnId("score"));
    }

    #[test]
    fn effective_column_order_respects_explicit_order() {
        let mut s = make_state();
        s.column_order = vec![ColumnId("score"), ColumnId("name")];
        let order = effective_column_order(&s);
        assert_eq!(order[0].id, ColumnId("score"));
        assert_eq!(order[1].id, ColumnId("name"));
    }

    #[test]
    fn effective_column_order_appends_unlisted_columns() {
        let mut s = make_state();
        // Only specify "score"; "name" should be appended at end.
        s.column_order = vec![ColumnId("score")];
        let order = effective_column_order(&s);
        assert_eq!(order[0].id, ColumnId("score"));
        assert_eq!(order[1].id, ColumnId("name"));
    }

    #[test]
    fn to_csv_respects_column_order() {
        let mut s = make_state();
        // Reorder: score first, then name.
        s.column_order = vec![ColumnId("score"), ColumnId("name")];
        let csv = to_csv(&s);
        let header = csv.lines().next().unwrap();
        // Score column should appear before Name in the header.
        let score_pos = header.find("Score").unwrap();
        let name_pos = header.find("Name").unwrap();
        assert!(score_pos < name_pos, "score should come before name");
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

    // -----------------------------------------------------------------------
    // Frozen column helpers (Item 10)
    // -----------------------------------------------------------------------

    use crate::column::FrozenSide;

    fn make_frozen_state() -> TableState<R> {
        let rows = vec![(
            RowId::new(),
            R {
                name: "A".into(),
                score: 1,
            },
        )];
        let columns = vec![
            ColumnDef::new(ColumnId("id"), "ID", |r: &R| CellValue::Integer(r.score))
                .frozen(FrozenSide::Left),
            ColumnDef::new(ColumnId("name"), "Name", |r: &R| {
                CellValue::Text(r.name.clone())
            })
            .frozen(FrozenSide::Left),
            ColumnDef::new(ColumnId("score"), "Score", |r: &R| {
                CellValue::Integer(r.score)
            }),
            ColumnDef::new(ColumnId("notes"), "Notes", |r: &R| {
                CellValue::Text(r.name.clone())
            }),
            ColumnDef::new(ColumnId("actions"), "Actions", |_: &R| CellValue::Empty)
                .frozen(FrozenSide::Right),
        ];
        TableState::new(rows, columns)
    }

    #[test]
    fn frozen_left_returns_left_frozen_in_order() {
        let state = make_frozen_state();
        let cols = frozen_left_columns(&state);
        assert_eq!(cols.len(), 2);
        assert_eq!(cols[0].id, ColumnId("id"));
        assert_eq!(cols[1].id, ColumnId("name"));
    }

    #[test]
    fn frozen_right_returns_right_frozen() {
        let state = make_frozen_state();
        let cols = frozen_right_columns(&state);
        assert_eq!(cols.len(), 1);
        assert_eq!(cols[0].id, ColumnId("actions"));
    }

    #[test]
    fn scrollable_columns_returns_non_frozen() {
        let state = make_frozen_state();
        let cols = scrollable_columns(&state);
        assert_eq!(cols.len(), 2);
        assert_eq!(cols[0].id, ColumnId("score"));
        assert_eq!(cols[1].id, ColumnId("notes"));
    }

    #[test]
    fn frozen_union_equals_all_visible_columns() {
        let state = make_frozen_state();
        let left = frozen_left_columns(&state);
        let right = frozen_right_columns(&state);
        let scroll = scrollable_columns(&state);
        let total = left.len() + right.len() + scroll.len();
        assert_eq!(total, state.columns.len());
    }

    #[test]
    fn frozen_helpers_empty_when_no_columns() {
        let state: TableState<R> = TableState::new(vec![], vec![]);
        assert!(frozen_left_columns(&state).is_empty());
        assert!(frozen_right_columns(&state).is_empty());
        assert!(scrollable_columns(&state).is_empty());
    }

    #[test]
    fn frozen_helpers_exclude_hidden_columns() {
        let state = make_frozen_state();
        let state = crate::transitions::set_column_visibility(&state, ColumnId("id"), false);
        let left = frozen_left_columns(&state);
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].id, ColumnId("name"));
    }

    #[test]
    fn frozen_helpers_respect_column_order() {
        let state = make_frozen_state();
        // Swap the two left-frozen columns.
        let state = crate::transitions::set_column_order(
            &state,
            vec![
                ColumnId("name"),
                ColumnId("id"),
                ColumnId("score"),
                ColumnId("notes"),
                ColumnId("actions"),
            ],
        )
        .unwrap();
        let left = frozen_left_columns(&state);
        assert_eq!(left[0].id, ColumnId("name"));
        assert_eq!(left[1].id, ColumnId("id"));
    }

    #[test]
    fn all_none_frozen_means_all_scrollable() {
        let state = make_state();
        assert!(frozen_left_columns(&state).is_empty());
        assert!(frozen_right_columns(&state).is_empty());
        assert_eq!(scrollable_columns(&state).len(), state.columns.len());
    }

    #[test]
    fn frozen_builder_sets_side() {
        let col =
            ColumnDef::new(ColumnId("x"), "X", |_: &R| CellValue::Empty).frozen(FrozenSide::Left);
        assert_eq!(col.frozen, FrozenSide::Left);
    }

    #[test]
    fn frozen_default_is_none() {
        let col = ColumnDef::new(ColumnId("x"), "X", |_: &R| CellValue::Empty);
        assert_eq!(col.frozen, FrozenSide::None);
    }

    // ---- visible_grouped_view (Item 8) ----------------------------------------

    #[derive(Clone, Debug, PartialEq)]
    struct GR {
        group: String,
        value: i64,
    }

    fn make_grouped_state() -> TableState<GR> {
        let rows = vec![
            (
                RowId::new(),
                GR {
                    group: "A".into(),
                    value: 10,
                },
            ),
            (
                RowId::new(),
                GR {
                    group: "A".into(),
                    value: 20,
                },
            ),
            (
                RowId::new(),
                GR {
                    group: "B".into(),
                    value: 30,
                },
            ),
        ];
        let columns = vec![
            ColumnDef::new(ColumnId("group"), "Group", |r: &GR| {
                CellValue::Text(r.group.clone())
            }),
            ColumnDef::new(ColumnId("value"), "Value", |r: &GR| {
                CellValue::Integer(r.value)
            })
            .aggregator(crate::column::AggregatorKind::Sum),
        ];
        let mut s = TableState::new(rows, columns);
        s.page_size = 100;
        s
    }

    #[test]
    fn visible_grouped_view_no_grouping_returns_data_rows() {
        let s = make_grouped_state();
        let view = visible_grouped_view(&s);
        assert_eq!(view.len(), 3);
        assert!(view.iter().all(|r| matches!(r, GroupedRow::Data(..))));
    }

    #[test]
    fn visible_grouped_view_groups_by_single_column() {
        let mut s = make_grouped_state();
        s.grouping = vec![ColumnId("group")];
        let view = visible_grouped_view(&s);
        // 2 headers (A, B) + 2 data rows (A) + 1 data row (B) = 5
        assert_eq!(view.len(), 5);
        assert!(
            matches!(&view[0], GroupedRow::Header { label, row_count, depth, .. } if label == "A" && *row_count == 2 && *depth == 0)
        );
        assert!(matches!(&view[1], GroupedRow::Data(..)));
        assert!(matches!(&view[2], GroupedRow::Data(..)));
        assert!(
            matches!(&view[3], GroupedRow::Header { label, row_count, .. } if label == "B" && *row_count == 1)
        );
        assert!(matches!(&view[4], GroupedRow::Data(..)));
    }

    #[test]
    fn visible_grouped_view_collapse_hides_data_rows() {
        let mut s = make_grouped_state();
        s.grouping = vec![ColumnId("group")];
        let key_a = GroupKey::from_values(&["A"]);
        s.collapsed_groups.insert(key_a);
        let view = visible_grouped_view(&s);
        // Header A (collapsed) + Header B + 1 data row = 3
        assert_eq!(view.len(), 3);
        assert!(
            matches!(&view[0], GroupedRow::Header { label, is_collapsed, .. } if label == "A" && *is_collapsed)
        );
        assert!(matches!(&view[1], GroupedRow::Header { label, .. } if label == "B"));
        assert!(matches!(&view[2], GroupedRow::Data(..)));
    }

    #[test]
    fn visible_grouped_view_aggregates_sum() {
        let mut s = make_grouped_state();
        s.grouping = vec![ColumnId("group")];
        let view = visible_grouped_view(&s);
        // Group A: sum of [10, 20] = 30; group B: sum of [30] = 30
        if let GroupedRow::Header {
            aggregates, label, ..
        } = &view[0]
        {
            assert_eq!(label, "A");
            // aggregates[1] = Some(Float(30.0)) (value column at index 1)
            assert_eq!(aggregates[1], Some(CellValue::Float(30.0)));
        } else {
            panic!("expected header at index 0");
        }
        if let GroupedRow::Header {
            aggregates, label, ..
        } = &view[3]
        {
            assert_eq!(label, "B");
            assert_eq!(aggregates[1], Some(CellValue::Float(30.0)));
        } else {
            panic!("expected header at index 3");
        }
    }

    #[test]
    fn visible_grouped_view_pagination_data_rows_only() {
        let mut s = make_grouped_state();
        s.grouping = vec![ColumnId("group")];
        s.page_size = 2; // 2 data rows per page
        s.page = 0;
        let view = visible_grouped_view(&s);
        // Page 0: data rows 0,1 (both in group A) → Header A + 2 data
        assert_eq!(view.len(), 3);
        assert!(matches!(&view[0], GroupedRow::Header { label, .. } if label == "A"));

        s.page = 1;
        let view2 = visible_grouped_view(&s);
        // Page 1: data row 2 (group B) → Header B + 1 data
        assert_eq!(view2.len(), 2);
        assert!(matches!(&view2[0], GroupedRow::Header { label, .. } if label == "B"));
    }

    #[test]
    fn visible_grouped_view_empty_grouping_on_empty_dataset() {
        let s: TableState<GR> = TableState::new(vec![], vec![]);
        let view = visible_grouped_view(&s);
        assert!(view.is_empty());
    }

    #[test]
    fn visible_grouped_view_virtualized_mode_ignores_pagination() {
        let mut s = make_grouped_state();
        s.grouping = vec![ColumnId("group")];
        s.grouped_pagination = GroupedPaginationMode::Virtualized;
        s.page_size = 1; // would normally show 1 data row per page
        let view = visible_grouped_view(&s);
        // Virtualized mode: all rows returned regardless of page_size
        assert_eq!(view.len(), 5);
    }

    // ---- InfiniteScroll mode (Item 11.0b) ------------------------------------

    #[test]
    fn visible_view_infinite_scroll_returns_loaded_count_rows() {
        let mut s = make_state(); // 3 rows
        s.pagination_mode = PaginationMode::InfiniteScroll;
        s.loaded_row_count = 2;
        let view = visible_view(&s);
        assert_eq!(view.len(), 2);
    }

    #[test]
    fn visible_view_infinite_scroll_loaded_count_zero_returns_empty() {
        let mut s = make_state();
        s.pagination_mode = PaginationMode::InfiniteScroll;
        s.loaded_row_count = 0;
        let view = visible_view(&s);
        assert!(view.is_empty());
    }

    #[test]
    fn visible_view_infinite_scroll_loaded_count_gt_total_clamps() {
        let mut s = make_state(); // 3 rows
        s.pagination_mode = PaginationMode::InfiniteScroll;
        s.loaded_row_count = 999;
        let view = visible_view(&s);
        assert_eq!(view.len(), 3);
    }

    #[test]
    fn visible_rows_and_ids_delegate_to_visible_view() {
        let mut s = make_state();
        s.pagination_mode = PaginationMode::InfiniteScroll;
        s.loaded_row_count = 2;
        let view = visible_view(&s);
        let rows = visible_rows(&s);
        let ids = visible_row_ids(&s);
        assert_eq!(rows.len(), view.len());
        assert_eq!(ids.len(), view.len());
        for (i, (id, row)) in view.iter().enumerate() {
            assert_eq!(ids[i], *id);
            assert_eq!(rows[i], *row);
        }
    }

    // ---- 3-column sort tiebreak (TESTS-1: multi-column sort produces correct order) ---

    #[test]
    fn filtered_sorted_pairs_three_column_sort_tiebreak() {
        // Rows where col1 ties across two rows, col2 ties between those same two rows,
        // and col3 breaks the tie. Verifies that the 3rd sort column is honoured.
        //
        // Dataset:
        //   row A: region="West", dept="Sales", score=30
        //   row B: region="West", dept="Sales", score=10   <- tie on col1+col2; col3 breaks it
        //   row C: region="West", dept="Eng",   score=20
        //   row D: region="East", dept="Sales", score=50
        //
        // Sort: region ASC, dept ASC, score ASC.
        // Expected order: D (East/Sales/50), C (West/Eng/20), B (West/Sales/10), A (West/Sales/30)
        //   → East < West; within West: Eng < Sales; within West/Sales: score 10 < 30.

        #[derive(Clone, PartialEq)]
        struct Row3 {
            region: &'static str,
            dept: &'static str,
            score: i64,
        }

        let rows = vec![
            (
                RowId::new(),
                Row3 {
                    region: "West",
                    dept: "Sales",
                    score: 30,
                },
            ),
            (
                RowId::new(),
                Row3 {
                    region: "West",
                    dept: "Sales",
                    score: 10,
                },
            ),
            (
                RowId::new(),
                Row3 {
                    region: "West",
                    dept: "Eng",
                    score: 20,
                },
            ),
            (
                RowId::new(),
                Row3 {
                    region: "East",
                    dept: "Sales",
                    score: 50,
                },
            ),
        ];
        let cols = vec![
            crate::column::ColumnDef::new(ColumnId("region"), "Region", |r: &Row3| {
                crate::types::CellValue::Text(r.region.to_string())
            })
            .sortable(),
            crate::column::ColumnDef::new(ColumnId("dept"), "Dept", |r: &Row3| {
                crate::types::CellValue::Text(r.dept.to_string())
            })
            .sortable(),
            crate::column::ColumnDef::new(ColumnId("score"), "Score", |r: &Row3| {
                crate::types::CellValue::Integer(r.score)
            })
            .sortable(),
        ];
        let s = crate::state::TableState::new(rows, cols);
        let s = crate::transitions::toggle_sort(&s, ColumnId("region"), SortAction::Append);
        let s = crate::transitions::toggle_sort(&s, ColumnId("dept"), SortAction::Append);
        let s = crate::transitions::toggle_sort(&s, ColumnId("score"), SortAction::Append);

        assert_eq!(s.sort.len(), 3, "must have 3 active sort columns");

        let view = visible_view(&s);
        assert_eq!(view.len(), 4);
        assert_eq!(view[0].1.region, "East"); // East < West
        assert_eq!(view[1].1.dept, "Eng"); // West/Eng < West/Sales
        assert_eq!(view[2].1.score, 10); // West/Sales/10 < West/Sales/30
        assert_eq!(view[3].1.score, 30);
    }
}
