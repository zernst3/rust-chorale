//! QA harness for chorale-dioxus.
//!
//! Run with: `dx serve --package qa-harness` (requires `cargo install dioxus-cli`)
//!
//! Generates a reproducible 10k-row Employee dataset and renders a page
//! scaffold with feature-toggle controls. Each v0.1 and v0.2.0 work-queue
//! item wires one toggle as the matching adapter feature lands.

use chorale_core::{
    AggregatorKind, Alignment, BadgeVariant, BadgeVariantMap, CellValue, ColumnDef, ColumnId,
    CommittedEdit, CurrencyCode, EditorKind, FilterKind, FrozenSide, GroupedPaginationMode, Labels,
    PaginationMode, RenderKind, RowId, TableState,
};
use chorale_derive::TableRow;
use chorale_dioxus::{use_table, CellRenderer, CellRenderers, Table};
use chrono::NaiveDate;
use dioxus::prelude::*;
use rand::{rngs::StdRng, Rng, SeedableRng};
use std::collections::HashMap;
use std::sync::Arc;

// Fixed seed guarantees the same dataset on every run (work-queue item 0).
const SEED: u64 = 42;
// Intentionally not a round multiple of the default page_size (50) so the
// last page renders only 10 rows. Exercises the partial-last-page edge case.
const ROW_COUNT: usize = 10_010;

// Spread the date range over 10 years starting from 2015-01-01.
const DATE_RANGE_DAYS: i64 = 3_650;

static FIRST_NAMES: &[&str] = &[
    "Alice", "Bob", "Charlie", "Diana", "Ethan", "Fiona", "George", "Hannah", "Ivan", "Julia",
    "Kevin", "Laura", "Mike", "Nancy", "Oscar", "Pam", "Quinn", "Rose", "Sam", "Tina", "Uma",
    "Victor", "Wendy", "Xander", "Yara", "Zoe",
];

static LAST_NAMES: &[&str] = &[
    "Smith", "Jones", "Williams", "Brown", "Davis", "Miller", "Wilson", "Moore", "Taylor",
    "Anderson", "Thomas", "Jackson", "White", "Harris", "Martin", "Thompson", "Garcia", "Martinez",
    "Robinson", "Clark",
];

static ROLES: &[&str] = &[
    "Engineer",
    "Designer",
    "Manager",
    "Analyst",
    "Director",
    "Coordinator",
    "Developer",
    "Architect",
];

static STATUSES: &[&str] = &["Active", "Inactive", "Pending", "Suspended"];

static EMAIL_DOMAINS: &[&str] = &["example.com", "corp.io", "company.net", "org.dev"];

#[derive(TableRow, Clone, PartialEq)]
struct Employee {
    name: String,
    email: String,
    joined_date: NaiveDate,
    role: String,
    status: String,
    salary: i64,
}

// ── Master/detail demo: per-employee order line items ────────────────────────
//
// Each row in the main table represents an Employee. When the master/detail
// toggle is on, clicking a row's expand chevron reveals a child `<Table>`
// showing that employee's order line items (qty + unit price). Demonstrates
// that `detail_renderer` can mount any `Element`, including a nested
// chorale-dioxus Table with its own state, sorting, and rendering rules.

#[derive(Clone, PartialEq)]
struct LineItem {
    label: &'static str,
    qty: i64,
    unit_price: f64,
}

static LINE_ITEM_LABELS: &[&str] = &[
    "Widget A",
    "Widget B",
    "Gadget C",
    "Gizmo D",
    "Doohickey E",
];

// Deterministic per-employee line items, seeded from the employee's email
// so the dataset is stable across re-renders.
#[must_use]
fn line_items_for_employee(email: &str) -> Vec<(RowId, LineItem)> {
    let mut seed: u64 = 0xCBF2_9CE4_8422_2325; // FNV-1a offset basis
    for b in email.bytes() {
        seed ^= u64::from(b);
        seed = seed.wrapping_mul(0x100_0000_01B3);
    }
    let mut rng = StdRng::seed_from_u64(seed);
    let count = rng.gen_range(2..6);
    (0..count)
        .map(|_| {
            (
                RowId::new(),
                LineItem {
                    label: LINE_ITEM_LABELS[rng.gen_range(0..LINE_ITEM_LABELS.len())],
                    qty: rng.gen_range(1..20),
                    unit_price: f64::from(rng.gen_range(500..50_000)) / 100.0,
                },
            )
        })
        .collect()
}

