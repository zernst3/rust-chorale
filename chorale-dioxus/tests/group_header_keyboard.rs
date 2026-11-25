//! Keyboard navigation over grouped rows, driven through the real `Table`
//! component with real keydown events (issue #51).
//!
//! The harness mounts `Table` in a `VirtualDom`, reads the element ids of its
//! event listeners from the mutation stream, and sends `keydown` events through
//! `Runtime::handle_event` the way a renderer does. A fake `Document` records
//! every `eval` script the table runs and answers measurement scripts with
//! geometry chosen by the test, so the scroll-follow logic runs without a
//! browser. What only a browser can show (real layout, real scrolling) is
//! covered by the hand-test steps in `docs/QA.md` section 16a.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::task::{Context, Poll};

use chorale_core::{
    visible_grouped_view, ActiveCell, CellValue, ColumnDef, ColumnId, EditorKind,
    GroupedPaginationMode, GroupedRow, RowId, TableState,
};
use chorale_dioxus::{use_table, GroupHeaderContext, Table, UseTableHandle};
use dioxus::dioxus_core::{AttributeValue, ElementId, Event, Mutation, Mutations};
use dioxus::document::{Document, Eval, EvalError, Evaluator};
use dioxus::prelude::*;
use dioxus::signals::Owner;
use dioxus_html::{
    Code, Key, Location, Modifiers, PlatformEventData, SerializedHtmlEventConverter,
    SerializedKeyboardData,
};

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

const GROUPS: usize = 8;
const ROWS_PER_GROUP: usize = 10;

#[derive(Clone, PartialEq, Debug)]
struct Item {
    group: String,
    name: String,
}

#[derive(Clone, Default)]
#[allow(clippy::struct_excessive_bools)]
struct Cfg {
    hook: bool,
    selection: bool,
    virtualized: bool,
    /// Give the name column a text editor (for Enter-to-edit checks).
    editable: bool,
}

/// Props for the fixture app. `slot` hands the table handle back to the test.
#[derive(Clone)]
struct Props {
    cfg: Cfg,
    slot: Rc<RefCell<Option<UseTableHandle<Item>>>>,
}

