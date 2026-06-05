use std::collections::HashMap;
use std::sync::Arc;

use crate::types::{Alignment, CellValue, ColumnId, CurrencyCode};

/// How to aggregate rows within a group for a column.
///
/// Set via [`ColumnDef::aggregator`]. Aggregation applies when grouping is
/// active (`state.grouping` is non-empty). The aggregated result appears in
/// each `GroupedRow::Header`'s `aggregates` vec at this column's position in
/// the effective column order.
///
/// `AggregatorKind` is `#[non_exhaustive]` so additional built-in aggregators
/// can be added in future minor releases.
#[non_exhaustive]
pub enum AggregatorKind<TRow> {
    /// Sum of numeric cell values (`CellValue::Integer` and `Float`).
    Sum,
    /// Average of numeric cell values. Returns `CellValue::Text("—")` when no
    /// numeric values are present.
    Average,
    /// Count of rows in the group. Always returns `CellValue::Integer`.
    Count,
    /// Minimum value (uses `CellValue::cmp_for_sort`).
    Min,
    /// Maximum value (uses `CellValue::cmp_for_sort`).
    Max,
    /// Host-supplied aggregation. Called with the group's rows; returns any `CellValue`.
    #[allow(clippy::type_complexity)]
    Custom(Arc<dyn Fn(&[&TRow]) -> CellValue + Send + Sync>),
}

impl<TRow> Clone for AggregatorKind<TRow> {
    fn clone(&self) -> Self {
        match self {
            Self::Sum => Self::Sum,
            Self::Average => Self::Average,
            Self::Count => Self::Count,
            Self::Min => Self::Min,
            Self::Max => Self::Max,
            Self::Custom(f) => Self::Custom(Arc::clone(f)),
        }
    }
}

impl<TRow> std::fmt::Debug for AggregatorKind<TRow> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sum => write!(f, "Sum"),
            Self::Average => write!(f, "Average"),
            Self::Count => write!(f, "Count"),
            Self::Min => write!(f, "Min"),
            Self::Max => write!(f, "Max"),
            Self::Custom(_) => write!(f, "Custom(<fn>)"),
        }
    }
}

/// Which edge a column is pinned to. Defaults to `None` (scrollable).
///
/// Set via [`ColumnDef::frozen`]. The adapter renders left-frozen columns at
/// the left edge (CSS `position: sticky; left: Xpx`), right-frozen columns at
/// the right edge, and scrollable columns in between.
///
/// `FrozenSide` is `#[non_exhaustive]` so additional pin positions (e.g. a
/// top/bottom axis for row pinning) can be added in future minor releases.
#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum FrozenSide {
    /// Column scrolls with the table (default).
    #[default]
    None,
    /// Column is pinned to the left edge.
    Left,
    /// Column is pinned to the right edge.
    Right,
}

/// Maps a `CellValue` text variant (or `Empty`) to a badge label and CSS
/// color token. Used by `RenderKind::Badge`.
///
/// The map key is the string the cell value carries when it is `CellValue::Text`.
/// `Empty` cells use the `empty_label` / `empty_color` fallback.
#[non_exhaustive]
#[derive(Clone, Debug, Default)]
pub struct BadgeVariantMap {
    /// Map from cell text value to badge display configuration.
    pub variants: HashMap<String, BadgeVariant>,
    /// Variant used when the cell value is not in `variants`.
    pub fallback: Option<BadgeVariant>,
}

