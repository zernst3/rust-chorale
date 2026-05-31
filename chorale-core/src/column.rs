use std::collections::HashMap;
use std::sync::Arc;

use crate::types::{Alignment, CellValue, ColumnId, CurrencyCode};

/// Maps a `CellValue` text variant (or `Empty`) to a badge label and CSS
/// color token. Used by `RenderKind::Badge`.
///
/// The map key is the string the cell value carries when it is `CellValue::Text`.
/// `Empty` cells use the `empty_label` / `empty_color` fallback.
#[derive(Clone, Debug, Default)]
pub struct BadgeVariantMap {
    pub variants: HashMap<String, BadgeVariant>,
    pub fallback: Option<BadgeVariant>,
}

impl BadgeVariantMap {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a mapping from a cell text value to a badge variant.
    #[must_use]
    pub fn with(mut self, text: impl Into<String>, variant: BadgeVariant) -> Self {
        self.variants.insert(text.into(), variant);
        self
    }

    /// Set the fallback variant for values not in the map.
    #[must_use]
    pub fn with_fallback(mut self, variant: BadgeVariant) -> Self {
        self.fallback = Some(variant);
        self
    }

    /// Resolve a badge variant for a cell text value.
    #[must_use]
    pub fn resolve(&self, text: &str) -> Option<&BadgeVariant> {
        self.variants.get(text).or(self.fallback.as_ref())
    }
}

/// A single badge variant: the display label and a CSS color token.
///
/// `color` is a short token (e.g. `"green"`, `"yellow"`, `"red"`) that the
/// adapter maps to a CSS class such as `chorale-badge--green`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BadgeVariant {
    pub label: String,
    pub color: String,
}

impl BadgeVariant {
    #[must_use]
    pub fn new(label: impl Into<String>, color: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            color: color.into(),
        }
    }
}

/// Per-column filter configuration: declares the filter UI to render and
/// how the column should be filtered.
///
/// Each variant pairs with a [`crate::FilterValue`] variant:
///
/// | `FilterKind` | `FilterValue`   | UI rendered by `chorale-dioxus` |
/// |---|---|---|
/// | `None`             | —                          | none (column not filterable) |
/// | `Text`             | `Text`                     | text input (case-insensitive substring) |
/// | `MultiSelect`      | `MultiSelect(HashSet)`     | dropdown / `<details>` with checkbox list |
/// | `NumericRange`     | `NumericRange { min, max }`| dual-handle range slider |
/// | `DateRange`        | `DateRange { min, max }`   | two `<input type="date">` fields |
/// | `Boolean`          | `Boolean(bool)`            | tri-state radio (All / Yes / No) |
///
/// The default is `None` so adding a `ColumnDef` without specifying a filter
/// produces a non-filterable column.
#[derive(Clone, Debug, Default)]
pub enum FilterKind {
    /// Column is not filterable. The filter row (if shown) renders an empty cell.
    #[default]
    None,
    /// Free-text substring search. Matches against `CellValue::Text`.
    Text,
    /// User picks zero or more values from a fixed option set. An empty
    /// selection passes all rows.
    MultiSelect { options: Vec<String> },
    /// Numeric range bounded by `min..=max` with the given UI step.
    /// `min` / `max` configure the slider extents AND the default unset state
    /// (an unset filter equals `NumericRange { min: None, max: None }`).
    NumericRange { min: f64, max: f64, step: f64 },
    /// Date range picker. No bounds — both endpoints are optional.
    DateRange,
    /// Tri-state filter (All / true / false). "All" = no filter active.
    Boolean,
}

/// How the adapter renders a cell's value by default.
///
/// `Custom` is intentionally absent: custom cell rendering requires Dioxus
/// types (`Element`, `EventHandler`) and lives in `chorale-dioxus`
/// per CHORALE-CORE-1. See recon-2 § 7b (and the CHORALE-CORE-1
/// auto-call entry 2026-06-03-B).
#[derive(Clone, Debug, Default)]
pub enum RenderKind {
    /// Left-aligned plain text; ellipsis on overflow.
    #[default]
    Text,
    /// Right-aligned with thousand-separators, no decimals.
    Number,
    /// Right-aligned with currency symbol prefix and two decimal places.
    Currency(CurrencyCode),
    /// Formatted date via the project date-helper (adapter-supplied).
    Date,
    /// Formatted date + time (adapter-supplied formatter).
    DateTime,
    /// Center-aligned; checkmark for `true`, cross for `false`.
    Boolean,
    /// Colored pill/chip; text value is resolved via the `BadgeVariantMap`.
    Badge(BadgeVariantMap),
}

/// Definition for a single column in the table.
///
/// The `accessor` closure extracts a `CellValue` from a row; it is the only
/// place in `chorale-core` that knows the row type's internal structure.
///
/// Per CHORALE-CORE-1: `ColumnDef` carries no framework types.
/// Per ROBUSTNESS-1: named struct fields, not a tuple or builder-only API.
pub struct ColumnDef<TRow> {
    pub id: ColumnId,
    pub header: String,
    /// Extract the cell value for this column from a row.
    pub accessor: Arc<dyn Fn(&TRow) -> CellValue + Send + Sync>,
    pub sortable: bool,
    /// Filter UI and matching strategy for this column. Defaults to
    /// `FilterKind::None` (not filterable).
    pub filter: FilterKind,
    /// Override the column's starting width in px. `None` = auto.
    pub initial_width: Option<f64>,
    pub alignment: Alignment,
    pub render_kind: RenderKind,
    /// Optional static CSS class applied to every header cell of this column.
    pub header_class: Option<String>,
    /// Optional dynamic CSS class resolver for body cells of this column.
    /// See `CellClassFn` in `crate::theme`.
    pub cell_class: Option<crate::theme::CellClassFn<TRow>>,
}

impl<TRow> ColumnDef<TRow> {
    /// True if the column has any filter UI configured (anything other than
    /// `FilterKind::None`).
    #[must_use]
    pub fn is_filterable(&self) -> bool {
        !matches!(self.filter, FilterKind::None)
    }
}

impl<TRow> Clone for ColumnDef<TRow> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            header: self.header.clone(),
            accessor: Arc::clone(&self.accessor),
            sortable: self.sortable,
            filter: self.filter.clone(),
            initial_width: self.initial_width,
            alignment: self.alignment,
            render_kind: self.render_kind.clone(),
            header_class: self.header_class.clone(),
            cell_class: self.cell_class.clone(),
        }
    }
}