impl PartialEq for Props {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

fn dataset() -> Vec<(RowId, Item)> {
    (0..GROUPS * ROWS_PER_GROUP)
        .map(|i| {
            (
                RowId::new(),
                Item {
                    group: format!("Group {}", i / ROWS_PER_GROUP),
                    name: format!("Item {i}"),
                },
            )
        })
        .collect()
}

#[allow(clippy::needless_pass_by_value)]
fn app(props: Props) -> Element {
    let cfg = props.cfg.clone();
    let table = use_table(move || {
        let name = ColumnDef::new(ColumnId("name"), "Name", |r: &Item| {
            CellValue::Text(r.name.clone())
        });
        let name = if cfg.editable {
            name.editor(EditorKind::Text)
        } else {
            name
        };
        let columns = vec![
            ColumnDef::new(ColumnId("group"), "Group", |r: &Item| {
                CellValue::Text(r.group.clone())
            }),
            name,
        ];
        let mut s = TableState::new(dataset(), columns);
        s.grouping = vec![ColumnId("group")];
        if cfg.virtualized {
            s.grouped_pagination = GroupedPaginationMode::Virtualized;
        }
        s
    });
    *props.slot.borrow_mut() = Some(table);
    let header_cb = Callback::new(|ctx: GroupHeaderContext| {
        rsx! {
            strong { "{ctx.label}" }
            select { aria_label: "Group action", option { "Keep" } }
        }
    });
    rsx! {
        Table::<Item> {
            handle: table,
            selection_enabled: props.cfg.selection,
            group_header_renderer: if props.cfg.hook { Some(header_cb) } else { None },
        }
    }
}

// ---------------------------------------------------------------------------
// Fake document: records eval scripts, answers measurement scripts
// ---------------------------------------------------------------------------

/// What the fake document answers for one script.
type Reply = Rc<dyn Fn(&str) -> serde_json::Value>;

struct FakeEval {
    value: serde_json::Value,
}

impl Evaluator for FakeEval {
    fn send(&self, _data: serde_json::Value) -> Result<(), EvalError> {
        Ok(())
    }
    fn poll_recv(&mut self, _: &mut Context<'_>) -> Poll<Result<serde_json::Value, EvalError>> {
        Poll::Ready(Ok(self.value.clone()))
    }
    fn poll_join(&mut self, _: &mut Context<'_>) -> Poll<Result<serde_json::Value, EvalError>> {
        Poll::Ready(Ok(self.value.clone()))
    }
}

struct FakeDocument {
    log: Rc<RefCell<Vec<String>>>,
    reply: Reply,
    // Keeps every eval's storage alive; dropping an owner ends its eval.
    owners: RefCell<Vec<Owner>>,
}

impl Document for FakeDocument {
    fn eval(&self, js: String) -> Eval {
        let value = (self.reply)(&js);
        self.log.borrow_mut().push(js);
        let owner = Owner::default();
        let boxed: Box<dyn Evaluator> = Box::new(FakeEval { value });
        let eval = Eval::new(owner.insert(boxed));
        self.owners.borrow_mut().push(owner);
        eval
    }
}

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

struct Harness {
    dom: VirtualDom,
    slot: Rc<RefCell<Option<UseTableHandle<Item>>>>,
    evals: Rc<RefCell<Vec<String>>>,
    /// Element id -> value of its dynamic `id` attribute.
    dom_ids: HashMap<ElementId, String>,
    /// Element id -> value of its dynamic `style` attribute.
    styles: HashMap<ElementId, String>,
    /// (event name, element id) of every live listener.
    listeners: Vec<(String, ElementId)>,
}

fn null_reply() -> Reply {
    Rc::new(|_| serde_json::Value::Null)
}

impl Harness {
    fn new(cfg: Cfg) -> Self {
        Self::with_reply(cfg, null_reply())
    }

    fn with_reply(cfg: Cfg, reply: Reply) -> Self {
        dioxus_html::set_event_converter(Box::new(SerializedHtmlEventConverter));
        let slot = Rc::new(RefCell::new(None));
        let evals = Rc::new(RefCell::new(Vec::new()));
        let dom = VirtualDom::new_with_props(
            app,
            Props {
                cfg,
                slot: slot.clone(),
            },
        );
        let doc: Rc<dyn Document> = Rc::new(FakeDocument {
            log: evals.clone(),
            reply,
            owners: RefCell::new(Vec::new()),
        });
        dom.provide_root_context(doc);
        let mut h = Self {
            dom,
            slot,
            evals,
            dom_ids: HashMap::new(),
            styles: HashMap::new(),
            listeners: Vec::new(),
        };
        let muts = h.dom.rebuild_to_vec();
        h.record(&muts);
        h.settle();
        h
    }

    fn record(&mut self, muts: &Mutations) {
        for m in &muts.edits {
            match m {
                Mutation::SetAttribute {
                    name,
                    value: AttributeValue::Text(v),
                    id,
                    ..
                } => {
                    if *name == "id" {
                        self.dom_ids.insert(*id, v.clone());
                    } else if *name == "style" {
                        self.styles.insert(*id, v.clone());
                    }
                }
                Mutation::NewEventListener { name, id } => {
                    self.listeners.push((name.clone(), *id));
                }
                Mutation::RemoveEventListener { name, id } => {
                    self.listeners.retain(|(n, i)| !(n == name && i == id));
                }
                _ => {}
            }
        }
    }

    /// Run spawned tasks and re-render until nothing is left to do.
    fn settle(&mut self) {
        for _ in 0..20 {
            self.dom.process_events();
            let muts = self.dom.render_immediate_to_vec();
            self.record(&muts);
        }
    }

    fn handle(&self) -> UseTableHandle<Item> {
        self.slot.borrow().expect("table mounted")
    }

    fn state(&self) -> TableState<Item> {
        let h = self.handle();
        self.dom.in_runtime(|| h.signal().peek().clone())
    }