impl BadgeVariantMap {
    /// Create an empty `BadgeVariantMap`.
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
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BadgeVariant {
    /// Text displayed inside the badge pill.
    pub label: String,
    /// CSS color token (e.g. `"green"`, `"red"`) used by the adapter to apply a style.
    pub color: String,
}

impl BadgeVariant {
    /// Create a `BadgeVariant` from a label and a color token.
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
#[non_exhaustive]
#[derive(Clone, Debug, Default)]
pub enum FilterKind {
    /// Column is not filterable. The filter row (if shown) renders an empty cell.
    #[default]
    None,
    /// Free-text substring search. Matches against `CellValue::Text`.
    Text,
    /// User picks zero or more values from a fixed option set. An empty
    /// selection passes all rows.
    MultiSelect {
        /// The list of allowed option strings shown in the filter dropdown.
        options: Vec<String>,
    },
    /// Numeric range bounded by `min..=max` with the given UI step.
    /// `min` / `max` configure the slider extents AND the default unset state
    /// (an unset filter equals `NumericRange { min: None, max: None }`).
    NumericRange {
        /// Inclusive lower bound of the slider range.
        min: f64,
        /// Inclusive upper bound of the slider range.
        max: f64,
        /// Step increment for the range-slider UI control.
        step: f64,
    },
    /// Date range picker. No bounds — both endpoints are optional.
    DateRange,
    /// Tri-state filter (All / true / false). "All" = no filter active.
    Boolean,
}

/// What kind of inline editor the adapter should render for an editable column.
///
/// Set via `ColumnDef::editor(kind)`. A column with `editor: None` (the default)
/// is read-only — `start_edit` will return `Err(ColumnNotEditable)` for it.
#[non_exhaustive]
#[derive(Clone, Debug)]
pub enum EditorKind {
    /// Free-text `<input type="text">`.
    Text,
    /// Numeric `<input type="number">` with optional bounds and step.
    Number {
        /// Inclusive minimum value, or `None` for unbounded.
        min: Option<f64>,
        /// Inclusive maximum value, or `None` for unbounded.
        max: Option<f64>,
        /// Step increment for the spin-button UI, or `None` to let the browser decide.
        step: Option<f64>,
    },
    /// Date picker `<input type="date">`.
    Date,
    /// Boolean toggle `<input type="checkbox">`.
    BoolToggle,
    /// Custom: the host supplies a renderer via the adapter's `cell_renderers` prop.
    /// The adapter falls back to a text input if no custom renderer is provided.
    Custom,
}

/// How the adapter renders a cell's value by default.
///
/// `Custom` is intentionally absent: custom cell rendering requires Dioxus
/// types (`Element`, `EventHandler`) and lives in `chorale-dioxus`
/// per CHORALE-CORE-1. See recon-2 § 7b (and the CHORALE-CORE-1
/// auto-call entry 2026-06-03-B).
#[non_exhaustive]
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
#[non_exhaustive]
pub struct ColumnDef<TRow> {
    /// Unique identifier for this column. Must be unique within a table.
    pub id: ColumnId,
    /// Text displayed in the column's header cell.
    pub header: String,
    /// Extract the cell value for this column from a row.
    pub accessor: Arc<dyn Fn(&TRow) -> CellValue + Send + Sync>,
    /// Whether the column header is clickable to sort the table by this column.
    pub sortable: bool,
    /// Filter UI and matching strategy for this column. Defaults to
    /// `FilterKind::None` (not filterable).
    pub filter: FilterKind,
    /// Override the column's starting width in px. `None` = auto.
    pub initial_width: Option<f64>,
    /// Horizontal text alignment for this column's header and body cells.
    pub alignment: Alignment,
    /// How the adapter renders cell values for this column by default.
    pub render_kind: RenderKind,
    /// Optional static CSS class applied to every header cell of this column.
    pub header_class: Option<String>,
    /// Optional dynamic CSS class resolver for body cells of this column.
    /// See `CellClassFn` in `crate::theme`.
    pub cell_class: Option<crate::theme::CellClassFn<TRow>>,
    /// What kind of inline editor to render when this cell is in edit mode.
    /// `None` (default) means the column is read-only; `start_edit` will return
    /// `Err(StateError::ColumnNotEditable)` for it.
    pub editor: Option<EditorKind>,
    /// Which edge this column is pinned to, or `None` for scrollable (default).
    ///
    /// Set via `.frozen(FrozenSide::Left)` / `.frozen(FrozenSide::Right)`.
    /// The adapter renders frozen columns with CSS `position: sticky` and a
    /// computed `left`/`right` offset.
    pub frozen: FrozenSide,
    /// How to aggregate this column's values within a group.
    ///
    /// `None` (default) means the column shows no aggregate in group headers.
    /// Only applies when grouping is active (`state.grouping` is non-empty).
    pub aggregator: Option<AggregatorKind<TRow>>,
}

impl<TRow> ColumnDef<TRow> {
    /// Create a new column with the three required fields. All optional fields
    /// take sensible defaults; use the builder methods below to override.
    pub fn new(
        id: ColumnId,
        header: impl Into<String>,
        accessor: impl Fn(&TRow) -> CellValue + Send + Sync + 'static,
    ) -> Self {
        Self {
            id,
            header: header.into(),
            accessor: Arc::new(accessor),
            sortable: false,
            filter: FilterKind::None,
            initial_width: None,
            alignment: Alignment::Left,
            render_kind: RenderKind::Text,
            header_class: None,
            cell_class: None,
            editor: None,
            frozen: FrozenSide::None,
            aggregator: None,
        }
    }

    /// Mark the column as sortable. Headers become clickable in the adapter
    /// (assuming `sort_enabled` is true on the `Table` component).
    #[must_use]
    pub fn sortable(mut self) -> Self {
        self.sortable = true;
        self
    }

    /// Set the column's filter UI / matching kind.
    #[must_use]
    pub fn filter(mut self, filter: FilterKind) -> Self {
        self.filter = filter;
        self
    }

    /// Set the column's initial width in pixels. `None` = auto.
    #[must_use]
    pub fn initial_width(mut self, width: f64) -> Self {
        self.initial_width = Some(width);
        self
    }

    /// Set the column's text alignment.
    #[must_use]
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Set the column's render kind.
    #[must_use]
    pub fn render_kind(mut self, render_kind: RenderKind) -> Self {
        self.render_kind = render_kind;
        self
    }

    /// Set a static CSS class applied to every header cell of this column.
    #[must_use]
    pub fn header_class(mut self, class: impl Into<String>) -> Self {
        self.header_class = Some(class.into());
        self
    }

    /// Set a dynamic CSS class resolver for body cells of this column.
    #[must_use]
    pub fn cell_class(mut self, class_fn: crate::theme::CellClassFn<TRow>) -> Self {
        self.cell_class = Some(class_fn);
        self
    }

    /// Mark this column as editable with the given editor kind.
    ///
    /// A column without `.editor(...)` is read-only: `start_edit` returns
    /// `Err(StateError::ColumnNotEditable)` for it.
    #[must_use]
    pub fn editor(mut self, kind: EditorKind) -> Self {
        self.editor = Some(kind);
        self
    }

    /// Pin this column to an edge. `FrozenSide::None` (the default) leaves the
    /// column scrollable; `Left` / `Right` fix it at that edge.
    #[must_use]
    pub fn frozen(mut self, side: FrozenSide) -> Self {
        self.frozen = side;
        self
    }

    /// Set an aggregation function for this column.
    ///
    /// When grouping is active, each group header shows the aggregated value
    /// for this column in its `aggregates` vec. `None` (the default) shows no aggregate.
    #[must_use]
    pub fn aggregator(mut self, kind: AggregatorKind<TRow>) -> Self {
        self.aggregator = Some(kind);
        self
    }

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
            editor: self.editor.clone(),
            frozen: self.frozen.clone(),
            aggregator: self.aggregator.clone(),
        }
    }
}
