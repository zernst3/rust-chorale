# Keyboard navigation

Chorale tables are fully keyboard navigable. Both adapters (Dioxus and Leptos)
share the same key bindings, driven by the same `chorale-core` transitions, so
everything below applies identically to each.

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

## Why the chevron-only entry rule

Tab is also the ordinary "next cell" key, so it has to mean something
unambiguous everywhere. Tying sub-table entry to the chevron — the one cell that
is conceptually "the row's handle" — keeps `Tab` predictable in data cells while
giving the tree a single, discoverable doorway. It mirrors how a file tree or an
outline behaves: you operate on a row from its disclosure arrow, not from its
content.
