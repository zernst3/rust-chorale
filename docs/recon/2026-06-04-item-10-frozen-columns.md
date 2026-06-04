# Item 10: Frozen Columns

## Problem

chorale v0.1.0 renders all columns in a single scrollable container. On wide tables —
the canonical use case that motivates column virtualization — a user scrolling right
loses the identity context columns ("Name", "ID", "Status") that anchor each row. The
entire row becomes ambiguous without the leading identifier.

Frozen columns ("sticky columns" in CSS, "pinned columns" in AG Grid terminology) hold
one or more columns at the left or right edge of the scroll area regardless of horizontal
scroll position. They are a prerequisite for any wide-table workflow: CRM pipelines,
finance grids, data analysis tables.

`table-rs` ships no frozen columns. `leptos-struct-table` ships no frozen columns. AG
Grid, TanStack Table, and every enterprise JS table ship frozen columns as a core feature.
v0.2.0 chorale can ship this gap-filler first among Rust table libraries.

## Proposed Public API

### `chorale-core`

```rust
/// Which edge a column is frozen to. Defaults to `None` (not frozen).
#[non_exhaustive]
pub enum FrozenSide {
    None,
    Left,
    Right,
}

/// Builder method added to ColumnDef.
impl<TRow> ColumnDef<TRow> {
    #[must_use]
    pub fn frozen(self, side: FrozenSide) -> Self;
}

/// Added field on ColumnDef (accessed via builder; not constructed directly by cross-crate callers).
pub frozen: FrozenSide,
```

No new `TableState` fields. Frozen status is per-column metadata, not per-table state.

Core also exposes two query helpers used by adapters to compute rendering geometry:

```rust
/// Returns the columns frozen to the left, in effective column order.
pub fn frozen_left_columns<TRow>(state: &TableState<TRow>) -> Vec<&ColumnDef<TRow>>;

/// Returns the columns frozen to the right, in effective column order.
pub fn frozen_right_columns<TRow>(state: &TableState<TRow>) -> Vec<&ColumnDef<TRow>>;

/// Returns the scrollable (non-frozen) columns, in effective column order.
pub fn scrollable_columns<TRow>(state: &TableState<TRow>) -> Vec<&ColumnDef<TRow>>;
```

These are pure functions over `state.columns` and `state.column_order`; no new state.

### `chorale-dioxus`

No new props required for basic frozen column support. The adapter's header and body
rendering already iterates columns; it will use the three helpers above to split columns
into three layout zones.

Optional prop for customization:

```rust
/// CSS z-index override for frozen column cells. Defaults to `2` (above scrollable
/// columns). Raise if custom cell renderers use z-index internally.
pub frozen_column_z_index: Option<i32>,
```

Callsite shape:

```rust
let columns: Vec<ColumnDef<Row>> = vec![
    ColumnDef::new("name", "Name", |r| CellValue::Text(r.name.clone()))
        .frozen(FrozenSide::Left),
    ColumnDef::new("id", "ID", |r| CellValue::Number(r.id as f64))
        .frozen(FrozenSide::Left),
    ColumnDef::new("amount", "Amount", |r| CellValue::Number(r.amount)),
    // scrollable columns ...
    ColumnDef::new("actions", "Actions", |r| CellValue::Text("…".into()))
        .frozen(FrozenSide::Right),
];
```

## Internal Design

**CSS layout strategy:** the table is wrapped in a horizontally scrollable container.
Frozen columns use `position: sticky` with computed `left` (for left-frozen) or `right`
(for right-frozen) offset values. The sticky offset for the `k`th left-frozen column is
the sum of widths of the `0..k` left-frozen columns. The sticky offset for the `j`th
right-frozen column (counting from the right) is the sum of widths of the `0..j`
right-frozen columns on the right.

