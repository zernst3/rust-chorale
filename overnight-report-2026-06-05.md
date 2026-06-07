# Overnight bot session — 2026-06-05
## Branch: draft-release/v0.2.0

### What was implemented

This session continued the v0.2.0 routing-item batch. The branch is 20 commits ahead of `main`.

| Item | Status |
|---|---|
| **6** — variable-row-height virtualization (VIRT-2) | ✅ Complete |
| **7** — in-cell editing state machine + adapter | ✅ Complete |
| **9** — column reorder (drag-and-drop) | ✅ Complete |
| **10** — frozen columns (CSS sticky) | ✅ Complete |
| **11** — selection_toolbar slot | ✅ Complete |
| **11.0a** — multi-column sort, SortAction, priority badges | ✅ Complete |
| **11.0b** — PaginationMode::InfiniteScroll | ✅ Complete (this session) |
| **11.0c** — user-overridable Labels | ✅ Complete (prior session) |
| **8** — grouping/aggregation | ⏳ Not started (very complex — defer?) |
| **11.0d** — chorale-derive proc-macro | ⏳ Not started |
| **11.5** — chorale-leptos adapter | ⏳ Not started |

---

### Item 11.0b detail (this session)

**Core (`chorale-core`)**

- `PaginationMode` enum (`Pages` default | `InfiniteScroll`), `#[non_exhaustive]`.
- `TableState` gained two new fields: `pagination_mode: PaginationMode` and `loaded_row_count: usize`.
- `StateError::InvalidModeForTransition` — new error variant.
- `set_pagination_mode(state, mode)` — switches modes, re-initialises `loaded_row_count` to `page_size` when entering InfiniteScroll (0 when returning to Pages).
- `load_more_rows(state)` — grows `loaded_row_count` by `page_size`, capped at `filtered_row_count`; errors in Pages mode.
- `set_filter`, `toggle_sort`, `remove_sort`, `clear_sort` — reset `loaded_row_count` to `page_size` (not 0) in InfiniteScroll so the list re-anchors at the top batch.
- `set_page` — returns `Err(InvalidModeForTransition)` in InfiniteScroll mode.
- `visible_view` — branches on `pagination_mode`: Pages uses existing page slice; InfiniteScroll slices `[..loaded_row_count]`.
- `visible_rows` / `visible_row_ids` — refactored to delegate to `visible_view` so all three agree.
- `Labels::load_more_label` — "Loading more rows…" (English default).
- 22 new unit tests. All 169 tests pass; clippy clean.

**Adapter (`chorale-dioxus`)**

- `UseTableHandle::set_pagination_mode` and `load_more_rows` methods.
- `view_key` memo now includes `loaded_row_count` so InfiniteScroll view refreshes when a batch loads.
- New prop `infinite_scroll_threshold_px: f64` (default 200 px) — distance from scroll bottom that triggers `load_more_rows`.
- `onscroll` handler detects threshold and fires `load_more_rows` in InfiniteScroll mode.
- Pagination bar hidden in InfiniteScroll mode; a "Loading more rows…" label appears at the bottom while more rows are available.

---

### Push status

**`git push origin HEAD` was not run** (permission was denied). The branch has 20 unpushed commits. Run:

```
git push origin draft-release/v0.2.0
```

---

### Suggested next items

1. **Interactive scroll verification** of Items 9, 10, 11.0b in the browser before signing off (gates phase advance).
2. **Item 11.0d** — chorale-derive proc-macro.
3. **Item 11.5** — chorale-leptos adapter.
4. **Item 8** — grouping/aggregation (recommend deferring to v0.3.0).
5. **WASM build verification** (`dx build --features web`) after the InfiniteScroll changes.
