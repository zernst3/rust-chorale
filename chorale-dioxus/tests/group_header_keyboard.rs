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

// ---------------------------------------------------------------------------
// EM review of #52 (A.2, A.3, A.7, A.8, A.9)
// ---------------------------------------------------------------------------

/// A.2: the grid is one Tab stop. Where Tab or Shift+Tab cannot move the
/// cursor (a header without controls, Shift+Tab on any header, the last or
/// first column) the browser default runs and focus leaves the grid.
#[test]
fn tab_and_shift_tab_leave_the_grid_where_the_cursor_cannot_move() {
    let mut h = Harness::new(Cfg::default());
    let kb = h.kb_element();
    h.set_active(11, "name");
    assert!(
        h.press_default(kb, Key::Tab, Modifiers::empty()),
        "Tab on a header without controls must leave the grid"
    );
    assert!(
        h.press_default(kb, Key::Tab, Modifiers::SHIFT),
        "Shift+Tab on a header must leave the grid"
    );
    h.set_active(12, "name");
    assert!(
        h.press_default(kb, Key::Tab, Modifiers::empty()),
        "Tab in the last column must leave the grid"
    );
    assert_eq!(h.active(), Some(ActiveCell::new(12, ColumnId("name"))));
    h.set_active(12, "group");
    assert!(
        h.press_default(kb, Key::Tab, Modifiers::SHIFT),
        "Shift+Tab in the first column must leave the grid"
    );
    assert!(
        !h.press_default(kb, Key::Tab, Modifiers::empty()),
        "Tab that moves the cursor keeps focus in the grid"
    );
    assert_eq!(h.active(), Some(ActiveCell::new(12, ColumnId("name"))));
    // A header with controls keeps Tab (focus moves into them) but not Shift+Tab.
    let mut h = Harness::new(Cfg {
        hook: true,
        ..Cfg::default()
    });
    let kb = h.kb_element();
    h.set_active(11, "name");
    assert!(!h.press_default(kb, Key::Tab, Modifiers::empty()));
    assert!(h.press_default(kb, Key::Tab, Modifiers::SHIFT));
    // Ungrouped tables: the same edges.
    let mut h = Harness::new(Cfg {
        ungrouped: true,
        ..Cfg::default()
    });
    let kb = h.kb_element();
    h.set_active(3, "name");
    assert!(h.press_default(kb, Key::Tab, Modifiers::empty()));
    h.set_active(3, "group");
    assert!(h.press_default(kb, Key::Tab, Modifiers::SHIFT));
    assert!(!h.press_default(kb, Key::Tab, Modifiers::empty()));
    assert_eq!(h.active(), Some(ActiveCell::new(3, ColumnId("name"))));
}

/// Element id of a header control: the group checkbox or the chevron of
/// `Group {group}`.
fn header_control(h: &Harness, group: usize, checkbox: bool) -> ElementId {
    let (cb, tg) = (
        format!("chorale-grp-cb-Group_{group}"),
        format!("-tg-Group_{group}-"),
    );
    *h.dom_ids
        .iter()
        .find(|(_, v)| if checkbox { **v == cb } else { v.contains(&tg) })
        .map(|(id, _)| id)
        .expect("header control")
}

/// A.3: with the hook on, arrow keys on a header's checkbox or chevron act
/// on that header: Left collapses, Right expands, Up and Down move from it,
/// and focus returns to the grid. Space on the checkbox stays with it.
#[test]
fn arrows_on_header_controls_act_on_that_header() {
    for checkbox in [true, false] {
        let mut h = Harness::new(Cfg {
            hook: true,
            selection: true,
            ..Cfg::default()
        });
        h.set_active(3, "name");
        let ctl = header_control(&h, 1, checkbox);
        let kb_id = h.dom_ids[&h.kb_element()].clone();
        let before = h.eval_count();
        h.press_on(ctl, Key::ArrowLeft, Modifiers::empty());
        assert!(
            header_collapsed(&h.state(), 11),
            "checkbox={checkbox}: Left on the control collapses its group"
        );
        assert_eq!(
            h.active().map(|a| a.row_idx),
            Some(11),
            "checkbox={checkbox}"
        );
        assert!(
            h.evals_since(before)
                .iter()
                .any(|js| js.contains(&format!("getElementById('{kb_id}')"))
                    && js.contains(".focus()")),
            "checkbox={checkbox}: focus returns to the grid"
        );
        let ctl = header_control(&h, 1, checkbox);
        h.press_on(ctl, Key::ArrowRight, Modifiers::empty());
        assert!(!header_collapsed(&h.state(), 11), "checkbox={checkbox}");
        let ctl = header_control(&h, 1, checkbox);
        h.press_on(ctl, Key::ArrowDown, Modifiers::empty());
        assert_eq!(
            h.active().map(|a| a.row_idx),
            Some(12),
            "checkbox={checkbox}"
        );
        let ctl = header_control(&h, 1, checkbox);
        h.press_on(ctl, Key::ArrowUp, Modifiers::empty());
        assert_eq!(
            h.active().map(|a| a.row_idx),
            Some(10),
            "checkbox={checkbox}"
        );
    }
    let mut h = Harness::new(Cfg {
        hook: true,
        selection: true,
        ..Cfg::default()
    });
    h.set_active(11, "name");
    let cb = header_control(&h, 1, true);
    h.press_on(cb, Key::Character(" ".into()), Modifiers::empty());
    assert!(
        !header_collapsed(&h.state(), 11),
        "Space stays with the checkbox"
    );
}

