# chorale QA Verification Guide

Manual test recipes for chorale features. Each section describes setup, the
canonical happy path, edge cases, and known-regression guards. Run before
merging any release branch.

**General setup:** Dioxus examples require the Dioxus CLI:
`cargo install dioxus-cli`, then `dx serve --package <example-name>`.

Leptos examples require Trunk:
`cargo install trunk`, then `trunk serve --open` inside the example directory,
or `trunk serve --open --package leptos-<example-name>` from the workspace root.

---

## v0.1 Feature Coverage

### 1. Sort (single-column)

**Setup:** `dx serve --package basic` or `dx serve --package qa-harness`

**Happy path:**
1. Click a sortable column header. Arrow appears; rows reorder (ASC).
2. Click the same header again. Arrow reverses; rows reorder (DESC).
3. Click the same header a third time. Arrow disappears; rows return to insertion order.
4. Click a different column header while one sort is active. Previous sort is replaced; new column sorts ASC.

**Edge cases:**
- Column with `sortable: false` has no sort arrow and no click response.
- Single-row dataset: sort produces the same row order regardless of direction.
- All rows with equal values for the sorted column: order is stable (insertion order preserved).

**Regressions to guard:**
- Sort does not scroll the table; `scroll_top` resets to 0 after sort (first row should be visible after sort).
- Selecting rows, then sorting: selected rows are tracked by `RowId`, so their checkboxes remain checked even after their visual position changes.

---

### 2. Typed filters (all 5 kinds)

**Setup:** `dx serve --package qa-harness` (all filter kinds are togglable in the harness)

#### 2a. Text filter
1. Enable filter row. Click into the text filter input on a text column.
2. Type a substring. Rows that don't match disappear; pagination resets to page 1.
3. Clear the input or click "Clear". All rows return.
4. Case-insensitivity: type `"ALICE"` — should match rows with `"alice"`, `"Alice"`, etc.

#### 2b. Numeric range filter
1. Set the min slider or input to a value. Rows below the min disappear.
2. Set the max slider. Rows above the max disappear. Rows between min and max remain.
3. Set min = max. Exactly the matching row(s) remain.
4. Clear filter. All rows return.

#### 2c. Date range filter
1. Set the "from" date. Rows before this date disappear.
2. Set the "to" date. Rows after this date disappear.
3. Leave one endpoint blank. Only the set endpoint filters.

#### 2d. Multi-select filter
1. Open the dropdown (click the `<details>` affordance).
2. Check one option. Only rows with that value remain.
3. Check multiple options. Rows matching ANY checked option remain.
4. Uncheck all options. All rows return (empty selection = no filter).
5. Click outside the dropdown. It closes.

**Outside-click regression guard:** clicking anywhere outside the multi-select
`<details>` element should close it without triggering a filter change.

#### 2e. Boolean filter
1. Select "Yes". Only rows with `true` cells remain.
2. Select "No". Only rows with `false` cells remain.
3. Select "All". All rows return.

**Edge case:** filter on a column that has no rows matching the filter. Table shows an empty body with the correct "no results" empty state.

---

### 3. Pagination and Go-to-page

**Setup:** `dx serve --package virtualized-10k-rows` (10,010 rows, page size 50 → 201 pages)

**Happy path:**
1. Load the table. Page 1 of 201 displays rows 1–50.
2. Click "›" (next). Page 2 shows rows 51–100. Scroll resets to top.
3. Click "‹" (prev). Page 1 returns.
4. Click a numbered page button in the window. That page appears.
5. Enter `150` in the "Go to" input and press Enter. Page 150 appears.
6. Click last page. Shows the final 10 rows (partial last page).

**Edge cases:**
- Page size = 1: every row is its own page. Prev/Next work correctly.
- Dataset filtered to 0 rows: pagination shows "1 of 1" and the table body is empty.
- `Go to` input: entering `0` or negative values should clamp to page 1.
- `Go to` input: entering non-numeric text should snap back without crashing.

**Regression to guard (blank-page-after-pagination):** after clicking "Next",
the table should immediately render the new page's rows at `scroll_top = 0`.
If `scroll_top` is not reset, the scroll container shows empty spacer space
("blank page") until the user manually scrolls.

---

### 4. Selection (per-row checkboxes + select-all)

