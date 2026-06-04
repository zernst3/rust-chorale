# Item 8: Grouping and Aggregation

## Problem

chorale v0.1.0 renders rows in a flat list (after filter and sort). Users who need to
group rows by a categorical column — and optionally aggregate numeric columns per group
(sum, average, count) — must implement this entirely outside the table. This means
duplicating the filter-sort pipeline, managing collapse/expand state by hand, and
interleaving header rows with data rows in a custom render loop.

`leptos-struct-table` has no built-in grouping. `table-rs` has no built-in grouping.
AG Grid, TanStack Table, and every enterprise-grade JS table library treat grouping as a
first-class primitive. v0.2.0 chorale can differentiate on both Dioxus and Leptos sides
by shipping grouping before either competitor does.

The primary use cases: (a) a CRM table grouped by deal stage, (b) a finance table grouped
by cost center with subtotals, (c) a product catalog grouped by category. All require
collapse/expand, a group-header row with a label and optional aggregate values, and
interaction with the existing sort and filter pipeline.

## Proposed Public API

### `chorale-core`

```rust
/// Ordered list of columns to group by. First element is the outermost group.
/// Added to `TableState` as an additive field.
pub grouping: Vec<ColumnId>,

/// Which group keys are collapsed. Empty = all expanded.
pub collapsed_groups: HashSet<GroupKey>,

/// Opaque key identifying a group (the concatenated group-by column values).
/// `Display` impl for debugging; `Hash + Eq` for set membership.
#[non_exhaustive]
pub struct GroupKey(pub(crate) String);

/// How to aggregate rows within a group for a column.
#[non_exhaustive]
pub enum AggregatorKind {
    Sum,
    Average,
    Count,
    Min,
    Max,
    /// Host supplies a custom aggregation closure.
    /// Signature: `fn(&[&TRow]) -> CellValue` — called with the group's rows.
    Custom(Arc<dyn Fn(&[&TRow]) -> CellValue + Send + Sync>),
}

/// Builder method added to ColumnDef.
impl<TRow> ColumnDef<TRow> {
    #[must_use]
    pub fn aggregator(self, kind: AggregatorKind) -> Self;
}

/// A row in the grouped view: either a group header or a data row.
#[non_exhaustive]
pub enum GroupedRow<TRow> {
    Header {
        key: GroupKey,
        label: String,
        depth: usize,
        row_count: usize,
        is_collapsed: bool,
        aggregates: Vec<Option<CellValue>>,  // one per column, None if no aggregator
    },
    Data(Row<TRow>),
}

/// Transitions:
pub fn set_grouping(state: &TableState<TRow>, columns: Vec<ColumnId>) -> TableState<TRow>;
pub fn toggle_group(state: &TableState<TRow>, key: &GroupKey) -> TableState<TRow>;
pub fn expand_all_groups(state: &TableState<TRow>) -> TableState<TRow>;
pub fn collapse_all_groups(state: &TableState<TRow>) -> TableState<TRow>;

/// View function: returns the interleaved group-header + data rows for the current
/// page, after filtering and sorting. Returns a flat list of GroupedRow items.
/// When `grouping` is empty, every item is `GroupedRow::Data`; callers may use the
/// simpler `visible_view` instead.
pub fn visible_grouped_view(state: &TableState<TRow>) -> Vec<GroupedRow<TRow>>;
```

### `chorale-dioxus`

No new props required for basic grouping — the adapter detects `state.grouping.is_empty()`
and switches between `visible_view` and `visible_grouped_view` internally. Rendering of
group-header rows is built in (indented label, collapse toggle, aggregate cells).

Optional prop for styling:

```rust
/// CSS class applied to group-header rows. Defaults to `"chorale-group-header"`.
pub group_header_class: Option<String>,
```

## Internal Design

**Group key construction:** `GroupKey` is formed by concatenating the string
representations of the group-by column values for a row, separated by a `\0` delimiter.
Order follows `state.grouping` (outermost first). Two rows are in the same group iff
their `GroupKey` is equal.

**Algorithm in `visible_grouped_view`:**
1. Run the existing filter + sort pipeline over all rows.
2. Partition rows into a tree of groups by iterating `state.grouping` left-to-right.
3. Flatten the tree into `Vec<GroupedRow>` with DFS traversal, skipping collapsed
   subtrees (their Data rows are omitted; their Header row is included with
   `is_collapsed: true`).
