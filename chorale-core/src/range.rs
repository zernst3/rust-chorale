//! Range-selection types: `RangeSelection`, `NormalizedRange`, `RangeError`, `fill_handle_targets`.

use crate::column::ColumnDef;
use crate::types::{CellValue, ColumnId};

/// A single contiguous rectangular cell range.
///
/// `anchor` is where the selection began (e.g. the mouse-down cell or the
/// active cell when Shift was first pressed). `focus` is where it currently
/// ends (mouse position or last Shift+arrow target).
///
/// Either corner may have a larger index than the other — callers normalise
/// via [`RangeSelection::normalized`] before iterating.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RangeSelection {
    /// Anchor cell: `(visible_row_idx, column_id)`.
    pub anchor: (usize, ColumnId),
    /// Focus cell: `(visible_row_idx, column_id)`.
    pub focus: (usize, ColumnId),
}

impl RangeSelection {
    /// Create a new `RangeSelection` with the same anchor and focus (single cell).
    #[must_use]
    pub fn new(anchor: (usize, ColumnId), focus: (usize, ColumnId)) -> Self {
        Self { anchor, focus }
    }

    /// Create a single-cell `RangeSelection`.
    #[must_use]
    pub fn single(row_idx: usize, col: ColumnId) -> Self {
        Self {
            anchor: (row_idx, col),
            focus: (row_idx, col),
        }
    }

    /// Resolve the anchor and focus into a normalised (min/max row + ordered columns) form.
    ///
    /// `column_order` must be the effective visible column order (e.g. from
    /// `effective_column_order`). Hidden columns are excluded from the result.
    #[must_use]
    pub fn normalized<TRow: Clone>(&self, columns: &[&ColumnDef<TRow>]) -> NormalizedRange {
        let min_row = self.anchor.0.min(self.focus.0);
        let max_row = self.anchor.0.max(self.focus.0);

        let anchor_col_idx = columns.iter().position(|c| c.id == self.anchor.1);
        let focus_col_idx = columns.iter().position(|c| c.id == self.focus.1);

        let (min_col_idx, max_col_idx) = match (anchor_col_idx, focus_col_idx) {
            (Some(a), Some(b)) => (a.min(b), a.max(b)),
            (Some(a), None) => (a, a),
            (None, Some(b)) => (b, b),
            (None, None) => {
                return NormalizedRange {
                    min_row,
                    max_row,
                    columns: vec![],
                }
            }
        };

        let cols: Vec<ColumnId> = columns[min_col_idx..=max_col_idx]
            .iter()
            .map(|c| c.id)
            .collect();

        NormalizedRange {
            min_row,
            max_row,
            columns: cols,
        }
    }
}

/// The resolved rectangular extent of a `RangeSelection`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NormalizedRange {
    /// Smallest visible row index in the range (inclusive).
    pub min_row: usize,
    /// Largest visible row index in the range (inclusive).
    pub max_row: usize,
    /// Column IDs within the range, ordered left-to-right per `column_order`.
    pub columns: Vec<ColumnId>,
}

/// Errors for range operations.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RangeError {
    /// No range is currently selected.
    NoRangeSelected,
    /// The operation does not support multi-rect selections.
    MultiRectNotSupportedForThisOperation,
    /// The range is too small to fill (e.g. single-row `fill_down`).
    RangeTooSmallToFill,
    /// A row or column index is out of bounds.
    IndexOutOfBounds,
}

impl std::fmt::Display for RangeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoRangeSelected => write!(f, "no range selected"),
            Self::MultiRectNotSupportedForThisOperation => {
                write!(f, "multi-rect selection not supported for this operation")
            }
            Self::RangeTooSmallToFill => write!(f, "range is too small to fill"),
            Self::IndexOutOfBounds => write!(f, "index out of bounds"),
        }
    }
}

impl std::error::Error for RangeError {}

// ---------------------------------------------------------------------------
// fill_handle_targets
// ---------------------------------------------------------------------------

