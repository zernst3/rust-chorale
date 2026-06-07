# Item 11: `selection_toolbar` Slot Prop

## Problem

chorale v0.1.0 ships selection (checkbox per row, select-all, `state.selection: HashSet<RowId>`)
but provides no built-in affordance for acting on the selection. The host app must render
its own "bulk actions" UI and read `state.selection` from the handle signal. This is
workable but means the toolbar floats outside the table visually with no standard placement
or lifecycle hook.

The standard pattern in every major table library (AG Grid, TanStack Table, MUI DataGrid)
is a "selection toolbar" that appears above (or below) the table when the selection is
non-empty and disappears when it's cleared. The host supplies the toolbar's content;
the library supplies the placement, visibility toggle, and the signal subscription.

This item adds a `selection_toolbar` slot prop to chorale-dioxus's `Table` component.
It is an adapter-only addition; no chorale-core changes are required.

## Proposed Public API

### `chorale-dioxus`

```rust
/// Optional slot rendered above the table when `state.selection` is non-empty.
/// When `None` (the default) or when the selection is empty, no slot is rendered.
pub selection_toolbar: Option<VNode>,
```

Callsite shape:

```rust
let selection_count = handle.selection_count();
let selected_ids = handle.selected_ids();

rsx! {
    Table {
        handle: handle,
        selection_enabled: true,
        selection_toolbar: if selection_count > 0 {
            Some(rsx! {
                div { class: "bulk-actions-bar",
                    span { "{selection_count} rows selected" }
                    button {
                        onclick: move |_| { delete_rows(selected_ids.clone()); },
                        "Delete"
                    }
                    button {
                        onclick: move |_| { handle.clear_selection(); },
                        "Clear selection"
                    }
                }
            })
        } else {
            None
        },
    }
}
```

The slot is a `VNode` (Dioxus's rendered element type), not a closure. This matches the
pattern used by other optional slot props in Dioxus components (e.g., Dioxus's own
`Router` breadcrumb slot).

### Visibility semantics

The adapter renders the slot when both conditions hold:
1. `selection_toolbar` is `Some(...)`.
2. `state.selection.is_empty()` is `false`.

When the user deselects all rows (by unchecking all checkboxes or clicking "select all"
again), the toolbar disappears automatically via signal subscription — the host doesn't
need to manage the toolbar's visibility separately.

## Internal Design

The `Table` component already has a `use_memo` subscription to `handle.signal()`. The
slot render is gated inline inside the component's RSX:

```rust
if let (Some(toolbar), false) = (&props.selection_toolbar, state.selection.is_empty()) {
    rsx! { div { class: "chorale-selection-toolbar", {toolbar.clone()} } }
}
```

No new signals, no new memos. The existing signal subscription drives the toolbar's
visibility as a side effect of re-rendering when selection changes.

The toolbar div wraps the slot in a styled container (`chorale-selection-toolbar`) so
the host can target it with CSS for positioning (e.g., `position: sticky; top: 0;`).

## Backwards Compatibility

`selection_toolbar: Option<VNode>` defaults to `None`. Existing `Table` callsites that
do not supply the prop see no change. The adapter's render path is unaffected when the
slot is `None`.

No chorale-core API changes. No `TableState` or `ColumnDef` changes.

## Test Plan

Per TESTS-1 and ORCH-NEW-PATH-TESTS-1:

- Render `Table` with `selection_toolbar: None` and selection non-empty → toolbar div
  absent from rendered output.
- Render `Table` with `selection_toolbar: Some(...)` and selection empty → toolbar div
  absent.
- Render `Table` with `selection_toolbar: Some(...)` and selection non-empty → toolbar
  div present; slot content rendered inside.
- Simulate row selection → toolbar appears; simulate deselection of all → toolbar disappears.
- Confirm `chorale-selection-toolbar` CSS class is present on the wrapper div.

## Open Questions / Decisions Zach Must Sign Off (API-1)

1. **`VNode` vs `Element` as the prop type.** Recommendation: `Element` — it's the
   return type of `#[component]` functions in Dioxus and is what RSX expressions produce.
   `VNode` is lower-level. Using `Element` is consistent with `CellRenderers` and other
   existing slot props.

2. **Toolbar placement: above the table, below, or configurable?** Recommendation:
   above (before the `<table>` element in DOM order). This is the AG Grid / MUI
   convention and puts the actions close to where the user's attention is after
   multi-select. Add a `selection_toolbar_position: SelectionToolbarPosition` enum prop
   in v0.3 if below is requested.

3. **Should the toolbar get the selection count / selected IDs injected automatically,
   or does the host read them from the handle signal?** Recommendation: host reads from
   the handle (as shown in the callsite above). Injecting them as arguments to a closure
   prop would change the prop type to `Option<Callback<SelectionToolbarArgs, Element>>`
   — more ergonomic for simple cases but harder to compose with complex host RSX. The
   handle API already offers `selection_count()` and `selected_ids()`, so injection is
   redundant.

## Decisions (signed off 2026-06-04)

All 3 recommendations accepted as written.

1. ✅ Prop type is `Element`. Consistent with `CellRenderers` and other slot props.
2. ✅ Placement above the table. `SelectionToolbarPosition` enum prop deferred to v0.3.
3. ✅ Host reads selection state from the handle (`selection_count()` /
   `selected_ids()`); no auto-injection.