#[must_use]
fn line_item_columns() -> Vec<ColumnDef<LineItem>> {
    vec![
        ColumnDef::new(ColumnId("li_label"), "Item", |li: &LineItem| {
            CellValue::Text(li.label.to_string())
        })
        .sortable()
        .initial_width(180.0),
        ColumnDef::new(ColumnId("li_qty"), "Qty", |li: &LineItem| {
            CellValue::Integer(li.qty)
        })
        .sortable()
        .initial_width(80.0)
        .alignment(Alignment::Right)
        .render_kind(RenderKind::Number),
        ColumnDef::new(ColumnId("li_price"), "Unit Price", |li: &LineItem| {
            CellValue::Float(li.unit_price)
        })
        .sortable()
        .initial_width(120.0)
        .alignment(Alignment::Right)
        .render_kind(RenderKind::Currency(CurrencyCode::USD)),
    ]
}

#[component]
fn EmployeeDetailPanel(employee: Employee) -> Element {
    let items = line_items_for_employee(&employee.email);
    let item_count = items.len();
    let total: f64 = items
        .iter()
        .map(|(_, li)| f64::from(li.qty as i32) * li.unit_price)
        .sum();
    // The child <Table> renders with `inline: true` (see the prop below),
    // which makes it render at natural height with no internal scroll
    // container. That's what's needed for a child table embedded inside a
    // parent's scrolling viewport: no nested scroll context, no wheel
    // hand-off discontinuity. Page-size is set to item_count so the child
    // never paginates either.
    let table = use_table(move || {
        let mut s = TableState::new(items.clone(), line_item_columns());
        s.page_size = item_count.max(1);
        s
    });
    rsx! {
        div {
            style: "padding: 12px 24px; background: #fafafa; \
                    border-top: 1px solid #e5e7eb;",
            div {
                style: "font-size: 0.75rem; font-weight: 600; color: #6b7280; \
                        margin-bottom: 8px; display: flex; \
                        justify-content: space-between; align-items: baseline;",
                span { "ORDER LINE ITEMS — {employee.name}" }
                span {
                    style: "font-weight: 500; color: #374151;",
                    "{item_count} item(s) — Total: ${total:.2}"
                }
            }
            Table {
                handle: table,
                sort_enabled: true,
                // inline: true → no internal scroll container, no virtualization;
                // child renders at natural height so the parent's scroll context
                // owns wheel events end-to-end.
                inline: true,
            }
        }
    }
}

#[must_use]
fn generate_dataset() -> Vec<(RowId, Employee)> {
    let mut rng = StdRng::seed_from_u64(SEED);
    // 2015-01-01 is always a valid Gregorian date.
    let base = NaiveDate::from_ymd_opt(2015, 1, 1).unwrap_or(NaiveDate::MIN);

    (0..ROW_COUNT)
        .map(|_| {
            let first = FIRST_NAMES[rng.gen_range(0..FIRST_NAMES.len())];
            let last = LAST_NAMES[rng.gen_range(0..LAST_NAMES.len())];
            let name = format!("{first} {last}");
            let domain = EMAIL_DOMAINS[rng.gen_range(0..EMAIL_DOMAINS.len())];
            let email = format!("{}.{}@{domain}", first.to_lowercase(), last.to_lowercase());
            let days: i64 = rng.gen_range(0..DATE_RANGE_DAYS);
            let joined_date = base + chrono::Duration::days(days);
            let role = ROLES[rng.gen_range(0..ROLES.len())].to_string();
            let status = STATUSES[rng.gen_range(0..STATUSES.len())].to_string();
            let salary: i64 = rng.gen_range(40_000..200_000);
            (
                RowId::new(),
                Employee {
                    name,
                    email,
                    joined_date,
                    role,
                    status,
                    salary,
                },
            )
        })
        .collect()
}