**Setup:** `dx serve --package with-selection`

**Happy path:**
1. Click a row checkbox. Row highlights blue. Counter increments.
2. Click it again. Highlight clears. Counter decrements.
3. Click the header checkbox. All visible rows on the current page are selected.
4. Click it again. All visible rows are deselected.
5. Select some rows, navigate to page 2. Page 2 rows start unselected. Navigate back. Page 1 selection is retained.
6. Select rows, then sort the table. Checkboxes follow the row's new visual position (stable `RowId` tracking).

**Regression to guard (row stays highlighted after deselect):** clicking a
checked checkbox must clear the row's blue background immediately.
Fix: deselected rows emit `background: transparent` explicitly. Verify this
by checking and unchecking the same row three times — color must clear each time.

---

### 5. Custom cells: Badge and CellRenderers

**Setup:** `dx serve --package with-custom-cells`

**Happy path:**
1. Columns using `RenderKind::Badge` show colored pill chips. Verify each variant
   renders with the correct color (per `BadgeVariantMap`).
2. Columns using `CellRenderers` show the custom markup instead of the
   default text/number render.

**Edge cases:**
- `CellValue::Empty` on a Badge column: fallback variant renders if a fallback is set; otherwise blank cell.

---

### 6. Column visibility toolbar

**Setup:** `dx serve --package qa-harness` with `column_toolbar = true`

**Happy path:**
1. Click the "Columns" button. A dropdown or panel appears listing all column names.
2. Uncheck a column. It disappears from the table header and data rows.
3. Re-check it. It reappears in the same position.
4. Uncheck all columns except one. Table renders with a single column.

**Edge cases:**
- Toggling visibility does not reset sort or filters.
- CSV export with some columns hidden: hidden columns are excluded from the CSV.

---

### 7. Column resize

**Setup:** `dx serve --package with-column-resize`

**Happy path:**
1. Hover over the right edge of a column header. A resize cursor appears.
2. Click and drag right. Column widens. Adjacent columns stay the same width.
3. Drag left. Column narrows.
4. Release. Width is locked in.

**Edge cases:**
- Column narrowed below the minimum (40 px): should clamp to 40 px and not go negative.

---

### 8. CSV export

**Setup:** `dx serve --package qa-harness` with `csv_export = true`

**Happy path:**
1. Apply a filter (e.g. text filter to match half the rows).
2. Navigate to page 2.
3. Click "Export CSV".
4. Open the downloaded file. Verify:
   - Header row contains visible column names.
   - ALL post-filter rows are present (not just the current page).
   - Rows match the active sort order.

**Edge cases:**
- Cell value containing a comma: the CSV field is quoted per RFC 4180.
- Cell value containing a double-quote: escaped as `""`.
- Zero rows after filter: CSV contains only the header row.

---

### 9. Fixed-row-height virtualization (10k and 1M rows)

**Setup (10k):** `dx serve --package virtualized-10k-rows`
**Setup (1M):** `dx serve --package virtualized-1m-rows`

**Happy path (10k):**
1. Table loads with rows 1–N visible (within the viewport).
2. Scroll down steadily. New rows appear; rows above the viewport disappear.
3. Scroll back to the top. First rows reappear.
4. Scroll to the bottom. The last page renders correctly (partial page OK).
5. Apply a text filter. The scroll container shrinks to the filtered row count.

**Happy path (1M):**
1. Table loads (brief "Initializing…" notice, then ~1-2 s).
2. Scroll from top to bottom quickly. Table keeps up.
3. No browser freeze or memory spiral.

**Scroll runaway regression guard:** the scroll container must have
`overflow-anchor: none`. Verify: scroll to the middle of a 1M-row table
and drag the scrollbar continuously. The scroll position should track user
input, not drift independently.

---

### 10. Example: `qa-harness`

**Setup:** `dx serve --package qa-harness`

The harness exposes runtime toggles for every v0.1 feature. Use it for
regression testing combinations:

- Sort + filter simultaneously.
- Selection + pagination: selected rows persist across page changes.
- Column visibility + CSV export: hidden columns absent from CSV.
- Resize + virtualization: scroll math holds after a column is resized.
- All features on simultaneously: no console errors, no visual glitches.

---

## v0.2.0 Feature Coverage

