//! Integration tests for `#[derive(TableRow)]`.
//!
//! These tests exercise the derive macro end-to-end by compiling real structs
//! and inspecting the generated `chorale_columns()` output at runtime.

// `float_cmp`: the data-aware filter tests assert exact min/max bounds that
// pass through the macro unmodified; strict equality is the point.
#![allow(clippy::unwrap_used, clippy::useless_vec, clippy::float_cmp)]

use chorale_core::{
    Alignment, CellValue, ColumnId, CurrencyCode, FilterKind, RenderKind, TableState,
};
use chorale_derive::TableRow;

// ---------------------------------------------------------------------------
// Helper structs
// ---------------------------------------------------------------------------

#[derive(TableRow, Clone, PartialEq)]
struct Simple {
    name: String,
    score: f64,
    active: bool,
    #[chorale(skip)]
    internal: String,
}

#[derive(TableRow, Clone, PartialEq)]
struct WithAttributes {
    #[chorale(header = "Full Name", initial_width = 200.0)]
    name: String,
    #[chorale(filter = "none")]
    code: String,
    #[chorale(align = "Center")]
    status: String,
}

#[derive(TableRow, Clone, PartialEq)]
struct NumericTypes {
    int_field: i32,
    float_field: f64,
    unsigned: u64,
}

#[derive(TableRow, Clone, PartialEq)]
struct OptionFields {
    required: String,
    optional_name: Option<String>,
    optional_int: Option<i32>,
}

#[derive(TableRow, Clone, PartialEq)]
struct DisplayFallback {
    name: String,
    // A custom type that implements Display but isn't a known type
    // We can't test this directly without a custom type, but we test that
    // String goes through the Text path.
}

#[derive(TableRow, Clone, PartialEq)]
struct MultiSelectField {
    #[chorale(filter = "MultiSelect", options = ["Active", "Inactive", "Pending"])]
    status: String,
}

// ---------------------------------------------------------------------------
// Basic happy-path tests
// ---------------------------------------------------------------------------

#[test]
fn simple_struct_generates_non_skipped_columns() {
    let cols = Simple::chorale_columns();
    // `internal` is skipped, so 3 columns: name, score, active
    assert_eq!(cols.len(), 3);
    let ids: Vec<ColumnId> = cols.iter().map(|c| c.id).collect();
    assert!(ids.contains(&ColumnId("name")));
    assert!(ids.contains(&ColumnId("score")));
    assert!(ids.contains(&ColumnId("active")));
    assert!(!ids.contains(&ColumnId("internal")));
}

#[test]
fn string_field_produces_text_cell_value() {
    let cols = Simple::chorale_columns();
    let name_col = cols.iter().find(|c| c.id == ColumnId("name")).unwrap();
    let row = Simple {
        name: "Alice".into(),
        score: 90.0,
        active: true,
        internal: "secret".into(),
    };
    assert_eq!((name_col.accessor)(&row), CellValue::Text("Alice".into()));
}

#[test]
fn float_field_produces_float_cell_value() {
    let cols = Simple::chorale_columns();
    let score_col = cols.iter().find(|c| c.id == ColumnId("score")).unwrap();
    let row = Simple {
        name: "Bob".into(),
        score: 75.5,
        active: false,
        internal: "x".into(),
    };
    assert_eq!((score_col.accessor)(&row), CellValue::Float(75.5));
}

#[test]
fn bool_field_produces_boolean_cell_value() {
    let cols = Simple::chorale_columns();
    let active_col = cols.iter().find(|c| c.id == ColumnId("active")).unwrap();
    let row = Simple {
        name: "C".into(),
        score: 0.0,
        active: true,
        internal: "x".into(),
    };
    assert_eq!((active_col.accessor)(&row), CellValue::Boolean(true));
}

#[test]
fn default_header_is_title_case_of_field_name() {
    let cols = Simple::chorale_columns();
    let name_col = cols.iter().find(|c| c.id == ColumnId("name")).unwrap();
    assert_eq!(name_col.header, "Name");
    let score_col = cols.iter().find(|c| c.id == ColumnId("score")).unwrap();
    assert_eq!(score_col.header, "Score");
}

#[test]
fn skip_attribute_excludes_field() {
    let cols = Simple::chorale_columns();
    assert!(cols.iter().all(|c| c.id != ColumnId("internal")));
}

// ---------------------------------------------------------------------------
// Attribute override tests
// ---------------------------------------------------------------------------

#[test]
fn header_attribute_overrides_default() {
    let cols = WithAttributes::chorale_columns();
    let name_col = cols.iter().find(|c| c.id == ColumnId("name")).unwrap();
    assert_eq!(name_col.header, "Full Name");
}