/// Detected pattern for a sequence of source cell values.
#[derive(Debug, Clone)]
enum FillPattern {
    /// Repeat the single value (or the uniform sequence).
    Repeat(CellValue),
    /// Numeric arithmetic progression with constant step.
    Arithmetic {
        first: f64,
        last: f64,
        step: f64,
        is_integer: bool,
    },
    /// Non-uniform sequence; extend by cycling.
    Cycle(Vec<CellValue>),
}

impl FillPattern {
    #[allow(clippy::cast_precision_loss)]
    fn from_values(values: &[CellValue]) -> Self {
        match values {
            [] => Self::Repeat(CellValue::Empty),
            [v] => Self::Repeat(v.clone()),
            values => {
                let nums: Option<Vec<f64>> = values
                    .iter()
                    .map(|v| match v {
                        CellValue::Integer(i) => Some(*i as f64),
                        CellValue::Float(f) => Some(*f),
                        _ => None,
                    })
                    .collect();

                if let Some(nums) = nums {
                    let step = nums[1] - nums[0];
                    let is_constant = nums.windows(2).all(|w| (w[1] - w[0] - step).abs() < 1e-9);
                    if is_constant {
                        let is_integer = values.iter().all(|v| matches!(v, CellValue::Integer(_)));
                        return Self::Arithmetic {
                            first: nums[0],
                            last: *nums.last().unwrap_or(&nums[0]),
                            step,
                            is_integer,
                        };
                    }
                }

                Self::Cycle(values.to_vec())
            }
        }
    }

    /// Value at `position` steps forward (away from source end).
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
    fn value_forward(&self, position: usize) -> CellValue {
        match self {
            Self::Repeat(v) => v.clone(),
            Self::Arithmetic {
                last,
                step,
                is_integer,
                ..
            } => {
                let v = last + step * (position as f64 + 1.0);
                if *is_integer {
                    CellValue::Integer(v.round() as i64)
                } else {
                    CellValue::Float(v)
                }
            }
            Self::Cycle(seq) => {
                let len = seq.len();
                seq[position % len].clone()
            }
        }
    }

    /// Value at `position` steps backward (away from source start).
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
    fn value_backward(&self, position: usize) -> CellValue {
        match self {
            Self::Repeat(v) => v.clone(),
            Self::Arithmetic {
                first,
                step,
                is_integer,
                ..
            } => {
                let v = first - step * (position as f64 + 1.0);
                if *is_integer {
                    CellValue::Integer(v.round() as i64)
                } else {
                    CellValue::Float(v)
                }
            }
            Self::Cycle(seq) => {
                let len = seq.len();
                seq[position % len].clone()
            }
        }
    }
}

