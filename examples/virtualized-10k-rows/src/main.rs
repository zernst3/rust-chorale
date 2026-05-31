//! Virtualization example: a 10 000-row dataset rendered through chorale's
//! fixed-row-height virtualization. Only the rows in the viewport (plus a
//! small overscan buffer) are mounted as DOM nodes; the rest are accounted
//! for by spacer rows so the scrollbar reflects the full dataset.
//!
//! No `virtualization_enabled` flag exists — virtualization is the
//! rendering strategy, always on.
//!
//! Run with: `dx serve --package virtualized-10k-rows`

use chorale_core::{
    Alignment, CellValue, ColumnDef, ColumnId, FilterKind, RenderKind, RowId, TableState,
};
use chorale_dioxus::{use_table, Table};
use dioxus::prelude::*;
use rand::{rngs::StdRng, Rng, SeedableRng};
use std::sync::Arc;

const SEED: u64 = 42;
// Intentionally not a round multiple of the default page_size (50) so the
// last page renders only 10 rows. Exercises the partial-last-page edge case
// in the virtualization window math and the pagination row count.
const ROW_COUNT: usize = 10_010;

#[derive(Clone, PartialEq)]
struct Event {
    id: i64,
    kind: String,
    actor: String,
}

fn events() -> Vec<(RowId, Event)> {
    let mut rng = StdRng::seed_from_u64(SEED);
    let kinds = ["pageview", "click", "submit", "error", "signup", "purchase"];
    let actors = [
        "anonymous",
        "alice",
        "bob",
        "charlie",
        "diana",
        "ethan",
        "fiona",
    ];
    (0..ROW_COUNT)
        .map(|i| {
            (
                RowId::new(),
                Event {
                    id: i64::try_from(i).unwrap_or(i64::MAX) + 1,
                    kind: kinds[rng.gen_range(0..kinds.len())].into(),
                    actor: actors[rng.gen_range(0..actors.len())].into(),
                },
            )
        })
        .collect()
}

fn columns() -> Vec<ColumnDef<Event>> {
    vec![
        ColumnDef {
            id: ColumnId("id"),
            header: "Event #".into(),
            accessor: Arc::new(|e: &Event| CellValue::Integer(e.id)),
            sortable: true,
            filter: FilterKind::None,
            initial_width: Some(120.0),
            alignment: Alignment::Right,
            render_kind: RenderKind::Number,
            header_class: None,
            cell_class: None,
        },
        ColumnDef {
            id: ColumnId("kind"),
            header: "Kind".into(),
            accessor: Arc::new(|e: &Event| CellValue::Text(e.kind.clone())),
            sortable: true,
            filter: FilterKind::Text,
            initial_width: Some(160.0),
            alignment: Alignment::Left,
            render_kind: RenderKind::Text,
            header_class: None,
            cell_class: None,
        },
        ColumnDef {
            id: ColumnId("actor"),
            header: "Actor".into(),
            accessor: Arc::new(|e: &Event| CellValue::Text(e.actor.clone())),
            sortable: true,
            filter: FilterKind::Text,
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
    let table = use_table(|| TableState::new(events(), columns()));
    let total = table.signal().read().rows.len();
    rsx! {
        div { style: "font-family: sans-serif; padding: 1rem; max-width: 900px; margin: 0 auto;",
            h1 { "10 000 rows, virtualized" }
            p {
                "Dataset: " strong { "{total} rows." }
                " Open DevTools and inspect the table body — there will only be a few "
                code { "<tr>" }
                " elements rendered at a time, regardless of where in the dataset you scroll."
            }
            Table {
                handle: table,
                sort_enabled: true,
                filter_enabled: true,
            }
        }
    }
}

fn main() {
    dioxus::launch(App);
}
