# Keyboard navigation

Chorale tables are fully keyboard navigable. Both adapters (Dioxus and Leptos)
share the same key bindings, driven by the same `chorale-core` transitions, so
everything below applies identically to each, except the group header rows of
a grouped table, which are keyboard stops in the Dioxus adapter only (see
[Group header rows](#group-header-rows-grouped-tables-dioxus)).

## Focus model

Every table — including each nested master/detail sub-table — is its own
keyboard region with a single focusable container (`tabindex="0"`). Keys only
act on the table that currently holds focus. You give a table focus by:

- **Clicking** any cell in it, or
- **Tabbing** to it from elsewhere on the page, or
- **Descending** into a sub-table from its parent (see
  [Master/detail navigation](#masterdetail-tree-navigation)).

The instant a table gains focus, if no cell is selected yet it highlights its
first cell, so arrow keys are immediately usable. Clicking a specific cell
selects that cell instead.

## Cell navigation

| Key | Action |
| --- | --- |
| `↑` `↓` `←` `→` | Move the active cell one step. Vertical moves skip over open detail panels. |
| `Shift` + arrow | Extend the range selection from the active cell. |
| `Ctrl`/`Cmd` + arrow | Jump to the data edge in that direction. |
| `Home` / `End` | First / last column of the current row. |
| `Ctrl`/`Cmd` + `Home` / `End` | First / last cell of the whole table. |
| `Page Up` / `Page Down` | Move up / down by one page of rows. |
| `Tab` / `Shift` + `Tab` | Move to the next / previous cell. |
| `Ctrl`/`Cmd` + `A` | Select all cells. |
| `Ctrl`/`Cmd` + `C` | Copy the selection (TSV). |
| `Ctrl`/`Cmd` + `V` | Paste into the selection. |

## Editing

| Key | Action |
| --- | --- |
| `Enter` or `F2` | Start editing the active data cell (if its column is editable). |
| `Enter` | Commit the edit. |
| `Esc` | Cancel the edit. With no edit open, clears the selection (and, in a sub-table, returns focus to the parent). |

## Master/detail (tree) navigation

When a table has a `detail_renderer`, a chevron (expand/collapse arrow) column
appears to the left of the data columns. **The chevron is a real navigable
column** — it participates in arrow-key navigation exactly like a data column.

### Expanding and collapsing a row

1. Navigate **`←` Left** until the **chevron itself is highlighted** (it is the
   leftmost column). `↑` / `↓` keep you in the chevron column as you move
   between rows; `→` Right moves into the data cells.
2. Press **`Enter`** to expand or collapse that row.

`Enter` only toggles the row when the **chevron** is the highlighted cell.
`Enter` on a data cell starts editing instead.

### Entering a sub-table

> **You must be highlighted on the chevron of an already-expanded row to enter
> its sub-table.** Pressing `Tab` anywhere else in the row just moves to the
> next cell — it will **not** descend into the sub-table. This is intentional:
> the chevron is the single, predictable doorway into the nested grid.

1. Expand the row (see above) so its sub-table is visible.
2. With the **chevron still highlighted**, press **`Tab`** once.
3. Focus moves into the sub-table and its first cell is highlighted, ready to
   navigate. (A single `Tab` is all it takes — the sub-table selects its first
   cell on entry so you never land on an invisible, empty selection.)

### Inside and back out of a sub-table

| Key | Action |
| --- | --- |
| `↑` `↓` `←` `→` | Navigate within the sub-table only. The parent table does not move. |
| `Esc` | Leave the sub-table and return focus to the parent row's chevron. |

Sub-tables nest arbitrarily: the same `Tab`-in / `Esc`-out model applies at
every level.

## Group header rows (grouped tables, Dioxus)

In a grouped table every group header row is a stop for the arrow keys, as in
the ARIA treegrid pattern. The header row under the cursor shows the same
outline as the active cell (`--chorale-active-cell-outline`). This works with
and without `group_header_renderer`.

| Key on a header row | Action |
| --- | --- |
| `↑` `↓` | Move to the previous / next row, header or data. The column is kept, so arrowing through a header returns to the same column. |
| `←` | Collapse the group (nothing if it is already collapsed). |
| `→` | Expand the group (nothing if it is already expanded). |
| `Enter` or `Space` | Toggle the group. |
| `Tab` | Move focus into the header's own controls: the group checkbox, the chevron and the host content. Nothing happens when the header has none (built-in header with selection off). |
| `Shift` + `Tab` | Nothing; the cursor stays on the header. |
| `Home` / `End`, `Page Up` / `Page Down`, `Ctrl`/`Cmd` + arrows and `Ctrl`/`Cmd` + `Home` / `End` | Move as for data rows, counting header rows. |

The Leptos adapter is unchanged and keeps the 0.2.3 behavior: in a grouped
table its cursor can still land on a header row with no outline drawn.

On the group checkbox or the chevron, `Space`, `Enter` and `Tab` stay with
that control (`Space` ticks the checkbox) and arrow keys move focus back to the
grid and move the cursor.

### Row model

A grouped table has one visible-row model: `chorale_core::visible_grouped_view`,
the group header rows and data rows in display order. The Dioxus adapter
renders row `i` of that list as body row `i` (`data-chorale-group-index="i"` on
a header, `data-chorale-index="i"` on a data row), and the keyboard cursor's
`ActiveCell::row_idx` is an index into it. Navigation that crosses rows, such
as a future "Enter or Tab moves the edit to the next row" (today
`next_editable_cell` cycles within one row), should walk that list and skip
header entries rather than re-derive row indexes.

Scroll-follow is keyboard driven: only an arrow, `Page Up` / `Page Down`,
`Home` / `End` or `Tab` move reveals the active row. Scrolling with the mouse,
trackpad or scrollbar never snaps the view back to it, including a scroll that
starts while a reveal is still settling.

Known limitation, unchanged from 0.2.3: range selection (`Shift` + arrows) and
copy and paste in a grouped table still use ungrouped row positions.

`Enter` or `F2` on a grouped data row edits that row. After every move the
active row is scrolled fully into view: below the sticky header inside the
table, and on screen when the page or another ancestor is what scrolls.

The grouped `<table>` has `role="treegrid"`. Header rows carry `aria-level`
(1 for the outermost group) and `aria-expanded`, and data rows carry
`aria-level` one deeper than the innermost group.

## Group headers with a host renderer

When `group_header_renderer` is set, the group chevron is a real
`<button class="chorale-group-toggle">` and the header row no longer toggles on
click. The header controls are focusable, reached by click, by `Tab` with the
cursor on the header row (see above), or `Shift+Tab` from below; `Tab` does NOT
enter a header from a data cell. Once focus is inside,
`Tab` and `Shift+Tab` move natively among the header controls, left to right.
Each header adds its own tab stops (checkbox, chevron and every focusable host
element):

1. The per-group selection checkbox (only when selection is enabled).
2. The chevron button: `Enter` or `Space` collapses or expands the group.
3. Each focusable element the host renders (selects, buttons, inputs), in DOM
   order.

Keys pressed inside the host content or on the checkbox, and `Enter`, `Space`
and `Tab` on the chevron, do not reach the grid's key handler, so they never
move the active cell or start an edit. Other keys on the chevron (arrows,
`Ctrl+C`, `Ctrl+A`) still reach the grid, and focus moves back to the grid's
keyboard container first, so a header that scrolls out of the virtualized
window cannot leave focus on `<body>`. The chevron focuses itself on click
(Safari and macOS Firefox do not focus a button on click). After a click on a
button inside the host content, focus stays in the header and every key goes
to the host content (arrows and `Ctrl+A` do nothing) until a data cell is
clicked. In virtualized mode, focus outlines
inside host content may clip at the fixed row height. Hosts that want a label
to toggle the group call `handle.toggle_group(ctx.key)`.

## Why the chevron-only entry rule

Tab is also the ordinary "next cell" key, so it has to mean something
unambiguous everywhere. Tying sub-table entry to the chevron — the one cell that
is conceptually "the row's handle" — keeps `Tab` predictable in data cells while
giving the tree a single, discoverable doorway. It mirrors how a file tree or an
outline behaves: you operate on a row from its disclosure arrow, not from its
content.