/// Build the hand-crafted column set. Always includes `AggregatorKind::Sum`
/// on salary (shown only when grouping is active). Editing and frozen-column
/// features are opt-in via the toggles.
#[must_use]
fn build_columns(editing: bool, frozen: bool) -> Vec<ColumnDef<Employee>> {
    let status_badges = BadgeVariantMap::new()
        .with("Active", BadgeVariant::new("Active", "green"))
        .with("Inactive", BadgeVariant::new("Inactive", "gray"))
        .with("Pending", BadgeVariant::new("Pending", "yellow"))
        .with("Suspended", BadgeVariant::new("Suspended", "red"));

    // Multi-sort QA note: the sample dataset has 10_010 rows generated from
    // first+last name pairs, which produces near-unique names. Sorting by
    // Name first disambiguates ~98% of rows on its own, so a 2nd or 3rd sort
    // column will appear to "do nothing" visually even though the sort
    // algorithm IS correctly chaining (see filtered_sorted_pairs in
    // chorale-core/src/views.rs and the 3-column unit test in
    // chorale-dioxus/src/components.rs#sort_3_columns_grows_to_3).
    //
    // To visually verify multi-column sort cascade, click in this order
    // instead:
    //   1. Status   (5 values → ~2,000 rows per group)
    //   2. Shift+Role  (within each Status, rows reorder by Role)
    //   3. Shift+Salary (within each Status+Role, rows reorder by Salary)
    let mut name_col = ColumnDef::new(ColumnId("name"), "Name", |r: &Employee| {
        CellValue::Text(r.name.clone())
    })
    .sortable()
    .filter(FilterKind::Text)
    .initial_width(160.0);
    if editing {
        name_col = name_col.editor(EditorKind::Text);
    }
    if frozen {
        name_col = name_col.frozen(FrozenSide::Left);
    }

    let email_col = ColumnDef::new(ColumnId("email"), "Email", |r: &Employee| {
        CellValue::Text(r.email.clone())
    })
    .sortable()
    .filter(FilterKind::Text)
    .initial_width(220.0);

    let joined_col = ColumnDef::new(ColumnId("joined_date"), "Joined", |r: &Employee| {
        CellValue::Date(r.joined_date)
    })
    .sortable()
    .filter(FilterKind::DateRange)
    .initial_width(180.0)
    .render_kind(RenderKind::Date);

    let role_col = ColumnDef::new(ColumnId("role"), "Role", |r: &Employee| {
        CellValue::Text(r.role.clone())
    })
    .sortable()
    .filter(FilterKind::MultiSelect {
        options: ROLES.iter().map(|s| (*s).to_string()).collect(),
    })
    .initial_width(140.0);

    let status_col = ColumnDef::new(ColumnId("status"), "Status", |r: &Employee| {
        CellValue::Text(r.status.clone())
    })
    .sortable()
    .filter(FilterKind::MultiSelect {
        options: STATUSES.iter().map(|s| (*s).to_string()).collect(),
    })
    .initial_width(120.0)
    .alignment(Alignment::Center)
    .render_kind(RenderKind::Badge(status_badges));

    let mut salary_col = ColumnDef::new(ColumnId("salary"), "Salary", |r: &Employee| {
        CellValue::Integer(r.salary)
    })
    .sortable()
    .filter(FilterKind::NumericRange {
        min: 40_000.0,
        max: 200_000.0,
        step: 1_000.0,
    })
    .initial_width(160.0)
    .alignment(Alignment::Right)
    .render_kind(RenderKind::Currency(CurrencyCode::USD))
    .aggregator(AggregatorKind::Sum);
    if frozen {
        salary_col = salary_col.frozen(FrozenSide::Right);
    }

    vec![
        name_col, email_col, joined_col, role_col, status_col, salary_col,
    ]
}

fn make_status_renderer() -> CellRenderer {
    Arc::new(|val: &CellValue| {
        let text = if let CellValue::Text(s) = val {
            s.as_str()
        } else {
            ""
        };
        let (prefix, color) = match text {
            "Active" => ("● ", "#065f46"),
            "Inactive" => ("○ ", "#374151"),
            "Pending" => ("◑ ", "#92400e"),
            "Suspended" => ("✕ ", "#991b1b"),
            _ => ("", "#333"),
        };
        rsx! {
            span {
                style: "color: {color}; font-weight: 500; font-size: 0.875rem;",
                "{prefix}{text}"
            }
        }
    })
}

