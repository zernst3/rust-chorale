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
    NaiveDate, PaginationMode, RenderKind, RowId, Theme,
};
use chorale_derive::TableRow;
use chorale_leptos::{
    use_chorale_table, CellRenderer, CellRenderers, DetailRenderer, RowCellRenderer,
    RowCellRenderers, Table,
};
use leptos::prelude::{StoredValue, *};
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
    #[chorale(header = "Joined")]
    joined_date: NaiveDate,
    #[chorale(filter = "MultiSelect")]
    role: String,
    #[chorale(filter = "MultiSelect")]
    status: String,
    #[chorale(render = "currency")]
    salary: i64,
}

// ── Master/detail demo: per-employee order line items ────────────────────────
//
// Each row in the main table represents an Employee. When the master/detail
// toggle is on, clicking a row's expand chevron reveals a child `<Table>`
// showing that employee's order line items (qty + unit price). Demonstrates
// that `detail_renderer` can mount any `AnyView`, including a nested
// chorale-leptos Table with its own state, sorting, and rendering rules.

#[derive(Clone, PartialEq)]
struct LineItem {
    label: &'static str,
    qty: i64,
    unit_price: f64,
}

static LINE_ITEM_LABELS: &[&str] = &["Widget A", "Widget B", "Gadget C", "Gizmo D", "Doohickey E"];

