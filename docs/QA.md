# chorale QA Verification Guide

Manual test recipes for chorale features. Each section describes setup, the
canonical happy path, edge cases, and known-regression guards. Run before
merging any release branch.

**General setup:** all Dioxus examples require the Dioxus CLI.
Install with `cargo install dioxus-cli`, then serve with
`dx serve --package <example-name>`.

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
4. Click "‹‹" (first) from any page. Page 1 appears.
5. Click "››" (last). Page 201 shows the final 10 rows (partial last page).
6. Click a numbered page button in the window. That page appears.
7. Enter `150` in the "Go to" input and press Enter. Page 150 appears.
8. Enter a number beyond the total page count and blur. Input snaps back to the current page.

**Edge cases:**
- Page size = 1: every row is its own page. Prev/Next work correctly.
- Dataset filtered to 0 rows: pagination shows "1 of 1" and the table body is empty.
- `Go to` input: entering `0` or negative values should clamp to page 1.
- `Go to` input: entering non-numeric text should snap back without crashing.

**Regression to guard (blank-page-after-pagination):** after clicking "Next",
the table should immediately render the new page's rows at `scroll_top = 0`.
If `scroll_top` is not reset, the scroll container shows empty spacer space
("blank page") until the user manually scrolls. This was a bug in early builds;
the DOM `scrollTop` reset on page change (`use_effect` keyed on `page_memo`) prevents it.

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
checked checkbox must clear the row's blue background immediately. In early
builds, the deselected branch emitted `style=""` which Dioxus's attribute diff
did not reliably propagate as "unset"; the row kept its blue background.
Fix: deselected rows emit `background: transparent` explicitly. Verify this
by checking and unchecking the same row three times — color must clear each time.

---

### 5. Custom cells: Badge and CellRenderers

**Setup:** `dx serve --package with-custom-cells`

**Happy path:**
1. Columns using `RenderKind::Badge` show colored pill chips. Verify each variant
   renders with the correct color (per `BadgeVariantMap`).
2. Columns using `CellRenderers` show the custom Dioxus RSX instead of the
   default text/number render.
3. Hover/interaction on a custom cell works (if the custom renderer has event handlers).

**Edge cases:**
- `CellValue::Empty` on a Badge column: fallback variant renders if a fallback is set; otherwise blank cell.
- Custom renderer returns an element that is wider than the column: column clips or overflows per its CSS.

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
5. Resize another column independently.

**Edge cases:**
- Column narrowed below the minimum (40 px): should clamp to 40 px and not go negative.
- Resize with filter row visible: filter input width should follow the column width.

---

### 8. CSV export

**Setup:** `dx serve --package qa-harness` with `csv_export = true`

**Happy path:**
1. Apply a filter (e.g. text filter to match half the rows).
2. Navigate to page 2.
3. Click "Download CSV".
4. Open the downloaded file. Verify:
   - Header row contains visible column names.
   - ALL post-filter rows are present (not just the current page).
   - Rows match the active sort order.

**Edge cases:**
- Cell value containing a comma: the CSV field is quoted per RFC 4180 (`"value, with comma"`).
- Cell value containing a double-quote: escaped as `""` (`"value ""with"" quotes"`).
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
1. Table loads. Browser DevTools → Performance: no janky frames during fast scroll.
2. Scroll from top to bottom quickly (hold Page Down). Table keeps up.
3. No browser freeze or memory spiral.

**Edge cases:**
- Viewport height changes (browser window resize): rows recompute automatically.
- Sort on a 1M-row dataset: may take a moment (expected); scrollbar position remains correct after sort.

**Scroll runaway regression guard:** the scroll container must have
`overflow-anchor: none` in its CSS. Without it, browser scroll anchoring
fights DOM mutations during virtualization, producing a runaway scroll loop
that continues until the top or bottom of the content. Verify: scroll to
the middle of a 1M-row table and hold a key or drag the scrollbar continuously.
The scroll position should track the user input, not drift independently.

---

### 10. Example: `qa-harness`

**Setup:** `dx serve --package qa-harness`

The harness exposes runtime toggles for every v0.1 feature. Use it for
regression testing combinations:

- Sort + filter simultaneously: filtered rows are sorted correctly.
- Selection + pagination: selected rows persist across page changes.
- Column visibility + CSV export: hidden columns absent from CSV.
- Resize + virtualization: scroll math holds after a column is resized.
- All features on simultaneously: no console errors, no visual glitches.

---

## v0.2.0 Feature Coverage (sections added as features ship)

_Sections to be added as each v0.2.0 feature is implemented:_

- **Selection ergonomics** (`selected_ids()`, `selection_count()`): verify the
  `with-selection` example displays IDs and count correctly via the new methods.
- **Fine-grained reactivity (PERF-1):** scroll a 1M-row table for 30 seconds.
  Open Chrome DevTools → Memory → Allocation Timeline. Verify no repeated 30 MB
  allocation bursts during scroll (only during initial sort/filter changes).
- Variable-row-height virtualization (Item 6) — pending sign-off.
- In-cell editing (Item 7) — pending sign-off.
- Grouping and aggregation (Item 8) — pending sign-off.
- Column reorder (Item 9) — pending sign-off.
- Frozen columns (Item 10) — pending sign-off.
- `selection_toolbar` slot (Item 11) — pending sign-off.
- Multi-column sort (Item 11.0a) — pending sign-off.
- Infinite scroll mode (Item 11.0b) — pending sign-off.
- User-overridable labels / i18n (Item 11.0c) — pending sign-off.
- `chorale-derive` proc-macro (Item 11.0d) — pending sign-off.
- `chorale-leptos` adapter (Item 11.5) — pending sign-off.
