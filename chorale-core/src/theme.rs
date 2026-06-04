use std::sync::Arc;

use crate::types::{ColumnId, RowId};

/// Which visual theme the adapter applies to the rendered table.
///
/// `Light` and `Dark` inject a pre-built stylesheet on first mount.
/// `Custom` suppresses the injected stylesheet; the consumer supplies their
/// own CSS targeting the structural class names (e.g. `chorale-row`,
/// `chorale-cell`).
///
/// Defined in recon-2 § 8a.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Theme {
    /// Injects the built-in light stylesheet on first mount.
    #[default]
    Light,
    /// Injects the built-in dark stylesheet on first mount.
    Dark,
    /// Suppresses the injected stylesheet; the consumer supplies their own CSS.
    Custom,
}

/// Row metadata passed to `RowClassFn` resolvers.
///
/// Defined in recon-2 § 8b.
#[derive(Clone, Debug)]
pub struct Row<TRow> {
    /// Stable identifier for this row across sort, filter, and pagination.
    pub id: RowId,
    /// The row's data value.
    pub data: TRow,
    /// Zero-based index within the current post-sort / post-filter /
    /// post-pagination visible rows slice.
    pub index: usize,
    /// Whether this row is currently in the selection set.
    pub is_selected: bool,
}

/// Cell metadata passed to `CellClassFn` resolvers.
///
/// Uses pure-data fields only (no Dioxus types) so this type stays in
/// `chorale-core` per CHORALE-CORE-1. This differs from the `CellContext`
/// shape in recon-2 § 7c (which included `EventHandler<TRow>` and belongs
/// in `chorale-dioxus`). See auto-call ledger entry 2026-06-03-B.
#[derive(Clone, Debug)]
pub struct CellInfo<'a, TRow> {
    /// Stable identifier for the row containing this cell.
    pub row_id: RowId,
    /// Identifier of the column this cell belongs to.
    pub column_id: ColumnId,
    /// Reference to the full row data.
    pub row: &'a TRow,
    /// Whether the row containing this cell is currently selected.
    pub is_selected: bool,
}

/// Closure type that resolves a CSS class string for a row.
/// Stored in `Arc` so `TableProps` can be `Clone`.
pub type RowClassFn<TRow> = Arc<dyn Fn(&Row<TRow>) -> String + Send + Sync>;

/// Closure type that resolves a CSS class string for a body cell.
/// Stored in `Arc` so `ColumnDef` can be `Clone`.
pub type CellClassFn<TRow> = Arc<dyn Fn(&CellInfo<TRow>) -> String + Send + Sync>;
