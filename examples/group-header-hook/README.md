# group-header-hook

A page for testing the `group_header_renderer` prop of the Dioxus `Table` by hand. It groups 3,000 rows into 300 groups of 10.

## Run it

Install the Dioxus CLI once with `cargo install dioxus-cli`, then from the repository root:

```
dx serve --package group-header-hook
```

Open the address `dx` prints. The pale blue panel above the table shows the focused element, the active cell row index, where the active row is (header or data row, and whether it is fully visible in the table and in the window) and the live `scroll_top`.

## What to try

Items 1 to 8 and 10 map to steps in section 16a of `docs/QA.md`: item 1 to step 1, 2 to 2, 3 to 3, 4 to 4, 5 to 5 (Tab order), 6 and 7 to the Enter, Space, arrow and Ctrl+A parts of step 5, 8 to step 6 and 10 to step 7. Items 14 to 23 map to steps 8 to 17 (header rows, scroll-follow and accessibility, issue #51). Item 13 is the edge case. Items 9, 11 and 12 have no 16a counterpart.

1. Leave "group_header_renderer on" checked. Each header shows the group name, a row count, a select and a Close or Open button. The expected result is that these replace the built-in label, count and aggregates.
2. Click the pale blue background of a header, or the group name. The group does not collapse, because the row has no click toggle with a host renderer.
3. Click the chevron. The group collapses and the chevron changes from the down arrow to the right arrow. Click it again to expand.
4. Click the host Close or Open button. The group toggles, because the host calls `toggle_group`. Afterwards the arrow keys and Ctrl+A do nothing until you click a data cell, because the host content keeps every key to itself and focus is still on the button.
5. Click the chevron, then press Tab. Focus moves to the select, then to the host button, and the focused element panel names each one. With Selection on, Shift+Tab from the chevron reaches the group checkbox. Tab from a grid cell does not enter a header, and each header adds its own tab stops (checkbox, chevron, host controls). The chevron focuses itself when clicked, because Safari and macOS Firefox do not focus a button on click by default; try this step in those browsers too.
6. With focus on the chevron, press Enter or Space. The group toggles each time and the active cell does not change.
7. Click a data cell, then click a chevron, then press the arrow keys. The active cell row index in the panel changes, so arrow keys still reach the grid. Ctrl+A selects all cells too.
8. Focus a header select and press the arrow keys and letters. The active cell row index does not change, and no cell edit starts.
9. Press "Collapse all", then "Expand all". Every group collapses, then every group expands.
10. Switch "Grouped pagination" to Virtualized. The table scrolls through all 300 groups, every header row keeps the same height, and long host content is clipped instead of growing the row. To see the clipping, narrow the browser window until the Close or Open button is cut off. Switch back to Paged to get page controls.
11. Scroll down in Virtualized mode, then press "Unmount and remount table". The table comes back at the same position. The line beside the button shows the DOM `scrollTop` (read from the page, not from table state) before and after, and they must be equal. It must not show blank space or a different set of rows.
12. Turn Selection on. Each header gains a group checkbox, and the indentation and row styling are unchanged.
13. Uncheck "group_header_renderer on". Headers return to the built in look, and clicking anywhere on the header row toggles the group.

Items 14 to 23 cover keyboard navigation over header rows, scroll-follow and accessibility (issue #51). Run each with "group_header_renderer on" checked and again unchecked, unless the item says otherwise.

14. Click Item 1 in Group 000, then press Up twice. The cursor lands on the Group 000 header row: the whole row gets the blue cursor outline and the panel says "Active row: header". Press Down: the cursor returns to Item 0 in the column you started in. Arrow down from Item 9 to cross into Group 001: the Group 001 header is a stop of its own.
15. With the cursor on a header row, press Left: the group collapses and the cursor stays on the header. Press Right: it expands. Enter and Space toggle it too. Collapse a group, then press Down: the cursor goes straight to the next header.
16. With the cursor on a header row, press Tab. With the hook on and Selection off, focus moves to the chevron, then Tab reaches the select and the host button. With Selection on (hook on or off), focus moves to the group checkbox first. With the hook off and Selection off the header has no controls, so Tab moves focus out of the grid (the focused element panel no longer names the table). From the chevron, an arrow key brings focus back to the grid and moves the cursor from the header.
17. Scroll-follow, Paged. Make the browser window short enough that the bottom of the table is below the window edge. Click Item 0 and press Down 20 times or more, across several headers. After every press the active row stays fully on screen and the panel says "fully visible in table: yes, in window: yes". Press Up back to the top, then Page Down, Ctrl+End and Ctrl+Home. The same holds after each key.
18. Scroll-follow, Virtualized. Switch "Grouped pagination" to Virtualized and repeat item 17. Ctrl+End jumps to Item 2999 in Group 299 and the row is fully visible. Each Page Down moves 50 rows, headers included, and the row stays visible. Before this fix the cursor stopped at row 49, and the table only scrolled through the first page of rows after switching modes.
19. Hook on only: with the cursor on a header row, focus that header's select and press arrows, letters, Enter and Space. The cursor does not move and the group does not toggle (item 8 still holds).
20. Accessibility tree (Chrome DevTools, Elements panel, Accessibility pane, or a screen reader). Arrow onto a header row: the table's keyboard container (the focused element) has `aria-activedescendant` naming that header row, which the accessibility tree shows as a row with `aria-expanded`. Arrow onto a data row: it names the active cell, a gridcell with the item's text. With VoiceOver or NVDA, each arrow press reads the new row or cell.
21. Tab out of the grid: click a cell in the Qty column (the last column) and press Tab. Focus leaves the grid instead of staying on the cell. Click a cell in the Group column (the first column) and press Shift+Tab: focus leaves the grid backwards. With the cursor on a header row, Shift+Tab also leaves the grid.
22. Hook on and Selection on: put the cursor on a header row, press Tab (focus is on the group checkbox), then press Left. The group collapses, the cursor is on that header and focus is back on the grid. Press Tab, then Space: the checkbox ticks and the group stays as it was. Do the same from the chevron.
23. Switch to Virtualized, scroll the table well down (past Group 005) with the mouse and click a data cell there. The active cell row index in the panel is that row's index (above 49), not 49. Collapse the cursor's group with its chevron: the cursor moves to that group's header.

## Comparing the hook off against 0.2.3

With the hook off, the markup is meant to be byte for byte what v0.2.3 renders, apart from the attributes 0.2.4 adds for keyboard navigation and accessibility (issue #51): `aria-level`, `aria-expanded`, `aria-rowindex`, `data-chorale-group-index`, `data-chorale-active-row` and an `id` on header rows; `aria-level`, `aria-rowindex` and an `id` on grouped data rows; `role="treegrid"` and `aria-rowcount` on the grouped table; `role="group"` and `aria-activedescendant` on the keyboard container. To compare, run `git checkout v0.2.3` in a second clone or worktree, run `dx serve --package qa-harness`, turn on "Grouping & Aggregation", and compare the headers and click behavior with item 13. The unit test `group_header_default_markup_matches_0_2_3` pins the same markup: it removes exactly those attributes and compares the rest byte for byte.