#[test]
fn initial_width_attribute_sets_width() {
    let cols = WithAttributes::chorale_columns();
    let name_col = cols.iter().find(|c| c.id == ColumnId("name")).unwrap();
    assert_eq!(name_col.initial_width, Some(200.0));
}

#[test]
fn filter_none_attribute_disables_filter() {
    let cols = WithAttributes::chorale_columns();
    let code_col = cols.iter().find(|c| c.id == ColumnId("code")).unwrap();
    assert!(matches!(code_col.filter, FilterKind::None));
}

#[test]
fn align_attribute_overrides_default() {
    let cols = WithAttributes::chorale_columns();
    let status_col = cols.iter().find(|c| c.id == ColumnId("status")).unwrap();
    assert_eq!(status_col.alignment, Alignment::Center);
}

// ---------------------------------------------------------------------------
// Numeric type tests
// ---------------------------------------------------------------------------

#[test]
fn integer_field_produces_integer_cell_value() {
    let cols = NumericTypes::chorale_columns();
    let col = cols.iter().find(|c| c.id == ColumnId("int_field")).unwrap();
    let row = NumericTypes {
        int_field: 42,
        float_field: 0.0,
        unsigned: 0,
    };
    assert_eq!((col.accessor)(&row), CellValue::Integer(42));
}

#[test]
fn numeric_fields_are_right_aligned_by_default() {
    let cols = NumericTypes::chorale_columns();
    for col in &cols {
        assert_eq!(
            col.alignment,
            Alignment::Right,
            "column {} should be right-aligned",
            col.id.0
        );
    }
}

// ---------------------------------------------------------------------------
// Option<T> tests
// ---------------------------------------------------------------------------

#[test]
fn option_string_some_produces_text() {
    let cols = OptionFields::chorale_columns();
    let col = cols
        .iter()
        .find(|c| c.id == ColumnId("optional_name"))
        .unwrap();
    let row = OptionFields {
        required: "r".into(),
        optional_name: Some("Alice".into()),
        optional_int: None,
    };
    assert_eq!((col.accessor)(&row), CellValue::Text("Alice".into()));
}

#[test]
fn option_string_none_produces_empty() {
    let cols = OptionFields::chorale_columns();
    let col = cols
        .iter()
        .find(|c| c.id == ColumnId("optional_name"))
        .unwrap();
    let row = OptionFields {
        required: "r".into(),
        optional_name: None,
        optional_int: Some(0),
    };
    assert_eq!((col.accessor)(&row), CellValue::Empty);
}

#[test]
fn option_int_some_produces_integer() {
    let cols = OptionFields::chorale_columns();
    let col = cols
        .iter()
        .find(|c| c.id == ColumnId("optional_int"))
        .unwrap();
    let row = OptionFields {
        required: "r".into(),
        optional_name: None,
        optional_int: Some(99),
    };
    assert_eq!((col.accessor)(&row), CellValue::Integer(99));
}

// ---------------------------------------------------------------------------
// MultiSelect filter test
// ---------------------------------------------------------------------------

#[test]
fn multi_select_filter_has_correct_options() {
    let cols = MultiSelectField::chorale_columns();
    let col = cols.iter().find(|c| c.id == ColumnId("status")).unwrap();
    if let FilterKind::MultiSelect { options } = &col.filter {
        assert_eq!(options, &["Active", "Inactive", "Pending"]);
    } else {
        panic!("expected MultiSelect filter");
    }
}

// ---------------------------------------------------------------------------
// Integration with TableState
// ---------------------------------------------------------------------------

#[test]
fn chorale_columns_plugs_into_table_state() {
    let rows = vec![
        Simple {
            name: "Alice".into(),
            score: 90.0,
            active: true,
            internal: "x".into(),
        },
        Simple {
            name: "Bob".into(),
            score: 75.0,
            active: false,
            internal: "y".into(),
        },
    ];
    let state = TableState::new(
        rows.iter()
            .map(|r| (chorale_core::RowId::new(), r.clone()))
            .collect(),
        Simple::chorale_columns(),
    );
    assert_eq!(state.rows.len(), 2);
    assert_eq!(state.columns.len(), 3);
}

// ---------------------------------------------------------------------------
// String filter inferred
// ---------------------------------------------------------------------------

#[test]
fn string_field_infers_text_filter() {
    let cols = Simple::chorale_columns();
    let name_col = cols.iter().find(|c| c.id == ColumnId("name")).unwrap();
    assert!(matches!(name_col.filter, FilterKind::Text));
}

