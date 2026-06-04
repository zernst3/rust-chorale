# Item 9: Column Reorder

## Problem

chorale v0.1.0 renders columns in the order they appear in the `Vec<ColumnDef<TRow>>`
supplied by the host. The order is fixed at construction time. Users who want to
rearrange columns — moving frequently-used columns closer together, customizing their
view of a wide table — have no supported path. They must either provide a new `columns`
vec and re-mount the whole component, or implement custom drag-and-drop outside chorale
entirely.

`table-rs` ships no column reordering. `leptos-struct-table` has no column reordering.
TanStack Table ships `columnOrder` as a first-class state field; AG Grid ships it as
"column drag-and-drop." v0.2.0 chorale can differentiate by shipping column reordering
with a clean headless state model.

The user story: in a wide data table with 15+ columns, a user drags the "Last Updated"
column to sit immediately after "Name," saves this preference (host app owns persistence),
and sees the columns in the new order on every subsequent render without re-mounting.

## Proposed Public API

### `chorale-core`

```rust
/// Ordered list of column IDs controlling display order.
/// When empty (the default), columns render in definition order.
/// When populated, renders columns in this order; IDs absent from the list are
/// appended at the end in definition order (so newly-added columns always appear).
pub column_order: Vec<ColumnId>,

/// Transitions:
/// Set an explicit column order. Validates that every ID in `order` exists
/// in `state.columns`; returns Err on unknown IDs.
pub fn set_column_order(
    state: &TableState<TRow>,
    order: Vec<ColumnId>,
) -> Result<TableState<TRow>, StateError>;

/// Move a single column from its current position to a new index.
/// Equivalent to splicing `column_order`.
pub fn move_column(
    state: &TableState<TRow>,
    column_id: &ColumnId,
    to_index: usize,
) -> Result<TableState<TRow>, StateError>;

/// Reset to definition order (clears column_order).
pub fn reset_column_order(state: &TableState<TRow>) -> TableState<TRow>;
```

The effective render order is derived in a helper (internal to core, not pub):

```rust
fn effective_column_order<TRow>(state: &TableState<TRow>) -> Vec<&ColumnDef<TRow>> {
    // 1. Start with state.column_order (user-specified positions).
    // 2. Append any column not mentioned in column_order (definition order).
    // 3. Filter by state.visible_columns if column visibility is active.
}
```

This helper is used by `visible_view`, `visible_grouped_view`, and the adapter's header
render. It is the single source of truth for column order.

### `chorale-dioxus`

No new props are required. The adapter's header rendering and cell rendering already
iterate the column definitions returned by the core helper; they will automatically
reflect `column_order` once the helper is wired.

An optional prop enables the drag-to-reorder affordance on column headers:

```rust
/// When true, column headers render a drag handle and wire HTML5 drag-and-drop
/// to fire `move_column` transitions. Defaults to false.
pub column_reorder_enabled: bool,
```

Callsite shape when the host wants to persist column order:

```rust
rsx! {
    Table {
        handle: handle,
        column_reorder_enabled: true,
        on_column_order_change: move |order: Vec<ColumnId>| {
            // host persists the new order (localStorage, user prefs API, etc.)
            spawn(async move { prefs::save_column_order(order).await });
        },
    }
}
```

## Internal Design

**State field:** `column_order: Vec<ColumnId>` defaults to empty. An empty vec means
"use definition order." Keeping the default empty avoids requiring every existing
`TableState::new` call to supply a column order.

**`move_column` algorithm:** build a mutable clone of `column_order` (or initialize it
from definition order if currently empty), remove the target column from its current
position, insert it at `to_index`, write back. O(n) in the number of columns; acceptable.

**Drag-and-drop (adapter):** HTML5 `draggable` + `ondragstart` / `ondragover` /
`ondrop` event handlers on `<th>` elements. The adapter tracks the dragged column ID in a
local signal and fires `move_column` on drop. No third-party drag library needed.

