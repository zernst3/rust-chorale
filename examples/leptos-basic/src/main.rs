//! Minimal chorale example: a sortable, text-filterable table.
//!
//! Run with: `trunk serve --open --package leptos-basic`

use chorale_core::{Alignment, CellValue, ColumnDef, ColumnId, FilterKind, RenderKind};
use chorale_leptos::{use_chorale_table, Table};
use leptos::prelude::*;

#[derive(Clone, PartialEq)]
struct Book {
    title: String,
    author: String,
    year: i64,
}

fn books() -> Vec<Book> {
    [
        ("The Pragmatic Programmer", "Hunt & Thomas", 1999),
        ("Designing Data-Intensive Applications", "Kleppmann", 2017),
        ("Clean Code", "Martin", 2008),
        ("Refactoring", "Fowler", 1999),
        ("Domain-Driven Design", "Evans", 2003),
        ("Programming Rust", "Blandy et al.", 2021),
    ]
    .into_iter()
    .map(|(t, a, y)| Book {
        title: t.into(),
        author: a.into(),
        year: y,
    })
    .collect()
}

fn columns() -> Vec<ColumnDef<Book>> {
    vec![
        ColumnDef::new(ColumnId("title"), "Title", |b: &Book| {
            CellValue::Text(b.title.clone())
        })
        .sortable()
        .filter(FilterKind::Text)
        .initial_width(280.0),
        ColumnDef::new(ColumnId("author"), "Author", |b: &Book| {
            CellValue::Text(b.author.clone())
        })
        .sortable()
        .filter(FilterKind::Text)
        .initial_width(160.0),
        ColumnDef::new(ColumnId("year"), "Year", |b: &Book| {
            CellValue::Integer(b.year)
        })
        .sortable()
        .initial_width(80.0)
        .alignment(Alignment::Right)
        .render_kind(RenderKind::Number),
    ]
}

#[component]
fn App() -> impl IntoView {
    let table = use_chorale_table(books(), columns());
    view! {
        <div style="font-family: sans-serif; padding: 1rem; max-width: 800px; margin: 0 auto;">
            <h1>"Basic table"</h1>
            <p>"Click a column header to sort. Type in the filter row to filter by substring."</p>
            <Table handle=table sort_enabled=true filter_enabled=true on_commit_edit=None />
        </div>
    }
}

fn main() {
    leptos::mount::mount_to_body(App);
}
