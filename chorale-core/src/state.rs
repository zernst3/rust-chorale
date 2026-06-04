use std::collections::HashMap;

use crate::column::ColumnDef;
use crate::types::{ColumnId, FilterValue, RowId, SortState};

/// The result of `visible_window()`: which rows to render and how large the
/// top/bottom spacer divs should be so the scrollbar reflects the full list.
///
/// Defined in recon-2 § 2.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VirtualWindow {
    /// First row index to render (inclusive), within the post-sort/post-filter
    /// / post-pagination visible rows slice.
    pub start_index: usize,
    /// Last row index to render (inclusive).
    pub end_index: usize,
    /// Height of the top spacer div in pixels.
    pub top_pad_px: f64,
    /// Height of the bottom spacer div in pixels.
    pub bottom_pad_px: f64,
}

/// Complete, serializable state for one table instance.
///
/// All state transitions take `&TableState<TRow>` and return a fresh
/// `TableState<TRow>` (CHORALE-CORE-2).  No `&mut self` methods here.
///
/// Per the work-queue spec (v0.1-core § 1).
pub struct TableState<TRow: Clone> {
    /// The full dataset as `(RowId, row)` pairs. `RowId` is stable across
    /// sort, filter, and pagination so selection + edits survive reordering.
    pub rows: Vec<(RowId, TRow)>,
    pub columns: Vec<ColumnDef<TRow>>,
    /// Active sort, or `None` for natural (insertion) order.
    pub sort: Option<SortState>,
    /// Active filters keyed by `ColumnId`. Missing entry = no filter.
    pub filters: HashMap<ColumnId, FilterValue>,
    /// Row IDs that are currently selected.
    pub selection: Vec<RowId>,
    /// Zero-based current page index.
    pub page: usize,
    /// Rows per page. Must be > 0.
    pub page_size: usize,
    /// Column visibility overrides. Missing entry = visible.
    pub column_visibility: HashMap<ColumnId, bool>,
    /// Column width overrides in px. Missing entry = `initial_width` or auto.
    pub column_widths: HashMap<ColumnId, f64>,
    // --- Virtualization fields (VIRT-1) ---
    /// Current scroll offset of the scroll container in px.
    pub scroll_top: f64,
    /// Visible height of the scroll container in px.
    pub viewport_height: f64,
    /// Fixed row height in px. v0.1 only supports fixed-height rows.
    pub row_height: f64,
    /// Number of rows to render beyond the visible window on each side
    /// (overscan). Defaults to 3 per recon-2 § 2.
    pub buffer_rows: usize,
}

impl<TRow: Clone + std::fmt::Debug> std::fmt::Debug for TableState<TRow> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TableState")
            .field("rows", &self.rows)
            .field("sort", &self.sort)
            .field("filters", &self.filters)
            .field("selection", &self.selection)
            .field("page", &self.page)
            .field("page_size", &self.page_size)
            .field("column_visibility", &self.column_visibility)
            .field("column_widths", &self.column_widths)
            .field("scroll_top", &self.scroll_top)
            .field("viewport_height", &self.viewport_height)
            .field("row_height", &self.row_height)
            .field("buffer_rows", &self.buffer_rows)
            .finish_non_exhaustive()
    }
}

impl<TRow: Clone> Clone for TableState<TRow> {
    fn clone(&self) -> Self {
        Self {
            rows: self.rows.clone(),
            columns: self.columns.clone(),
            sort: self.sort,
            filters: self.filters.clone(),
            selection: self.selection.clone(),
            page: self.page,
            page_size: self.page_size,
            column_visibility: self.column_visibility.clone(),
            column_widths: self.column_widths.clone(),
            scroll_top: self.scroll_top,
            viewport_height: self.viewport_height,
            row_height: self.row_height,
            buffer_rows: self.buffer_rows,
        }
    }
}

impl<TRow: Clone> TableState<TRow> {
    /// Create a `TableState` with sensible defaults.
    ///
    /// - `page_size = 50`, `page = 0`
    /// - `row_height = 40.0` px, `viewport_height = 500.0` px
    /// - `buffer_rows = 3` (overscan rows rendered beyond the visible window)
    /// - No active sort, filters, selection, or column overrides.
    ///
    /// # Example
    ///
    /// ```rust
    /// use chorale_core::{TableState, RowId};
    ///
    /// // An empty table; rows and columns are typically provided by the host app.
    /// let state: TableState<String> = TableState::new(vec![], vec![]);
    /// assert_eq!(state.page, 0);
    /// assert_eq!(state.page_size, 50);
    /// assert_eq!(state.row_height, 40.0);
    /// ```
    #[must_use]
    pub fn new(rows: Vec<(RowId, TRow)>, columns: Vec<ColumnDef<TRow>>) -> Self {
        Self {
            rows,
            columns,
            sort: None,
            filters: HashMap::new(),
            selection: Vec::new(),
            page: 0,
            page_size: 50,
            column_visibility: HashMap::new(),
            column_widths: HashMap::new(),
            scroll_top: 0.0,
            viewport_height: 500.0,
            row_height: 40.0,
            buffer_rows: 3,
        }
    }

    /// Returns `true` if the given column is currently visible.
    ///
    /// Defaults to `true` when no explicit visibility override is set.
    #[must_use]
    pub fn is_column_visible(&self, col: ColumnId) -> bool {
        *self.column_visibility.get(&col).unwrap_or(&true)
    }

    /// Total number of pages given the current filter and page size.
    #[must_use]
    pub fn total_pages(&self) -> usize {
        let filtered = self.filtered_row_count();
        if filtered == 0 {
            return 1;
        }
        filtered.div_ceil(self.page_size)
    }

    /// Number of rows after filters are applied (before pagination).
    #[must_use]
    pub fn filtered_row_count(&self) -> usize {
        if self.filters.is_empty() {
            return self.rows.len();
        }
        self.rows
            .iter()
            .filter(|(_, row)| self.row_passes_filters(row))
            .count()
    }

    /// Whether `row` passes all active filters.
    #[must_use]
    pub(crate) fn row_passes_filters(&self, row: &TRow) -> bool {
        for (col_id, filter) in &self.filters {
            if let Some(col) = self.columns.iter().find(|c| &c.id == col_id) {
                let cell = (col.accessor)(row);
                if !cell.matches_filter(filter) {
                    return false;
                }
            }
        }
        true
    }
}