/// A.7: a click on a grouped row past the data-row page size puts the
/// cursor on that row (`start_range_selection` clamped it to row 49).
#[test]
fn click_past_the_page_size_puts_the_cursor_on_that_grouped_row() {
    // Paged sanity check for the cell lookup.
    let mut h = Harness::new(Cfg::default());
    let data: Vec<usize> = h
        .rendered_rows()
        .into_iter()
        .filter(|i| !is_header(&h.state(), *i))
        .collect();
    let cells = h.data_cells();
    assert_eq!(cells.len(), data.len() * 2);
    h.click(cells[5], Modifiers::empty());
    assert_eq!(h.active(), Some(ActiveCell::new(data[2], ColumnId("name"))));

    let mut h = Harness::new(Cfg {
        virtualized: true,
        initial_scroll: 2400.0,
        ..Cfg::default()
    });
    let data: Vec<usize> = h
        .rendered_rows()
        .into_iter()
        .filter(|i| !is_header(&h.state(), *i))
        .collect();
    let cells = h.data_cells();
    assert_eq!(cells.len(), data.len() * 2);
    let (first, third) = (data[0], data[2]);
    assert!(first > 49, "{data:?}");
    h.click(cells[1], Modifiers::empty());
    assert_eq!(h.active(), Some(ActiveCell::new(first, ColumnId("name"))));
    // Shift+click extends the range in the grouped index space.
    h.click(cells[5], Modifiers::SHIFT);
    let s = h.state();
    assert_eq!(
        s.active_cell,
        Some(ActiveCell::new(third, ColumnId("name")))
    );
    assert_eq!(s.range_selection.last().map(|r| r.focus.0), Some(third));
    assert_eq!(s.range_selection.last().map(|r| r.anchor.0), Some(first));
    // Ctrl+click adds a range there.
    h.click(cells[3], Modifiers::CONTROL);
    assert_eq!(h.state().range_selection.len(), 2);
    assert_eq!(h.active().map(|a| a.row_idx), Some(data[1]));
}

/// A.8: Shift+Arrow extends the range in the grouped index space.
#[test]
fn shift_arrow_extends_the_range_over_grouped_rows() {
    let mut h = Harness::new(Cfg {
        virtualized: true,
        ..Cfg::default()
    });
    h.set_active(49, "name");
    h.press_mod(Key::ArrowDown, Modifiers::SHIFT);
    let s = h.state();
    let r = s.range_selection.last().cloned().expect("range");
    assert_eq!(r.anchor, (49, ColumnId("name")));
    assert_eq!(r.focus, (50, ColumnId("name")));
    assert_eq!(s.active_cell, Some(ActiveCell::new(50, ColumnId("name"))));
}

/// The text of the last `navigator.clipboard.writeText(..)` script.
fn clipboard_write(scripts: &[String]) -> Option<String> {
    scripts
        .iter()
        .rev()
        .find(|js| js.contains("navigator.clipboard.writeText("))
        .cloned()
}

