//! Range-selection types: `RangeSelection`, `NormalizedRange`, `RangeError`.

use crate::column::ColumnDef;
use crate::types::ColumnId;

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
