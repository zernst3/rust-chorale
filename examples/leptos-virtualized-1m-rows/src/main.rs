//! Virtualization stress test: 1,000,000 rows.
//!
//! What this demonstrates:
//! - Scroll-and-paginate performance at 1M rows. Only the rows in the
//!   viewport (plus the overscan buffer) are mounted as `<tr>` elements
//!   regardless of dataset size.
//! - The "Go to" input on the pagination row. With `page_size=50` the
//!   dataset spans 20,000 pages.
//! - The fixed-row-height virtualization math at scale.
//!
//! What this intentionally does NOT enable:
//! - Sort, filter. Each currently re-clones the full row `Vec` on every
//!   state change. At 1M rows that re-clone is ~30 MB per scroll event.
//!
//! What to expect on first load:
//! - The page renders an "initializing" message, then ~1-2 seconds later
//!   the table appears. WASM is roughly 3-5x slower than native Rust here.
//!
//! Run with: `trunk serve --open --package leptos-virtualized-1m-rows`

use chorale_core::{Alignment, CellValue, ColumnDef, ColumnId, RenderKind};
use chorale_leptos::{use_chorale_table, Table, UseTableHandle};
use leptos::prelude::*;
use rand::{rngs::StdRng, Rng, SeedableRng};

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

/// Compact row: byte indices into static `&str` slices stand in for
/// owned `String`s; the column accessor materializes a `String` only for
/// the 12-20 rows currently visible.
#[derive(Clone, PartialEq)]
struct Event {
    id: u64,
    kind_idx: u8,
    actor_idx: u8,
}

fn events() -> Vec<Event> {
    let mut rng = StdRng::seed_from_u64(SEED);
    let mut v: Vec<Event> = Vec::with_capacity(ROW_COUNT);
    let kinds_len = KINDS.len();
    let actors_len = ACTORS.len();
    for i in 0..ROW_COUNT {
        #[allow(clippy::cast_possible_truncation)]
        v.push(Event {
            id: (i as u64) + 1,
            kind_idx: rng.gen_range(0..kinds_len) as u8,
            actor_idx: rng.gen_range(0..actors_len) as u8,
        });
    }
    v
}

fn columns() -> Vec<ColumnDef<Event>> {
    vec![
        ColumnDef::new(ColumnId("id"), "Event #", |e: &Event| {
            #[allow(clippy::cast_possible_wrap)]
            CellValue::Integer(e.id as i64)
        })
        .initial_width(140.0)
        .alignment(Alignment::Right)
        .render_kind(RenderKind::Number),
        ColumnDef::new(ColumnId("kind"), "Kind", |e: &Event| {
            CellValue::Text(KINDS[e.kind_idx as usize].to_string())
        })
        .initial_width(160.0),
        ColumnDef::new(ColumnId("actor"), "Actor", |e: &Event| {
            CellValue::Text(ACTORS[e.actor_idx as usize].to_string())
        })
        .initial_width(160.0),
    ]
}

#[component]
fn App() -> impl IntoView {
    // Two-stage render. The first pass shows the "initializing" notice with
    // no dataset; the second pass (triggered by Effect after mount)
    // generates the 1M rows and re-renders with the table.
    let table: RwSignal<Option<UseTableHandle<Event>>> = RwSignal::new(None);

    Effect::new(move |_| {
        if table.get_untracked().is_none() {
            let handle = use_chorale_table(events(), columns());
            table.set(Some(handle));
        }
    });

    view! {
        <div style="font-family: sans-serif; padding: 1rem; max-width: 900px; margin: 0 auto;">
            <h1>"1,000,000 rows, virtualized"</h1>
            <p>
                "Stress test: 1M rows in a single "
                <code>"TableState"</code>
                ". Sort and filter are disabled here because at this scale each "
                "re-clones the row Vec on every state change. Scroll, pagination, "
                "and the "
                <strong>"Go to"</strong>
                " input on the pagination row all remain O(1) per event."
            </p>
            {move || table.get().map(|h| view! {
                <Table handle=h sort_enabled=false filter_enabled=false on_commit_edit=None />
            })}
            {move || table.get().is_none().then(|| view! {
                <div style="padding: 1rem; border: 1px solid #ddd; border-radius: 4px; background: #fafafa; color: #555;">
                    <p><strong>"Initializing 1,000,000 rows…"</strong></p>
                    <p style="font-size: 0.875rem; margin: 0;">
                        "Expect ~1-2 seconds in WASM. The tab will appear frozen during generation."
                    </p>
                </div>
            })}
        </div>
    }
}

fn main() {
    leptos::mount::mount_to_body(App);
}