/// A.8: Ctrl+C copies the data rows under the range, skipping headers; a
/// header alone is not a copy target.
#[test]
fn copy_in_a_grouped_table_copies_the_rows_under_the_range() {
    let mut h = Harness::new(Cfg::default());
    // Row 13 is Item 11.
    h.set_active(13, "name");
    let before = h.eval_count();
    h.press_mod(Key::Character("c".into()), Modifiers::CONTROL);
    let js = clipboard_write(&h.evals_since(before)).expect("a clipboard write");
    assert!(js.contains("writeText(\"Item 11\")"), "{js}");
    // Header 11 to row 13: the header is skipped.
    let handle = h.handle();
    h.dom.in_runtime(|| {
        let mut sig = handle.signal();
        let mut s = sig.peek().clone();
        s.range_selection = vec![chorale_core::RangeSelection::new(
            (11, ColumnId("name")),
            (13, ColumnId("name")),
        )];
        sig.set(s);
    });
    h.settle();
    let before = h.eval_count();
    h.press_mod(Key::Character("c".into()), Modifiers::CONTROL);
    let js = clipboard_write(&h.evals_since(before)).expect("a clipboard write");
    assert!(js.contains("writeText(\"Item 10\\nItem 11\")"), "{js}");
    // A header alone copies nothing.
    h.set_active(11, "name");
    let before = h.eval_count();
    h.press_mod(Key::Character("c".into()), Modifiers::CONTROL);
    assert_eq!(clipboard_write(&h.evals_since(before)), None);
}

fn paste_reply(tsv: &'static str) -> Reply {
    Rc::new(move |js: &str| {
        if js.contains("clipboard.readText") {
            serde_json::Value::String(tsv.to_owned())
        } else {
            serde_json::Value::Null
        }
    })
}

/// A.8: Ctrl+V targets the data rows from the cursor down, skipping
/// headers; on a header it does nothing.
#[test]
fn paste_in_a_grouped_table_targets_data_rows_only() {
    let mut h = Harness::with_reply(Cfg::default(), paste_reply("A\nB\nC"));
    // Rows 20 and 21 are Items 18 and 19; row 22 is Group 2's header; row 23
    // is Item 20.
    h.set_active(20, "name");
    h.press_mod(Key::Character("v".into()), Modifiers::CONTROL);
    let r = h.state().range_selection.last().cloned().expect("range");
    assert_eq!(r.anchor, (20, ColumnId("name")));
    assert_eq!(
        r.focus,
        (23, ColumnId("name")),
        "three data rows, header skipped"
    );
    h.set_active(22, "name");
    h.press_mod(Key::Character("v".into()), Modifiers::CONTROL);
    let r = h.state().range_selection.last().cloned().expect("range");
    assert_eq!(r.anchor, (22, ColumnId("name")));
    assert_eq!(
        r.focus,
        (22, ColumnId("name")),
        "a header is not a paste target"
    );
}

/// A.8: Enter on the detail-expander column of a grouped data row toggles
/// that row's expansion (resolved through the grouped view).
#[test]
fn enter_on_grouped_expander_toggles_that_row() {
    let mut h = Harness::new(Cfg {
        detail: true,
        ..Cfg::default()
    });
    h.set_active(13, chorale_core::DETAIL_EXPANDER_COLUMN.0);
    h.press(Key::Enter);
    let s = h.state();
    let id = match view_of(&s).get(13) {
        Some(GroupedRow::Data(id, row)) => {
            assert_eq!(row.name, "Item 11");
            *id
        }
        _ => panic!("row 13 is not a data row"),
    };
    assert!(s.expanded_rows.contains(&id), "Item 11 expanded");
    assert_eq!(s.expanded_rows.len(), 1);
}

/// A.9: collapsing the cursor's group by mouse or a host call moves the
/// cursor to that group's header; other view changes keep it on its row.
#[test]
fn cursor_follows_its_row_when_groups_change_under_it() {
    let mut h = Harness::new(Cfg::default());
    h.set_active(13, "name"); // Item 11 in Group 1
    let key_of = |h: &Harness, idx: usize| match view_of(&h.state()).get(idx) {
        Some(GroupedRow::Header { key, .. }) => key.clone(),
        _ => panic!("row {idx} is not a header"),
    };
    // Collapse Group 0 (above the cursor): Item 11 moves up to row 3.
    let g0 = key_of(&h, 0);
    let handle = h.handle();
    h.dom.in_runtime(|| handle.toggle_group(g0));
    h.settle();
    assert_eq!(h.active(), Some(ActiveCell::new(3, ColumnId("name"))));
    // Collapse the cursor's own group: the cursor moves to its header.
    let g1 = key_of(&h, 1);
    h.dom.in_runtime(|| handle.toggle_group(g1));
    h.settle();
    assert_eq!(h.active().map(|a| a.row_idx), Some(1));
    assert!(is_header(&h.state(), 1));
}

