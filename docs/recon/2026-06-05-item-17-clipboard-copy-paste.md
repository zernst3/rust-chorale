# Item 17: Clipboard (Copy / Paste / Cut / Delete-Range)

## Problem

chorale v0.2.0 ships no clipboard integration. Users who select a range of cells (Item 16)
and press Ctrl+C get browser-default behavior (copying the entire page's selected text, or
nothing). There is no way to paste tabular data from Excel or another source into the grid, no
way to cut a range, and no way to clear a range with the Delete key — all of which are baseline
expectations for any tool that markets itself as "Excel-like."

AG Grid ships clipboard operations as a premium Enterprise feature, but provides read-only
paste (no multi-rect, no tile-repeat) in Community. MUI X DataGrid ships Ctrl+C copy only in
its free tier, with paste behind a premium wall. TanStack Table has no clipboard primitives;
hosts must build their own. chorale v0.2.0 will ship full copy + paste + cut + delete + fill-down
in the open-source tier, making it the most capable open-source data-grid clipboard API in the
Rust/WASM ecosystem.

This item depends on Items 15 (active-cell model) and 16 (range selection) landing first —
clipboard operations act on `state.range_selection` and require `RangeSelection` types.

## Proposed Public API

### `chorale-core`

```rust
/// Serialize the active range to a TSV string, with value formatters applied.
/// If `range_selection` is empty, returns `Ok("")`.
/// If `range_selection.len() > 1`, returns `Err(ClipboardError::MultiRectCopyNotSupported)`.
/// The TSV format: cells separated by `\t`, rows separated by `\n` (Unix newline).
/// Per-column `value_formatter` functions are called if set; otherwise `CellValue::Display`
/// is used (the same rendering path as the visible cell).
pub fn to_clipboard_tsv(
    state: &TableState<TRow>,
) -> Result<String, ClipboardError>;

/// Parse a TSV string and write values into the active range.
/// Shape mismatch rules (matching Excel):
///   - If the paste payload is SMALLER than the target range, the payload repeats
///     tile-style to fill the range (Excel: "repeat the copied cells").
///   - If the paste payload is LARGER than the target range, the range is EXPANDED
///     to fit the payload (Excel: if you paste a 5-row payload into a 2-row selection,
///     all 5 rows are pasted starting from the top-left of the selection).
///   - Columns past the visible column count are ignored.
/// Returns `Err` on `range_selection.len() > 1`, empty range, or invalid TSV.
pub fn paste_tsv_into_range(
    state: &TableState<TRow>,
    tsv: &str,
) -> Result<TableState<TRow>, ClipboardError>;

/// Clear all cell values in the active range selection.
/// Works on multi-rect selections (Delete key behavior; no clipboard interaction).
/// Read-only columns (those without an `EditorKind` set on their `ColumnDef`) are
/// silently skipped — their values are left unchanged.
pub fn clear_range_values(
    state: &TableState<TRow>,
) -> Result<TableState<TRow>, ClipboardError>;

/// Clipboard operation errors.
#[non_exhaustive]
pub enum ClipboardError {
    /// Clipboard copy on a multi-rect selection. Lock this in v0.2.0.
    MultiRectCopyNotSupported,
    /// The browser denied clipboard read access (user denied the permission prompt).
    ClipboardReadDenied,
    /// `range_selection` is empty.
    NoRangeSelected,
    /// Pasted TSV has more columns than the table; informational only (extra are dropped).
    /// This is a warning variant; callers may ignore or log it.
    ColumnCountTruncated { tsv_columns: usize, table_columns: usize },
}

/// Ctrl+D: fill down from the top row of the active range.
/// Re-exported from the Item 16 surface for discoverability in clipboard context.
/// Calls fill_down internally.
pub fn fill_down_clipboard(
    state: &TableState<TRow>,
) -> Result<TableState<TRow>, ClipboardError>;
```

### Adapter wiring (chorale-dioxus / chorale-leptos)

```rust
// Keyboard handlers wired internally by the adapter; no new props required from hosts
// unless the host wants to hook into the events.

// Optional props for observability / persistence integration:
pub on_copy: Option<EventHandler<ClipboardCopyEvent>>,
pub on_paste: Option<EventHandler<ClipboardPasteEvent>>,
pub on_cut: Option<EventHandler<ClipboardCutEvent>>,

pub struct ClipboardCopyEvent {
    pub tsv: String,
    pub range: RangeSelection,
}

pub struct ClipboardPasteEvent {
    pub tsv: String,
    pub range: RangeSelection,    // the target range after expansion
}

pub struct ClipboardCutEvent {
    pub tsv: String,
    pub source_range: RangeSelection,
}
```

