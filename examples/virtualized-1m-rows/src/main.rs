//! Virtualization stress test: 1,000,000 rows.
//!
//! What this demonstrates:
//! - Scroll-and-paginate performance at 1M rows. Only the rows in the
//!   viewport (plus the overscan buffer) are mounted as `<tr>` elements
//!   regardless of dataset size. The scroll handler is O(1).
//! - The "Go to" input on the pagination row. With `page_size=50` the
//!   dataset spans 20,000 pages; jumping to page 13,492 by typing it is
//!   the only navigation that scales.
//! - The fixed-row-height virtualization math holds well past the
//!   point where rendering every row would be tractable.
//!
//! What this intentionally does NOT enable:
//! - Sort, filter. Each currently re-clones the full row `Vec` on every
//!   state change (the v0.2 fine-grained-reactivity residual called out
//!   in the README). At 1M rows that re-clone is ~30 MB per scroll
//!   event, which is enough to make scroll feel sluggish. Sort + filter
//!   are validated in the smaller examples; this one isolates
//!   virtualization.
//!
//! What to expect on first load:
//! - The page renders an "initializing" message, then ~1-2 seconds later
//!   the table appears. The wait is 1M `RowId::new()` calls (each a
//!   UUID v4 generation), 1M `Event` struct allocations, and the `Vec`
//!   grow-and-copy. WASM is roughly 3-5x slower than native Rust here.
//!
//! Run with: `dx serve --package virtualized-1m-rows`

use chorale_core::{
    Alignment, CellValue, ColumnDef, ColumnId, FilterKind, RenderKind, RowId, TableState,
};
use chorale_dioxus::{use_table, Table, UseTableHandle};
use dioxus::prelude::*;
use rand::{rngs::StdRng, Rng, SeedableRng};
use std::sync::Arc;

const SEED: u64 = 42;
const ROW_COUNT: usize = 1_000_000;

static KINDS: &[&str] = &["pageview", "click", "submit", "error", "signup", "purchase"];
static ACTORS: &[&str] = &[
    "anonymous",
    "alice",
    "bob",
    "charlie",
    "diana",
    "ethan",
    "fiona",
];

/// Compact row: 16 bytes total before padding. At 1M rows that is roughly
/// 16 MB for the `Event` `Vec` plus roughly 16 MB for the `(RowId, Event)`
/// tuple wrapping. Byte indices into static `&str` slices stand in for
/// owned `String`s; the column accessor materializes a `String` only for
/// the 12-20 rows currently visible.
#[derive(Clone, PartialEq)]
struct Event {
    id: u64,
    kind_idx: u8,
    actor_idx: u8,
}

fn events() -> Vec<(RowId, Event)> {
    let mut rng = StdRng::seed_from_u64(SEED);
    let mut v: Vec<(RowId, Event)> = Vec::with_capacity(ROW_COUNT);
    let kinds_len = KINDS.len();
    let actors_len = ACTORS.len();
    for i in 0..ROW_COUNT {
        // `as u8` is sound: KINDS.len() and ACTORS.len() are statically tiny
        // (each well under 256), so the indices fit. Asserted at function entry
        // so a future expansion of either slice past 255 fails loudly here.
        debug_assert!(u8::try_from(kinds_len).is_ok());
        debug_assert!(u8::try_from(actors_len).is_ok());
        #[allow(clippy::cast_possible_truncation)]
        v.push((
            RowId::new(),
            Event {
                id: (i as u64) + 1,
                kind_idx: rng.gen_range(0..kinds_len) as u8,
                actor_idx: rng.gen_range(0..actors_len) as u8,
            },
        ));
    }
    v
}

fn columns() -> Vec<ColumnDef<Event>> {
    vec![
        ColumnDef {
            id: ColumnId("id"),
            header: "Event #".into(),
            accessor: Arc::new(|e: &Event| {
                // `as i64` is safe: row count is capped at 1M, well within i64.
                #[allow(clippy::cast_possible_wrap)]
                CellValue::Integer(e.id as i64)
            }),
            sortable: false,
            filter: FilterKind::None,
            initial_width: Some(140.0),
            alignment: Alignment::Right,
            render_kind: RenderKind::Number,
            header_class: None,
            cell_class: None,
        },
        ColumnDef {
            id: ColumnId("kind"),
            header: "Kind".into(),
            accessor: Arc::new(|e: &Event| CellValue::Text(KINDS[e.kind_idx as usize].to_string())),
            sortable: false,
            filter: FilterKind::None,
            initial_width: Some(160.0),
            alignment: Alignment::Left,
            render_kind: RenderKind::Text,
            header_class: None,
            cell_class: None,
        },
        ColumnDef {
            id: ColumnId("actor"),
            header: "Actor".into(),
            accessor: Arc::new(|e: &Event| {
                CellValue::Text(ACTORS[e.actor_idx as usize].to_string())
            }),
            sortable: false,
            filter: FilterKind::None,
            initial_width: Some(160.0),
            alignment: Alignment::Left,
            render_kind: RenderKind::Text,
            header_class: None,
            cell_class: None,
        },
    ]
}

#[component]
fn App() -> Element {
    // Two-stage render. The first pass shows the "initializing" notice with
    // no dataset; the second pass (triggered by use_effect after mount)
    // generates the 1M rows and re-renders with the table. Without this,
    // the user stares at a blank page for the full generation window.
    let mut handle: Signal<Option<UseTableHandle<Event>>> = use_signal(|| None);

    use_effect(move || {
        if handle.read().is_none() {
            let t = use_table(|| TableState::new(events(), columns()));
            handle.set(Some(t));
        }
    });

    rsx! {
        div { style: "font-family: sans-serif; padding: 1rem; max-width: 900px; margin: 0 auto;",
            h1 { "1,000,000 rows, virtualized" }
            p {
                "Stress test: 1M rows in a single "
                code { "TableState" }
                ". Sort and filter are disabled here because at this scale each "
                "re-clones the row Vec on every state change (see the README's "
                "v0.2 residual on fine-grained reactivity). Scroll, pagination, "
                "and the "
                strong { "Go to" }
                " input on the pagination row all remain O(1) per event."
            }
            match &*handle.read() {
                Some(h) => rsx! {
                    Table {
                        handle: *h,
                        sort_enabled: false,
                        filter_enabled: false,
                    }
                },
                None => rsx! {
                    div {
                        style: "padding: 1rem; border: 1px solid #ddd; border-radius: 4px; \
                                background: #fafafa; color: #555;",
                        p { strong { "Initializing 1,000,000 rows…" } }
                        p { style: "font-size: 0.875rem; margin: 0;",
                            "Expect ~1-2 seconds in WASM. The tab will appear frozen \
                             during generation."
                        }
                    }
                },
            }
        }
    }
}

fn main() {
    dioxus::launch(App);
}
