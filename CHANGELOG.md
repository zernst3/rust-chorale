# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

_No unreleased changes._

## [0.2.3] — 2026-06-17

Dogfooding fixes + a small feature surface, surfaced while using chorale in a real consumer.

### Fixed

- **Nested-grouping collapse no longer corrupts the parent (#36).** With two-level grouping, collapsing a depth-1 (child) group header corrupted the depth-0 (parent) render — the parent appeared collapsed/indented and counts jumbled. Root cause: the grouped render loop emitted each row's `<tr>` without a Dioxus `key`, so the list diffed positionally and, when a collapse removed a contiguous run of rows, patched a header `<tr>` (one colspan cell) against a data `<tr>` (N cells) in place. Each grouped row now keys on its identity (group headers on their `GroupKey`, data rows on their `RowId`). Ships with a dedicated `examples/nested-collapse-qa` harness and a core invariant test. (Leptos builds a fresh view each render and was unaffected.)

### Added

- **Badge palette: `blue`, `purple`, `orange` + a custom-color escape hatch (#33).** The built-in palette was green/yellow/red/gray only; other colors silently rendered as gray. Added three first-class CSS-variable-backed colors (`--chorale-badge-<color>-bg/-text`) and an escape hatch — any other key resolves to `--chorale-badge-<key>-bg/-text` with the neutral default as the nested fallback, so consumers extend the palette via CSS without forking. Dioxus + Leptos.
- **Per-row conditional styling hook `row_class` (#32).** New `Table` prop taking a `Fn(&TRow) -> Option<String>` whose class is appended to each data row's `<tr>`; composes with selection, grouping, and virtualization. The predicate is consumer code; the core stays I/O-free. Dioxus + Leptos.
- **Set-filter with OR-contains for list-valued cells (#35).** New `FilterKind::MultiSelectContains { options, separator }` (+ matching `FilterValue`). The cell text is split on `separator` and a row matches when ANY token is among the picked options (per-value OR / set-intersection), so a list column like "Applies to: a, b, c" gets a real per-value picker — distinct from `Text`'s raw substring. Same checkbox UI as `MultiSelect`. Dioxus + Leptos.
- **Per-group "select all" (#31).** New filter-aware `chorale-core` transitions `toggle_select_group` and `group_selection_state` (None/Partial/All), driving a tri-state checkbox in the group header when selection is enabled (Dioxus sets the native `indeterminate` property; Leptos uses `prop:indeterminate`). Selecting a group never touches filtered-out rows or other groups.

### Docs

- **`CellRenderer` `Send + Sync` + interactive cells (#34).** Documented why a cell renderer cannot capture a Dioxus signal (signals are `!Sync`) and the working pattern: return a small component that reads the signal from context instead of capturing it.

## [0.2.2] — 2026-06-14

### Added

- **Row-set mutation API.** New pure transitions in `chorale-core` (re-exported from the crate root): `set_rows`, `insert_row`, `append_rows`, `remove_row`, and `remove_rows`. Until now `chorale-core` had only `update_row` (replace one row's *content*); these mutate the row *set* itself, which is what a consumer with live or streaming data needs. Each transition reconciles all derived state so `TableState` stays coherent: `RowId`-based state (selection, `expanded_rows`, editing) drops removed ids; index-based state (`active_cell`, `range_selection`, `row_heights`) clears; `page` and `loaded_row_count` reset or clamp so the view never sits past the new end; and `data_generation` bumps. `append_rows` is the gentle case (no `RowId` is removed, so selection/expanded/editing survive) and an empty input is a no-op. Both adapters' `UseTableHandle` gain matching wrappers: `set_rows`, `insert_row(position, id, row)` (0 = prepend, past the end = append), `append_rows`, `remove_row(id)`, and `remove_rows(&[RowId])`. Both QA harnesses gain a "Row mutation" control group (Append row, Insert at top, Remove selected, Reset dataset) plus a live row count, so each transition is exercisable by hand. Fully additive.
- **Full keyboard navigation for master/detail (child) tables.** The detail-expander chevron is now a real, keyboard-navigable column rather than a mouse-only control. A reserved `DETAIL_EXPANDER_COLUMN` id and a new `detail_column_enabled` flag on `TableState` (set by the adapter when a `detail_renderer` is configured) prepend the chevron to the keyboard column order. New transitions back the behavior: `toggle_active_row_expansion`, `set_detail_column_enabled`, `ensure_active_cell` (selects the first navigable cell on focus-in), and `is_active_cell_editable`. The interaction model: `ArrowLeft` from the first data column lands the active cell on the chevron, `Enter` there expands or collapses the row, `Tab` (while the chevron is highlighted) descends into an open sub-table, and `Esc` returns to the parent. Tabbing from a data cell does not enter the sub-table, so the chevron is the single, predictable doorway. Arrow navigation skips over full-width detail-panel rows rather than landing on them. Both adapters' `UseTableHandle` gain `set_detail_column_enabled` and `ensure_active_cell`. Additive; tables without a `detail_renderer` are unaffected.

### Fixed

- **Detail-expander (chevron) column header now carries the header underline (both adapters).** The 0.2.1 fix for #21 over-corrected: removing the chevron header's `border-bottom` left a visible *gap* in the header underline directly above the chevron column, which read worse than the stray segment it was avoiding. The chevron header now carries the same `border-bottom` as every other header cell, so the underline is continuous across all columns.
- **Leptos: row-selection checkbox is now centered in its column.** The selection cell was missing `text-align: center` (the Dioxus adapter already had it), so checkboxes sat left-aligned and misaligned with the centered header checkbox.
- **Dioxus: header underline no longer disappears when Sort is toggled.** Enabling sort added `cursor: pointer` to the header cell's inline style; because the `<th>` key did not include sort state, Dioxus 0.7 diffed the style in place and unreliably dropped the `border-bottom` declaration, erasing the underline under every data column. The `<th>` key now includes `is_sortable`, so it is recreated (not in-place diffed) when sort toggles and the underline holds.

### Documentation

- New `docs/keyboard-navigation.md` with the complete key reference, and a "Keyboard navigation" section added to the README.

## [0.2.1] — 2026-06-12

### Fixed

- **Group-header aggregates were computed but never rendered (both adapters).** Core computed each group's per-column aggregates (`GroupedRow::Header.aggregates`) and the README/CHANGELOG advertised "aggregators appear in group header rows," but `chorale-leptos` never received the values and `chorale-dioxus` took them as an unused `_aggregates` parameter — so grouping showed no totals. Both adapters now render a per-column aggregate summary in the group-header row (e.g. `Σ Salary: $147,520,249`), formatting each value through the same renderer / `RenderKind` the data cells use so currency and number formatting match. A short prefix (`Σ`, `avg`, `min`, `max`, `count`) hints at the aggregator.
- **`AggregatorKind::Sum` of an all-integer column now returns `CellValue::Integer` instead of `CellValue::Float`.** A `Float` sum bypassed thousands-separator formatting (rendering `147520249.00` instead of `147,520,249`) and broke integer-only cell renderers. Sums fall back to `Float` only when a `Float` value actually contributed (or the total exceeds the exact-integer range of `f64`).
- **Dioxus: selected rows were not visually highlighted (#20).** The selected-row background was set as an inline `background` declaration on the `<tr>`, but Dioxus 0.7's inline-style diff reliably updates CSS custom properties while dropping standard-property changes on `<tr>` — confirmed by reading the live DOM (a selected row kept the updated selected divider custom property but a stale/absent `background`). The highlight now rides a `data-chorale-row-selected` attribute plus a stylesheet rule in `theme_stylesheet()`; data attributes diff reliably, so selected rows paint correctly (matches the Leptos adapter).
- **Detail-expander (chevron) column header drew a stray underline (#21).** The empty 24px expander column header carried a `border-bottom`, drawing a line under an empty utility column. Removed it in both adapters (header and filter rows) so the header underline begins at the first data column.

### Changed

- QA harness grouping toggle renamed **"Group by Role" → "Grouping & Aggregation"** to reflect that aggregates now render.

## [0.2.0] — 2026-06-12

### Added

- **Select cell-editor (`EditorKind::Select { options }`).** A native `<select>`
  dropdown editor constrained to a fixed set of options; the committed value is the
  chosen option string, so a closed category/status set cannot be mistyped —
  membership is enforced by construction, no free-text entry. Wired in both
  adapters' editor paths: `chorale-dioxus` renders the `<select>` and commits on
  change through `on_commit_edit` (validate → commit, Esc cancels);
  `chorale-leptos` renders the `<select>` mirroring its text-editor path. Demoed on
  the **Role** column of both QA harnesses (`qa-harness`, `leptos-qa-harness`) when
  the editing toggle is on. Columns without `.editor(...)`, and the other
  `EditorKind` variants, are unaffected. (`EditorKind` is `#[non_exhaustive]`, so
  the new variant is a non-breaking addition.)
- **Row-aware cell renderers.** New `RowCellRenderer<TRow>` type (`Fn(&TRow, &CellValue) -> Element` in Dioxus / `-> AnyView` in Leptos), `RowCellRenderers<TRow>` per-column map, and `row_cell_renderers` prop on `Table`. Per-column precedence: `row_cell_renderers` > `cell_renderers` > the column's `RenderKind`. Enables composite cells (avatar + name), action columns, and link cells that need sibling fields on the row. Fully additive: the value-only `CellRenderer` / `CellRenderers` / `cell_renderers` API is unchanged.
- **`on_row_click` prop on `Table`** (`Option<Callback<RowId>>`, default `None`). Fires with the row's `RowId` on a plain left-click on any data cell of a data row. Ctrl/Cmd/Shift-clicks remain range-selection operations; clicks on the selection checkbox, the detail-expander chevron, and cells in edit mode do not fire it. `None` preserves prior behavior exactly.
- **Light / dark theming out of the box.** New `Theme` enum in `chorale-core` (`Light` default / `Dark` / `Custom`) and a `theme` prop on `<Table>` in both adapters. `Theme::Light` and `Theme::Dark` are built in: the table injects a shipped CSS-variable stylesheet (`theme_stylesheet()`, ~39 `--chorale-*` tokens with light + dark blocks) on mount and sets `data-chorale-theme` on its root, so `theme=Theme::Dark` themes the entire table — header, body rows, toolbars, frozen cells, selection toolbar, and master/detail sub-tables — with no configuration. `Theme::Light` uses the historical hardcoded colors verbatim, so it is a pixel-identical no-op upgrade. `Theme::Custom` suppresses the injected stylesheet, leaving the `--chorale-*` tokens for the consumer to define (brand palette, additional themes, system-preference switching); nested tables inherit the parent's tokens via the CSS cascade. Both QA harnesses gain a runtime dark-mode toggle. Fully additive — the prop defaults to `Theme::Light`.

### Fixed

- **Leptos: fixed-row-height rows rendered taller than `row_height`, causing a scroll "bounce" at the bottom.** The fixed-mode cell set `height:{row_height}px` without `box-sizing:border-box`, so cell padding + border leaked past `row_height` (rows rendered ~57px against a 40px `row_height`). The virtualization spacer math reserves exactly `row_height` per row, so as the rendered:spacer row ratio shifted during scroll the container `scrollHeight` wobbled and collapsed toward the bottom, clamping `scrollTop` upward — felt as a bounce on a trackpad. Added `box-sizing:border-box` to the fixed-mode data and editor cell branches so each row is exactly `row_height` and `scrollHeight` stays constant top-to-bottom. Matches the `chorale-dioxus` cell branches, which already set border-box. Variable-row-height mode is unaffected (it measures each row's natural height).
- **CHANGELOG: `detail_renderer` prop type corrected.** The 0.2.0 master/detail
  entry described the prop as `EventHandler<TRow, Element>`; the actual
  `chorale-dioxus` `<Table>` prop is `Callback<TRow, Element>`
  (`chorale-dioxus/src/components.rs`). Corrected in the 0.2.0 entry below.

### ⚠ Breaking changes

**`toggle_sort` requires a `SortAction` parameter.** Multi-column sort (Item 11.0a) added a `SortAction` enum (`Replace` / `Append`) to distinguish "click a header" (replace the sort list) from "Shift-click a header" (append to it). Every call to `toggle_sort` must now pass an action.

```rust
// v0.1.0
state.toggle_sort(ColumnId("name"));
handle.toggle_sort(ColumnId("name"));

// v0.2.0 — pass SortAction::Replace for prior single-column behavior
use chorale_core::SortAction;
state.toggle_sort(ColumnId("name"), SortAction::Replace);
handle.toggle_sort(ColumnId("name"), SortAction::Replace);
```

Affects both the `chorale_core::toggle_sort` free function and the `UseTableHandle::toggle_sort` method in `chorale-dioxus` and `chorale-leptos`. No silent migration is possible — the compiler will flag every site.

### Added

**`chorale-core`**
- `#![warn(missing_docs)]` on the crate root; all public items carry doc-comments.
- Unit-test coverage: 182 tests across `state`, `types`, `transitions`, `views`, `labels`.
- **Multi-column sort (Item 11.0a).** `SortAction` enum (`Replace` / `Append`); `toggle_sort` accepts an action parameter. `SortState` carries priority index for badge rendering.
- **Infinite scroll (Item 11.0b).** `PaginationMode` enum (`Pages` / `InfiniteScroll`); `loaded_row_count` field on `TableState`; `set_pagination_mode`, `load_more_rows` transitions; `visible_view` branches on mode.
- **User-overridable labels (Item 11.0c).** `Labels` struct (`#[non_exhaustive]`) with all user-visible strings and a `page_count` closure for token-reordering languages.
- **Variable-row-height virtualization (Item 6).** `row_heights: HashMap<RowId, f64>` on `TableState`; `visible_window_variable` in `views`.
- **In-cell editing (Item 7).** `EditorKind` enum on `ColumnDef`; `EditTarget`, `CommittedEdit` types; `start_edit`, `commit_edit`, `cancel_edit`, `next_editable_cell`, `prev_editable_cell` transitions.
- **Grouping and aggregation (Item 8).** `grouping: Vec<ColumnId>`, `collapsed_groups: HashSet<GroupKey>` on `TableState`; `GroupKey`, `GroupedRow`, `GroupedPaginationMode`; `AggregatorKind` on `ColumnDef`; `set_grouping`, `toggle_group`, `expand_all_groups`, `collapse_all_groups` transitions; `visible_grouped_view`.
- **Column reorder (Item 9).** `column_order: Vec<ColumnId>` on `TableState`; `move_column` transition.
- **Frozen columns (Item 10).** `FrozenSide` on `ColumnDef`; `frozen_left_columns`, `frozen_right_columns`, `scrollable_columns` view helpers.
- **Master/detail (sub-tables, Item 12).** Expandable rows reveal a per-row
  detail panel. `expanded_rows: HashSet<RowId>` on `TableState`;
  `toggle_row_expansion`, `collapse_all_rows` transitions; `RenderRow<TRow>`
  view-stream enum with `DetailPanel { parent_row_id }` injection from
  `visible_view`. Optional `detail_renderer: Callback<TRow, Element>`
  prop on `<Table>` — when set, a 24px chevron column appears at index 0 and
  detail panels render as a full-width `<tr><td colspan>` directly under
  their parent. Consumer mounts a child `<Table>` (or any `Element`) inside
  the renderer for the nested-grid use case. Variable-height virtualization
  handles panel height via the v0.2.0 path.

  Originally routed to v0.3.0 Tier 2 (gap-analysis line 209); pulled into
  v0.2.0 on 2026-06-06 for a consumer-app dependency.
- **XLSX export (Item 18).** `to_xlsx` serializes the full post-filter / post-sort
  dataset to an Excel-compatible `.xlsx` workbook. Behind a `xlsx` Cargo feature
  (`chorale-core/xlsx` + `chorale-dioxus/xlsx`) so consumers who don't need it
  don't pull in the `rust_xlsxwriter` dependency. Mirror prop `xlsx_export: bool`
  on `<Table>` shows an "Export Excel" button in the pagination footer with the
  same styling as `csv_export`; a standalone `ExportXlsxButton` component is
  also re-exported for layouts that want the button elsewhere. Per-column
  `Currency`, `Date`, and `Number` render kinds map to native Excel cell formats.
- `StateError::InvalidModeForTransition`, `StateError::UnknownColumnId` variants.
- `NaiveDate` re-export so adapter crates do not need a direct `chrono` dependency.

**`chorale-dioxus`**
- `UseTableHandle::selected_ids() -> Vec<RowId>` and `selection_count() -> usize`.
- `UseTableHandle::move_column`, `set_pagination_mode`, `load_more_rows`, `set_grouping`, `toggle_group`, `expand_all_groups`, `collapse_all_groups`, `start_edit`, `commit_edit`, `cancel_edit` methods.
- `Table` props: `column_reorder_enabled`, `frozen_column_z_index`, `group_header_class`, `infinite_scroll_threshold_px`, `labels`, `selection_toolbar`, `validate_edit`, `on_commit_edit`.
- PERF-1 two-level memo: `view_key` tracks cheap fields; scroll/selection no longer retrigger the filter/sort/paginate pipeline.
- `#![warn(missing_docs)]` on the crate root.
- Unit-test coverage: 50 tests.

**`chorale-leptos` (new crate, Item 11.5)**
- `use_chorale_table(rows: Vec<TRow>, columns: Vec<ColumnDef<TRow>>) -> UseTableHandle<TRow>` — hook that wraps `TableState` in a Leptos `RwSignal`. Takes `Vec<TRow>` (assigns `RowId`s internally).
- `UseTableHandle<TRow>: Copy` — thin `RwSignal` wrapper with one typed method per core transition.
- `Table` component — same feature set as `chorale-dioxus`: sort headers, filter row, PERF-1 two-level memo virtualization, pagination, infinite scroll, selection, `selection_toolbar` slot, column visibility toolbar, CSV export, column resize, column reorder, grouping, frozen columns, in-cell editing, i18n labels.
- `CellRenderers`, `CellRenderer` (`Arc<dyn Fn(&CellValue) -> AnyView + Send + Sync>`), `ValidateEditFn`, `EditValidation` public types.
- WASM-only `trigger_csv_download` behind `#[cfg(target_arch = "wasm32")]`.
- Leptos examples (Item 11.7): `leptos-basic`, `leptos-with-selection`, `leptos-with-custom-cells`, `leptos-with-column-resize`, `leptos-virtualized-10k-rows`, `leptos-virtualized-1m-rows`, `leptos-qa-harness`.

**`chorale-derive` (new crate, Item 11.0d)**
- `#[derive(TableRow)]` proc-macro generates `fn chorale_columns() -> Vec<ColumnDef<Self>>` from struct fields.
- Supported attributes: `#[chorale(header = "…")]`, `#[chorale(sortable)]`, `#[chorale(filter = "text|multi_select|numeric_range|date_range|boolean")]`, `#[chorale(initial_width = N.0)]`, `#[chorale(alignment = "left|center|right")]`, `#[chorale(render_kind = "number|currency")]`, `#[chorale(skip)]`.

**Examples**
- All 7 Dioxus examples updated for v0.2.0 features.
- 7 new Leptos examples (`leptos-*`) mirroring every Dioxus example.

### Documentation
- `CHANGELOG.md` — this file.
- `docs/QA.md` — extended with v0.2.0 feature verification recipes and a Leptos vs Dioxus behavioral parity checklist.
- README updated: v0.2.0 features, Leptos quickstart, framework comparison table, updated architecture section.

### Documentation
- `CHANGELOG.md` created with full v0.1.0 backfill.
- `docs/QA.md` — manual verification guide with 10 sections covering all v0.1 features.
- `docs/perf-2026-06-04-fine-grained-reactivity.md` — decision record for the PERF-1 two-level memo strategy.

---

## [0.1.0] — 2026-06-03

### Added

**`chorale-core`**
- `TableState<TRow>` — unified, serializable table state struct with immutable-return transitions (CHORALE-CORE-2).
- Sort: single-column ASC/DESC/none cycle via `toggle_sort`. Comparison routes through the column's `CellValue` accessor.
- Filter: per-column typed `FilterKind` on `ColumnDef`. Five kinds: `Text` (case-insensitive substring), `MultiSelect` (set membership with static options), `NumericRange` (dual bound), `DateRange` (dual bound), `Boolean` (tri-state).
- Pagination: `set_page`, `set_page_size`, `total_pages()`, `filtered_row_count()`. Page change resets `scroll_top` to 0.
- Selection: `set_selection`, `toggle_select_all`. Selection persists by stable `RowId` across sort/filter/page changes.
- Column visibility: `set_column_visibility`, `is_column_visible`.
- Column resize: `set_column_width`.
- Fixed-row-height virtualization: `visible_window` computes start/end row indices and top/bottom spacer heights from `scroll_top`, `viewport_height`, and `row_height`.
- CSV export: `to_csv` serializes the full post-filter/post-sort dataset to RFC 4180 format.
- `CellValue` enum: `Text`, `Integer`, `Float`, `Boolean`, `Date`, `DateTime`, `Empty`. Implements `cmp_for_sort` and `matches_filter`.
- `RowId(Uuid)` and `ColumnId(&'static str)` newtypes (ROBUSTNESS-1).

**`chorale-dioxus`**
- `use_table` hook: initializes a `Signal<TableState<TRow>>` and returns a `UseTableHandle<TRow>`.
- `UseTableHandle<TRow>`: `Copy` typed handle with methods for every core transition (`toggle_sort`, `set_filter`, `set_page`, `set_page_size`, `set_selection`, `toggle_select_all`, `set_column_visibility`, `set_column_width`, `set_scroll`, `update_row`).
- `Table<TRow>` component with props: `handle`, `sort_enabled`, `filter_enabled`, `selection_enabled`, `cell_renderers`, `column_toolbar`, `csv_export`, `resize_enabled`.
- `CellRenderers`: per-column `Arc<dyn Fn(&CellValue) -> Element>` custom render map.
- `RenderKind::Badge`: declarative pill rendering with per-value color variants via `BadgeVariantMap`.
- Sticky column headers, fixed-row-height virtualization, `overflow-anchor: none` scroll container, synchronous `scroll_top` reads.
- Row separators via `inset box-shadow` so each row consumes exactly `row_height` pixels.
- DOM `scrollTop` reset on page change (prevents blank spacer view after pagination).

**Examples (7 crates)**
- `examples/basic` — sort + text filter on a 50-row dataset.
- `examples/with-selection` — per-row checkboxes and select-all with live count display.
- `examples/with-custom-cells` — `Badge` vs `CellRenderers` side by side.
- `examples/with-column-resize` — drag-to-resize column borders.
- `examples/virtualized-10k-rows` — 10,010 rows (covers partial last-page edge case).
- `examples/virtualized-1m-rows` — 1M-row stress test.
- `examples/qa-harness` — all features behind runtime toggles for manual QA.

### Notes
- 79 unit tests at release: `chorale-core` 43, `chorale-dioxus` 33, `qa-harness` 3.
- v0.1 does not support variable-row-height virtualization (deferred to v0.2.0, VIRT-1).
- Published to crates.io: `chorale-core 0.1.0`, `chorale-dioxus 0.1.0`.

---

## [0.1.0-patch] — 2026-06-03

### Fixed
- **chorale-dioxus:** Deselected rows retained blue highlight after checkbox toggled off. The deselected branch of `data_tr` emitted `style=""` which Dioxus 0.7's attribute diff did not reliably propagate as "unset". Fixed by emitting `background: transparent` explicitly so the deselected state always overrides the prior inline style. (`0dea27f`)

[Unreleased]: https://github.com/zernst3/rust-chorale/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/zernst3/rust-chorale/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/zernst3/rust-chorale/releases/tag/v0.1.0
