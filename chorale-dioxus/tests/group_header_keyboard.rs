//! Keyboard navigation over grouped rows, driven through the real `Table` with real keydown events (issue #51).
//!
//! See `tests/common/mod.rs` for the harness.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::wildcard_imports)]

mod common;

use common::*;

// ---------------------------------------------------------------------------
// Fix 1: header rows are real arrow-key stops
// ---------------------------------------------------------------------------

#[test]
fn up_from_first_data_row_lands_on_header_with_visible_cursor() {
    for hook in ON_OFF {
        let mut h = Harness::new(Cfg {
            hook,
            ..Cfg::default()
        });
        // Grouped row 1 is Item 0, the first data row of Group 0.
        h.set_active(1, "name");
        h.press(Key::ArrowUp);
        assert_eq!(h.active().map(|a| a.row_idx), Some(0), "hook={hook}");
        let tag = h.tr_tag(0);
        assert!(
            tag.is_some(),
            "hook={hook}: header row 0 carries no row index"
        );
        let tag = tag.unwrap();
        assert!(tag.contains("chorale-group-header"), "hook={hook}: {tag}");
        assert!(
            tag.contains("data-chorale-active-row=\"true\""),
            "hook={hook}: header shows no cursor: {tag}"
        );
        // Only the active row carries the cursor.
        let other = h.tr_tag(11).unwrap();
        assert!(
            other.contains("data-chorale-active-row=\"false\""),
            "hook={hook}: {other}"
        );
    }
}

#[test]
fn down_crosses_group_boundary_through_the_header() {
    for hook in ON_OFF {
        let mut h = Harness::new(Cfg {
            hook,
            ..Cfg::default()
        });
        // Row 10 is Item 9, the last data row of Group 0; row 11 is Group 1's header.
        h.set_active(10, "name");
        h.press(Key::ArrowDown);
        assert_eq!(h.active().map(|a| a.row_idx), Some(11), "hook={hook}");
        assert!(is_header(&h.state(), 11));
        h.press(Key::ArrowDown);
        let ac = h.active().unwrap();
        assert_eq!(
            ac.row_idx, 12,
            "hook={hook}: arrows continue into data rows"
        );
        // The column is remembered across the header.
        assert_eq!(ac.column_id, ColumnId("name"), "hook={hook}");
        h.press(Key::ArrowUp);
        h.press(Key::ArrowUp);
        assert_eq!(h.active().map(|a| a.row_idx), Some(10), "hook={hook}");
    }
}

#[test]
fn left_collapses_right_expands_header() {
    for hook in ON_OFF {
        let mut h = Harness::new(Cfg {
            hook,
            ..Cfg::default()
        });
        h.set_active(11, "name");
        h.press(Key::ArrowLeft);
        assert!(
            header_collapsed(&h.state(), 11),
            "hook={hook}: Left collapses"
        );
        assert_eq!(h.active().map(|a| a.row_idx), Some(11), "hook={hook}");
        // Left again on a collapsed header changes nothing.
        h.press(Key::ArrowLeft);
        assert!(header_collapsed(&h.state(), 11), "hook={hook}");
        h.press(Key::ArrowRight);
        assert!(
            !header_collapsed(&h.state(), 11),
            "hook={hook}: Right expands"
        );
        assert_eq!(h.active().map(|a| a.row_idx), Some(11), "hook={hook}");
        h.press(Key::ArrowRight);
        assert!(!header_collapsed(&h.state(), 11), "hook={hook}");
        // The header row reports its state through aria-expanded.
        let tag = h.tr_tag(11).unwrap();
        assert!(tag.contains("aria-expanded=\"true\""), "hook={hook}: {tag}");
    }
}

#[test]
fn enter_and_space_toggle_header() {
    for hook in ON_OFF {
        let mut h = Harness::new(Cfg {
            hook,
            ..Cfg::default()
        });
        h.set_active(0, "name");
        h.press(Key::Enter);
        assert!(
            header_collapsed(&h.state(), 0),
            "hook={hook}: Enter collapses"
        );
        h.press(Key::Enter);
        assert!(
            !header_collapsed(&h.state(), 0),
            "hook={hook}: Enter expands"
        );
        h.press(Key::Character(" ".into()));
        assert!(
            header_collapsed(&h.state(), 0),
            "hook={hook}: Space collapses"
        );
        h.press(Key::Character(" ".into()));
        assert!(
            !header_collapsed(&h.state(), 0),
            "hook={hook}: Space expands"
        );
        assert_eq!(h.active().map(|a| a.row_idx), Some(0), "hook={hook}");
    }
}

