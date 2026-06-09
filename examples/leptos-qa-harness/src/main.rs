//! QA harness for chorale-leptos.
//!
//! Run with: `trunk serve --open --package leptos-qa-harness`
//!
//! Generates a reproducible 10k-row Employee dataset and renders a page
//! scaffold with feature-toggle controls. Each v0.1 and v0.2.0 work-queue
//! item wires one toggle as the matching adapter feature lands.

use chorale_core::{
    AggregatorKind, Alignment, BadgeVariant, BadgeVariantMap, CellValue, ColumnDef, ColumnId,
    CommittedEdit, CurrencyCode, EditorKind, FilterKind, FrozenSide, GroupedPaginationMode, Labels,
    NaiveDate, PaginationMode, RenderKind,
};
use chorale_derive::TableRow;
use chorale_leptos::{use_chorale_table, CellRenderer, CellRenderers, Table};
use leptos::prelude::*;
use rand::{rngs::StdRng, Rng, SeedableRng};
use std::collections::HashMap;
use std::sync::Arc;

// Fixed seed guarantees the same dataset on every run.
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

fn generate_dataset() -> Vec<Employee> {
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
            Employee {
                name,
                email,
                joined_date,
                role,
                status,
                salary,
            }
        })
        .collect()
}

/// Build the hand-crafted column set. Always includes `AggregatorKind::Sum`
/// on salary (shown only when grouping is active). Editing and frozen-column
/// features are opt-in via the toggles.
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

    vec![
        name_col, email_col, joined_col, role_col, status_col, salary_col,
    ]
}

fn make_status_renderer() -> CellRenderer {
    Arc::new(|val: &CellValue| {
        let text = if let CellValue::Text(s) = val {
            s.clone()
        } else {
            String::new()
        };
        let (prefix, color) = match text.as_str() {
            "Active" => ("● ", "#065f46"),
            "Inactive" => ("○ ", "#374151"),
            "Pending" => ("◑ ", "#92400e"),
            "Suspended" => ("✕ ", "#991b1b"),
            _ => ("", "#333"),
        };
        let label = format!("{prefix}{text}");
        view! {
            <span style=format!("color: {color}; font-weight: 500; font-size: 0.875rem;")>
                {label}
            </span>
        }
        .into_any()
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
        view! {
            <div style="padding: 2px 0;">
                <div style="font-weight: 500;">{name.clone()}</div>
                {(extra >= 1).then(|| view! {
                    <div style="font-size: 0.72rem; color: #6b7280; margin-top: 1px;">
                        "▸ note A: joined employee record"
                    </div>
                })}
                {(extra >= 2).then(|| view! {
                    <div style="font-size: 0.72rem; color: #6b7280; margin-top: 1px;">
                        "▸ note B: pending review cycle"
                    </div>
                })}
            </div>
        }
        .into_any()
    })
}