### 11. Multi-column sort (v0.2.0, Item 11.0a)

**Setup:** `dx serve --package qa-harness` with sort enabled.

**Happy path:**
1. Click "Name" header. Single sort (ASC) activates. Badge shows `1`.
2. Hold Shift and click "Salary" header. Two-sort stack: Name ASC primary,
   Salary ASC secondary. Both badges appear.
3. Hold Shift and click "Salary" header again. Salary flips to DESC.
4. Hold Shift and click "Salary" once more. Salary is removed from the stack;
   Name ASC is the only sort.
5. Click "Name" without Shift. All sort is replaced by Name ASC.

**Edge cases:**
- Click same column twice (no Shift): cycles ASC → DESC → unsorted.
- Sort priority badge is visible (1, 2, …) for each column in the stack.

---

### 12. Infinite scroll (v0.2.0, Item 11.0b)

**Setup:** `dx serve --package qa-harness` — switch pagination mode to "Infinite scroll".

**Happy path:**
1. Infinite scroll shows the first page_size rows.
2. Scroll to near the bottom (within `infinite_scroll_threshold_px`). More rows appear.
3. Repeat until all rows are loaded. The "Loading more rows…" indicator
   disappears when all rows are visible.
4. Apply a text filter. The loaded count resets to the first batch.

**Edge cases:**
- Switch back to Pages mode. Pagination bar reappears; `set_page` works.
- Infinite scroll + filter: loaded count resets on filter change.

---

### 13. User-overridable labels / i18n (v0.2.0, Item 11.0c)

**Setup:** construct a table with a custom `Labels` struct passed as the `labels` prop.

**Verification:**
1. Set `labels.filter_placeholder = "Suche…"`. The filter input placeholder
   shows the custom text.
2. Set `labels.export_csv_label = "CSV herunterladen"`. The export button shows
   the custom text.
3. Override `labels.page_count` to a closure that formats `"{t}ページ中{p}ページ"`.
   The "Go to" affordance shows the custom format.
4. All adapter components that render user-visible text read from `labels`,
   never from hardcoded string literals.

---

### 14. Variable-row-height virtualization (v0.2.0, Item 6)

**Setup:** set `row_heights` on individual rows in `TableState`.

**Verification:**
1. Rows with different heights render at their specified heights.
2. Scrollbar reflects the correct total content height (sum of all row heights).
3. Virtualization window math holds: only visible rows are mounted as DOM nodes.

---

### 15. In-cell editing (v0.2.0, Item 7)

**Setup:** `dx serve --package qa-harness` with editable columns.

**Happy path:**
1. Double-click an editable cell. An input field appears.
2. Type a new value and press Enter. The edit commits; the cell shows the new value.
3. Press Escape. The edit cancels; the cell shows the original value.
4. Tab to move to the next editable cell.

**Edge cases:**
- Validation rejection: if `validate_edit` returns `Err`, the input shows
  an error message and the Enter key does not commit.
- Click outside: edit cancels.

---

### 16. Grouping and aggregation (v0.2.0, Item 8)

**Setup:** `dx serve --package qa-harness` with grouping enabled.

**Happy path:**
1. Group by "Role". Rows collapse into group headers labeled with each role.
2. Click a group header. The group collapses, hiding its data rows.
3. Click again. The group expands.
4. Aggregated values appear in the group header row for columns with aggregators.

**Edge cases:**
- Nested grouping: group by two columns produces sub-groups.
- Empty group (all rows filtered out): group header disappears.

---

### 17. Column reorder (v0.2.0, Item 9)

**Setup:** `dx serve --package qa-harness` with `column_reorder_enabled: true`.

**Happy path:**
1. Drag a column header to a new position. The column moves to that position.
2. Release. The new order persists.
3. Sort and filter still work correctly after reorder.

---

### 18. Frozen columns (v0.2.0, Item 10)

**Setup:** `dx serve --package qa-harness` with frozen columns.

**Happy path:**
1. Columns marked `FrozenSide::Left` stay visible while the user scrolls right.
2. Columns marked `FrozenSide::Right` stay visible while the user scrolls left.
3. Non-frozen columns scroll normally between frozen columns.

---

### 19. `selection_toolbar` slot (v0.2.0, Item 11)