    fn active(&self) -> Option<ActiveCell> {
        self.state().active_cell
    }

    fn set_active(&mut self, row_idx: usize, col: &'static str) {
        let h = self.handle();
        self.dom.in_runtime(|| {
            let mut sig = h.signal();
            let mut s = sig.peek().clone();
            s.active_cell = Some(ActiveCell::new(row_idx, ColumnId(col)));
            s.range_selection = vec![chorale_core::RangeSelection::single(row_idx, ColumnId(col))];
            sig.set(s);
        });
        self.settle();
    }

    /// The table's keyboard container: the element whose dynamic id ends in `-kb`.
    fn kb_element(&self) -> ElementId {
        let (_, id) = self
            .listeners
            .iter()
            .find(|(n, id)| {
                n == "keydown" && self.dom_ids.get(id).is_some_and(|v| v.ends_with("-kb"))
            })
            .expect("keyboard container listener");
        *id
    }

    fn press_on(&mut self, element: ElementId, key: Key, modifiers: Modifiers) {
        let data = SerializedKeyboardData::new(
            key,
            Code::Unidentified,
            Location::Standard,
            false,
            modifiers,
            false,
        );
        let payload: Rc<dyn Any> = Rc::new(PlatformEventData::new(Box::new(data)));
        self.dom
            .runtime()
            .handle_event("keydown", Event::new(payload, true), element);
        self.settle();
    }

    fn press(&mut self, key: Key) {
        let kb = self.kb_element();
        self.press_on(kb, key, Modifiers::empty());
    }

    fn press_mod(&mut self, key: Key, modifiers: Modifiers) {
        let kb = self.kb_element();
        self.press_on(kb, key, modifiers);
    }

    fn html(&self) -> String {
        dioxus_ssr::render(&self.dom)
    }

    /// Opening tag of the `<tr>` that renders grouped row `idx`.
    fn tr_tag(&self, idx: usize) -> Option<String> {
        let html = self.html();
        let needle = format!("data-chorale-index=\"{idx}\"");
        html.split("<tr ")
            .skip(1)
            .map(|t| t.split('>').next().unwrap_or("").to_owned())
            .find(|t| t.contains(&needle))
    }

    fn evals_since(&self, from: usize) -> Vec<String> {
        self.evals.borrow()[from..].to_vec()
    }

    fn eval_count(&self) -> usize {
        self.evals.borrow().len()
    }
}

fn view_of(s: &TableState<Item>) -> Vec<GroupedRow<Item>> {
    visible_grouped_view(s)
}

fn is_header(s: &TableState<Item>, idx: usize) -> bool {
    matches!(view_of(s).get(idx), Some(GroupedRow::Header { .. }))
}

fn header_collapsed(s: &TableState<Item>, idx: usize) -> bool {
    match view_of(s).get(idx) {
        Some(GroupedRow::Header { is_collapsed, .. }) => *is_collapsed,
        _ => panic!("row {idx} is not a header"),
    }
}

const ON_OFF: [bool; 2] = [true, false];

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
            "hook={hook}: header row 0 carries no data-chorale-index"
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
        let focus = scripts
            .iter()
            .find(|js| js.contains("tr[data-chorale-index=\"11\"]") && js.contains(".focus()"));
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

#[test]
fn grouped_rows_carry_treegrid_aria() {
    for hook in ON_OFF {
        let h = Harness::new(Cfg {
            hook,
            ..Cfg::default()
        });
        let html = h.html();
        assert!(html.contains("role=\"treegrid\""), "hook={hook}");
        let header = h.tr_tag(0).unwrap();
        assert!(header.contains("aria-level=\"1\""), "hook={hook}: {header}");
        assert!(
            header.contains("aria-expanded=\"true\""),
            "hook={hook}: {header}"
        );
        let data = h.tr_tag(1).unwrap();
        assert!(data.contains("aria-level=\"2\""), "hook={hook}: {data}");
        assert!(!data.contains("aria-expanded"), "hook={hook}: {data}");
    }
}
