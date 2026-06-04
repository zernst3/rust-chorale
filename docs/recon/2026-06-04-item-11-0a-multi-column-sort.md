# Item 11.0a: Multi-Column Sort

## Problem

chorale v0.1.0 ships single-column sort: `state.sort: Vec<SortState>` holds at most one
element; `toggle_sort` replaces the vec contents on every call. Users who need
priority-ordered multi-column sort — "sort by department first, then by name within
department" — have no supported path.

`leptos-struct-table` ships multi-column sort. AG Grid and TanStack Table ship it as a
core primitive with Shift+click UX. v0.2.0 chorale needs parity.

Critically, the **type change has already shipped:** `state.sort: Vec<SortState>` is in
v0.1.0. The type can hold multiple entries today. What v0.2.0 adds is (a) new
`toggle_sort` semantics that append rather than replace on Shift+click, (b) a
`remove_sort` transition, (c) sort-priority badge rendering on headers, and (d) the
Shift+click detection in the chorale-dioxus adapter.

## Proposed Public API

### `chorale-core`

No new types. The changes are behavioral:

```rust
/// New enum for the caller to specify which action to take on sort toggle.
/// The adapter passes this based on whether Shift was held during the click.
pub enum SortAction {
    /// Replace the entire sort with this column (single-column, plain click).
    Replace,
    /// Append this column as the lowest-priority sort (Shift+click / multi-column).
    Append,
}

/// Updated transition (breaking change to the function signature — see §Backwards Compat).
/// Replaces the v0.1.0 `toggle_sort(state, col) -> TableState`.
pub fn toggle_sort(
    state: &TableState<TRow>,
    column_id: &ColumnId,
    action: SortAction,
) -> TableState<TRow>;

/// New transition: remove a specific column from the sort list entirely.
pub fn remove_sort(state: &TableState<TRow>, column_id: &ColumnId) -> TableState<TRow>;

/// New transition: clear all sort columns.
pub fn clear_sort(state: &TableState<TRow>) -> TableState<TRow>;
```

**`toggle_sort` semantics:**
- `SortAction::Replace`:
  - If the column is not in `state.sort`, set `sort = [SortState::new(col, Ascending)]`.
  - If the column is `Ascending` in `sort[0]` (or anywhere), set to `Descending` in-place and move to front.
  - If the column is `Descending` in `sort[0]`, remove it (no sort on this column).
  - Clears all other sort columns (replaces the vec).
- `SortAction::Append`:
  - If the column is not in `state.sort`, append `SortState::new(col, Ascending)`.
  - If the column is already `Ascending` in the sort, flip to `Descending` in place.
  - If the column is already `Descending` in the sort, remove it from the list.
  - Does NOT modify the priority of other columns.

### `chorale-dioxus`

The header's sort icon click handler passes `SortAction::Replace` for a plain click and
`SortAction::Append` for a Shift+click:

```rust
// Inside the adapter's header onclick handler:
let action = if keyboard_modifiers().shift() {
    SortAction::Append
} else {
    SortAction::Replace
};
handle.toggle_sort(&column_id, action);
```

No new props on `Table`. The sort-priority badge (1, 2, 3 ...) is rendered on each
sorted column's header when `state.sort.len() > 1`:

```rust
// In header cell render:
if let Some(pos) = state.sort.iter().position(|s| s.column_id == col.id) {
    if state.sort.len() > 1 {
        rsx! { span { class: "chorale-sort-badge", "{pos + 1}" } }
    }
}
```

The badge is hidden when only one column is sorted (no need to show "1" for a
single-column sort).

## Backwards Compatibility

**The `Vec<SortState>` type is already shipped.** Cross-crate callers that read
`state.sort` receive a `Vec<SortState>` today. No type change in this item.

**`toggle_sort` signature change is breaking for direct callers.** The v0.1.0 signature
is `toggle_sort(state, col) -> TableState`; v0.2.0 adds the `action: SortAction`
parameter. Any caller that calls `toggle_sort` directly must add `SortAction::Replace`
(or `SortAction::Append`) as the third argument.

This is a genuine source-breaking change. However, it is a deliberate one: the v0.1.0
`toggle_sort` had no way to express multi-column append, and the parameter addition is
the minimal change needed. Callers who want the old single-column behavior pass
`SortAction::Replace` — identical semantics. The migration path is mechanical: add a
third argument.

**`SortAction` is a new enum** (not yet `#[non_exhaustive]` because it has only two
variants and is not expected to grow; consult open question #4). Cross-crate code that
pattern-matches on `SortAction` would need a wildcard arm if it were `#[non_exhaustive]`;
see question #4.

**`remove_sort` and `clear_sort` are new functions.** Adding functions is non-breaking.

**Adapter users** (the 99% case — calling `Table { ... }` in RSX) are unaffected: the
adapter's header click handler is the only caller of `toggle_sort`, and the adapter is
updated in the same PR.

## Test Plan

Per TESTS-1:

- `toggle_sort` Replace on unsorted → `sort == [SortState(col, Ascending)]`.
- `toggle_sort` Replace on Ascending → `sort == [SortState(col, Descending)]`.
- `toggle_sort` Replace on Descending → `sort == []`.
- `toggle_sort` Replace with an existing multi-sort → clears other columns, sorts only on
  the target column.
- `toggle_sort` Append on unsorted → appends to existing sort list without replacing.
- `toggle_sort` Append on an already-sorted column → flips direction in place.
- `toggle_sort` Append on Descending → removes that column from the list; others unchanged.
- `remove_sort`: removes the target column; others unchanged; no-op if not in list.
- `clear_sort`: returns `sort == []`.
- Multi-column sort order correctness: two rows that are equal on column A are ordered
  by column B (priority test).
- Shift+click detection: adapter passes `SortAction::Append` when Shift held (integration
  test using Dioxus's test renderer or a manual QA step; see Item 11.6).

## Open Questions / Decisions Zach Must Sign Off (API-1)

1. **The v0.2.0 `toggle_sort` signature breaks callers who call it directly. Should we
   provide a `toggle_sort_single(state, col)` wrapper for the single-column case to make
   migration easier?** Recommendation: no — the migration is mechanical (add
   `SortAction::Replace`) and a wrapper would create two entry points with overlapping
   semantics. The semver bump to 0.2.0 signals that breaking changes may occur.

2. **`toggle_sort` Append: when the column is already the highest priority and Descending,
   should removing it shift the remaining columns up in priority automatically?**
   Recommendation: yes — the remaining columns in the vec are already in priority order;
   removing an element naturally shifts the rest. No extra logic needed.

3. **Sort badge rendering: number (1, 2, 3) vs sort-priority indicators (arrows with
   subscripts) vs a secondary sort icon?** Recommendation: number badge — simplest to
   implement, consistent with AG Grid's behavior, legible at small header sizes.

4. **Should `SortAction` be `#[non_exhaustive]`?** Recommendation: no — it has exactly
   two semantically distinct actions (Replace / Append), and cross-crate code that
   matches on it exhaustively is correct. A future `Shift+Ctrl+click` action (if any)
   would be a new variant added in a major version. Keeping it exhaustive lets the
   compiler catch incomplete match arms immediately.

5. **Should the sort-priority badge be shown when only one column is sorted?**
   Recommendation: no — hide the badge for single-column sort. A lone "1" badge is
   visual noise when there's only one sort active. The existing arrow icon conveys
   direction without a number.