### Adapter clipboard flow (internal, not public API)

```
Ctrl+C pressed:
  1. Call `to_clipboard_tsv(&state)`.
  2a. On Ok(tsv): call `navigator.clipboard.writeText(tsv)` (async, awaited via spawn).
      Fire `on_copy` if set.
  2b. On Err(MultiRectCopyNotSupported): show a brief non-blocking toast ("Cannot copy
      disjointed selection") and do nothing else.

Ctrl+V pressed:
  1. Await `navigator.clipboard.readText()`.
  2a. On Ok(tsv): call `paste_tsv_into_range(&state, &tsv)`.
      On Ok(new_state): update signal with new_state; fire `on_paste`.
      On Err: log error (no user-visible toast for parse errors in v0.2.0).
  2b. On Err (browser denied): surface `ClipboardError::ClipboardReadDenied`
      — show a brief toast "Clipboard access denied. Allow in browser settings."

Ctrl+X pressed:
  1. Call `to_clipboard_tsv(&state)`.
  2. On Ok(tsv): write to clipboard (same as Ctrl+C), then call `clear_range_values`.
     Fire `on_cut` if set.

Delete key pressed (when not editing):
  1. Call `clear_range_values(&state)`.
  2. Update signal. No clipboard interaction.

Ctrl+D pressed:
  1. Call `fill_down(&state)` (from Item 16 API).
  2. Update signal.
```

### Callsite shape

```rust
// Most hosts need no extra wiring.
rsx! {
    Table {
        handle: handle,
        // Optional: hook into clipboard events for persistence
        on_paste: move |evt: ClipboardPasteEvent| {
            spawn(async move {
                // persist the pasted cells
                api::bulk_update_cells(evt.range, evt.tsv).await;
            });
        },
    }
}
```

## Internal Design

**TSV format:** tab between cells (`\t`), Unix newline between rows (`\n`). Cells whose
`CellValue::Display` contains a tab character are wrapped in double-quotes per de-facto
Excel TSV behavior (Excel wraps cells with tabs). Cells with embedded newlines have the
newline replaced with a space (Excel behavior: embedded newlines in TSV become spaces when
Excel writes to clipboard). This is the minimum needed for round-trip fidelity with Excel;
full RFC-4180 TSV escaping is not implemented in v0.2.0.

**Paste shape matching:**
- Parse TSV into a 2D `Vec<Vec<String>>` (rows × cols).
- Compare `(payload_rows, payload_cols)` with `(target_rows, target_cols)` from the
  normalized active range.
- **Smaller payload:** tile the payload across the target range. E.g., a 2×2 payload
  pasted into a 4×4 target fills all 4×4 cells by repeating. This uses modular indexing
  into the payload.
- **Larger payload:** expand the active range's focus to accommodate the payload. The new
  focus is `(anchor_row + payload_rows - 1, col_at_anchor_col_idx + payload_cols - 1)`.
  If the expanded range exceeds table bounds, truncate rows/cols at the edge.

**Read-only column skipping in `clear_range_values`:** iterate the range columns; for each
column, check `column_def.editor.is_none()`. If `None`, skip. The implementation updates
`state.rows` in-place (returning a new `TableState` per CHORALE-CORE-2) by setting the
matched `CellValue` to its zero/default for the column's `RenderKind`.

**Browser clipboard API:** `navigator.clipboard.writeText()` and `navigator.clipboard.readText()`
are the modern async Clipboard API. In Dioxus, these are called via `wasm_bindgen` / `web_sys`
inside a `spawn(async { ... })`. In Leptos, equivalent `spawn_local`. The `execCommand('copy')`
fallback is **not implemented** in v0.2.0; chorale targets modern browsers (Chrome 66+,
Firefox 63+, Safari 13.1+) where the async Clipboard API is available.

**CHORALE-CORE-1 compliance:** `to_clipboard_tsv` and `paste_tsv_into_range` are pure
functions in `chorale-core`. They take `&TableState<TRow>` and return `String` or
`Result<TableState<TRow>, _>`. The `navigator.clipboard` async call lives entirely in the
adapter — core has no knowledge of it.