**Setup:** pass a `selection_toolbar` slot to the `<Table>` component.

**Happy path:**
1. Select one or more rows. The custom toolbar appears above the table.
2. Deselect all rows. The toolbar disappears.

---

### 20. `chorale-derive` proc-macro (v0.2.0, Item 11.0d)

**Verification:**
1. Add `#[derive(TableRow)]` to a struct. `cargo check` succeeds.
2. Call `MyStruct::chorale_columns()`. The returned `Vec<ColumnDef<MyStruct>>`
   matches the hand-written equivalent: one column per field, with the
   `#[chorale(...)]` attributes respected.
3. Override with `#[chorale(header = "Custom Header")]`. The column header
   matches the override, not the field name.
4. `#[chorale(skip)]` omits the field from the generated columns.

---

## v0.2.0 Leptos Adapter Coverage

The Leptos examples mirror the Dioxus examples. Build tool: `trunk` instead
of `dx`. From inside each example directory, run `trunk serve --open`.
Or from the workspace root: `cd examples/leptos-basic && trunk serve --open`.

### 21. Leptos adapter parity (Item 11.5)

For each Leptos example, verify the same feature checklist from the
corresponding Dioxus section passes without modification. Key things to
confirm:

1. **leptos-basic** — sort + filter work identically (§1, §2).
2. **leptos-with-selection** — selection + count display (§4). Reactive count
   updates as rows are checked/unchecked without a full re-render.
3. **leptos-with-custom-cells** — `RenderKind::Badge` and `CellRenderers` both
   render in the Leptos adapter (§5).
4. **leptos-with-column-resize** — drag resize works (§7).
5. **leptos-virtualized-10k-rows** — scroll through all 10k rows; virtualization
   behaves identically to the Dioxus version (§9).
6. **leptos-virtualized-1m-rows** — two-stage render shows "Initializing…"
   message, then the table appears ~1-2 s later (§9).
7. **leptos-qa-harness** — all v0.2.0 feature toggles work (§11–19).

**PERF-1 regression guard (Leptos):** scroll a large table (10k+ rows) while
watching the browser's JavaScript profiler. Scroll events should NOT trigger
the filter/sort/paginate pipeline; only the virtualization window should
recompute on scroll. The two-level memo (`view_key` → `visible`) is the
mechanism; if `view_key` re-fires on scroll, PERF-1 is broken.

---

## Leptos vs Dioxus behavioral parity checklist

Run this after any change to either adapter to confirm behavioral equivalence:

| Feature | Dioxus | Leptos | Status |
|---|---|---|---|
| Sort (single-column) | `basic` | `leptos-basic` | parity expected |
| Multi-column sort | `qa-harness` | `leptos-qa-harness` | parity expected |
| Text filter | `basic` | `leptos-basic` | parity expected |
| All 5 filter kinds | `qa-harness` | `leptos-qa-harness` | parity expected |
| Pagination | `virtualized-10k-rows` | `leptos-virtualized-10k-rows` | parity expected |
| Infinite scroll | `qa-harness` | `leptos-qa-harness` | parity expected |
| Selection | `with-selection` | `leptos-with-selection` | parity expected |
| Custom cells | `with-custom-cells` | `leptos-with-custom-cells` | parity expected |
| Column visibility | `qa-harness` | `leptos-qa-harness` | parity expected |
| Column resize | `with-column-resize` | `leptos-with-column-resize` | parity expected |
| CSV export | `qa-harness` | `leptos-qa-harness` | parity expected |
| Virtualization (10k) | `virtualized-10k-rows` | `leptos-virtualized-10k-rows` | parity expected |
| Virtualization (1M) | `virtualized-1m-rows` | `leptos-virtualized-1m-rows` | parity expected |
| Grouping | `qa-harness` | `leptos-qa-harness` | parity expected |
| Column reorder | `qa-harness` | `leptos-qa-harness` | parity expected |
| Frozen columns | `qa-harness` | `leptos-qa-harness` | parity expected |
| `selection_toolbar` slot | `qa-harness` | `leptos-qa-harness` | parity expected |
| In-cell editing | `qa-harness` | `leptos-qa-harness` | parity expected |
| Labels / i18n | `qa-harness` | `leptos-qa-harness` | parity expected |