// ---------------------------------------------------------------------------
// Sortable by default
// ---------------------------------------------------------------------------

#[test]
fn all_columns_sortable_by_default() {
    let cols = Simple::chorale_columns();
    for col in &cols {
        assert!(col.sortable, "column {} should be sortable", col.id.0);
    }
}

// ---------------------------------------------------------------------------
// Data-aware columns: chorale_columns_with_rows
// ---------------------------------------------------------------------------

#[derive(TableRow, Clone, PartialEq)]
struct Employee {
    name: String,
    #[chorale(render = "currency")]
    salary: f64,
    age: u32,
    bonus: Option<i64>,
    #[chorale(filter = "MultiSelect")]
    department: String,
}

fn sample_employees() -> Vec<Employee> {
    vec![
        Employee {
            name: "Alice".into(),
            salary: 95_000.5,
            age: 41,
            bonus: Some(12_000),
            department: "Engineering".into(),
        },
        Employee {
            name: "Bob".into(),
            salary: 60_250.0,
            age: 28,
            bonus: None,
            department: "Sales".into(),
        },
        Employee {
            name: "Carol".into(),
            salary: 120_000.0,
            age: 35,
            bonus: Some(3_000),
            department: "Engineering".into(),
        },
    ]
}

fn find<'a>(
    cols: &'a [chorale_core::ColumnDef<Employee>],
    id: &'static str,
) -> &'a chorale_core::ColumnDef<Employee> {
    cols.iter().find(|c| c.id == ColumnId(id)).unwrap()
}

#[test]
fn with_rows_computes_real_float_min_max() {
    let rows = sample_employees();
    let cols = Employee::chorale_columns_with_rows(&rows);
    let FilterKind::NumericRange { min, max, step } = find(&cols, "salary").filter else {
        panic!("expected NumericRange on salary");
    };
    assert_eq!(min, 60_250.0);
    assert_eq!(max, 120_000.0);
    // (120_000 - 60_250) / 100 = 597.5 → snapped down to 100.
    assert_eq!(step, 100.0);
}

#[test]
fn with_rows_computes_real_integer_min_max() {
    let rows = sample_employees();
    let cols = Employee::chorale_columns_with_rows(&rows);
    let FilterKind::NumericRange { min, max, step } = find(&cols, "age").filter else {
        panic!("expected NumericRange on age");
    };
    assert_eq!(min, 28.0);
    assert_eq!(max, 41.0);
    // (41 - 28) / 100 = 0.13 → snapped to 0.1 → clamped to 1.0 for integers.
    assert_eq!(step, 1.0);
}

#[test]
fn with_rows_skips_none_values_in_option_numeric() {
    let rows = sample_employees();
    let cols = Employee::chorale_columns_with_rows(&rows);
    let FilterKind::NumericRange { min, max, .. } = find(&cols, "bonus").filter else {
        panic!("expected NumericRange on bonus");
    };
    assert_eq!(min, 3_000.0);
    assert_eq!(max, 12_000.0);
}

#[test]
fn with_rows_empty_falls_back_to_static_defaults() {
    let cols = Employee::chorale_columns_with_rows(&[]);
    let FilterKind::NumericRange { min, max, step } = find(&cols, "salary").filter else {
        panic!("expected NumericRange on salary");
    };
    assert_eq!((min, max, step), (0.0, 100.0, 0.1));
    let FilterKind::NumericRange { min, max, step } = find(&cols, "age").filter else {
        panic!("expected NumericRange on age");
    };
    assert_eq!((min, max, step), (0.0, 1_000_000.0, 1_000.0));
}

#[test]
fn with_rows_multi_select_without_options_collects_sorted_distinct() {
    let rows = sample_employees();
    let cols = Employee::chorale_columns_with_rows(&rows);
    let FilterKind::MultiSelect { options } = &find(&cols, "department").filter else {
        panic!("expected MultiSelect on department");
    };
    assert_eq!(options, &["Engineering", "Sales"]);
}

#[test]
fn with_rows_multi_select_caps_options_at_50() {
    let rows: Vec<Employee> = (0..120)
        .map(|i| Employee {
            name: format!("emp{i}"),
            salary: 1.0,
            age: 30,
            bonus: None,
            department: format!("dept{i:03}"),
        })
        .collect();
    let cols = Employee::chorale_columns_with_rows(&rows);
    let FilterKind::MultiSelect { options } = &find(&cols, "department").filter else {
        panic!("expected MultiSelect on department");
    };
    assert_eq!(options.len(), 50);
    assert_eq!(options[0], "dept000");
    assert_eq!(options[49], "dept049");
}