This avoids a separate DOM layer for frozen columns (unlike some implementations that use
three separate `<table>` elements). `position: sticky` is supported in all browsers that
support WASM (per chorale's compatibility baseline).

**Width tracking:** frozen offset computation depends on knowing each frozen column's
rendered width. chorale v0.1.0 already tracks `column_widths: HashMap<ColumnId, f64>`
in `TableState` for the column-resize feature. The adapter reads those widths to compute
`left`/`right` CSS values. If a width is not yet measured (before the first render), a
fallback of `initial_width` (from `ColumnDef`) is used.

**Rendering order:** the DOM order of columns must match the visual order (left-frozen,
then scrollable, then right-frozen) so that `position: sticky` works correctly. The
adapter applies `frozen_left_columns + scrollable_columns + frozen_right_columns` order
regardless of `column_order`. Column reorder (Item 9) applies within each zone: users
can reorder among left-frozen columns, among scrollable columns, or among right-frozen
columns, but cannot drag a column across the frozen/scrollable boundary without changing
its `frozen` setting.

**Shadow divider:** a box-shadow on the last left-frozen column and the first right-frozen
column indicates the scroll boundary. The adapter renders this via CSS `box-shadow` on the
sticky cells, mirroring the row-separator convention from v0.1.0 virtualization.

## Backwards Compatibility

`frozen: FrozenSide` is a new field on `ColumnDef<TRow>`. `ColumnDef` is `#[non_exhaustive]`
in v0.1.0, so cross-crate callers cannot use struct-literal construction; adding the field
does not break compilation downstream. The field defaults to `FrozenSide::None` in
`ColumnDef::new`, so existing column definitions are not frozen.

`FrozenSide` is a new `#[non_exhaustive]` enum; no existing code matches on it.

`frozen_left_columns`, `frozen_right_columns`, and `scrollable_columns` are new public
functions. Adding functions is never a breaking change.

When `all` columns have `FrozenSide::None` (the default), `scrollable_columns` returns
all columns in definition/order order, and the table renders identically to v0.1.0.

## Test Plan

Per TESTS-1:

- `frozen_left_columns`: returns only left-frozen columns in effective order.
- `frozen_right_columns`: returns only right-frozen columns in effective order.
- `scrollable_columns`: returns non-frozen columns; union of all three is equal to all
  columns.
- All three helpers: empty table (no columns) → each returns empty vec.
- Mixed configuration: 2 left-frozen, 3 scrollable, 1 right-frozen — each helper returns
  the correct partition.
- Builder: `ColumnDef::new(...).frozen(FrozenSide::Left)` yields `frozen == Left`.
- Default: `ColumnDef::new(...)` without `.frozen(...)` yields `frozen == None`.
- Interaction with column reorder: `column_order` is respected within each frozen zone.
- Interaction with column visibility: hidden frozen columns are excluded from their zone.

## Open Questions / Decisions Zach Must Sign Off (API-1)

1. **Should users be able to change `FrozenSide` at runtime (via a state transition), or
   is it fixed at column-definition time?** Recommendation: fixed at definition time (on
   `ColumnDef`, not `TableState`). Runtime toggling adds a `set_frozen` transition and a
   per-column state field; deferred to v0.3. Most apps define column frozenness
   statically.

2. **Can a user drag a column from a frozen zone to the scrollable zone at runtime?**
   Recommendation: no — this would require changing `FrozenSide` at runtime (see #1).
   Document this as v0.3 scope. Dragging within a zone (reordering frozen-left columns
   among themselves) is supported via Item 9.

3. **Are `frozen_left_columns`, `frozen_right_columns`, and `scrollable_columns` necessary
   as public API, or should they be pub(crate)?** Recommendation: public, because
   chorale-leptos (Item 11.5) will need them too. If they're pub(crate), the Leptos
   adapter must duplicate the logic. A single public helper benefits all adapters.

4. **What should happen when a frozen column has no explicit `initial_width` and no
   measured width yet?** Recommendation: fall back to `150.0` px (the v0.1.0 default
   column width) for sticky offset computation. This avoids a 0-width frozen column on
   first render. Once measured, the correct offset applies immediately.

5. **Should the "frozen divider" shadow be styled via a CSS variable or a hardcoded
   `box-shadow`?** Recommendation: CSS variable (`--chorale-frozen-divider-shadow`) with
   a default value matching the row-separator convention. This lets host apps theme the
   divider without overriding Tailwind classes.