## Backwards Compatibility

`to_clipboard_tsv` and `paste_tsv_into_range` are new free functions in `chorale-core`.
Adding them is purely additive.

`clear_range_values` is a new free function. Additive.

`ClipboardError` is a new `#[non_exhaustive]` enum. No existing matches break.

The optional `on_copy` / `on_paste` / `on_cut` props on `Table` default to `None`.
Existing `Table` callsites without these props compile and behave identically — the adapter
runs Ctrl+C/V/X without firing any callback.

The adapter gaining a `keydown` handler for Ctrl+C/V/X/D and the Delete key does not
conflict with existing keyboard handling because these key combos were previously unhandled
by chorale (the browser received them raw).

## Test Plan

Per TESTS-1:

**`to_clipboard_tsv` (~8 tests):**
- Happy path single rect: correct tab/newline-separated output.
- Single cell: one value, no tabs or newlines.
- Empty `range_selection` → `Ok("")`.
- Multi-rect → `Err(ClipboardError::MultiRectCopyNotSupported)`.
- Cell containing a tab character: wrapped in double-quotes.
- Cell containing a newline: newline replaced with space.
- `value_formatter` applied when set on column.
- Respects `column_order` and `column_visibility`.

**`paste_tsv_into_range` (~12 tests):**
- Exact-size match: 3×2 payload into 3×2 target → all 6 cells updated.
- Smaller payload tiling: 2×2 payload into 4×4 target → tiled correctly.
- Larger payload expansion: 4-row payload into 2-row selection → state has 4 rows updated.
- Single-cell target, multi-cell payload → expands.
- Payload columns > table columns → extras truncated, `ColumnCountTruncated` variant.
- Empty `range_selection` → `Err(ClipboardError::NoRangeSelected)`.
- Multi-rect → `Err(ClipboardError::MultiRectCopyNotSupported)`.
- Payload with tab-in-quoted-cell: parses correctly.
- TSV with trailing newline: handled gracefully (no phantom empty row).
- Read-only column in range: that column's cells are unchanged after paste.
- Out-of-bounds expansion (payload extends past last row): truncated at table edge.
- Round-trip: `paste_tsv_into_range(state, to_clipboard_tsv(state).unwrap()).unwrap()` produces expected state.

**`clear_range_values` (~6 tests):**
- Single-rect range: all cells in range set to zero/empty.
- Multi-rect range: all rects cleared.
- Read-only column (no `EditorKind`): skipped, value unchanged.
- Empty range → `Err(ClipboardError::NoRangeSelected)`.
- Idempotent: clearing already-empty cells is a no-op.
- Non-range state fields (sort, filter, active_cell) unchanged after clear.

**Invariants (~4 tests):**
- `to_clipboard_tsv` never panics on any valid `TableState`.
- `paste_tsv_into_range(state, "")` is equivalent to `clear_range_selection` (empty paste clears).
  (Or should empty paste be a no-op? — see Open Questions.)
- `clear_range_values` does not change `range_selection` (the range stays selected after clear,
  matching Excel behavior where you can see what you just deleted).
- `to_clipboard_tsv` output contains exactly `(rows - 1)` newlines.

## Open Questions / Decisions Zach Must Sign Off (API-1)

1. **TSV escaping: minimal Excel-compatible vs full RFC-4180.** Recommendation: minimal
   Excel-compatible in v0.2.0 (tab-between-cells, newlines-replaced-with-spaces, tab-in-cell
   wrapped in double quotes). Full RFC-4180 TSV adds complexity for edge cases that are rare
   in data-grid cells. v0.3.0 can upgrade the escaping without breaking the API. Confirm
   minimal-Excel scope.

2. **Paste shape mismatch: smaller-than-range → tile-style repeat (Excel); larger-than-range
   → expand selection (Excel).** Recommendation: match Excel for both directions. This is the
   most user-intuitive behavior for Excel-literate users. The alternative (error on any
   mismatch) would frustrate users who paste from Excel with one extra row. Confirm Excel
   behavior for both cases.