#[test]
fn down_from_collapsed_header_goes_to_next_header() {
    let mut h = Harness::new(Cfg {
        hook: true,
        ..Cfg::default()
    });
    h.set_active(0, "name");
    h.press(Key::ArrowLeft);
    h.press(Key::ArrowDown);
    // Group 0 is collapsed, so row 1 is now Group 1's header.
    assert_eq!(h.active().map(|a| a.row_idx), Some(1));
    assert!(is_header(&h.state(), 1));
}

#[test]
fn every_grouped_row_is_reachable_in_virtualized_mode() {
    for hook in ON_OFF {
        let mut h = Harness::new(Cfg {
            hook,
            virtualized: true,
            ..Cfg::default()
        });
        let total = view_of(&h.state()).len();
        assert_eq!(total, GROUPS * (ROWS_PER_GROUP + 1));
        // Past the data-row page size the cursor used to stop moving.
        h.set_active(49, "name");
        h.press(Key::ArrowDown);
        assert_eq!(h.active().map(|a| a.row_idx), Some(50), "hook={hook}");
        h.press_mod(Key::ArrowDown, Modifiers::CONTROL);
        assert_eq!(
            h.active().map(|a| a.row_idx),
            Some(total - 1),
            "hook={hook}: Ctrl+Down reaches the last grouped row"
        );
        h.press_mod(Key::End, Modifiers::CONTROL);
        assert_eq!(h.active().map(|a| a.row_idx), Some(total - 1));
        h.press_mod(Key::Home, Modifiers::CONTROL);
        assert_eq!(h.active().map(|a| a.row_idx), Some(0), "hook={hook}");
    }
}

#[test]
fn page_down_and_horizontal_moves_keep_grouped_row() {
    let mut h = Harness::new(Cfg {
        virtualized: true,
        ..Cfg::default()
    });
    let total = view_of(&h.state()).len();
    // Row 70 is past the data-row count of one page; Left/Right must not clamp it.
    h.set_active(70, "name");
    h.press(Key::ArrowLeft);
    assert_eq!(h.active(), Some(ActiveCell::new(70, ColumnId("group"))));
    h.press(Key::ArrowRight);
    assert_eq!(h.active(), Some(ActiveCell::new(70, ColumnId("name"))));
    h.press(Key::Home);
    assert_eq!(h.active(), Some(ActiveCell::new(70, ColumnId("group"))));
    h.press(Key::End);
    assert_eq!(h.active(), Some(ActiveCell::new(70, ColumnId("name"))));
    h.press(Key::PageDown);
    assert_eq!(h.active().map(|a| a.row_idx), Some(total - 1));
    h.press(Key::PageUp);
    assert_eq!(h.active().map(|a| a.row_idx), Some(total - 1 - 50));
}

#[test]
fn tab_from_header_focuses_its_first_control() {
    for (hook, selection) in [(true, false), (true, true), (false, true)] {
        let mut h = Harness::new(Cfg {
            hook,
            selection,
            ..Cfg::default()
        });
        h.set_active(11, "name");
        let before = h.eval_count();
        h.press(Key::Tab);
        let scripts = h.evals_since(before);
        let focus = scripts.iter().find(|js| {
            js.contains("tr[data-chorale-group-index=\"11\"]") && js.contains(".focus()")
        });
        assert!(
            focus.is_some(),
            "hook={hook} selection={selection}: no focus script for header 11: {scripts:?}"
        );
        // The cursor stays on the header; Tab does not move it to a cell.
        assert_eq!(
            h.active(),
            Some(ActiveCell::new(11, ColumnId("name"))),
            "hook={hook} selection={selection}"
        );
    }
}

#[test]
fn tab_from_header_without_controls_keeps_cursor() {
    let mut h = Harness::new(Cfg::default());
    h.set_active(11, "group");
    h.press(Key::Tab);
    assert_eq!(h.active(), Some(ActiveCell::new(11, ColumnId("group"))));
}