#[component]
fn App() -> impl IntoView {
    let table = use_chorale_table(generate_dataset(), build_columns(false, false));
    let row_count = table.signal().with_untracked(|s| s.rows.len());

    // ── v0.1 toggles ────────────────────────────────────────────────────────
    let sort_on = RwSignal::new(false);
    let filter_on = RwSignal::new(false);
    let selection_on = RwSignal::new(false);
    let col_toolbar_on = RwSignal::new(false);
    let csv_export_on = RwSignal::new(false);
    let resize_on = RwSignal::new(false);

    // ── v0.2.0 toggles ──────────────────────────────────────────────────────
    let infinite_scroll_on = RwSignal::new(false);
    let labels_french_on = RwSignal::new(false);
    let variable_height_on = RwSignal::new(false);
    let editing_on = RwSignal::new(false);
    let grouping_on = RwSignal::new(false);
    let grouped_pagination_virt = RwSignal::new(false);
    let column_reorder_on = RwSignal::new(false);
    let frozen_columns_on = RwSignal::new(false);
    let selection_toolbar_on = RwSignal::new(false);
    let use_derive_on = RwSignal::new(false);
    let xlsx_export_on = RwSignal::new(false);

    // ── Cell renderers (rebuilt when variable_height_on changes) ─────────────
    let cell_renderers = Memo::new(move |_| {
        let mut m = HashMap::new();
        m.insert(ColumnId("status"), make_status_renderer());
        if variable_height_on.get() {
            m.insert(ColumnId("name"), make_variable_height_renderer());
        }
        CellRenderers::new(m)
    });

    // ── French labels ─────────────────────────────────────────────────────────
    let french_labels = {
        let mut l = Labels::default();
        l.filter_placeholder = "Filtrer\u{2026}".into();
        l.export_csv_label = "Exporter CSV".into();
        l.previous_page_label = "\u{2039} Pr\u{e9}c".into();
        l.next_page_label = "Suiv \u{203a}".into();
        l.go_to_page_label = "Aller \u{e0}".into();
        l.no_rows_label = "Aucune ligne ne correspond au filtre.".into();
        l.load_more_label = "Charger plus\u{2026}".into();
        l
    };

    // ── Effect: rebuild columns when editing / frozen / derive toggles change ─
    Effect::new(move |_| {
        let cols = if use_derive_on.get() {
            Employee::chorale_columns()
        } else {
            build_columns(editing_on.get(), frozen_columns_on.get())
        };
        table.signal().update(|s| s.columns = cols);
    });

    // ── Effect: pagination mode ───────────────────────────────────────────────
    Effect::new(move |_| {
        if infinite_scroll_on.get() {
            table.set_pagination_mode(PaginationMode::InfiniteScroll);
        } else {
            table.set_pagination_mode(PaginationMode::Pages);
        }
    });

    // ── Effect: grouping ─────────────────────────────────────────────────────
    Effect::new(move |_| {
        if grouping_on.get() {
            table.set_grouping(vec![ColumnId("role")]);
        } else {
            table.set_grouping(vec![]);
        }
    });

    // ── Effect: grouped pagination mode ──────────────────────────────────────
    Effect::new(move |_| {
        let mode = if grouped_pagination_virt.get() {
            GroupedPaginationMode::Virtualized
        } else {
            GroupedPaginationMode::DataRowsOnly
        };
        table.signal().update(|s| s.grouped_pagination = mode);
    });

    view! {
        <div style="font-family: sans-serif; padding: 1rem; max-width: 1400px; margin: 0 auto;">
            <h1>"chorale QA Harness (Leptos)"</h1>
            <p>"Dataset: "{row_count}" rows"</p>

            // ── Feature toggles ──────────────────────────────────────────────
            //
            // chorale-leptos is new in v0.2.0; every feature exposed here
            // ships with that release. The two visual groups below mirror
            // the original v0.1-vs-v0.2.0 split from the Dioxus harness
            // (where the Dioxus adapter spanned both versions), kept for
            // organizational parity but both labelled v0.2.0 here.
            <p style="margin: 0.25rem 0; font-size: 0.75rem; font-weight: 700; text-transform: uppercase; color: #6b7280;">
                "v0.2.0 features"
            </p>
            <div style="display:flex; gap:1rem; flex-wrap:wrap; margin-bottom:0.5rem; padding:0.75rem; background:#eff6ff; border-radius:4px; border: 1px solid #bfdbfe;">
                <label>
                    <input
                        type="checkbox"
                        prop:checked=move || sort_on.get()
                        on:change=move |_| sort_on.update(|v| *v = !*v)
                    />
                    " Sort"
                </label>
                <label>
                    <input
                        type="checkbox"
                        prop:checked=move || filter_on.get()
                        on:change=move |_| filter_on.update(|v| *v = !*v)
                    />
                    " Filter"
                </label>
                <span style="color: #555; font-size: 0.875rem; align-self: center;">
                    "Virtualization: always on"
                </span>
                <label>
                    <input
                        type="checkbox"
                        prop:checked=move || selection_on.get()
                        on:change=move |_| selection_on.update(|v| *v = !*v)
                    />
                    " Selection"
                </label>
                <label>
                    <input
                        type="checkbox"
                        prop:checked=move || col_toolbar_on.get()
                        on:change=move |_| col_toolbar_on.update(|v| *v = !*v)
                    />
                    " Column Visibility"
                </label>
                <label>
                    <input
                        type="checkbox"
                        prop:checked=move || csv_export_on.get()
                        on:change=move |_| csv_export_on.update(|v| *v = !*v)
                    />
                    " CSV Export"
                </label>
                <label>
                    <input
                        type="checkbox"
                        prop:checked=move || resize_on.get()
                        on:change=move |_| resize_on.update(|v| *v = !*v)
                    />
                    " Column Resize"
                </label>
                <label style="display: flex; align-items: center; gap: 0.25rem;">
                    " Page size: "
                    <select on:change=move |e| {
                        let val = event_target_value(&e);
                        if let Ok(n) = val.parse::<usize>() {
                            table.set_page_size(n).ok();
                        }
                    }>
                        <option value="10">"10"</option>
                        <option value="25">"25"</option>
                        <option value="50" selected=true>"50"</option>
                        <option value="100">"100"</option>
                        <option value="200">"200"</option>
                    </select>
                </label>
            </div>

            <div style="display:flex; gap:1rem; flex-wrap:wrap; margin-bottom:1rem; padding:0.75rem; background:#eff6ff; border-radius:4px; border: 1px solid #bfdbfe;">
                <label>
                    <input
                        type="checkbox"
                        prop:checked=move || infinite_scroll_on.get()
                        on:change=move |_| infinite_scroll_on.update(|v| *v = !*v)
                    />
                    " Infinite Scroll"
                </label>
                <label>
                    <input
                        type="checkbox"
                        prop:checked=move || labels_french_on.get()
                        on:change=move |_| labels_french_on.update(|v| *v = !*v)
                    />
                    " French Labels"
                </label>
                <label>
                    <input
                        type="checkbox"
                        prop:checked=move || variable_height_on.get()
                        on:change=move |_| variable_height_on.update(|v| *v = !*v)
                    />
                    " Variable Row Height (renderer only)"
                </label>
                <label>
                    <input
                        type="checkbox"
                        prop:checked=move || editing_on.get()
                        on:change=move |_| editing_on.update(|v| *v = !*v)
                    />
                    " In-cell Editing (Name)"
                </label>
                <label>
                    <input
                        type="checkbox"
                        prop:checked=move || grouping_on.get()
                        on:change=move |_| grouping_on.update(|v| *v = !*v)
                    />
                    " Group by Role"
                </label>
                <label style="display: flex; align-items: center; gap: 0.25rem;">
                    " Grouped pagination: "
                    <select on:change=move |e| {
                        let val = event_target_value(&e);
                        grouped_pagination_virt.set(val == "virtualized");
                    }>
                        <option value="data_rows_only">"DataRowsOnly"</option>
                        <option value="virtualized">"Virtualized"</option>
                    </select>
                </label>
                <label>
                    <input
                        type="checkbox"
                        prop:checked=move || column_reorder_on.get()
                        on:change=move |_| column_reorder_on.update(|v| *v = !*v)
                    />
                    " Column Reorder"
                </label>
                <label>
                    <input
                        type="checkbox"
                        prop:checked=move || frozen_columns_on.get()
                        on:change=move |_| frozen_columns_on.update(|v| *v = !*v)
                    />
                    " Frozen Columns (Name=Left, Salary=Right)"
                </label>
                <label>
                    <input
                        type="checkbox"
                        prop:checked=move || selection_toolbar_on.get()
                        on:change=move |_| selection_toolbar_on.update(|v| *v = !*v)
                    />
                    " Selection Toolbar"
                </label>
                <label>
                    <input
                        type="checkbox"
                        prop:checked=move || use_derive_on.get()
                        on:change=move |_| use_derive_on.update(|v| *v = !*v)
                    />
                    " Use #[derive(TableRow)] columns"
                </label>
                <label>
                    <input
                        type="checkbox"
                        prop:checked=move || xlsx_export_on.get()
                        on:change=move |_| xlsx_export_on.update(|v| *v = !*v)
                    />
                    " Excel Export"
                </label>
            </div>

            {move || {
                let sort = sort_on.get();
                let filter = filter_on.get();
                let selection = selection_on.get();
                let toolbar = col_toolbar_on.get();
                let csv = csv_export_on.get();
                let resize = resize_on.get();
                let col_reorder = column_reorder_on.get();
                let xlsx = xlsx_export_on.get();
                let renderers = cell_renderers.get();

                // Labels: always pass a concrete value (default or French).
                let labels_val: Labels = if labels_french_on.get() {
                    french_labels.clone()
                } else {
                    Labels::default()
                };

                // on_commit_edit: pass Option<Callback<_>> directly.
                let commit_cb: Option<Callback<CommittedEdit<Employee>>> = if editing_on.get() {
                    Some(Callback::new(move |edit: CommittedEdit<Employee>| {
                        let current_row = table.signal().with(|s| {
                            s.rows.iter()
                                .find(|(id, _)| *id == edit.row_id)
                                .map(|(_, r)| r.clone())
                        });
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

                // ChildrenFn = Arc<dyn Fn() -> AnyView + Send + Sync>.
                let toolbar_fn: ChildrenFn = Arc::new(move || {
                    view! {
                        {move || {
                            selection_toolbar_on.get().then(|| {
                                let count = table.signal().with(|s| s.selection.len());
                                view! {
                                    <div style="display: flex; align-items: center; gap: 1rem; \
                                                padding: 0.75rem 1rem; background: #1d4ed8; \
                                                color: white; font-size: 0.875rem; font-weight: 600; \
                                                width: 100%; box-sizing: border-box; flex-wrap: wrap;">
                                        <span>{count}" row(s) selected"</span>
                                        <div class="chorale-bulk-actions" style="display: flex; gap: 8px;">
                                            <button
                                                on:click=move |_| table.select_all_visible_page()
                                                style="padding: 0.25rem 0.75rem; \
                                                       background: rgba(255,255,255,0.2); \
                                                       color: white; \
                                                       border: 1px solid rgba(255,255,255,0.4); \
                                                       border-radius: 3px; cursor: pointer; \
                                                       font-size: 0.8rem;">
                                                "Select page"
                                            </button>
                                            <button
                                                on:click=move |_| table.select_all_filtered()
                                                style="padding: 0.25rem 0.75rem; \
                                                       background: rgba(255,255,255,0.2); \
                                                       color: white; \
                                                       border: 1px solid rgba(255,255,255,0.4); \
                                                       border-radius: 3px; cursor: pointer; \
                                                       font-size: 0.8rem;">
                                                "Select all"
                                            </button>
                                            <button
                                                on:click=move |_| table.deselect_all_visible_page()
                                                style="padding: 0.25rem 0.75rem; \
                                                       background: rgba(255,255,255,0.2); \
                                                       color: white; \
                                                       border: 1px solid rgba(255,255,255,0.4); \
                                                       border-radius: 3px; cursor: pointer; \
                                                       font-size: 0.8rem;">
                                                "Deselect page"
                                            </button>
                                            <button
                                                on:click=move |_| table.deselect_all()
                                                style="padding: 0.25rem 0.75rem; \
                                                       background: rgba(255,255,255,0.2); \
                                                       color: white; \
                                                       border: 1px solid rgba(255,255,255,0.4); \
                                                       border-radius: 3px; cursor: pointer; \
                                                       font-size: 0.8rem;">
                                                "Deselect all"
                                            </button>
                                        </div>
                                    </div>
                                }
                            })
                        }}
                    }
                    .into_any()
                });

                view! {
                    {move || selection_on.get().then(|| {
                        let count = table.signal().with(|s| s.selection.len());
                        view! {
                            <div style="margin-bottom: 0.25rem; font-size: 0.875rem; \
                                        color: #374151; font-weight: 500;">
                                "Selection: "{count}" row(s)"
                            </div>
                        }
                    })}
                    <Table
                        handle=table
                        sort_enabled=sort
                        filter_enabled=filter
                        selection_enabled=selection
                        cell_renderers=renderers
                        column_toolbar=toolbar
                        csv_export=csv
                        xlsx_export=xlsx
                        resize_enabled=resize
                        column_reorder_enabled=col_reorder
                        labels=labels_val
                        on_commit_edit=commit_cb
                        selection_toolbar=toolbar_fn
                    />
                }
            }}
        </div>
    }
}

fn main() {
    leptos::mount::mount_to_body(App);
}