fn make_variable_height_renderer() -> CellRenderer {
    Arc::new(|val: &CellValue| {
        let name = if let CellValue::Text(s) = val {
            s.clone()
        } else {
            String::new()
        };
        // Hash name length into 0-2 extra note lines so rows visibly vary in height.
        let extra = name.len() % 3;
        rsx! {
            div { style: "padding: 2px 0;",
                div { style: "font-weight: 500;", "{name}" }
                if extra >= 1 {
                    div {
                        style: "font-size: 0.72rem; color: #6b7280; margin-top: 1px;",
                        "▸ note A: joined employee record"
                    }
                }
                if extra >= 2 {
                    div {
                        style: "font-size: 0.72rem; color: #6b7280; margin-top: 1px;",
                        "▸ note B: pending review cycle"
                    }
                }
            }
        }
    })
}

#[component]
fn App() -> Element {
    let table = use_table(|| TableState::new(generate_dataset(), build_columns(false, false)));
    let row_count = table.signal().read().rows.len();

    // ── v0.1 toggles ────────────────────────────────────────────────────────
    let mut sort_on = use_signal(|| false);
    let mut filter_on = use_signal(|| false);
    let mut selection_on = use_signal(|| false);
    let mut col_toolbar_on = use_signal(|| false);
    let mut csv_export_on = use_signal(|| false);
    let mut resize_on = use_signal(|| false);

    // ── v0.2.0 toggles ──────────────────────────────────────────────────────
    let mut infinite_scroll_on = use_signal(|| false);
    let mut labels_french_on = use_signal(|| false);
    let mut variable_height_on = use_signal(|| false);
    let mut editing_on = use_signal(|| false);
    let mut grouping_on = use_signal(|| false);
    let mut grouped_pagination_virt = use_signal(|| false);
    let mut column_reorder_on = use_signal(|| false);
    let mut frozen_columns_on = use_signal(|| false);
    let mut selection_toolbar_on = use_signal(|| false);
    let mut master_detail_on = use_signal(|| false);
    let mut use_derive_on = use_signal(|| false);

    // ── Cell renderers (re-built when variable_height_on changes) ────────────
    let cell_renderers = use_memo(move || {
        let mut m = HashMap::new();
        m.insert(ColumnId("status"), make_status_renderer());
        if *variable_height_on.read() {
            m.insert(ColumnId("name"), make_variable_height_renderer());
        }
        CellRenderers::new(m)
    });

    // ── French labels (created once) ─────────────────────────────────────────
    let french_labels = use_memo(|| {
        let mut l = Labels::default();
        l.filter_placeholder = "Filtrer\u{2026}".into();
        l.export_csv_label = "Exporter CSV".into();
        l.previous_page_label = "\u{2039} Pr\u{e9}c".into();
        l.next_page_label = "Suiv \u{203a}".into();
        l.go_to_page_label = "Aller \u{e0}".into();
        l.no_rows_label = "Aucune ligne ne correspond au filtre.".into();
        l.load_more_label = "Charger plus\u{2026}".into();
        l
    });

    // ── Effect: rebuild columns when editing / frozen / derive toggles change ─
    use_effect(move || {
        let cols = if *use_derive_on.read() {
            Employee::chorale_columns()
        } else {
            build_columns(*editing_on.read(), *frozen_columns_on.read())
        };
        table.signal().write().columns = cols;
    });

    // ── Effect: pagination mode ───────────────────────────────────────────────
    use_effect(move || {
        if *infinite_scroll_on.read() {
            table.set_pagination_mode(PaginationMode::InfiniteScroll);
        } else {
            table.set_pagination_mode(PaginationMode::Pages);
        }
    });

    // ── Effect: grouping ─────────────────────────────────────────────────────
    use_effect(move || {
        if *grouping_on.read() {
            table.set_grouping(vec![ColumnId("role")]);
        } else {
            table.set_grouping(vec![]);
        }
    });

    // ── Effect: grouped pagination mode ──────────────────────────────────────
    use_effect(move || {
        table.signal().write().grouped_pagination = if *grouped_pagination_virt.read() {
            GroupedPaginationMode::Virtualized
        } else {
            GroupedPaginationMode::DataRowsOnly
        };
    });

    // ── Computed props for the Table ─────────────────────────────────────────
    let commit_handler: Option<EventHandler<CommittedEdit<Employee>>> = if *editing_on.read() {
        Some(EventHandler::new(move |edit: CommittedEdit<Employee>| {
            let current_row = table
                .signal()
                .read()
                .rows
                .iter()
                .find(|(id, _)| *id == edit.row_id)
                .map(|(_, r)| r.clone());
            if let Some(mut row) = current_row {
                if edit.column_id == ColumnId("name") {
                    row.name.clone_from(&edit.value);
                }
                table.update_row(edit.row_id, row);
            }
        }))
    } else {
        None
    };

    let toolbar_el: Option<Element> = if *selection_toolbar_on.read() {
        let count = table.signal().read().selection.len();
        Some(rsx! {
            div {
                style: "display: flex; align-items: center; gap: 1rem; \
                        padding: 0.75rem 1rem; background: #1d4ed8; color: white; \
                        font-size: 0.875rem; font-weight: 600; width: 100%; \
                        box-sizing: border-box; flex-wrap: wrap;",
                span { "{count} row(s) selected" }
                div { class: "chorale-bulk-actions",
                    style: "display: flex; gap: 8px;",
                    button {
                        onclick: move |_| table.select_all_visible_page(),
                        style: "padding: 0.25rem 0.75rem; background: rgba(255,255,255,0.2); \
                                color: white; border: 1px solid rgba(255,255,255,0.4); \
                                border-radius: 3px; cursor: pointer; font-size: 0.8rem;",
                        "Select page"
                    }
                    button {
                        onclick: move |_| table.select_all_filtered(),
                        style: "padding: 0.25rem 0.75rem; background: rgba(255,255,255,0.2); \
                                color: white; border: 1px solid rgba(255,255,255,0.4); \
                                border-radius: 3px; cursor: pointer; font-size: 0.8rem;",
                        "Select all"
                    }
                    button {
                        onclick: move |_| table.deselect_all_visible_page(),
                        style: "padding: 0.25rem 0.75rem; background: rgba(255,255,255,0.2); \
                                color: white; border: 1px solid rgba(255,255,255,0.4); \
                                border-radius: 3px; cursor: pointer; font-size: 0.8rem;",
                        "Deselect page"
                    }
                    button {
                        onclick: move |_| table.deselect_all(),
                        style: "padding: 0.25rem 0.75rem; background: rgba(255,255,255,0.2); \
                                color: white; border: 1px solid rgba(255,255,255,0.4); \
                                border-radius: 3px; cursor: pointer; font-size: 0.8rem;",
                        "Deselect all"
                    }
                }
                button {
                    style: "padding: 0.25rem 0.75rem; background: rgba(255,255,255,0.2); \
                            color: white; border: 1px solid rgba(255,255,255,0.4); \
                            border-radius: 3px; cursor: pointer; font-size: 0.8rem;",
                    "[Delete Selected]"
                }
                button {
                    style: "padding: 0.25rem 0.75rem; background: rgba(255,255,255,0.2); \
                            color: white; border: 1px solid rgba(255,255,255,0.4); \
                            border-radius: 3px; cursor: pointer; font-size: 0.8rem;",
                    "[Export Selected]"
                }
            }
        })
    } else {
        None
    };

    let labels_opt: Option<Labels> = if *labels_french_on.read() {
        Some(french_labels.read().clone())
    } else {
        None
    };

    rsx! {
        div {
            style: "font-family: sans-serif; padding: 1rem; max-width: 1400px; margin: 0 auto;",

            h1 { "chorale QA Harness" }
            p { "Dataset: {row_count} rows" }

            // ── v0.1 feature toggles ─────────────────────────────────────────
            p {
                style: "margin: 0.25rem 0; font-size: 0.75rem; font-weight: 700; \
                        text-transform: uppercase; color: #6b7280;",
                "v0.1 features"
            }
            div {
                style: "display:flex; gap:1rem; flex-wrap:wrap; margin-bottom:0.5rem; \
                        padding:0.75rem; background:#f5f5f5; border-radius:4px;",

                label {
                    input {
                        r#type: "checkbox",
                        checked: *sort_on.read(),
                        onchange: move |_| { let v = *sort_on.read(); sort_on.set(!v); },
                    }
                    " Sort"
                }
                label {
                    input {
                        r#type: "checkbox",
                        checked: *filter_on.read(),
                        onchange: move |_| { let v = *filter_on.read(); filter_on.set(!v); },
                    }
                    " Filter"
                }
                span {
                    style: "color: #555; font-size: 0.875rem; align-self: center;",
                    "Virtualization: always on"
                }
                label {
                    input {
                        r#type: "checkbox",
                        checked: *selection_on.read(),
                        onchange: move |_| { let v = *selection_on.read(); selection_on.set(!v); },
                    }
                    " Selection"
                }
                label {
                    input {
                        r#type: "checkbox",
                        checked: *col_toolbar_on.read(),
                        onchange: move |_| { let v = *col_toolbar_on.read(); col_toolbar_on.set(!v); },
                    }
                    " Column Visibility"
                }
                label {
                    input {
                        r#type: "checkbox",
                        checked: *csv_export_on.read(),
                        onchange: move |_| { let v = *csv_export_on.read(); csv_export_on.set(!v); },
                    }
                    " CSV Export"
                }
                label {
                    input {
                        r#type: "checkbox",
                        checked: *resize_on.read(),
                        onchange: move |_| { let v = *resize_on.read(); resize_on.set(!v); },
                    }
                    " Column Resize"
                }
                label {
                    style: "display: flex; align-items: center; gap: 0.25rem;",
                    " Page size: "
                    select {
                        onchange: move |e| {
                            if let Ok(n) = e.value().parse::<usize>() {
                                table.set_page_size(n).ok();
                            }
                        },
                        option { value: "10", "10" }
                        option { value: "25", "25" }
                        option { value: "50", selected: true, "50" }
                        option { value: "100", "100" }
                        option { value: "200", "200" }
                    }
                }
            }

            // ── v0.2.0 feature toggles ───────────────────────────────────────
            p {
                style: "margin: 0.25rem 0; font-size: 0.75rem; font-weight: 700; \
                        text-transform: uppercase; color: #6b7280;",
                "v0.2.0 features"
            }
            div {
                style: "display:flex; gap:1rem; flex-wrap:wrap; margin-bottom:1rem; \
                        padding:0.75rem; background:#eff6ff; border-radius:4px; \
                        border: 1px solid #bfdbfe;",

                label {
                    input {
                        r#type: "checkbox",
                        checked: *infinite_scroll_on.read(),
                        onchange: move |_| {
                            let v = *infinite_scroll_on.read();
                            infinite_scroll_on.set(!v);
                        },
                    }
                    " Infinite Scroll"
                }
                label {
                    input {
                        r#type: "checkbox",
                        checked: *labels_french_on.read(),
                        onchange: move |_| {
                            let v = *labels_french_on.read();
                            labels_french_on.set(!v);
                        },
                    }
                    " French Labels"
                }
                label {
                    input {
                        r#type: "checkbox",
                        checked: *variable_height_on.read(),
                        onchange: move |_| {
                            let v = *variable_height_on.read();
                            variable_height_on.set(!v);
                        },
                    }
                    " Variable Row Height"
                }
                label {
                    input {
                        r#type: "checkbox",
                        checked: *editing_on.read(),
                        onchange: move |_| {
                            let v = *editing_on.read();
                            editing_on.set(!v);
                        },
                    }
                    " In-cell Editing (Name)"
                }
                label {
                    input {
                        r#type: "checkbox",
                        checked: *grouping_on.read(),
                        onchange: move |_| {
                            let v = *grouping_on.read();
                            grouping_on.set(!v);
                        },
                    }
                    " Group by Role"
                }
                label {
                    style: "display: flex; align-items: center; gap: 0.25rem;",
                    " Grouped pagination: "
                    select {
                        onchange: move |e| {
                            grouped_pagination_virt.set(e.value() == "virtualized");
                        },
                        option { value: "data_rows_only", "DataRowsOnly" }
                        option { value: "virtualized", "Virtualized" }
                    }
                }
                label {
                    input {
                        r#type: "checkbox",
                        checked: *column_reorder_on.read(),
                        onchange: move |_| {
                            let v = *column_reorder_on.read();
                            column_reorder_on.set(!v);
                        },
                    }
                    " Column Reorder"
                }
                label {
                    input {
                        r#type: "checkbox",
                        checked: *frozen_columns_on.read(),
                        onchange: move |_| {
                            let v = *frozen_columns_on.read();
                            frozen_columns_on.set(!v);
                        },
                    }
                    " Frozen Columns (Name=Left, Salary=Right)"
                }
                label {
                    input {
                        r#type: "checkbox",
                        checked: *selection_toolbar_on.read(),
                        onchange: move |_| {
                            let v = *selection_toolbar_on.read();
                            selection_toolbar_on.set(!v);
                        },
                    }
                    " Selection Toolbar"
                }
                label {
                    input {
                        r#type: "checkbox",
                        checked: *master_detail_on.read(),
                        onchange: move |_| {
                            let v = *master_detail_on.read();
                            master_detail_on.set(!v);
                        },
                    }
                    " Master/Detail (sub-table per row)"
                }
                label {
                    input {
                        r#type: "checkbox",
                        checked: *use_derive_on.read(),
                        onchange: move |_| {
                            let v = *use_derive_on.read();
                            use_derive_on.set(!v);
                        },
                    }
                    " Use #[derive(TableRow)] columns"
                }
            }

            if *selection_on.read() {
                div {
                    style: "margin-bottom: 0.25rem; font-size: 0.875rem; color: #374151; \
                            font-weight: 500;",
                    "Selection: {table.signal().read().selection.len()} row(s)"
                }
            }

            Table {
                handle: table,
                sort_enabled: *sort_on.read(),
                filter_enabled: *filter_on.read(),
                selection_enabled: *selection_on.read(),
                cell_renderers: cell_renderers.read().clone(),
                column_toolbar: *col_toolbar_on.read(),
                csv_export: *csv_export_on.read(),
                resize_enabled: *resize_on.read(),
                variable_row_height: *variable_height_on.read(),
                on_commit_edit: commit_handler,
                selection_toolbar: toolbar_el,
                labels: labels_opt,
                column_reorder_enabled: *column_reorder_on.read(),
                detail_renderer: if *master_detail_on.read() {
                    Some(Callback::new(|employee: Employee| {
                        rsx! { EmployeeDetailPanel { employee } }
                    }))
                } else {
                    None
                },
            }
        }
    }
}

