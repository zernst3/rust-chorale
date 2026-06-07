# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] — 2026-06-05

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
  `visible_view`. Optional `detail_renderer: EventHandler<TRow, Element>`
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
