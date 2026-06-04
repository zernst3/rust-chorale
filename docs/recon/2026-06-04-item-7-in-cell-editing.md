# Item 7: In-Cell Editing

## Problem

chorale v0.1.0 renders every cell as read-only. When a user needs to edit data directly
in the table — updating a value, correcting a field, toggling a boolean — they must
implement a custom cell renderer that manages its own input state, wires its own commit
logic, and co-ordinates with the parent app's persistence layer. This is significant
boilerplate for what users expect as a first-class feature.

`leptos-struct-table` ships inline cell editing via a slot-based escape hatch that lets
the caller render any element in edit mode. `table-rs` does not ship cell editing at all.
v0.2.0 adds a principled in-cell editing API to chorale-core so any adapter can implement
the feature consistently without duplicating state management.

The design centers on two orthogonal concerns: (a) which cell is currently being edited
(`TableState` tracks this as `editing: Option<(RowId, ColumnId)>`), and (b) what kind of
input to render (`ColumnDef` carries `editor: Option<EditorKind>`). The adapter handles
the rendering and event wiring; core handles the state machine.

## Proposed Public API

### `chorale-core`

```rust
/// Which cell (if any) is open for editing.
/// Added to `TableState` as an additive field.
pub editing: Option<EditTarget>,

/// Identifies the cell currently open for editing.
#[non_exhaustive]
pub struct EditTarget {
    pub row_id: RowId,
    pub column_id: ColumnId,
}

/// What kind of editor the adapter should render for a column.
#[non_exhaustive]
pub enum EditorKind {
    Text,
    Number { min: Option<f64>, max: Option<f64>, step: Option<f64> },
    Date,
    BoolToggle,
    /// Caller supplies a custom render component via the adapter's cell-renderer hook.
    Custom,
}

/// Builder method added to ColumnDef.
impl<TRow> ColumnDef<TRow> {
    #[must_use]
    pub fn editor(self, kind: EditorKind) -> Self;
}

/// State transitions (pure, per CHORALE-CORE-2):

/// Open an editor for a specific cell. Returns Err if the column has no EditorKind set.
pub fn start_edit(
    state: &TableState<TRow>,
    row_id: RowId,
    column_id: ColumnId,
) -> Result<TableState<TRow>, StateError>;

/// Close the editor and return a state with `editing: None`.
/// The caller is responsible for persisting the new value before calling this.
pub fn commit_edit(state: &TableState<TRow>) -> TableState<TRow>;

/// Cancel the editor without persisting; returns state with `editing: None`.
pub fn cancel_edit(state: &TableState<TRow>) -> TableState<TRow>;
```

### Validation hook (adapter boundary)

Validation is the host app's responsibility, not chorale-core's. The adapter exposes an
optional callback prop:

```rust
/// Called before `commit_edit` is dispatched. If the callback returns `Err(msg)`,
/// the edit is not committed and the error message is rendered below the input.
pub on_validate_edit: Option<EventHandler<EditValidation>>,

pub struct EditValidation {
    pub row_id: RowId,
    pub column_id: ColumnId,
    pub raw_value: String,   // the text the user typed
}
// Host returns Result<(), String> — Ok continues to commit, Err shows the message.
```

This keeps validation logic in the host app and out of chorale-core, consistent with
CHORALE-CORE-1.

### Callsite shape

```rust
let columns: Vec<ColumnDef<Invoice>> = vec![
    ColumnDef::new("amount", "Amount", |r| CellValue::Currency(r.amount, CurrencyCode::USD))
        .editor(EditorKind::Number { min: Some(0.0), max: None, step: Some(0.01) }),
    ColumnDef::new("note", "Note", |r| CellValue::Text(r.note.clone()))
        .editor(EditorKind::Text),
];

rsx! {
    Table {
        handle: handle,
        on_validate_edit: move |v: EditValidation| {
            // host validates; returns Ok(()) or Err("must be positive".into())
            if v.raw_value.parse::<f64>().map(|x| x > 0.0).unwrap_or(false) {
                Ok(())
            } else {
                Err("Amount must be a positive number.".into())
            }
        },
        on_commit_edit: move |committed: CommittedEdit| {
            // host persists; chorale has already updated TableState
            spawn(async move { api::update_row(committed.row_id, committed.column_id, committed.value).await });
        },
    }
}
```

