use std::sync::Arc;

use crate::types::{ColumnId, RowId};

/// Which visual theme the adapter applies to the rendered table.
///
/// `Light` and `Dark` inject a pre-built stylesheet on first mount.
/// `Custom` suppresses the injected stylesheet; the consumer supplies their
/// own CSS targeting the structural class names (e.g. `chorale-row`,
/// `chorale-cell`).
///
/// Defined in recon-2 § 8a.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Theme {
    /// Injects the built-in light stylesheet on first mount.
    #[default]
    Light,
    /// Injects the built-in dark stylesheet on first mount.
    Dark,
    /// Suppresses the injected stylesheet; the consumer supplies their own CSS.
    Custom,
}

impl Theme {
    /// The value an adapter sets on the [`THEME_ATTRIBUTE`] attribute of the
    /// table root element for this theme.
    ///
    /// Returns `"light"`, `"dark"`, or `"custom"`. The stylesheet returned by
    /// [`theme_stylesheet`] only defines token blocks for `"light"` and
    /// `"dark"`; `"custom"` deliberately matches no block, so every
    /// `var(--chorale-*, <fallback>)` reference in the components resolves to
    /// either the consumer's own definitions or the inline light fallback.
    ///
    /// Switching themes at runtime is a single attribute swap — no
    /// stylesheet re-injection is required.
    #[must_use]
    pub fn attribute_value(self) -> &'static str {
        match self {
            Theme::Light => "light",
            Theme::Dark => "dark",
            Theme::Custom => "custom",
        }
    }
}

/// Class name the adapters place on the table root element.
///
/// The shipped stylesheet scopes every token block under
/// `.chorale-root[data-chorale-theme="..."]`, so themes apply only inside
/// chorale tables and never leak variables into the host page. Multiple
/// tables on one page can carry different `data-chorale-theme` values
/// simultaneously.
pub const THEME_ROOT_CLASS: &str = "chorale-root";

/// Attribute the adapters set on the table root element to select which
/// token block from [`theme_stylesheet`] applies.
///
/// Valid values are produced by [`Theme::attribute_value`].
pub const THEME_ATTRIBUTE: &str = "data-chorale-theme";

/// Returns the built-in chorale theme stylesheet as a `<style>`-injectable
/// CSS string.
///
/// The stylesheet defines the full `--chorale-*` design-token contract twice:
/// once scoped to `.chorale-root[data-chorale-theme="light"]` and once to
/// `.chorale-root[data-chorale-theme="dark"]`. Adapters inject it exactly
/// once per document (idempotently, e.g. keyed by an element id) and set
/// [`THEME_ATTRIBUTE`] on the table root from the `theme` prop via
/// [`Theme::attribute_value`]. Because both palettes ship in the same
/// stylesheet, toggling between light and dark at runtime is a pure
/// attribute swap.
///
/// # Token contract
///
/// Components reference tokens as `var(--chorale-<token>, <light-default>)`,
/// where the inline fallback equals the light value below — so a table
/// renders correctly even if the stylesheet was never injected
/// (`Theme::Custom` consumers who define only a subset of tokens get light
/// defaults for the rest).
///
/// | Token | Controls |
/// |---|---|
/// | `--chorale-surface` | table/background surface; opaque bg of frozen (sticky) cells and the filter header row |
/// | `--chorale-text` | primary text (cells, buttons) |
/// | `--chorale-text-muted` | secondary text (pagination summary, range-filter bounds, toolbar labels) |
/// | `--chorale-text-subtle` | tertiary text (empty-state, ellipsis, group row counts, clear-filter glyphs) |
/// | `--chorale-text-disabled` | disabled control text |
/// | `--chorale-border` | structural borders (table outline, header bottom, toolbar/footer dividers, nav buttons) |
/// | `--chorale-divider` | light row/cell separators |
/// | `--chorale-header-bg` | column-header cell background |
/// | `--chorale-toolbar-bg` | toolbar and pagination-footer background |
/// | `--chorale-row-bg` | data-row background |
/// | `--chorale-row-hover-bg` | row hover background (reserved: adapters do not style hover yet) |
/// | `--chorale-row-selected-bg` | selected-row background |
/// | `--chorale-row-selected-divider` | row separator inside selected rows |
/// | `--chorale-accent` | accent blue: sort badges, drag outlines, editor borders, active page, ghost buttons, group chevrons |
/// | `--chorale-accent-contrast` | text on accent-filled controls |
/// | `--chorale-accent-strong` | strong accent (selection-toolbar bottom border) |
/// | `--chorale-active-cell-outline` | active-cell outline and the fill handle |
/// | `--chorale-range-bg` | range-selection / active-cell wash |
/// | `--chorale-input-bg` | filter/editor input background |
/// | `--chorale-input-border` | filter/editor input border |
/// | `--chorale-popover-bg` | dropdown / filter-popover background |
/// | `--chorale-popover-shadow` | dropdown / filter-popover box-shadow |
/// | `--chorale-frozen-shadow-color` | frozen-column divider shadow color (direction stays per-side in components) |
/// | `--chorale-button-bg` | pagination/nav button background |
/// | `--chorale-button-disabled-bg` | disabled button background |
/// | `--chorale-group-header-bg` | group-header row background |
/// | `--chorale-group-header-border` | group-header row bottom border |
/// | `--chorale-error` | validation-error text |
/// | `--chorale-badge-{green,yellow,red,gray,default}-{bg,text}` | badge palette pairs |
///
/// Each block also sets [`color-scheme`], so native widgets (checkboxes,
/// selects, scrollbars) follow the active theme without bespoke styling.
///
/// The pre-existing `--chorale-frozen-divider-shadow` escape hatch (a full
/// `box-shadow` value consumers may override) is intentionally *not* defined
/// here: its value is direction-dependent (left- vs right-frozen columns use
/// mirrored offsets), so a single themed value would be wrong for one side.
/// The theme instead drives the color component via
/// `--chorale-frozen-shadow-color`.
///
/// [`color-scheme`]: https://developer.mozilla.org/en-US/docs/Web/CSS/color-scheme
#[must_use]
pub fn theme_stylesheet() -> &'static str {
    THEME_STYLESHEET
}