4. Apply pagination to the flat interleaved list (headers count toward the page size,
   which may surprise users — open question #3 below).

**Aggregation:** computed during step 2 for each group node. Built-in aggregators
(`Sum`, `Average`, `Count`, `Min`, `Max`) operate on the `CellValue` returned by the
column's accessor. Numeric operations (`Sum`, `Average`, `Min`, `Max`) coerce
`CellValue::Number`, `CellValue::Currency`, and `CellValue::Percentage` to `f64`;
return `CellValue::Text("—")` for non-numeric cells.

**Interaction with sort:** when grouping is active, sort applies within groups (rows in
each group are sorted by `state.sort`; group order is determined by the group key).
Sorting by a group-by column is a no-op (the column is already the group key).

**Interaction with filter:** filter applies to rows before grouping. A group with all
rows filtered out is omitted from the view entirely.

**Interaction with variable-row-height (Item 6):** group-header rows are measured as
ordinary render-index entries in `row_heights`. No special handling required.

## Backwards Compatibility

`grouping: Vec<ColumnId>` and `collapsed_groups: HashSet<GroupKey>` are additive fields
on `TableState`. `TableState` is `#[non_exhaustive]` in v0.1.0, so cross-crate callers
cannot use struct-literal construction; adding these fields does not break compilation
downstream. Both default to empty (no grouping) in `TableState::new`.

`AggregatorKind` is a new `#[non_exhaustive]` enum. Cross-crate matches already require
a wildcard arm. The `.aggregator(kind)` builder method is additive on `ColumnDef`.

`GroupedRow<TRow>` is a new `#[non_exhaustive]` enum; no existing code matches on it.

`visible_grouped_view` is a new function; it does not replace `visible_view`. Callers that
use `visible_view` today continue unchanged.

## Test Plan

Per TESTS-1:

- `set_grouping`: happy path — state has `grouping` updated; `collapsed_groups` cleared.
- `toggle_group`: collapses an expanded group (adds key to set); expands a collapsed group
  (removes key from set).
- `expand_all_groups` / `collapse_all_groups`: set is empty / set contains all keys.
- `visible_grouped_view` with no grouping: identical result to `visible_view` (modulo
  the `GroupedRow::Data` wrapper).
- `visible_grouped_view` with one group-by column: correct group headers interleaved,
  correct row counts.
- `visible_grouped_view` with a collapsed group: data rows for that group absent, header
  present with `is_collapsed: true`.
- Aggregation: `Sum` over a numeric column returns correct `CellValue::Number`.
- Aggregation: `Count` over any column returns `CellValue::Number` equal to group size.
- Filter interaction: rows filtered out do not appear in any group; empty groups are omitted.
- Sort interaction: rows within a group are sorted by `state.sort`.
- Nested grouping (two group-by columns): outer group headers contain inner group headers.
- Edge: single row in a group — header has `row_count: 1`.
- Edge: all rows filtered → `visible_grouped_view` returns empty vec.

## Open Questions / Decisions Zach Must Sign Off (API-1)

1. **Does pagination apply to the interleaved list (headers + rows) or only to data rows?**
   Recommendation: apply pagination to data rows only; headers for visible groups are
   always included on whatever page their rows appear. This is more intuitive but requires
   a two-pass page calculation. Alternative: paginate the flat interleaved list (simpler,
   but a page may show a header with no data rows if the group boundary falls at a page
   edge). Zach should decide which feels more "spreadsheet-natural."

2. **Multi-level grouping depth limit.** Recommendation: no artificial limit; allow any
   depth `state.grouping.len()`. In practice users group by 1–3 columns; deep trees are
   self-imposed and easy to document.

3. **`GroupKey` opacity: should host code be able to construct a `GroupKey` directly (to
   pre-collapse specific groups)?** Recommendation: expose a `GroupKey::from_values(vals:
   &[String]) -> GroupKey` constructor so hosts can write `toggle_group(state, &GroupKey::
   from_values(&["Open"]))` without scraping the group-header row. The internal format
   remains an implementation detail.

4. **Aggregation result type for `Custom` aggregator: `CellValue` or a callback that
   returns `String`?** Recommendation: `CellValue`, so custom aggregates can participate
   in numeric formatting (currency, percentage). If `CellValue` is too restrictive for
   some use case, the host can return `CellValue::Text` with a pre-formatted string.

5. **Should `visible_grouped_view` use pagination from `TableState` (current page /
   page size) or always return all grouped rows?** Recommendation: respect pagination
   (uses the existing `current_page` / `page_size` fields). Returning all rows at once
   breaks virtualization on large data sets. This ties into open question #1.
