# group-header-hook

A page for testing the `group_header_renderer` prop of the Dioxus `Table` by hand. It groups 3,000 rows into 300 groups of 10.

## Run it

Install the Dioxus CLI once with `cargo install dioxus-cli`, then from the repository root:

```
dx serve --package group-header-hook
```

Open the address `dx` prints. The grey panel above the table shows the focused element, the active cell row index and the live `scroll_top`.

## What to try

Each item matches section 16a of `docs/QA.md`.

1. Leave "group_header_renderer on" checked. Each header shows the group name, a row count, a select and a Close or Open button. The expected result is that these replace the built-in label, count and aggregates.
2. Click the grey background of a header, or the group name. The group does not collapse, because the row has no click toggle with a host renderer.
3. Click the chevron. The group collapses and the chevron changes from the down arrow to the right arrow. Click it again to expand.
4. Click the host Close or Open button. The group toggles, because the host calls `toggle_group`.
5. Click the chevron, then press Tab. Focus moves to the select, then to the host button, and the focused element panel names each one. With Selection on, Shift+Tab from the chevron reaches the group checkbox. Tab from a grid cell does not enter a header.
6. With focus on the chevron, press Enter or Space. The group toggles each time and the active cell does not change.
7. Click a data cell, then click a chevron, then press the arrow keys. The active cell row index in the panel changes, so arrow keys still reach the grid. Ctrl+A selects all cells too.
8. Focus a header select and press the arrow keys and letters. The active cell row index does not change, and no cell edit starts.
9. Press "Collapse all", then "Expand all". Every group collapses, then every group expands.
10. Switch "Grouped pagination" to Virtualized. The table scrolls through all 300 groups, every header row keeps the same height, and long host content is clipped instead of growing the row. Switch back to Paged to get page controls.
11. Scroll down in Virtualized mode, then press "Unmount and remount table". The table comes back at the same position and the line beside the button shows `scroll_top` before and after with the same value. It must not show blank space.
12. Turn Selection on. Each header gains a group checkbox, and the indentation and row styling are unchanged.
13. Uncheck "group_header_renderer on". Headers return to the built in look, and clicking anywhere on the header row toggles the group.

## Comparing the hook off against 0.2.3

With the hook off, the markup is meant to be byte for byte what v0.2.3 renders. To compare, run `git checkout v0.2.3` in a second clone or worktree, run `dx serve --package qa-harness`, turn on "Grouping & Aggregation", and compare the headers and click behavior with item 13. The unit test `group_header_default_markup_matches_0_2_3` pins the same markup.