3. **`processCellForClipboard` / `processCellFromClipboard` per-cell hooks (AG Grid feature):
   deferred to v0.3.0?** Recommendation: yes — v0.2.0 ships a single optional
   `clipboard_value_formatter: Option<Arc<dyn Fn(&CellValue, &ColumnId) -> String>>` on the
   `Table` adapter component. Per-cell hooks that can mutate paste values add significant
   API surface; defer to v0.3.0 where they layer in additively. Confirm deferral.

4. **Browser clipboard API: async `navigator.clipboard` only; no `execCommand('copy')`
   fallback.** Recommendation: yes, drop the synchronous `execCommand` fallback. chorale
   targets modern browsers; the async API has 97% global browser coverage as of 2026. The
   synchronous fallback is deprecated and requires a user-gesture timing hack. Confirm
   modern-only.

5. **Cut behavior on read-only cells: skip silently or error?** Recommendation: skip silently
   (the clipboard still gets the copied TSV; only the clear step skips read-only columns).
   This matches Excel behavior — you can copy a read-only cell, but cutting it leaves the
   source value in place. Confirm silent skip.

6. **Empty paste payload (empty string or all-whitespace TSV): clear range or no-op?**
   Recommendation: no-op. An empty paste is likely accidental (user pressed Ctrl+V with
   nothing on the clipboard). Confirm no-op (vs treating empty string as "clear all cells").

7. **`on_paste` / `on_copy` / `on_cut` event props: include in v0.2.0 or defer?**
   Recommendation: include. These let hosts wire persistence (save after paste), analytics,
   and undo history without polling the signal. The API surface is small and additive.
   Confirm v0.2.0.

## Decisions (signed off 2026-06-05)

**Item 17 v0.2.0 scope is narrowed** to copy + paste only. Cut,
delete-range (Delete key), and Ctrl+D fill-down all DEFER to v0.3.0.
**Reasoning:** Zach wants the grid to *feel* like Excel but not
*function* autonomously like Excel — all cell mutations route through
the backend. Copy is read-only (no backend involvement). Paste's per-cell
writes route through the existing Item 7 edit pipeline
(`on_validate_edit` + `on_commit_edit`), so the host controls each API
call. Cut, delete-range, and Ctrl+D fill-down are deferred until a
v0.3.0 design pass can decide how multi-cell-clear-via-shortcut should
hook into the backend.

**Renamed for clarity:** the memo's title remains "Clipboard (Copy /
Paste / Cut / Delete-Range)" for historical traceability, but
implementation scope is **Copy + Paste only**.

1. ✅ TSV escaping = minimal Excel-compatible. Tab between cells,
   newlines-in-cell replaced with spaces, tab-in-cell wrapped in double
   quotes. Full RFC-4180 can upgrade in v0.3 additively.
2. ✅ Paste shape mismatch = Excel behavior. Smaller-than-range tiles;
   larger-than-range expands selection. Both match Excel intuition.
3. ✅ Per-cell `processCellForClipboard` / `processCellFromClipboard`
   hooks defer to v0.3.0. v0.2.0 ships a single
   `clipboard_value_formatter: Option<Arc<dyn Fn(&CellValue, &ColumnId) -> String>>`
   prop on the adapter component.
4. ✅ Async `navigator.clipboard` only — no `execCommand` fallback.
   chorale targets modern browsers (97% async-clipboard coverage as of 2026).
5. ⏸ DEFERRED — Cut on read-only cells. (Cut itself deferred to v0.3.0.)
6. ✅ Empty paste payload = no-op. Not "clear range."
7. ⚙️ **Event props scope-cut:** `on_copy` + `on_paste` props ship in
   v0.2.0. `on_cut` prop deferred to v0.3.0 alongside cut. Hosts wire
   persistence on paste, analytics on copy.

**Paste write semantics (added 2026-06-05 follow-up):** every cell
written by paste routes through the existing Item 7 edit pipeline.
Specifically, paste decomposes the clipboard TSV into a per-cell
write list, then for each cell calls `commit_edit` semantics, which
triggers `on_validate_edit` (host may reject) and `on_commit_edit`
(host calls backend). The host gets the same hook as a single-cell
edit, one per pasted cell. Paste does NOT mutate cells autonomously;
the backend remains the source of truth.

**Items deferred to v0.3.0** (out of this memo's implementation scope):
- Cut (Ctrl+X)
- Delete-range (Delete key)
- Ctrl+D fill-down
- `on_cut` event prop
- Per-cell processCellForClipboard / processCellFromClipboard hooks