/// Shipped light + dark token definitions. Light values are the historical
/// hardcoded inline colors, so `Theme::Light` is pixel-identical to the
/// pre-token rendering.
const THEME_STYLESHEET: &str = r#".chorale-root[data-chorale-theme="light"] {
  color-scheme: light;
  --chorale-surface: #fff;
  --chorale-text: #333;
  --chorale-text-muted: #555;
  --chorale-text-subtle: #999;
  --chorale-text-disabled: #aaa;
  --chorale-border: #ddd;
  --chorale-divider: #eee;
  --chorale-header-bg: #f8f9fa;
  --chorale-toolbar-bg: #fafafa;
  --chorale-row-bg: #fff;
  --chorale-row-hover-bg: #f5f5f5;
  --chorale-row-selected-bg: #eff6ff;
  --chorale-row-selected-divider: #dbeafe;
  --chorale-accent: #4a90e2;
  --chorale-accent-contrast: #fff;
  --chorale-accent-strong: #1d4ed8;
  --chorale-active-cell-outline: #0078d4;
  --chorale-range-bg: rgba(0, 120, 212, 0.1);
  --chorale-input-bg: #fff;
  --chorale-input-border: #ccc;
  --chorale-popover-bg: #fff;
  --chorale-popover-shadow: 0 2px 8px rgba(0, 0, 0, 0.15);
  --chorale-frozen-shadow-color: rgba(0, 0, 0, 0.15);
  --chorale-button-bg: #fff;
  --chorale-button-disabled-bg: #f0f0f0;
  --chorale-group-header-bg: #f0f4ff;
  --chorale-group-header-border: #dce4ff;
  --chorale-error: #dc2626;
  --chorale-badge-green-bg: #d1fae5;
  --chorale-badge-green-text: #065f46;
  --chorale-badge-yellow-bg: #fef3c7;
  --chorale-badge-yellow-text: #92400e;
  --chorale-badge-red-bg: #fee2e2;
  --chorale-badge-red-text: #991b1b;
  --chorale-badge-gray-bg: #f3f4f6;
  --chorale-badge-gray-text: #374151;
  --chorale-badge-default-bg: #e5e7eb;
  --chorale-badge-default-text: #1f2937;
}
.chorale-root[data-chorale-theme="dark"] {
  color-scheme: dark;
  --chorale-surface: #1e1e1e;
  --chorale-text: #d4d4d4;
  --chorale-text-muted: #a0a0a0;
  --chorale-text-subtle: #8a8a8a;
  --chorale-text-disabled: #6b6b6b;
  --chorale-border: #3c3c3c;
  --chorale-divider: #2e2e2e;
  --chorale-header-bg: #252526;
  --chorale-toolbar-bg: #252526;
  --chorale-row-bg: #1e1e1e;
  --chorale-row-hover-bg: #2a2d2e;
  --chorale-row-selected-bg: #264f78;
  --chorale-row-selected-divider: #3a5a80;
  --chorale-accent: #64a4ec;
  --chorale-accent-contrast: #10243e;
  --chorale-accent-strong: #4d7fd6;
  --chorale-active-cell-outline: #3794ff;
  --chorale-range-bg: rgba(55, 148, 255, 0.18);
  --chorale-input-bg: #3c3c3c;
  --chorale-input-border: #4f4f4f;
  --chorale-popover-bg: #252526;
  --chorale-popover-shadow: 0 2px 8px rgba(0, 0, 0, 0.6);
  --chorale-frozen-shadow-color: rgba(0, 0, 0, 0.55);
  --chorale-button-bg: #2d2d30;
  --chorale-button-disabled-bg: #2a2a2a;
  --chorale-group-header-bg: #263246;
  --chorale-group-header-border: #3a4a66;
  --chorale-error: #f48771;
  --chorale-badge-green-bg: #1c3d31;
  --chorale-badge-green-text: #6ee7b7;
  --chorale-badge-yellow-bg: #3d3320;
  --chorale-badge-yellow-text: #fcd34d;
  --chorale-badge-red-bg: #421c1c;
  --chorale-badge-red-text: #fca5a5;
  --chorale-badge-gray-bg: #34373c;
  --chorale-badge-gray-text: #d1d5db;
  --chorale-badge-default-bg: #3a3d41;
  --chorale-badge-default-text: #e5e7eb;
}
"#;

