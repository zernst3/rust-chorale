# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

_v0.2.0 entries accumulate here as items ship._

### Added
- `UseTableHandle::selected_ids() -> Vec<RowId>` — convenience method to read the current selection without reaching into the signal directly.
- `UseTableHandle::selection_count() -> usize` — convenience method for the selection length.

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

[Unreleased]: https://github.com/zernst3/rust-chorale/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/zernst3/rust-chorale/releases/tag/v0.1.0
