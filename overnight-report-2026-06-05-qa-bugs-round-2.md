# Overnight Report — QA Bugs Round 2 (2026-06-05)

Branch: `draft-release/v0.2.0`
Session window: continuation from impl-batch-2 session 2

## Summary

All 11 bugs and the 1 feature from the QA-bugs-round-2 specification were
addressed across two sessions. This report covers the second session (the
half that was incomplete after the context compaction).

---

## Bugs Fixed

### Bug 1 — Infinite scroll "Loading more…" always visible
**Root cause**: `has_more_rows` compared against `view_read.len()` (virtual
window, ~20 rows) instead of `state.loaded_row_count`.
**Fix**: `let has_more_rows = is_infinite_scroll && state.loaded_row_count < total_rows;`
**File**: `chorale-dioxus/src/components.rs`

### Bug 2 + Bug 3 — Filter input / go-to-page unfocused on table click
**Root cause**: outer div `onclick` ran `el.focus()` unconditionally,
stealing focus from any input the user had just clicked.
**Fix**: JS checks `document.activeElement.nodeName` at click time; skips
focus steal when the active element is INPUT, SELECT, TEXTAREA, or BUTTON.
**File**: `chorale-dioxus/src/components.rs`

### Bug 4 — Multi-sort Shift+click stopped working with 3+ sort columns
**Root cause**: `e.shift()` is deprecated / returns wrong value; actual
modifier check needed `.contains(Modifiers::SHIFT)`.
**Fix**: `let action = if e.modifiers().contains(Modifiers::SHIFT) { Append } else { Replace };`
**File**: `chorale-dioxus/src/components.rs`
**Test added**: `multi_sort_append_grows_to_three_columns`

### Bug 5 — Variable row height not visually testable
**Fix**: Added `make_variable_height_renderer()` to both Dioxus and Leptos
harnesses. The name column renders 1–3 lines based on `name.len() % 3`,
making height variation immediately visible in the viewport.
Leptos toggle upgraded from a static notice to a live `variable_height_on`
`RwSignal` wired to a `Memo<CellRenderers>`.
**Files**: `examples/qa-harness/src/main.rs`,
`examples/leptos-qa-harness/src/main.rs`

### Bug 6 — Active-cell highlight stuck after clicking outside table
**Fix**: Spawned a one-shot JS eval that registers a capturing `mousedown`
listener on `document`; listener sends `1` to Rust when the click target is
outside the keyboard container. A second `use_effect` reacts to a
`Signal<u32>` counter bump and calls `clear_active_cell` + `clear_range_selection`.
Fixed borrow-after-peek in both counter increment and the effect body.
`kb_id` moved to be computed immediately after `scroll_id` so it is in scope
for the effect block.
**File**: `chorale-dioxus/src/components.rs`

### Bug 7 — Double-click did not start in-cell editing
**Fix**: Added `ondoubleclick` handler on every `data_td`; calls
`handle.start_edit(row_id, col_id)`. Added `start_edit` method to
`UseTableHandle` in `hooks.rs`. F2 key path also wired.
Replaced deprecated `ondblclick` with `ondoubleclick` (clippy error).
**Files**: `chorale-dioxus/src/components.rs`, `chorale-dioxus/src/hooks.rs`

### Bug 8 — Collapse-all removed group headers from view
**Root cause**: `paginate_grouped` early-returned `vec![]` when
`data_flat_indices.is_empty()` (all groups collapsed → no data rows →
`0 >= 0` short-circuited the guard).
**Fix**: Guard condition changed to:
```rust
if !data_flat_indices.is_empty() && start >= data_flat_indices.len() {
```
**Files**: `chorale-core/src/views.rs`
**Test added**: `collapse_all_groups_still_shows_headers` (type annotation
corrected from `GroupedRow<R>` → `GroupedRow<GR>` to match `make_grouped_state()`)

### Bug 9 — Column reorder drop-target outline stuck on all columns
**Root cause**: `is_drag_over` used `drag_over_col.read() == Some(col_id)`
without dereferencing the `GenerationalRef` guard, causing a type mismatch
at compile time. Separately the indicator was applied to all non-dragged
columns instead of only the hovered one.
**Fix**: Added `drag_over_col: Signal<Option<ColumnId>>` tracking the
specific hovered column. `ondragenter` / `ondragleave` set / clear it.
Fixed comparison with `*drag_over_col.read() == Some(col_id)`.
**File**: `chorale-dioxus/src/components.rs`

### Bug 10 — Selection toolbar visually unobtrusive
**Fix (Dioxus)**: Full-width blue bar with "Delete Selected" / "Export
Selected" placeholder buttons.
**Fix (Leptos)**: Mirrored the same full-width blue bar with buttons;
previously just a narrow rounded pill.
**Files**: `examples/qa-harness/src/main.rs`,
`examples/leptos-qa-harness/src/main.rs`

### Bug 11 — Range drag-select did not include last cell (inclusive bound)
**Investigation**: `NormalizedRange` already used inclusive `min_row..=max_row`;
adapter render loop also used `..=`. Correctness verified.
**Tests added**: `range_selection_single_cell_covers_one_cell`,
`range_selection_3x2_covers_six_cells` — both pass.
**File**: `chorale-dioxus/src/components.rs`

---

## Feature — Selection counter in qa-harness UI

Added a `"Selection: N row(s)"` strip just above the `<Table>` element in
both Dioxus and Leptos harnesses, visible whenever `selection_enabled` is
true. This is distinct from the in-table selection toolbar (Bug 10) and
updates reactively as rows are selected/deselected.

---

## Validation Gates

| Gate | Result |
|------|--------|
| `cargo fmt --all -- --check` | ✅ Clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | ✅ Clean |
| `cargo test --workspace --features xlsx` | ✅ 0 failures |

Test counts (non-zero crates):
- `chorale-core` lib: 266 passed
- `chorale-core` integration: 19 passed
- `chorale-dioxus` lib: 55 passed
- `qa-harness`: 6 passed
- `leptos-qa-harness`: 7 passed

---

## Commits (this session)

```
631f893 fix(views): guard paginate_grouped early-return on empty data + regression test (Bug 8)
dd1d9ee fix(dioxus): Bugs 1-9/11 — adapter fixes, clippy clean, compile errors resolved
971e447 feat(harness): Bug 5 variable-height renderer + Bug 10 toolbar + selection counter
```

---

## Outstanding / Not Started

- **Interactive scroll verification**: Zach needs to run `dx serve --package qa-harness`
  and manually verify scroll, grouping, range-select, and editing flows before
  phase advance to v0.2.0 release candidate.
- **WASM build gate**: `dx build --package qa-harness --release` to confirm WASM
  output is clean (getrandom/js + uuid/js features required).
- **Leptos variable_row_height**: The Leptos adapter does not implement
  `variable_row_height` as a prop (no fixed-height virtualization override).
  Multi-line renderer cells render correctly but scroll position may drift.
  Tracked as future work.

---

*Report generated 2026-06-05 — rust-chorale v0.2.0 QA session 2*