/// A.9: Space on a grouped data row does not let the browser scroll the page.
#[test]
fn space_on_a_grouped_data_row_is_prevented() {
    let mut h = Harness::new(Cfg::default());
    let kb = h.kb_element();
    h.set_active(13, "name");
    assert!(!h.press_default(kb, Key::Character(" ".into()), Modifiers::empty()));
    assert_eq!(h.active(), Some(ActiveCell::new(13, ColumnId("name"))));
}

// ---------------------------------------------------------------------------
// EM review round 2 of #52 (H1, H2, M1): keys on controls the grid contains
// ---------------------------------------------------------------------------

/// H1: with selection on and the cursor on a header, Tab from a row checkbox
/// moves on natively; it must not bubble to the grid and jump back to the
/// header's controls (a Tab trap).
#[test]
fn tab_on_a_row_checkbox_moves_on_instead_of_jumping_back() {
    let mut h = Harness::new(Cfg {
        selection: true,
        ..Cfg::default()
    });
    h.set_active(11, "name");
    let boxes = h.plain_checkboxes();
    // boxes[0] is the select-all checkbox; boxes[1] is the first row's.
    let row_box = boxes[1];
    let before = h.eval_count();
    let default = h.press_default(row_box, Key::Tab, Modifiers::empty());
    let refocus = h
        .evals_since(before)
        .into_iter()
        .find(|js| js.contains("data-chorale-group-index") && js.contains(".focus()"));
    assert_eq!(
        refocus, None,
        "Tab on a row checkbox jumped back to the header"
    );
    assert!(default, "Tab on a row checkbox keeps its native focus move");
    assert_eq!(h.active(), Some(ActiveCell::new(11, ColumnId("name"))));
}

/// H2: Space, Enter and Tab on a filter input, the select-all and row
/// checkboxes and a pagination button keep their native meaning: no
/// `prevent_default`, no cell edit, no group toggle, no cursor move.
#[test]
fn space_enter_and_tab_on_table_controls_keep_their_native_meaning() {
    for (what, header_cursor) in [("data row", false), ("header row", true)] {
        let mut h = Harness::new(Cfg {
            selection: true,
            filter: true,
            editable: true,
            ..Cfg::default()
        });
        let (row, col) = if header_cursor {
            (11, "name")
        } else {
            (13, "name")
        };
        let boxes = h.plain_checkboxes();
        let filter = h.first_listener("input").expect("filter input");
        let next_page = h
            .listeners
            .iter()
            .rev()
            .filter(|(n, id)| {
                n == "click"
                    && h.styles
                        .get(id)
                        .is_some_and(|s| s.starts_with("padding:0.25rem 0.6rem"))
            })
            .map(|(_, id)| *id)
            .next()
            .expect("pagination button");
        let controls = [
            ("filter input", filter),
            ("select-all checkbox", boxes[0]),
            ("row checkbox", boxes[1]),
            ("pagination button", next_page),
        ];
        for (name, el) in controls {
            for key in [Key::Character(" ".into()), Key::Enter, Key::Tab] {
                h.set_active(row, col);
                let default = h.press_default(el, key.clone(), Modifiers::empty());
                let s = h.state();
                assert!(
                    default,
                    "{what}: {key:?} on the {name} must keep its browser default"
                );
                assert!(
                    s.editing.is_none(),
                    "{what}: {key:?} on the {name} started an edit"
                );
                assert!(
                    s.collapsed_groups.is_empty(),
                    "{what}: {key:?} on the {name} toggled a group"
                );
                assert_eq!(
                    s.active_cell,
                    Some(ActiveCell::new(row, ColumnId(col))),
                    "{what}: {key:?} on the {name} moved the cursor"
                );
            }
        }
    }
}

/// M1: Ctrl+A selects every grouped row, header rows included in the span.
#[test]
fn ctrl_a_selects_every_grouped_row() {
    for virtualized in [false, true] {
        let mut h = Harness::new(Cfg {
            virtualized,
            ..Cfg::default()
        });
        h.set_active(3, "name");
        h.press_mod(Key::Character("a".into()), Modifiers::CONTROL);
        let s = h.state();
        let last = view_of(&s).len() - 1;
        let r = s.range_selection.last().cloned().expect("range");
        assert_eq!(
            r.anchor,
            (0, ColumnId("group")),
            "virtualized={virtualized}"
        );
        assert_eq!(
            r.focus,
            (last, ColumnId("name")),
            "virtualized={virtualized}"
        );
    }
}