/// Compute the set of cells to write for a fill-handle drag operation.
///
/// `source` is the currently selected range (the selection before the drag
/// began). `target_row` and `target_col` define the cell where the user
/// released the fill handle.
///
/// Returns `(visible_row_idx, column_id, value)` for each cell in the
/// extension area outside `source`. Source cells are excluded. Returns
/// an empty vec when the target is inside the source range or there
/// are no visible rows.
///
/// # Pattern detection
///
/// - 1 source cell → repeat.
/// - 2+ numeric cells in a straight line with constant step → arithmetic
///   progression.
/// - Numeric cells with irregular steps → cycle.
/// - Non-numeric cells → cycle (repeat for single-cell sources).
/// - Rectangle source → fill-down / fill-up applies per column; fill-right /
///   fill-left applies per row.
///
/// Results are returned in ascending row-index, then visible-column order.
///
/// # Panics
///
/// Does not panic. The internal `unwrap_or(0)` / `unwrap_or_default` guards
/// ensure that out-of-bounds column look-ups degrade gracefully to an empty
/// result rather than panicking.
#[must_use]
pub fn fill_handle_targets<TRow: Clone>(
    state: &crate::state::TableState<TRow>,
    source: &RangeSelection,
    target_row: usize,
    target_col: ColumnId,
) -> Vec<(usize, ColumnId, CellValue)> {
    use crate::views::{effective_column_order, visible_view};

    let visible_cols: Vec<&ColumnDef<TRow>> = effective_column_order(state)
        .into_iter()
        .filter(|c| state.is_column_visible(c.id))
        .collect();
    let visible_col_ids: Vec<ColumnId> = visible_cols.iter().map(|c| c.id).collect();

    let normalized = source.normalized(&visible_cols);
    let rows = visible_view(state);

    if rows.is_empty() || normalized.columns.is_empty() {
        return vec![];
    }

    let src_min_col_idx = visible_col_ids
        .iter()
        .position(|&id| id == normalized.columns[0])
        .unwrap_or(0);
    let src_max_col_idx = normalized
        .columns
        .last()
        .and_then(|&last_col| visible_col_ids.iter().position(|&id| id == last_col))
        .unwrap_or(src_min_col_idx);
    let target_col_idx = visible_col_ids
        .iter()
        .position(|&id| id == target_col)
        .unwrap_or(src_max_col_idx);

    let fill_down = target_row > normalized.max_row;
    let fill_up = target_row < normalized.min_row;
    let fill_vertical = fill_down || fill_up;
    let fill_right = !fill_vertical && target_col_idx > src_max_col_idx;
    let fill_left = !fill_vertical && target_col_idx < src_min_col_idx;

    if !fill_vertical && !fill_right && !fill_left {
        return vec![];
    }

    let mut result = Vec::new();

    if fill_vertical {
        for &col_id in &normalized.columns {
            let Some(col_def) = visible_cols.iter().find(|c| c.id == col_id) else {
                continue;
            };
            let src_values: Vec<CellValue> = (normalized.min_row..=normalized.max_row)
                .filter_map(|ri| rows.get(ri).map(|(_, r)| (col_def.accessor)(r)))
                .collect();
            let pattern = FillPattern::from_values(&src_values);

            if fill_down {
                for (ext_i, row_idx) in ((normalized.max_row + 1)..=target_row).enumerate() {
                    if row_idx >= rows.len() {
                        break;
                    }
                    result.push((row_idx, col_id, pattern.value_forward(ext_i)));
                }
            } else {
                // fill_up: enumerate from closest-to-source down to target_row.
                // We iterate in reverse so ext_i 0 == min_row-1 (nearest).
                for (ext_i, row_idx) in (target_row..normalized.min_row).rev().enumerate() {
                    result.push((row_idx, col_id, pattern.value_backward(ext_i)));
                }
            }
        }
        // Sort ascending by row so the output is in row-major order.
        result.sort_by_key(|(r, _, _)| *r);
    } else {
        for row_idx in normalized.min_row..=normalized.max_row {
            if row_idx >= rows.len() {
                break;
            }
            let (_, row) = &rows[row_idx];
            let src_values: Vec<CellValue> = normalized
                .columns
                .iter()
                .filter_map(|&col_id| {
                    visible_cols
                        .iter()
                        .find(|c| c.id == col_id)
                        .map(|c| (c.accessor)(row))
                })
                .collect();
            let pattern = FillPattern::from_values(&src_values);

            if fill_right {
                for (ext_i, col_idx) in ((src_max_col_idx + 1)..=target_col_idx).enumerate() {
                    if let Some(&ext_col_id) = visible_col_ids.get(col_idx) {
                        result.push((row_idx, ext_col_id, pattern.value_forward(ext_i)));
                    }
                }
            } else {
                // fill_left: enumerate from closest-to-source leftward.
                for (ext_i, col_idx) in (target_col_idx..src_min_col_idx).rev().enumerate() {
                    if let Some(&ext_col_id) = visible_col_ids.get(col_idx) {
                        result.push((row_idx, ext_col_id, pattern.value_backward(ext_i)));
                    }
                }
            }
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::column::ColumnDef;
    use crate::state::TableState;
    use crate::transitions::start_range_selection;
    use crate::types::RowId;

    fn col_a() -> ColumnId {
        ColumnId("a")
    }
    fn col_b() -> ColumnId {
        ColumnId("b")
    }
    fn col_c() -> ColumnId {
        ColumnId("c")
    }

    #[derive(Clone, Debug, PartialEq)]
    struct Row {
        a: CellValue,
        b: CellValue,
        c: CellValue,
    }

    fn row(a: CellValue, b: CellValue, c: CellValue) -> Row {
        Row { a, b, c }
    }

    fn make_columns() -> Vec<ColumnDef<Row>> {
        vec![
            ColumnDef::new(col_a(), "A", |r: &Row| r.a.clone()),
            ColumnDef::new(col_b(), "B", |r: &Row| r.b.clone()),
            ColumnDef::new(col_c(), "C", |r: &Row| r.c.clone()),
        ]
    }

    fn make_state(rows: Vec<Row>) -> TableState<Row> {
        let pairs: Vec<(RowId, Row)> = rows.into_iter().map(|r| (RowId::new(), r)).collect();
        TableState::new(pairs, make_columns())
    }

    fn int(n: i64) -> CellValue {
        CellValue::Integer(n)
    }
    fn text(s: &str) -> CellValue {
        CellValue::Text(s.to_string())
    }
    fn float(f: f64) -> CellValue {
        CellValue::Float(f)
    }

    // ---- normalized() 3×3 range includes focus cell -------------------------

    #[test]
    fn drag_select_3x3_includes_focus_cell() {
        let state = make_state(vec![
            row(int(1), int(2), int(3)),
            row(int(4), int(5), int(6)),
            row(int(7), int(8), int(9)),
        ]);
        let cols = make_columns();
        let col_refs: Vec<&ColumnDef<Row>> = cols.iter().collect();

        let range = RangeSelection::new((0, col_a()), (2, col_c()));
        let nr = range.normalized(&col_refs);

        // Collect cells using the same inclusive iteration as the adapters.
        let cells: Vec<(usize, ColumnId)> = (nr.min_row..=nr.max_row)
            .flat_map(|r| nr.columns.iter().map(move |&c| (r, c)))
            .collect();

        assert_eq!(
            cells.len(),
            9,
            "expected 9 cells in 3×3 range, got {}",
            cells.len()
        );
        assert!(
            cells.iter().any(|&c| c == (2, col_c())),
            "focus cell (2, col_c) must be included in the range"
        );
    }

    // ---- single-cell repeat ------------------------------------------------

    #[test]
    fn single_cell_repeat_fills_down() {
        let state = make_state(vec![
            row(int(10), int(0), int(0)),
            row(int(0), int(0), int(0)),
            row(int(0), int(0), int(0)),
            row(int(0), int(0), int(0)),
        ]);
        let state = start_range_selection(&state, 0, col_a());
        let writes = fill_handle_targets(&state, &state.range_selection[0], 3, col_a());
        assert_eq!(writes.len(), 3);
        assert!(writes.iter().all(|(_, _, v)| *v == int(10)));
        let rows: Vec<usize> = writes.iter().map(|(r, _, _)| *r).collect();
        assert_eq!(rows, vec![1, 2, 3]);
    }

    // ---- ascending arithmetic progression ----------------------------------

    #[test]
    fn ascending_arithmetic_fills_down() {
        let state = make_state(vec![
            row(int(10), int(0), int(0)),
            row(int(20), int(0), int(0)),
            row(int(0), int(0), int(0)),
            row(int(0), int(0), int(0)),
            row(int(0), int(0), int(0)),
        ]);
        let mut s = state.clone();
        s.range_selection = vec![RangeSelection::new((0, col_a()), (1, col_a()))];
        let writes = fill_handle_targets(&s, &s.range_selection[0], 4, col_a());
        assert_eq!(writes.len(), 3);
        assert_eq!(writes[0].2, int(30));
        assert_eq!(writes[1].2, int(40));
        assert_eq!(writes[2].2, int(50));
    }

    // ---- descending arithmetic progression ---------------------------------

    #[test]
    fn descending_arithmetic_fills_down() {
        let state = make_state(vec![
            row(int(30), int(0), int(0)),
            row(int(20), int(0), int(0)),
            row(int(10), int(0), int(0)),
            row(int(0), int(0), int(0)),
            row(int(0), int(0), int(0)),
        ]);
        let mut s = state.clone();
        s.range_selection = vec![RangeSelection::new((0, col_a()), (2, col_a()))];
        let writes = fill_handle_targets(&s, &s.range_selection[0], 4, col_a());
        assert_eq!(writes.len(), 2);
        assert_eq!(writes[0].2, int(0));
        assert_eq!(writes[1].2, int(-10));
    }

    // ---- floating-point arithmetic progression -----------------------------

    #[test]
    fn float_arithmetic_fills_down() {
        let state = make_state(vec![
            row(float(1.0), int(0), int(0)),
            row(float(1.5), int(0), int(0)),
            row(float(2.0), int(0), int(0)),
            row(int(0), int(0), int(0)),
            row(int(0), int(0), int(0)),
        ]);
        let mut s = state.clone();
        s.range_selection = vec![RangeSelection::new((0, col_a()), (2, col_a()))];
        let writes = fill_handle_targets(&s, &s.range_selection[0], 4, col_a());
        assert_eq!(writes.len(), 2);
        // 2.0 + 0.5 = 2.5, 2.0 + 1.0 = 3.0
        let v0 = match &writes[0].2 {
            CellValue::Float(f) => *f,
            _ => panic!("expected Float"),
        };
        let v1 = match &writes[1].2 {
            CellValue::Float(f) => *f,
            _ => panic!("expected Float"),
        };
        assert!((v0 - 2.5).abs() < 1e-9);
        assert!((v1 - 3.0).abs() < 1e-9);
    }

    // ---- repeat on irregular numeric pattern -------------------------------

    #[test]
    fn irregular_pattern_cycles() {
        // [10, 30, 20] - steps [20, -10] are not constant → cycle
        let state = make_state(vec![
            row(int(10), int(0), int(0)),
            row(int(30), int(0), int(0)),
            row(int(20), int(0), int(0)),
            row(int(0), int(0), int(0)),
            row(int(0), int(0), int(0)),
            row(int(0), int(0), int(0)),
        ]);
        let mut s = state.clone();
        s.range_selection = vec![RangeSelection::new((0, col_a()), (2, col_a()))];
        let writes = fill_handle_targets(&s, &s.range_selection[0], 5, col_a());
        assert_eq!(writes.len(), 3);
        assert_eq!(writes[0].2, int(10)); // cycle pos 0
        assert_eq!(writes[1].2, int(30)); // cycle pos 1
        assert_eq!(writes[2].2, int(20)); // cycle pos 2
    }

    // ---- repeat on non-numeric (text) values -------------------------------

    #[test]
    fn text_values_cycle() {
        let state = make_state(vec![
            row(text("Alice"), int(0), int(0)),
            row(text("Bob"), int(0), int(0)),
            row(text(""), int(0), int(0)),
            row(text(""), int(0), int(0)),
            row(text(""), int(0), int(0)),
        ]);
        let mut s = state.clone();
        s.range_selection = vec![RangeSelection::new((0, col_a()), (1, col_a()))];
        let writes = fill_handle_targets(&s, &s.range_selection[0], 4, col_a());
        assert_eq!(writes.len(), 3);
        assert_eq!(writes[0].2, text("Alice")); // cycle pos 0
        assert_eq!(writes[1].2, text("Bob")); // cycle pos 1
        assert_eq!(writes[2].2, text("Alice")); // cycle pos 2
    }

    // ---- target inside source → empty result --------------------------------

    #[test]
    fn target_inside_source_returns_empty() {
        let state = make_state(vec![
            row(int(1), int(0), int(0)),
            row(int(2), int(0), int(0)),
            row(int(3), int(0), int(0)),
        ]);
        let mut s = state;
        s.range_selection = vec![RangeSelection::new((0, col_a()), (2, col_a()))];
        let writes = fill_handle_targets(&s, &s.range_selection[0], 1, col_a());
        assert!(writes.is_empty());
    }
}
