//! Virtualization example: a 10 000-row dataset rendered through chorale's
//! fixed-row-height virtualization. Only the rows in the viewport (plus a
//! small overscan buffer) are mounted as DOM nodes; the rest are accounted
//! for by spacer rows so the scrollbar reflects the full dataset.
//!
//! Run with: `trunk serve --open --package leptos-virtualized-10k-rows`

use chorale_core::{Alignment, CellValue, ColumnDef, ColumnId, FilterKind, RenderKind};
use chorale_leptos::{use_chorale_table, Table};
use leptos::prelude::*;
use rand::{rngs::StdRng, Rng, SeedableRng};

const SEED: u64 = 42;
// Intentionally not a round multiple of the default page_size (50) so the
// last page renders only 10 rows. Exercises the partial-last-page edge case.
const ROW_COUNT: usize = 10_010;

#[derive(Clone, PartialEq)]
struct Event {
    id: i64,
    kind: String,
    actor: String,
}

fn events() -> Vec<Event> {
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
        .map(|i| Event {
            id: i64::try_from(i).unwrap_or(i64::MAX) + 1,
            kind: kinds[rng.gen_range(0..kinds.len())].into(),
            actor: actors[rng.gen_range(0..actors.len())].into(),
        })
        .collect()
}

fn columns() -> Vec<ColumnDef<Event>> {
    vec![
        ColumnDef::new(ColumnId("id"), "Event #", |e: &Event| {
            CellValue::Integer(e.id)
        })
        .sortable()
        .initial_width(120.0)
        .alignment(Alignment::Right)
        .render_kind(RenderKind::Number),
        ColumnDef::new(ColumnId("kind"), "Kind", |e: &Event| {
            CellValue::Text(e.kind.clone())
        })
        .sortable()
        .filter(FilterKind::Text)
        .initial_width(160.0),
        ColumnDef::new(ColumnId("actor"), "Actor", |e: &Event| {
            CellValue::Text(e.actor.clone())
        })
        .sortable()
        .filter(FilterKind::Text)
        .initial_width(160.0),
    ]
}

#[component]
fn App() -> impl IntoView {
    let table = use_chorale_table(events(), columns());
    let total = table.signal().with_untracked(|s| s.rows.len());
    view! {
        <div style="font-family: sans-serif; padding: 1rem; max-width: 900px; margin: 0 auto;">
            <h1>"10 000 rows, virtualized"</h1>
            <p>
                "Dataset: " <strong>{total}" rows."</strong>
                " Open DevTools and inspect the table body — there will only be a few "
                <code>"<tr>"</code>
                " elements rendered at a time, regardless of where in the dataset you scroll."
            </p>
            <Table handle=table sort_enabled=true filter_enabled=true on_commit_edit=None />
        </div>
    }
}

fn main() {
    leptos::mount::mount_to_body(App);
}