#[test]
fn with_rows_explicit_options_are_preserved() {
    let rows = vec![MultiSelectFieldRow {
        status: "Other".into(),
    }];
    let cols = MultiSelectFieldRow::chorale_columns_with_rows(&rows);
    let col = cols.iter().find(|c| c.id == ColumnId("status")).unwrap();
    let FilterKind::MultiSelect { options } = &col.filter else {
        panic!("expected MultiSelect on status");
    };
    // Explicit options win over data-derived values.
    assert_eq!(options, &["Active", "Inactive"]);
}

#[derive(TableRow, Clone, PartialEq)]
struct MultiSelectFieldRow {
    #[chorale(filter = "MultiSelect", options = ["Active", "Inactive"])]
    status: String,
}

#[test]
fn with_rows_non_numeric_columns_identical_to_static() {
    let rows = sample_employees();
    let static_cols = Employee::chorale_columns();
    let rows_cols = Employee::chorale_columns_with_rows(&rows);
    assert_eq!(static_cols.len(), rows_cols.len());
    for (s, r) in static_cols.iter().zip(rows_cols.iter()) {
        assert_eq!(s.id, r.id);
        assert_eq!(s.header, r.header);
        assert_eq!(s.sortable, r.sortable);
        assert_eq!(s.alignment, r.alignment);
        assert_eq!(s.initial_width, r.initial_width);
    }
    // The name column (Text filter) is byte-for-byte the same configuration.
    let name = rows_cols.iter().find(|c| c.id == ColumnId("name")).unwrap();
    assert!(matches!(name.filter, FilterKind::Text));
}

#[test]
fn static_chorale_columns_unchanged_by_data_aware_additions() {
    let cols = Employee::chorale_columns();
    // Numeric columns keep the static defaults.
    let FilterKind::NumericRange { min, max, step } = find(&cols, "salary").filter else {
        panic!("expected NumericRange on salary");
    };
    assert_eq!((min, max, step), (0.0, 100.0, 0.1));
    let FilterKind::NumericRange { min, max, step } = find(&cols, "age").filter else {
        panic!("expected NumericRange on age");
    };
    assert_eq!((min, max, step), (0.0, 1_000_000.0, 1_000.0));
    // MultiSelect without options stays empty on the static path.
    let FilterKind::MultiSelect { options } = &find(&cols, "department").filter else {
        panic!("expected MultiSelect on department");
    };
    assert!(options.is_empty());
}

// ---------------------------------------------------------------------------
// render = "..." attribute
// ---------------------------------------------------------------------------

#[derive(TableRow, Clone, PartialEq)]
struct Rendered {
    #[chorale(render = "currency")]
    price_usd: f64,
    #[chorale(render = "currency:EUR")]
    price_eur: f64,
    #[chorale(render = "number")]
    quantity: i64,
    plain: String,
}

#[test]
fn render_currency_defaults_to_usd() {
    let cols = Rendered::chorale_columns();
    let col = cols.iter().find(|c| c.id == ColumnId("price_usd")).unwrap();
    let RenderKind::Currency(code) = &col.render_kind else {
        panic!("expected Currency render kind");
    };
    assert_eq!(*code, CurrencyCode::USD);
}

#[test]
fn render_currency_accepts_explicit_code() {
    let cols = Rendered::chorale_columns();
    let col = cols.iter().find(|c| c.id == ColumnId("price_eur")).unwrap();
    let RenderKind::Currency(code) = &col.render_kind else {
        panic!("expected Currency render kind");
    };
    assert_eq!(*code, CurrencyCode::EUR);
}

#[test]
fn render_number_emits_number_kind() {
    let cols = Rendered::chorale_columns();
    let col = cols.iter().find(|c| c.id == ColumnId("quantity")).unwrap();
    assert!(matches!(col.render_kind, RenderKind::Number));
}

#[test]
fn no_render_attribute_keeps_text_default() {
    let cols = Rendered::chorale_columns();
    let col = cols.iter().find(|c| c.id == ColumnId("plain")).unwrap();
    assert!(matches!(col.render_kind, RenderKind::Text));
}

#[test]
fn render_attribute_applies_to_rows_aware_path_too() {
    let rows: Vec<Rendered> = vec![];
    let cols = Rendered::chorale_columns_with_rows(&rows);
    let col = cols.iter().find(|c| c.id == ColumnId("price_usd")).unwrap();
    assert!(matches!(col.render_kind, RenderKind::Currency(_)));
}
