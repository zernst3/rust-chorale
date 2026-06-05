# leptos-virtualized-1m-rows

Leptos example demonstrating two-stage render with 1,000,000 rows. The page
shows an "Initializing…" message for ~1–2 s while the dataset is built, then
renders the virtualized table.

## Running

```sh
# From this directory:
trunk serve --open

# Or from the workspace root:
cd examples/leptos-virtualized-1m-rows && trunk serve --open
```

Requires [Trunk](https://trunkrs.dev): `cargo install trunk`

> **Note:** This is a Leptos/WASM example. Do **not** use `dx serve` (that is for
> Dioxus examples). Use `trunk serve` as shown above.
