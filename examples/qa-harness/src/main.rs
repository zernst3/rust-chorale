//! QA harness for chorale-dioxus.
//!
//! Run with: `dx serve --package qa-harness` (requires `cargo install dioxus-cli`)
//!
//! Generates a reproducible 10k-row Employee dataset and renders a page
//! scaffold with feature-toggle controls. Each v0.1 and v0.2.0 work-queue
//! item wires one toggle as the matching adapter feature lands.

use chorale_core::{
    AggregatorKind, Alignment, BadgeVariant, BadgeVariantMap, CellValue, ColumnDef, ColumnId,
    CommittedEdit, CurrencyCode, EditorKind, FilterKind, FrozenSide, GroupedPaginationMode,
    Labels, PaginationMode, RenderKind, RowId, TableState,
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

    vec![name_col, email_col, joined_col, role_col, status_col, salary_col]
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
    let mut use_derive_on = use_signal(|| false);

    // ── Status cell renderer (stable across renders) ─────────────────────────
    let status_renderers = use_memo(|| {
        let mut m = HashMap::new();
        m.insert(ColumnId("status"), make_status_renderer());
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
    let commit_handler: Option<EventHandler<CommittedEdit<Employee>>> =
        if *editing_on.read() {
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
                        row.name = edit.value.clone();
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
                style: "padding: 0.5rem 1rem; background: #1d4ed8; color: white; \
                        border-radius: 4px; font-size: 0.875rem; font-weight: 600;",
                "{count} row(s) selected"
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
                        checked: *use_derive_on.read(),
                        onchange: move |_| {
                            let v = *use_derive_on.read();
                            use_derive_on.set(!v);
                        },
                    }
                    " Use #[derive(TableRow)] columns"
                }
            }

            Table {
                handle: table,
                sort_enabled: *sort_on.read(),
                filter_enabled: *filter_on.read(),
                selection_enabled: *selection_on.read(),
                cell_renderers: status_renderers.read().clone(),
                column_toolbar: *col_toolbar_on.read(),
                csv_export: *csv_export_on.read(),
                resize_enabled: *resize_on.read(),
                variable_row_height: *variable_height_on.read(),
                on_commit_edit: commit_handler,
                selection_toolbar: toolbar_el,
                labels: labels_opt,
                column_reorder_enabled: *column_reorder_on.read(),
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
        let name_col = cols.iter().find(|c| c.id == ColumnId("name")).unwrap();
        assert!(name_col.editor.is_some(), "editing=true must set editor on name column");
    }

    #[test]
    fn build_columns_frozen_pins_name_left_salary_right() {
        let cols = build_columns(false, true);
        let name_col = cols.iter().find(|c| c.id == ColumnId("name")).unwrap();
        let salary_col = cols.iter().find(|c| c.id == ColumnId("salary")).unwrap();
        assert_eq!(name_col.frozen, FrozenSide::Left);
        assert_eq!(salary_col.frozen, FrozenSide::Right);
    }

    #[test]
    fn employee_derive_generates_six_columns() {
        let cols = Employee::chorale_columns();
        assert_eq!(cols.len(), 6);
    }
}
