//! Column resize example: drag the right edge of any column header to
//! resize it. Set `resize_enabled: true` on `<Table>`. Initial widths come
//! from each `ColumnDef::initial_width`; per-column drag overrides are
//! persisted in `TableState::column_widths`.
//!
//! Run with: `dx serve --package with-column-resize`

use chorale_core::{
    Alignment, CellValue, ColumnDef, ColumnId, CurrencyCode, FilterKind, RenderKind, RowId,
    TableState,
};
use chorale_dioxus::{use_table, Table};
use dioxus::prelude::*;
use std::sync::Arc;

#[derive(Clone, PartialEq)]
struct Invoice {
    number: String,
    customer: String,
    line_item: String,
    amount: i64,
}

fn invoices() -> Vec<(RowId, Invoice)> {
    [
        ("INV-1001", "Acme Corp", "Annual subscription", 24_000),
        ("INV-1002", "Globex", "Implementation services", 84_500),
        ("INV-1003", "Initech", "Quarterly retainer", 18_750),
        ("INV-1004", "Umbrella Inc.", "Hardware procurement", 152_300),
        ("INV-1005", "Stark Industries", "Audit + compliance", 67_200),
    ]
    .into_iter()
    .map(|(n, c, l, a)| {
        (
            RowId::new(),
            Invoice {
                number: n.into(),
                customer: c.into(),
                line_item: l.into(),
                amount: a,
            },
        )
    })
    .collect()
}

fn columns() -> Vec<ColumnDef<Invoice>> {
    vec![
        ColumnDef {
            id: ColumnId("number"),
            header: "Invoice #".into(),
            accessor: Arc::new(|i: &Invoice| CellValue::Text(i.number.clone())),
            sortable: true,
            filter: FilterKind::Text,
            initial_width: Some(110.0),
            alignment: Alignment::Left,
            render_kind: RenderKind::Text,
            header_class: None,
            cell_class: None,
        },
        ColumnDef {
            id: ColumnId("customer"),
            header: "Customer".into(),
            accessor: Arc::new(|i: &Invoice| CellValue::Text(i.customer.clone())),
            sortable: true,
            filter: FilterKind::Text,
            initial_width: Some(160.0),
            alignment: Alignment::Left,
            render_kind: RenderKind::Text,
            header_class: None,
            cell_class: None,
        },
        ColumnDef {
            id: ColumnId("line_item"),
            header: "Line item".into(),
            accessor: Arc::new(|i: &Invoice| CellValue::Text(i.line_item.clone())),
            sortable: true,
            filter: FilterKind::Text,
            initial_width: Some(220.0),
            alignment: Alignment::Left,
            render_kind: RenderKind::Text,
            header_class: None,
            cell_class: None,
        },
        ColumnDef {
            id: ColumnId("amount"),
            header: "Amount".into(),
            accessor: Arc::new(|i: &Invoice| CellValue::Integer(i.amount)),
            sortable: true,
            filter: FilterKind::None,
            initial_width: Some(140.0),
            alignment: Alignment::Right,
            render_kind: RenderKind::Currency(CurrencyCode::USD),
            header_class: None,
            cell_class: None,
        },
    ]
}

#[component]
fn App() -> Element {
    let table = use_table(|| TableState::new(invoices(), columns()));
    rsx! {
        div { style: "font-family: sans-serif; padding: 1rem; max-width: 900px; margin: 0 auto;",
            h1 { "Column resize example" }
            p {
                "Hover the right edge of any column header. The cursor becomes a "
                code { "col-resize" }
                " handle. Click and drag to resize. Widths are kept in "
                code { "TableState::column_widths" }
                " and persist across re-renders."
            }
            Table {
                handle: table,
                sort_enabled: true,
                resize_enabled: true,
            }
        }
    }
}

fn main() {
    dioxus::launch(App);
}