## Internal Design

**State machine:** `editing` tracks which cell is open. `start_edit` validates that the
target column has an `EditorKind` set (returns `StateError::ColumnNotEditable` otherwise)
and returns a new state with `editing: Some(EditTarget { row_id, column_id })`.

**Adapter rendering:** when `state.editing == Some(target)`, the adapter renders an
`<input>` (or `<select>` / `<input type="date">` etc.) in place of the read-only cell.
On blur or Enter, the adapter fires `on_validate_edit`; if Ok, calls `commit_edit` and
fires `on_commit_edit`. On Escape, calls `cancel_edit`.

**Optimistic vs pessimistic:** the design is optimistic — `commit_edit` closes the editor
immediately; the host decides whether to roll back on a persistence failure. This matches
the TanStack Table convention and keeps the state machine synchronous (CHORALE-CORE-2).
Pessimistic commit (keeping the editor open until the server responds) would require async
in core, violating CHORALE-CORE-2.

**Keyboard navigation:** Tab key moves editing to the next editable cell (same row, next
column with an `EditorKind`). Shift+Tab moves backwards. This is wired in the adapter,
not core.

## Backwards Compatibility

`editing: Option<EditTarget>` on `TableState` is additive. `TableState` is
`#[non_exhaustive]` in v0.1.0, so cross-crate callers cannot use struct-literal
construction; adding the field does not break compilation downstream. The field defaults
to `None` in `TableState::new`, so existing callers see no change.

`EditorKind` is a new `#[non_exhaustive]` enum. Cross-crate matches already require a
wildcard arm, so adding variants later is non-breaking.

The `.editor(kind)` builder method is additive; existing `ColumnDef` chains without it
produce `editor: None` (no editor — same as v0.1.0 behavior).

The `on_validate_edit` and `on_commit_edit` props on `Table` are optional. Existing
`Table` callsites without these props compile and behave identically to v0.1.0.

## Test Plan

Per TESTS-1:

- `start_edit`: happy path — returns state with `editing: Some(EditTarget { ... })`.
- `start_edit`: error path — column with no `EditorKind` returns `Err(StateError::ColumnNotEditable)`.
- `start_edit`: already editing a different cell — prior edit is implicitly cancelled,
  new target is set (no orphaned lock).
- `commit_edit`: returns state with `editing: None`; all other fields unchanged.
- `cancel_edit`: returns state with `editing: None`; all other fields unchanged.
- `cancel_edit` when `editing: None` — no-op (returns state unchanged).
- Column builder: `ColumnDef::new(...).editor(EditorKind::Text)` yields a column with the
  expected `editor` field.
- Invariant: `commit_edit(cancel_edit(state)) == cancel_edit(state)` (idempotent close).

## Open Questions / Decisions Zach Must Sign Off (API-1)

1. **`EditTarget` struct vs a `(RowId, ColumnId)` tuple in `TableState`.** Recommendation:
   named `EditTarget` struct per ROBUSTNESS-1 — field names survive refactors and are
   self-documenting. Tuple is terser but fragile.

2. **Optimistic vs pessimistic commit.** Recommendation: optimistic (editor closes
   immediately, host rolls back on error). Pessimistic would require async state in core,
   violating CHORALE-CORE-2. Zach should confirm this is acceptable for the v0.2.0 scope;
   a pessimistic mode can be added in v0.3 if users request it.

3. **`on_commit_edit` callback receives the new value as `CellValue` or as `String`?**
   Recommendation: `String` (the raw text). The host app knows its domain type and can
   parse accordingly. Delivering `CellValue` would require chorale to parse the input,
   coupling core parsing to adapter input format.

4. **Should `start_edit` fail silently (return `Ok(state)` unchanged) or return `Err`
   when the column has no editor?** Recommendation: return `Err` to surface programming
   errors early. Silent no-op would hide mistakes in code that wires `start_edit` to a
   row-click handler without checking column editability.

5. **Tab/Shift+Tab navigation: in adapter only, or should `next_editable_cell` / 
   `prev_editable_cell` transitions live in core?** Recommendation: core, to be
   reusable across adapters. Simple enough (`Vec<ColumnId>` walk with `EditorKind` check).
   This is a new public function; include in this batch's API-1 surface.