// Deterministic per-employee line items, seeded from the employee's email
// so the dataset is stable across re-renders. (The Dioxus harness pairs each
// item with a RowId here; in Leptos, `use_chorale_table` assigns RowIds
// itself, so this returns bare rows.)
#[must_use]
fn line_items_for_employee(email: &str) -> Vec<LineItem> {
    let mut seed: u64 = 0xCBF2_9CE4_8422_2325; // FNV-1a offset basis
    for b in email.bytes() {
        seed ^= u64::from(b);
        seed = seed.wrapping_mul(0x100_0000_01B3);
    }
    let mut rng = StdRng::seed_from_u64(seed);
    let count = rng.gen_range(2..6);
    (0..count)
        .map(|_| LineItem {
            label: LINE_ITEM_LABELS[rng.gen_range(0..LINE_ITEM_LABELS.len())],
            qty: rng.gen_range(1..20),
            unit_price: f64::from(rng.gen_range(500..50_000)) / 100.0,
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
fn EmployeeDetailPanel(employee: Employee) -> impl IntoView {
    let items = line_items_for_employee(&employee.email);
    let item_count = items.len();
    let total: f64 = items
        .iter()
        .map(|li| f64::from(li.qty as i32) * li.unit_price)
        .sum();
    // The child <Table> renders with `inline=true` (see the prop below),
    // which makes it render at natural height with no internal scroll
    // container. That's what's needed for a child table embedded inside a
    // parent's scrolling viewport: no nested scroll context, no wheel
    // hand-off discontinuity. Page-size is set to item_count so the child
    // never paginates either. update_untracked is safe here: the signal was
    // just created and has no subscribers yet.
    let detail_table = use_chorale_table(items, line_item_columns());
    detail_table
        .signal()
        .update_untracked(|s| s.page_size = item_count.max(1));
    let name = employee.name.clone();
    let summary = format!("{item_count} item(s) — Total: ${total:.2}");
    view! {
        <div style="padding:12px 24px;background:var(--chorale-toolbar-bg, #fafafa);border-top:1px solid var(--chorale-border, #e5e7eb);">
            <div style="font-size:0.75rem;font-weight:600;color:var(--chorale-text-muted, #6b7280);\
                        margin-bottom:8px;display:flex;\
                        justify-content:space-between;align-items:baseline;">
                <span>"ORDER LINE ITEMS — "{name}</span>
                <span style="font-weight:500;color:var(--chorale-text, #374151);">{summary}</span>
            </div>
            // inline=true → no internal scroll container, no virtualization;
            // child renders at natural height so the parent's scroll context
            // owns wheel events end-to-end. on_commit_edit is a required
            // Option prop on the Leptos Table; the read-only child passes None.
            // Theme::Custom => data-chorale-theme="custom" matches no built-in
            // block, so this nested table inherits the parent's themed
            // --chorale-* variables via CSS cascade instead of forcing light.
            <Table handle=detail_table sort_enabled=true inline=true theme=Theme::Custom on_commit_edit=None />
        </div>
    }
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

/// One labelled employee for the row-mutation controls (issue #25), so
/// inserted/appended rows are visually distinct from the seeded dataset.
fn make_employee(n: usize) -> Employee {
    let base = NaiveDate::from_ymd_opt(2020, 1, 1).unwrap_or(NaiveDate::MIN);
    Employee {
        name: format!("New Row {n}"),
        email: format!("new.row{n}@example.com"),
        joined_date: base,
        role: "Analyst".into(),
        status: "Active".into(),
        salary: 100_000 + i64::try_from(n % 50).unwrap_or(0) * 1_000,
    }
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

    let mut role_col = ColumnDef::new(ColumnId("role"), "Role", |r: &Employee| {
        CellValue::Text(r.role.clone())
    })
    .sortable()
    .filter(FilterKind::MultiSelect {
        options: ROLES.iter().map(|s| (*s).to_string()).collect(),
    })
    .initial_width(140.0);
    // Select editor demo: when editing is on, Role becomes a dropdown
    // constrained to ROLES (EditorKind::Select).
    if editing {
        role_col = role_col.editor(EditorKind::Select {
            options: ROLES.iter().map(|s| (*s).to_string()).collect(),
        });
    }

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

fn fmt_thousands(n: i64) -> String {
    let s = n.abs().to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.insert(0, ',');
        }
        out.insert(0, c);
    }
    if n < 0 {
        out.insert(0, '-');
    }
    out
}

fn make_salary_renderer() -> CellRenderer {
    Arc::new(|val: &CellValue| {
        let formatted = if let CellValue::Integer(n) = val {
            format!("${}", fmt_thousands(*n))
        } else {
            String::new()
        };
        view! { <span>{formatted}</span> }.into_any()
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

/// Row-aware composite cell for the "name" column: name with the row's
/// email underneath. Exercises `RowCellRenderer`'s access to sibling fields.
fn make_name_email_renderer() -> RowCellRenderer<Employee> {
    Arc::new(|emp: &Employee, _val: &CellValue| {
        let name = emp.name.clone();
        let email = emp.email.clone();
        view! {
            <div style="line-height:1.2;">
                <div style="font-weight:600;">{name}</div>
                <div style="font-size:0.72rem;color:#6b7280;">{email}</div>
            </div>
        }
        .into_any()
    })
}

#[component]
fn App() -> impl IntoView {
    // Generate once; keep a copy in a StoredValue so the derive-mode effect
    // can pass it to chorale_columns_with_rows without re-generating.
    let dataset = generate_dataset();
    let stored_dataset = StoredValue::new(dataset.clone());
    let table = use_chorale_table(dataset, build_columns(false, false));
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
    let master_detail_on = RwSignal::new(false);
    let use_derive_on = RwSignal::new(false);
    let xlsx_export_on = RwSignal::new(false);
    let row_renderers_on = RwSignal::new(false);
    let row_click_on = RwSignal::new(false);
    let dark_mode_on = RwSignal::new(false);
    let last_clicked: RwSignal<Option<String>> = RwSignal::new(None);
    // Row-set mutation controls (issue #25): a monotonic counter labels each
    // inserted/appended row uniquely.
    let mut_counter = RwSignal::new(0usize);

    // ── Cell renderers (rebuilt when variable_height_on / use_derive_on changes) ─
    // In derive mode, the macro emits RenderKind::Currency on salary, so the
    // adapter's built-in currency renderer handles formatting. Injecting a
    // custom salary cell_renderer on top would mask it; omit it in derive mode
    // so both harnesses look identical when the derive toggle is on.
    let cell_renderers = Memo::new(move |_| {
        let mut m = HashMap::new();
        m.insert(ColumnId("status"), make_status_renderer());
        if !use_derive_on.get() {
            m.insert(ColumnId("salary"), make_salary_renderer());
        }
        if variable_height_on.get() {
            m.insert(ColumnId("name"), make_variable_height_renderer());
        }
        CellRenderers::new(m)
    });

    let row_cell_renderers_memo = Memo::new(move |_| {
        let mut m: HashMap<ColumnId, RowCellRenderer<Employee>> = HashMap::new();
        if row_renderers_on.get() {
            m.insert(ColumnId("name"), make_name_email_renderer());
        }
        RowCellRenderers::new(m)
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
            // Pass the stored dataset so numeric bounds and MultiSelect options
            // are derived from real data (salary: ~40k-200k, role/status distinct
            // values). stored_dataset.get_value() clones the Vec once per toggle.
            stored_dataset.with_value(|ds| Employee::chorale_columns_with_rows(ds))
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
        // Page background follows the theme (faint off-white in light, very dark
        // blue/purple in dark) so the whole viewport stays a touch off from the
        // table surface for contrast. Targets <body> via a reactive <style>.
        <style inner_html=move || {
            if dark_mode_on.get() { "body{background:#14121e;}" } else { "body{background:#f5f5f7;}" }
        }></style>
        <div style="font-family: sans-serif; padding: 1rem; max-width: 1400px; margin: 0 auto;">
            <h1 style=move || if dark_mode_on.get() { "color:#e6e6e6;" } else { "color:#1a1a1a;" }>
                "Chorale QA Harness (Leptos)"
            </h1>
            <p style=move || if dark_mode_on.get() { "color:#c8c8c8;" } else { "color:#444;" }>
                "Dataset: "{move || table.signal().with(|s| s.rows.len())}" rows"
                " (seeded "{row_count}")"
            </p>

            // ── Row-set mutation controls (v0.2.2, issue #25) ────────────────
            <p style="margin: 0.25rem 0; font-size: 0.75rem; font-weight: 700; text-transform: uppercase; color: #6b7280;">
                "Row mutation (v0.2.2)"
            </p>
            <div style="display:flex; gap:0.5rem; flex-wrap:wrap; margin-bottom:0.5rem; padding:0.75rem; background:#f0fdf4; border-radius:4px; border: 1px solid #86efac;">
                <button on:click=move |_| {
                    let n = mut_counter.get(); mut_counter.set(n + 1);
                    table.append_rows(vec![(RowId::new(), make_employee(n))]);
                }>"Append row"</button>
                <button on:click=move |_| {
                    let n = mut_counter.get(); mut_counter.set(n + 1);
                    table.insert_row(0, RowId::new(), make_employee(n));
                }>"Insert at top"</button>
                <button on:click=move |_| {
                    let ids = table.selected_ids();
                    if !ids.is_empty() { table.remove_rows(&ids); }
                }>"Remove selected"</button>
                <button on:click=move |_| {
                    let rows = stored_dataset
                        .get_value()
                        .into_iter()
                        .map(|e| (RowId::new(), e))
                        .collect();
                    table.set_rows(rows);
                }>"Reset dataset (set_rows)"</button>
            </div>

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
                        prop:checked=move || dark_mode_on.get()
                        on:change=move |_| dark_mode_on.update(|v| *v = !*v)
                    />
                    " Dark mode"
                </label>
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
                    " Variable Row Height"
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
                    " Grouping & Aggregation"
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
                    " Frozen Columns and Rows (Name=Left, Salary=Right, header sticky)"
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
                        prop:checked=move || master_detail_on.get()
                        on:change=move |_| master_detail_on.update(|v| *v = !*v)
                    />
                    " Master/Detail (sub-table per row)"
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
                <label>
                    <input
                        type="checkbox"
                        prop:checked=move || row_renderers_on.get()
                        on:change=move |_| row_renderers_on.update(|v| *v = !*v)
                    />
                    " Row-aware name+email cell"
                </label>
                <label>
                    <input
                        type="checkbox"
                        prop:checked=move || row_click_on.get()
                        on:change=move |_| row_click_on.update(|v| *v = !*v)
                    />
                    " on_row_click (last-clicked readout)"
                </label>
            </div>

            {move || {
                let sort = sort_on.get();
                // Reading dark_mode_on here makes this block (and the Table it
                // renders) re-run when the toggle flips, so the plain
                // `theme: Theme` prop re-applies live.
                let theme = if dark_mode_on.get() { Theme::Dark } else { Theme::Light };
                let filter = filter_on.get();
                let selection = selection_on.get();
                let toolbar = col_toolbar_on.get();
                let csv = csv_export_on.get();
                let resize = resize_on.get();
                let col_reorder = column_reorder_on.get();
                let xlsx = xlsx_export_on.get();
                // Drives the Table's variable_row_height prop (real VIRT-2
                // measurement + variable windowing). The toggle also swaps in
                // the multi-line "name" cell renderer (see cell_renderers
                // memo) so rows actually vary in height when it's on.
                let variable_height = variable_height_on.get();
                let renderers = cell_renderers.get();
                let row_renderers_val = row_cell_renderers_memo.get();
                // Always provide a Callback; it checks row_click_on at call
                // time so disabling the toggle silently no-ops without
                // requiring a conditional prop (which Leptos's typed-builder
                // does not support inside view! blocks).
                let row_click_cb: Callback<RowId> = Callback::new(move |rid: RowId| {
                    if !row_click_on.get_untracked() {
                        return;
                    }
                    let name = table.signal().with_untracked(|s| {
                        s.rows
                            .iter()
                            .find(|(id, _)| *id == rid)
                            .map(|(_, r)| r.name.clone())
                    });
                    last_clicked.set(Some(name.unwrap_or_else(|| format!("{rid:?}"))));
                });

                // Labels: always pass a concrete value (default or French).
                let labels_val: Labels = if labels_french_on.get() {
                    french_labels.clone()
                } else {
                    Labels::default()
                };

                // Master/detail: build the per-row detail renderer only when
                // the toggle is on. The renderer mounts a nested chorale
                // <Table> (inline mode) per expanded row.
                let detail_renderer_val: Option<DetailRenderer<Employee>> =
                    master_detail_on.get().then(|| {
                        let renderer: DetailRenderer<Employee> =
                            Arc::new(move |employee: Employee| {
                                view! { <EmployeeDetailPanel employee /> }.into_any()
                            });
                        renderer
                    });

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
                            } else if edit.column_id == ColumnId("role") {
                                row.role.clone_from(&edit.value);
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
                                // Toolbar colors follow the theme: pale blue in light,
                                // a medium dark blue (some contrast, not black) in dark.
                                let (tb_bg, tb_border, tb_text, tb_btn_bg, tb_btn_border) =
                                    if dark_mode_on.get() {
                                        ("#21324d", "#3a5680", "#d6e4ff", "#2e466b", "#3f5d85")
                                    } else {
                                        ("#eff6ff", "#bfdbfe", "#1e3a8a", "#dbeafe", "#93c5fd")
                                    };
                                view! {
                                    <div style=format!("display: flex; align-items: center; gap: 1rem; \
                                                padding: 0.75rem 1rem; background: {tb_bg}; \
                                                border: 1px solid {tb_border}; \
                                                color: {tb_text}; font-size: 0.875rem; font-weight: 600; \
                                                width: 100%; box-sizing: border-box; flex-wrap: wrap;")>
                                        <span>{count}" row(s) selected"</span>
                                        <div class="chorale-bulk-actions" style="display: flex; gap: 8px;">
                                            <button
                                                on:click=move |_| table.select_all_visible_page()
                                                style=format!("padding: 0.25rem 0.75rem; \
                                                       background: {tb_btn_bg}; \
                                                       color: {tb_text}; \
                                                       border: 1px solid {tb_btn_border}; \
                                                       border-radius: 3px; cursor: pointer; \
                                                       font-size: 0.8rem;")>
                                                "Select page"
                                            </button>
                                            <button
                                                on:click=move |_| table.select_all_filtered()
                                                style=format!("padding: 0.25rem 0.75rem; \
                                                       background: {tb_btn_bg}; \
                                                       color: {tb_text}; \
                                                       border: 1px solid {tb_btn_border}; \
                                                       border-radius: 3px; cursor: pointer; \
                                                       font-size: 0.8rem;")>
                                                "Select all"
                                            </button>
                                            <button
                                                on:click=move |_| table.deselect_all_visible_page()
                                                style=format!("padding: 0.25rem 0.75rem; \
                                                       background: {tb_btn_bg}; \
                                                       color: {tb_text}; \
                                                       border: 1px solid {tb_btn_border}; \
                                                       border-radius: 3px; cursor: pointer; \
                                                       font-size: 0.8rem;")>
                                                "Deselect page"
                                            </button>
                                            <button
                                                on:click=move |_| table.deselect_all()
                                                style=format!("padding: 0.25rem 0.75rem; \
                                                       background: {tb_btn_bg}; \
                                                       color: {tb_text}; \
                                                       border: 1px solid {tb_btn_border}; \
                                                       border-radius: 3px; cursor: pointer; \
                                                       font-size: 0.8rem;")>
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
                    {move || row_click_on.get().then(|| {
                        let txt = last_clicked.get().unwrap_or_else(|| String::from("(none yet)"));
                        view! {
                            <div style="margin-bottom: 0.25rem; font-size: 0.875rem; \
                                        color: #374151; font-weight: 500;">
                                "Last clicked row: "{txt}
                            </div>
                        }
                    })}
                    {move || master_detail_on.get().then(|| view! {
                        <div style="margin-bottom: 0.5rem; padding: 0.5rem 0.75rem; \
                                    font-size: 0.8rem; line-height: 1.55; color: #374151; \
                                    background: #eef2ff; border: 1px solid #c7d2fe; \
                                    border-radius: 4px;">
                            <div style="font-weight: 600; margin-bottom: 0.25rem;">
                                "Master/detail keyboard navigation"
                            </div>
                            <div>"Press ▶ "<b>"←"</b>" onto the chevron column, then "<b>"Enter"</b>" to expand or collapse the row."</div>
                            <div>"With the chevron highlighted on an expanded row, press "<b>"Tab"</b>" to enter its sub-table (the first cell selects automatically)."</div>
                            <div>"Inside a sub-table, arrows navigate it and "<b>"Esc"</b>" returns to the parent row."</div>
                            <div style="color: #6b7280; margin-top: 0.25rem;">
                                "Note: Tabbing from a data cell moves to the next cell. You must be on the chevron to enter the sub-table."
                            </div>
                        </div>
                    })}
                    <Table
                        handle=table
                        theme=theme
                        sort_enabled=sort
                        filter_enabled=filter
                        selection_enabled=selection
                        cell_renderers=renderers
                        row_cell_renderers=row_renderers_val
                        column_toolbar=toolbar
                        group_expand_toggle=true
                        csv_export=csv
                        xlsx_export=xlsx
                        resize_enabled=resize
                        column_reorder_enabled=col_reorder
                        variable_row_height=variable_height
                        sticky_header=frozen_columns_on.get()
                        labels=labels_val
                        on_commit_edit=commit_cb
                        on_row_click=row_click_cb
                        detail_renderer=detail_renderer_val
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