#[test]
fn keys_inside_host_header_content_stay_there() {
    let mut h = Harness::new(Cfg {
        hook: true,
        ..Cfg::default()
    });
    h.set_active(1, "name");
    // The host content wrapper span (its style pins `inline-block`).
    let span = h
        .listeners
        .iter()
        .find(|(n, id)| {
            n == "keydown"
                && h.styles
                    .get(id)
                    .is_some_and(|s| s.starts_with("display: inline-block"))
        })
        .map(|(_, id)| *id)
        .expect("host content span listener");
    for key in [Key::ArrowDown, Key::ArrowUp, Key::Enter, Key::Tab] {
        h.press_on(span, key, Modifiers::empty());
    }
    h.press_on(span, Key::Character(" ".into()), Modifiers::empty());
    assert_eq!(h.active(), Some(ActiveCell::new(1, ColumnId("name"))));
    assert!(!header_collapsed(&h.state(), 0));
}

#[test]
fn enter_on_grouped_data_row_edits_that_row() {
    let mut h = Harness::new(Cfg {
        editable: true,
        ..Cfg::default()
    });
    // Row 13 is Item 11 (Group 1's header is row 11).
    h.set_active(13, "name");
    h.press(Key::Enter);
    let s = h.state();
    let expected = match view_of(&s).get(13) {
        Some(GroupedRow::Data(id, row)) => {
            assert_eq!(row.name, "Item 11");
            *id
        }
        _ => panic!("row 13 is not a data row"),
    };
    let editing = s.editing.expect("Enter starts an edit");
    assert_eq!(
        editing.row_id, expected,
        "Enter edits the row under the cursor"
    );
    // Enter on a header never starts an edit.
    let mut h = Harness::new(Cfg {
        editable: true,
        ..Cfg::default()
    });
    h.set_active(11, "name");
    h.press(Key::Enter);
    assert!(h.state().editing.is_none());
}

/// Switching to Virtualized at runtime (the test page's select) must
/// re-derive the grouped view: every group, not the first page's rows.
#[test]
fn runtime_switch_to_virtualized_renders_and_reaches_every_group() {
    let mut h = Harness::new(Cfg::default());
    let handle = h.handle();
    h.dom
        .in_runtime(|| handle.set_grouped_pagination(GroupedPaginationMode::Virtualized));
    h.settle();
    let total = GROUPS * (ROWS_PER_GROUP + 1);
    assert_eq!(view_of(&h.state()).len(), total);
    h.set_active(0, "name");
    h.press_mod(Key::End, Modifiers::CONTROL);
    assert_eq!(
        h.active().map(|a| a.row_idx),
        Some(total - 1),
        "Ctrl+End reaches the last group's last row"
    );
    // The last row is rendered once it is the target (bottom of the window).
    assert!(
        h.tr_tag(total - 1).is_some(),
        "row {} is not rendered",
        total - 1
    );
}

/// Review finding: with the built-in header and selection on, Tab from a
/// header focuses its group checkbox. Keys pressed there must behave like on
/// the chevron: Space and Tab stay with the checkbox (the browser ticks it or
/// moves focus on), arrows go back to the grid.
#[test]
fn builtin_header_checkbox_keeps_space_and_tab() {
    let mut h = Harness::new(Cfg {
        selection: true,
        ..Cfg::default()
    });
    h.set_active(11, "name");
    h.press(Key::Tab);
    let checkbox = *h
        .dom_ids
        .iter()
        .find(|(_, v)| v.starts_with("chorale-grp-cb-Group_1"))
        .map(|(id, _)| id)
        .expect("group checkbox of Group 1");
    let before = h.eval_count();
    h.press_on(checkbox, Key::Character(" ".into()), Modifiers::empty());
    assert!(
        !header_collapsed(&h.state(), 11),
        "Space on the checkbox must not toggle the group"
    );
    h.press_on(checkbox, Key::Tab, Modifiers::empty());
    let scripts = h.evals_since(before);
    assert!(
        !scripts.iter().any(|js| js.contains(".focus()")),
        "Tab on the checkbox must not refocus it: {scripts:?}"
    );
    // An arrow key returns focus to the grid and moves the cursor.
    let before = h.eval_count();
    h.press_on(checkbox, Key::ArrowDown, Modifiers::empty());
    let kb_id = h.dom_ids[&h.kb_element()].clone();
    assert!(
        h.evals_since(before)
            .iter()
            .any(|js| js.contains(&format!("getElementById('{kb_id}')")) && js.contains(".focus()")),
        "arrow on the checkbox refocuses the grid"
    );
    assert_eq!(h.active().map(|a| a.row_idx), Some(12));
}
