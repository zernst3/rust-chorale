//! Integration tests for `#[derive(TableRow)]`.
//!
//! These tests exercise the derive macro end-to-end by compiling real structs
//! and inspecting the generated `chorale_columns()` output at runtime.

#![allow(clippy::unwrap_used, clippy::useless_vec)]

use chorale_core::{Alignment, CellValue, ColumnId, FilterKind, TableState};
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
