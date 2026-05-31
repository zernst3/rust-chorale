//! Minimal chorale example: a sortable, text-filterable table.
//!
//! Run with: `dx serve --package basic`

use chorale_core::{
    Alignment, CellValue, ColumnDef, ColumnId, FilterKind, RenderKind, RowId, TableState,
};
use chorale_dioxus::{use_table, Table};
use dioxus::prelude::*;
use std::sync::Arc;

#[derive(Clone, PartialEq)]
struct Book {
    title: String,
    author: String,
    year: i64,
}

fn books() -> Vec<(RowId, Book)> {
    [
        ("The Pragmatic Programmer", "Hunt & Thomas", 1999),
        ("Designing Data-Intensive Applications", "Kleppmann", 2017),
        ("Clean Code", "Martin", 2008),
        ("Refactoring", "Fowler", 1999),
        ("Domain-Driven Design", "Evans", 2003),
        ("Programming Rust", "Blandy et al.", 2021),
    ]
    .into_iter()
    .map(|(t, a, y)| {
        (
            RowId::new(),
            Book {
                title: t.into(),
                author: a.into(),
                year: y,
            },
        )
    })
    .collect()
}

fn columns() -> Vec<ColumnDef<Book>> {
    vec![
        ColumnDef {
            id: ColumnId("title"),
            header: "Title".into(),
            accessor: Arc::new(|b: &Book| CellValue::Text(b.title.clone())),
            sortable: true,
            filter: FilterKind::Text,
            initial_width: Some(280.0),
            alignment: Alignment::Left,
            render_kind: RenderKind::Text,
            header_class: None,
            cell_class: None,
        },
        ColumnDef {
            id: ColumnId("author"),
            header: "Author".into(),
            accessor: Arc::new(|b: &Book| CellValue::Text(b.author.clone())),
            sortable: true,
            filter: FilterKind::Text,
            initial_width: Some(160.0),
            alignment: Alignment::Left,
            render_kind: RenderKind::Text,
            header_class: None,
            cell_class: None,
        },
        ColumnDef {
            id: ColumnId("year"),
            header: "Year".into(),
            accessor: Arc::new(|b: &Book| CellValue::Integer(b.year)),
            sortable: true,
            filter: FilterKind::None,
            initial_width: Some(80.0),
            alignment: Alignment::Right,
            render_kind: RenderKind::Number,
            header_class: None,
            cell_class: None,
        },
    ]
}

#[component]
fn App() -> Element {
    let table = use_table(|| TableState::new(books(), columns()));
    rsx! {
        div { style: "font-family: sans-serif; padding: 1rem; max-width: 800px; margin: 0 auto;",
            h1 { "Basic table" }
            p { "Click a column header to sort. Type in the filter row to filter by substring." }
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
