# Keyboard navigation

Chorale tables are fully keyboard navigable. Both adapters (Dioxus and Leptos)
share the same key bindings, driven by the same `chorale-core` transitions, so
everything below applies identically to each, with these Dioxus-only
exceptions:
- grouped tables, where header rows are keyboard stops and every key resolves
  rows in the grouped view (see
  [Group header rows](#group-header-rows-grouped-tables-dioxus));
- scroll-follow, which keeps the active row on screen (see
  [Scroll-follow](#scroll-follow-dioxus)) in ungrouped tables too;
- `Tab` leaving the grid at the last or first column.

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
| `Tab` / `Shift` + `Tab` | Move to the next / previous cell. In the last / first column the browser moves focus out of the grid, which is one Tab stop (Dioxus). |
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
| `F2` | Nothing (a header has no cell to edit). |
| `Tab` | Move focus into the header's own controls: the group checkbox, the chevron and the host content. When the header has none (built-in header with selection off) the browser moves focus out of the grid. |
| `Shift` + `Tab` | The browser moves focus out of the grid; the cursor stays on the header. |
| `Home` / `End`, `Page Up` / `Page Down`, `Ctrl`/`Cmd` + arrows and `Ctrl`/`Cmd` + `Home` / `End` | Move as for data rows, counting header rows (`Page Up` / `Page Down` move by `page_size` rows, headers included). |

On a header's group checkbox (hook on or off) or its chevron, `Space`, `Enter`
and `Tab` stay with that control (`Space` ticks the checkbox, `Enter` and
`Space` on the chevron toggle the group). An arrow key puts the cursor on that
header, moves focus back to the grid and acts there: `←` collapses, `→` expands,
`↑` / `↓` move from the header. With the hook and selection on, `Tab` from a
header row lands on the checkbox first.

On a grouped data row, `Enter` or `F2` edits that row, `Enter` on the
detail-expander column toggles that row's panel, and `Space` does nothing (it
no longer scrolls the page). `Shift` + arrows extend the range over grouped
rows. `Ctrl`/`Cmd` + `C` copies the data rows under the range and
`Ctrl`/`Cmd` + `V` pastes onto consecutive data rows from a data row; a header
row is never a copy, paste or fill target. When a group is collapsed or
expanded by mouse or by the host, the cursor stays on its row, or moves to its
group's header when that group collapsed.

The Leptos adapter is unchanged and keeps the 0.2.3 behavior: in a grouped
table its cursor can still land on a header row with no outline drawn.

### Row model

A grouped table has one visible-row model: `chorale_core::visible_grouped_view`,
the group header rows and data rows in display order. The Dioxus adapter
renders row `i` of that list as body row `i` (`data-chorale-group-index="i"` on
a header, `data-chorale-index="i"` on a data row), and the keyboard cursor's
`ActiveCell::row_idx` is an index into it. Clicks, `Shift` + arrows, copy,
paste and fill all resolve rows through it. Navigation that crosses rows, such
as a future "Enter or Tab moves the edit to the next row" (today
`next_editable_cell` cycles within one row), should walk that list and skip
header entries rather than re-derive row indexes.

### Screen readers

The grouped `<table>` has `role="treegrid"` and `aria-rowcount` (all rows of
the grouped view plus the `<thead>` rows, rendered or not). Every row has
`aria-rowindex`; header rows carry `aria-level` (1 for the outermost group)
and `aria-expanded`, and data rows carry `aria-level` one deeper than the
innermost group. Focus stays on the table's keyboard container, a
`role="group"` whose `aria-activedescendant` names the active header row or
data cell, so assistive technology follows the cursor. While virtualization
has the active row unmounted, nothing is named.

## Scroll-follow (Dioxus)

After an arrow, `Page Up` / `Page Down`, `Home` / `End` or `Tab` move, and
after `Space` on a grouped data row, the active row is scrolled fully into
view: below the sticky header inside the table, then on screen in every
scrolling ancestor and the window. This applies to grouped and ungrouped
`Paged` and virtualized tables, so arrow keys can scroll the page. A fixed app
bar laid over the page can still cover the row.

Scroll-follow is keyboard driven only. Scrolling with the mouse, trackpad or
scrollbar never snaps the view back to the active row, including a scroll of
the table, an ancestor or the window that starts while a reveal is still
settling.

Exception: in Inline mode (`inline: true`) the table has no scroll container
of its own and leaves scrolling to the host.

## Group headers with a host renderer

When `group_header_renderer` is set, the group chevron is a real
`<button class="chorale-group-toggle">` and the header row no longer toggles on
click. The header controls are focusable, reached by click, by `Tab` with the
cursor on the header row (see above), or `Shift+Tab` from a later tab stop;
`Tab` does NOT enter a header from a data cell. Once focus is inside,
`Tab` and `Shift+Tab` move natively among the header controls, left to right.
Each header adds its own tab stops (checkbox, chevron and every focusable host
element):

1. The per-group selection checkbox (only when selection is enabled).
2. The chevron button: `Enter` or `Space` collapses or expands the group.
3. Each focusable element the host renders (selects, buttons, inputs), in DOM
   order.

Keys pressed inside the host content, and `Enter`, `Space` and `Tab` on the
checkbox or the chevron, do not reach the grid's key handler, so they never
move the active cell or start an edit. Other keys on the checkbox or the
chevron (arrows, `Ctrl+C`, `Ctrl+A`) still reach the grid, and focus moves back
to the grid's keyboard container first, so a header that scrolls out of the
virtualized window cannot leave focus on `<body>`. An arrow key also puts the
cursor on that header first, so `←` / `→` collapse and expand it (see
[Group header rows](#group-header-rows-grouped-tables-dioxus)). The chevron focuses itself on click
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