/// Row metadata passed to `RowClassFn` resolvers.
///
/// Defined in recon-2 § 8b.
#[non_exhaustive]
#[derive(Clone, Debug)]
pub struct Row<TRow> {
    /// Stable identifier for this row across sort, filter, and pagination.
    pub id: RowId,
    /// The row's data value.
    pub data: TRow,
    /// Zero-based index within the current post-sort / post-filter /
    /// post-pagination visible rows slice.
    pub index: usize,
    /// Whether this row is currently in the selection set.
    pub is_selected: bool,
}

/// Cell metadata passed to `CellClassFn` resolvers.
///
/// Pure-data fields only (no Dioxus types) so this type stays in
/// `chorale-core` per CHORALE-CORE-1.
#[non_exhaustive]
#[derive(Clone, Debug)]
pub struct CellInfo<'a, TRow> {
    /// Stable identifier for the row containing this cell.
    pub row_id: RowId,
    /// Identifier of the column this cell belongs to.
    pub column_id: ColumnId,
    /// Reference to the full row data.
    pub row: &'a TRow,
    /// Whether the row containing this cell is currently selected.
    pub is_selected: bool,
}

/// Closure type that resolves a CSS class string for a row.
/// Stored in `Arc` so `TableProps` can be `Clone`.
pub type RowClassFn<TRow> = Arc<dyn Fn(&Row<TRow>) -> String + Send + Sync>;

/// Closure type that resolves a CSS class string for a body cell.
/// Stored in `Arc` so `ColumnDef` can be `Clone`.
pub type CellClassFn<TRow> = Arc<dyn Fn(&CellInfo<TRow>) -> String + Send + Sync>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attribute_value_maps_every_variant() {
        assert_eq!(Theme::Light.attribute_value(), "light");
        assert_eq!(Theme::Dark.attribute_value(), "dark");
        assert_eq!(Theme::Custom.attribute_value(), "custom");
    }

    #[test]
    fn stylesheet_defines_a_block_for_light_and_dark_but_not_custom() {
        let css = theme_stylesheet();
        assert!(css.contains(r#".chorale-root[data-chorale-theme="light"]"#));
        assert!(css.contains(r#".chorale-root[data-chorale-theme="dark"]"#));
        // Theme::Custom must resolve to no token block — the consumer
        // supplies their own definitions.
        assert!(!css.contains(r#"data-chorale-theme="custom""#));
    }

    #[test]
    fn every_token_is_defined_in_both_theme_blocks() {
        let css = theme_stylesheet();
        let dark_start = css
            .find(r#"[data-chorale-theme="dark"]"#)
            .expect("dark block present");
        let (light_block, dark_block) = css.split_at(dark_start);

        // Collect token names declared in the light block and require each
        // to also be declared in the dark block (and vice versa via count).
        let light_tokens: Vec<&str> = light_block
            .lines()
            .filter_map(|l| l.trim().strip_prefix("--chorale-"))
            .filter_map(|l| l.split(':').next())
            .collect();
        assert!(
            !light_tokens.is_empty(),
            "light block declares at least one --chorale-* token"
        );
        for token in &light_tokens {
            let decl = format!("--chorale-{token}:");
            assert!(
                dark_block.contains(&decl),
                "dark block missing token --chorale-{token}"
            );
        }
        let dark_count = dark_block
            .lines()
            .filter(|l| l.trim().starts_with("--chorale-"))
            .count();
        assert_eq!(
            light_tokens.len(),
            dark_count,
            "light and dark blocks declare the same number of tokens"
        );
    }

    #[test]
    fn light_values_preserve_the_historical_inline_colors() {
        let css = theme_stylesheet();
        let dark_start = css
            .find(r#"[data-chorale-theme="dark"]"#)
            .expect("dark block present");
        let light_block = &css[..dark_start];
        // Spot-check the load-bearing light values against the colors that
        // were hardcoded in the adapters before tokenization, so Light mode
        // stays pixel-identical through Phase 2.
        for expected in [
            "--chorale-accent: #4a90e2",
            "--chorale-active-cell-outline: #0078d4",
            "--chorale-range-bg: rgba(0, 120, 212, 0.1)",
            "--chorale-row-selected-bg: #eff6ff",
            "--chorale-header-bg: #f8f9fa",
            "--chorale-group-header-bg: #f0f4ff",
            "--chorale-border: #ddd",
            "--chorale-divider: #eee",
        ] {
            assert!(light_block.contains(expected), "missing: {expected}");
        }
    }

    #[test]
    fn frozen_divider_shadow_escape_hatch_is_not_themed() {
        // --chorale-frozen-divider-shadow is a per-side full box-shadow
        // override; theming it would mirror one side's shadow incorrectly.
        assert!(!theme_stylesheet().contains("--chorale-frozen-divider-shadow"));
    }
}