**Interaction with column visibility (Item not in this batch, but shipped in v0.1.0):**
`effective_column_order` respects `visible_columns` after applying `column_order`. A
column that is in `column_order` but hidden is still in the order list (its position is
preserved); it just doesn't render. This avoids order corruption when the user hides
and later re-shows a column.

**Interaction with frozen columns (Item 10):** frozen columns should stay at their frozen
edge regardless of `column_order`. The adapter's rendering layer pins frozen columns to
their side after applying the effective order. Core's `effective_column_order` does not
know about `FrozenSide`; the adapter applies the frozen override on top of the order.

## Backwards Compatibility

`column_order: Vec<ColumnId>` is additive on `TableState`. `TableState` is
`#[non_exhaustive]` in v0.1.0, so cross-crate callers cannot use struct-literal
construction; adding the field does not break compilation downstream. The empty-default
means existing callers see no behavioral change (definition order is preserved).

`set_column_order`, `move_column`, and `reset_column_order` are new public functions.
Adding functions is never a breaking change.

The `column_reorder_enabled` prop on `Table` defaults to `false`. Existing call sites see
no change.

The `on_column_order_change` callback prop is optional; existing call sites without it
still work (they just can't persist the order).

## Test Plan

Per TESTS-1:

- `set_column_order`: happy path — returns state with `column_order` set.
- `set_column_order`: unknown column ID in `order` → `Err(StateError::UnknownColumnId)`.
- `move_column`: column moved from index 2 to index 0 — column appears first in
  `effective_column_order`.
- `move_column`: target index == current position → no-op (state unchanged).
- `move_column`: target index beyond length → clamped to last position (or Err; Zach
  decides in open question #2).
- `reset_column_order`: returns state with empty `column_order`; `effective_column_order`
  equals definition order.
- `effective_column_order` with partially-specified `column_order`: unlisted columns
  appended in definition order.
- Interaction with column visibility: hidden columns excluded from render list, retained
  in `column_order`.
- Idempotency: `set_column_order(state, order)` called twice with same `order` → same
  result.

## Open Questions / Decisions Zach Must Sign Off (API-1)

1. **Should the effective column order helper be public (`pub fn effective_column_order`)
   or remain internal?** Recommendation: internal (pub(crate)). Host apps should read
   column order via `state.column_order` and construct their own view if needed.
   Exposing the helper would couple the public API to the internal rendering logic.

2. **`move_column` with an out-of-bounds `to_index`: clamp or return `Err`?**
   Recommendation: clamp to the last valid index. Silently clamping is more ergonomic
   for drag-and-drop (where the drop target can transiently be "past the end" during
   the drag gesture). Return `Err` only for an unknown `column_id`.

3. **Should `column_order` allow duplicate IDs?** Recommendation: no — `set_column_order`
   validates uniqueness and returns `Err(StateError::DuplicateColumnId)` on duplicates.
   Duplicates would cause a column to render twice, which is never intentional.

4. **`on_column_order_change` callback: fires on every intermediate drag position or only
   on drop?** Recommendation: only on drop (when `ondrop` fires). Firing on every
   `ondragover` would produce a state transition and re-render per pixel of drag movement.

5. **How does the reorder interact with the column-visibility toolbar (if the host
   renders one)?** The toolbar likely shows a `Vec<ColumnId>` in some order; should it
   reflect `column_order`? Recommendation: yes — the visibility toolbar should iterate
   `effective_column_order`, so the drag-reordered sequence is visible in the toolbar
   too. The adapter's toolbar rendering already reads the column list; it just needs to
   use the ordered version.

## Decisions (signed off 2026-06-04)

All 5 recommendations accepted as written.

1. ✅ `effective_column_order` stays `pub(crate)`. Hosts read `state.column_order`.
2. ✅ `move_column` clamps `to_index` to the last valid position; `Err` only
   on unknown `column_id`.
3. ✅ `set_column_order` rejects duplicates with `StateError::DuplicateColumnId`.
4. ✅ `on_column_order_change` fires only on drop.
5. ✅ Visibility toolbar iterates `effective_column_order`.