fn main() {
    dioxus::launch(App);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_dataset_returns_correct_count() {
        let rows = generate_dataset();
        assert_eq!(rows.len(), ROW_COUNT);
    }

    #[test]
    fn generate_dataset_is_deterministic() {
        let rows1 = generate_dataset();
        let rows2 = generate_dataset();
        assert_eq!(rows1[0].1.name, rows2[0].1.name);
        assert_eq!(rows1[0].1.salary, rows2[0].1.salary);
        assert_eq!(rows1[ROW_COUNT - 1].1.email, rows2[ROW_COUNT - 1].1.email);
    }

    #[test]
    fn build_columns_definition_has_six_columns() {
        let cols = build_columns(false, false);
        assert_eq!(cols.len(), 6);
    }

    #[test]
    fn build_columns_editing_adds_editor_to_name() {
        let cols = build_columns(true, false);
        let Some(name_col) = cols.iter().find(|c| c.id == ColumnId("name")) else {
            panic!("name column not found in build_columns result");
        };
        assert!(
            name_col.editor.is_some(),
            "editing=true must set editor on name column"
        );
    }

    #[test]
    fn build_columns_frozen_pins_name_left_salary_right() {
        let cols = build_columns(false, true);
        let Some(name_col) = cols.iter().find(|c| c.id == ColumnId("name")) else {
            panic!("name column not found in build_columns result");
        };
        let Some(salary_col) = cols.iter().find(|c| c.id == ColumnId("salary")) else {
            panic!("salary column not found in build_columns result");
        };
        assert_eq!(name_col.frozen, FrozenSide::Left);
        assert_eq!(salary_col.frozen, FrozenSide::Right);
    }

    #[test]
    fn employee_derive_generates_six_columns() {
        let cols = Employee::chorale_columns();
        assert_eq!(cols.len(), 6);
    }
}
