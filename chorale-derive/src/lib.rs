//! `chorale-derive`: `#[derive(TableRow)]` for automatic `ColumnDef` generation.
//!
//! Apply to a concrete (non-generic) struct whose fields you want to expose as
//! chorale table columns:
//!
//! ```rust,ignore
//! use chorale_derive::TableRow;
//!
//! #[derive(TableRow, Clone, PartialEq)]
//! pub struct Employee {
//!     pub id: u32,
//!     pub name: String,
//!     pub salary: f64,
//!     pub active: bool,
//!     #[chorale(skip)]
//!     pub internal_token: String,
//! }
//!
//! // Generated:
//! // impl Employee {
//! //     pub fn chorale_columns() -> Vec<chorale_core::ColumnDef<Employee>> { ... }
//! //     pub fn chorale_columns_with_rows(rows: &[Employee]) -> Vec<chorale_core::ColumnDef<Employee>> { ... }
//! // }
//! ```
//!
//! `chorale_columns()` uses static defaults. `chorale_columns_with_rows(rows)`
//! additionally derives data-aware filter bounds from the provided rows:
//!
//! - Numeric columns (integers / floats, including `Option<T>`) without an
//!   explicit `filter = "..."` override get a `FilterKind::NumericRange` whose
//!   `min` / `max` are the real minimum / maximum over `rows`. The `step` is
//!   `(max - min) / 100` snapped **down** to the nearest power of 10 (clamped
//!   to at least `1.0` for integer columns). When `rows` is empty (or contains
//!   no finite values), the static defaults are used instead, so the range is
//!   never inverted.
//! - Columns annotated `#[chorale(filter = "MultiSelect")]` **without** an
//!   `options = [...]` list get their options populated from the sorted
//!   distinct stringified values in `rows`, capped at the first 50 (sorted).
//!

extern crate proc_macro;

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{
    parse_macro_input, spanned::Spanned, Data, DeriveInput, Error, Field, Fields, GenericParam,
    Lit, Type, TypePath,
};

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Derive macro that generates a `chorale_columns() -> Vec<ColumnDef<Self>>`
/// inherent method on the annotated struct.
#[proc_macro_derive(TableRow, attributes(chorale))]
pub fn table_row_derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand(input)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

// ---------------------------------------------------------------------------
// Field configuration (parsed from #[chorale(...)] attributes)
// ---------------------------------------------------------------------------

#[derive(Default)]
struct FieldConfig {
    skip: bool,
    header: Option<String>,
    initial_width: Option<f64>,
    sortable_override: Option<bool>,
    filter_override: Option<FilterOverride>,
    align_override: Option<String>,
    render_override: Option<RenderOverride>,
}

enum FilterOverride {
    None,
    Text,
    Boolean,
    DateRange,
    MultiSelect(Vec<String>),
}

/// Parsed `#[chorale(render = "...")]` value. Maps 1:1 onto
/// `chorale_core::RenderKind` variants the macro can emit.
///
/// `Badge` is intentionally absent: `RenderKind::Badge` requires a
/// `BadgeVariantMap` the macro cannot infer, and an empty map renders plain
/// text (not a neutral pill) in both adapters, so `render = "badge"` is
/// rejected at compile time with a pointer to the hand-written column path.
enum RenderOverride {
    Text,
    Number,
    /// ISO 4217 currency code (e.g. `"USD"`, `"EUR"`). Parsed from
    /// `render = "currency"` (defaults to USD) or `render = "currency:EUR"`.
    Currency(String),
    Date,
    DateTime,
    Boolean,
}

// ---------------------------------------------------------------------------
// Main expansion
// ---------------------------------------------------------------------------

fn expand(input: DeriveInput) -> Result<TokenStream2, Error> {
    // Only structs.
    let Data::Struct(struct_data) = input.data else {
        return Err(Error::new(
            input.ident.span(),
            "#[derive(TableRow)] only supports structs",
        ));
    };

    // Generic structs not supported.
    let bad_param = input.generics.params.iter().find(|p| {
        matches!(
            p,
            GenericParam::Type(_) | GenericParam::Lifetime(_) | GenericParam::Const(_)
        )
    });
    if let Some(p) = bad_param {
        return Err(Error::new(
            p.span(),
            "#[derive(TableRow)] does not support generic structs; use the explicit ColumnDef API instead",
        ));
    }

    // Only named-field structs.
    let Fields::Named(named_fields) = struct_data.fields else {
        return Err(Error::new(
            input.ident.span(),
            "#[derive(TableRow)] only supports structs with named fields",
        ));
    };

    let struct_name = &input.ident;

    // Parse and generate each column definition — once for the static path
    // and once for the rows-aware path.
    let mut col_defs: Vec<TokenStream2> = Vec::new();
    let mut col_defs_with_rows: Vec<TokenStream2> = Vec::new();
    for field in &named_fields.named {
        let cfg = parse_field_config(field)?;
        if cfg.skip {
            continue;
        }
        col_defs.push(generate_col_def(field, &cfg, ColumnsMode::Static));
        col_defs_with_rows.push(generate_col_def(field, &cfg, ColumnsMode::RowsAware));
    }

    // Bounds check: emit a const that fails if TRow doesn't satisfy Clone + PartialEq.
    let bounds_check = quote! {
        const _: fn() = || {
            fn _assert_bounds<T: ::core::clone::Clone + ::core::cmp::PartialEq>() {}
            _assert_bounds::<#struct_name>();
        };
    };

    Ok(quote! {
        #bounds_check

        impl #struct_name {
            /// Returns the `ColumnDef` list for this struct, generated by `#[derive(TableRow)]`.
            ///
            /// Pass the result to `TableState::new(rows, Self::chorale_columns())`.
            #[must_use]
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_possible_wrap,
                clippy::cast_precision_loss,
                clippy::cast_sign_loss,
            )]
            pub fn chorale_columns() -> ::std::vec::Vec<::chorale_core::ColumnDef<Self>> {
                vec![
                    #(#col_defs),*
                ]
            }

            /// Like [`Self::chorale_columns`], but derives data-aware filter
            /// configuration from `rows`:
            ///
            /// - Numeric columns without an explicit `filter` override get
            ///   `FilterKind::NumericRange` bounds computed from the actual
            ///   min/max in `rows` (step: `(max - min) / 100` snapped down to a
            ///   power of 10, at least `1.0` for integer columns). Empty `rows`
            ///   falls back to the static defaults.
            /// - `filter = "MultiSelect"` columns without `options` get the
            ///   sorted distinct stringified values from `rows` (first 50,
            ///   sorted).
            ///
            /// All other column properties are identical to
            /// [`Self::chorale_columns`].
            #[must_use]
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_possible_wrap,
                clippy::cast_precision_loss,
                clippy::cast_sign_loss,
                clippy::too_many_lines,
            )]
            pub fn chorale_columns_with_rows(
                rows: &[Self],
            ) -> ::std::vec::Vec<::chorale_core::ColumnDef<Self>> {
                // `rows` is only read by data-aware columns; suppress the
                // unused-variable warning for structs that have none.
                let _ = rows;
                vec![
                    #(#col_defs_with_rows),*
                ]
            }
        }
    })
}

// ---------------------------------------------------------------------------
// Attribute parsing
// ---------------------------------------------------------------------------

fn parse_field_config(field: &Field) -> Result<FieldConfig, Error> {
    let mut cfg = FieldConfig::default();

    for attr in &field.attrs {
        if !attr.path().is_ident("chorale") {
            continue;
        }
        // Parse #[chorale(key, key = value, ...)]
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("skip") {
                cfg.skip = true;
                return Ok(());
            }

            if meta.path.is_ident("header") {
                let value = meta.value()?;
                let s: syn::LitStr = value.parse()?;
                cfg.header = Some(s.value());
                return Ok(());
            }

            if meta.path.is_ident("initial_width") {
                let value = meta.value()?;
                let lit: Lit = value.parse()?;
                cfg.initial_width = Some(lit_to_f64(&lit, meta.path.span())?);
                return Ok(());
            }

            if meta.path.is_ident("sortable") {
                let value = meta.value()?;
                let lit: syn::LitBool = value.parse()?;
                cfg.sortable_override = Some(lit.value);
                return Ok(());
            }

            if meta.path.is_ident("filter") {
                let value = meta.value()?;
                let s: syn::LitStr = value.parse()?;
                cfg.filter_override = Some(match s.value().as_str() {
                    "none" | "None" => FilterOverride::None,
                    "Text" | "text" => FilterOverride::Text,
                    "Boolean" | "boolean" => FilterOverride::Boolean,
                    "Date" | "DateRange" | "date" => FilterOverride::DateRange,
                    "MultiSelect" | "multiselect" => {
                        // options must come separately; default to empty until overridden
                        FilterOverride::MultiSelect(vec![])
                    }
                    other => {
                        return Err(meta.error(format!(
                            "unknown filter kind `{other}`; expected one of: \
                             none, Text, Boolean, Date, MultiSelect"
                        )))
                    }
                });
                return Ok(());
            }

            if meta.path.is_ident("options") {
                // #[chorale(options = ["a", "b", "c"])]
                // Parse as a bracketed list of string literals.
                let value = meta.value()?;
                let arr: syn::ExprArray = value.parse()?;
                let mut opts = Vec::new();
                for elem in &arr.elems {
                    match elem {
                        syn::Expr::Lit(syn::ExprLit {
                            lit: Lit::Str(s), ..
                        }) => opts.push(s.value()),
                        _ => {
                            return Err(Error::new(elem.span(), "options must be string literals"))
                        }
                    }
                }
                // Update or create the MultiSelect override.
                cfg.filter_override = Some(FilterOverride::MultiSelect(opts));
                return Ok(());
            }

            if meta.path.is_ident("align") {
                let value = meta.value()?;
                let s: syn::LitStr = value.parse()?;
                cfg.align_override = Some(s.value());
                return Ok(());
            }

            if meta.path.is_ident("render") {
                let value = meta.value()?;
                let s: syn::LitStr = value.parse()?;
                cfg.render_override = Some(parse_render_value(&s.value(), &meta)?);
                return Ok(());
            }

            Err(meta.error(format!(
                "unknown chorale attribute `{}`",
                meta.path
                    .get_ident()
                    .map_or_else(|| "?".to_owned(), ToString::to_string)
            )))
        })?;
    }

    Ok(cfg)
}

/// Parse the string value of `#[chorale(render = "...")]`.
///
/// Accepted values (case-insensitive): `text`, `number`, `currency`,
/// `currency:<ISO-4217 code>` (e.g. `currency:EUR`), `date`, `datetime`,
/// `boolean`. `badge` is rejected with a pointer to the hand-written column
/// path, because `RenderKind::Badge` needs a `BadgeVariantMap` the macro
/// cannot infer.
fn parse_render_value(
    raw: &str,
    meta: &syn::meta::ParseNestedMeta<'_>,
) -> Result<RenderOverride, Error> {
    let lower = raw.to_ascii_lowercase();
    match lower.as_str() {
        "text" => Ok(RenderOverride::Text),
        "number" => Ok(RenderOverride::Number),
        "currency" => Ok(RenderOverride::Currency("USD".to_owned())),
        "date" => Ok(RenderOverride::Date),
        "datetime" => Ok(RenderOverride::DateTime),
        "boolean" | "bool" => Ok(RenderOverride::Boolean),
        "badge" => Err(meta.error(
            "render = \"badge\" is not supported by #[derive(TableRow)]: \
             RenderKind::Badge requires a BadgeVariantMap that the macro cannot \
             infer from a field type. Define this column by hand with \
             `ColumnDef::new(...).render_kind(RenderKind::Badge(map))`, or use a \
             custom cell renderer in the adapter.",
        )),
        _ => {
            if let Some(code) = lower.strip_prefix("currency:") {
                if !code.is_empty()
                    && code.len() <= 8
                    && code.chars().all(|c| c.is_ascii_alphabetic())
                {
                    return Ok(RenderOverride::Currency(code.to_ascii_uppercase()));
                }
                return Err(meta.error(format!(
                    "invalid currency code `{code}` in render = \"{raw}\"; \
                     expected an alphabetic ISO 4217 code, e.g. render = \"currency:EUR\""
                )));
            }
            Err(meta.error(format!(
                "unknown render kind `{raw}`; expected one of: \
                 text, number, currency, currency:<CODE>, date, datetime, boolean"
            )))
        }
    }
}

fn lit_to_f64(lit: &Lit, span: Span) -> Result<f64, Error> {
    match lit {
        Lit::Float(f) => f.base10_parse().map_err(|e| Error::new(span, e)),
        Lit::Int(i) => i
            .base10_parse::<i64>()
            .map(|n| {
                #[allow(clippy::cast_precision_loss)]
                let f = n as f64;
                f
            })
            .map_err(|e| Error::new(span, e)),
        _ => Err(Error::new(span, "expected a numeric literal")),
    }
}

// ---------------------------------------------------------------------------
// Type inference
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
enum InferredKind {
    Text,
    Integer,
    Float,
    Boolean,
    Date,
    OptionText,
    OptionInteger,
    OptionFloat,
    OptionBoolean,
    OptionDate,
    OptionDisplay,
    Display,
}

impl InferredKind {
    fn is_numeric(self) -> bool {
        matches!(
            self,
            Self::Integer | Self::Float | Self::OptionInteger | Self::OptionFloat
        )
    }
}

fn infer_kind(ty: &Type) -> InferredKind {
    let Type::Path(TypePath { path, .. }) = ty else {
        return InferredKind::Display;
    };

    let Some(last) = path.segments.last() else {
        return InferredKind::Display;
    };

    let ident = last.ident.to_string();

    // Check Option<T>
    if ident == "Option" {
        if let syn::PathArguments::AngleBracketed(args) = &last.arguments {
            if let Some(syn::GenericArgument::Type(inner)) = args.args.first() {
                return match infer_kind(inner) {
                    InferredKind::Text => InferredKind::OptionText,
                    InferredKind::Integer => InferredKind::OptionInteger,
                    InferredKind::Float => InferredKind::OptionFloat,
                    InferredKind::Boolean => InferredKind::OptionBoolean,
                    InferredKind::Date => InferredKind::OptionDate,
                    _ => InferredKind::OptionDisplay,
                };
            }
        }
        return InferredKind::OptionDisplay;
    }

    match ident.as_str() {
        "String" => InferredKind::Text,
        "u8" | "u16" | "u32" | "u64" | "u128" | "usize" | "i8" | "i16" | "i32" | "i64" | "i128"
        | "isize" => InferredKind::Integer,
        "f32" | "f64" => InferredKind::Float,
        "bool" => InferredKind::Boolean,
        "NaiveDate" => {
            if cfg!(feature = "chrono") {
                InferredKind::Date
            } else {
                InferredKind::Display
            }
        }
        _ => InferredKind::Display,
    }
}

// ---------------------------------------------------------------------------
// Code generation per field
// ---------------------------------------------------------------------------

/// Which generated method a column definition is being emitted for.
#[derive(Clone, Copy, PartialEq)]
enum ColumnsMode {
    /// `chorale_columns()`: static defaults only (no `rows` in scope).
    Static,
    /// `chorale_columns_with_rows(rows)`: may emit runtime code that folds
    /// over the in-scope `rows: &[Self]` slice to derive filter bounds.
    RowsAware,
}

fn generate_col_def(field: &Field, cfg: &FieldConfig, mode: ColumnsMode) -> TokenStream2 {
    let Some(field_name) = field.ident.as_ref() else {
        unreachable!("named struct fields always have idents")
    };
    let field_name_str = field_name.to_string();

    // Header: attribute override → Title Case of field name
    let header_str = cfg
        .header
        .clone()
        .unwrap_or_else(|| snake_to_title(&field_name_str));

    let kind = infer_kind(&field.ty);

    // Accessor
    let accessor = build_accessor(field_name, kind);

    // Sortable: default true for all types the core can sort
    let sortable = cfg.sortable_override.unwrap_or(true);

    // Filter
    let filter_tokens = build_filter(cfg, kind, field_name, mode);

    // Render kind (only emitted when #[chorale(render = "...")] is present)
    let render_tokens = build_render(cfg);

    // Alignment
    let align_tokens = build_align(cfg, kind);

    // Initial width
    let width_tokens = if let Some(w) = cfg.initial_width {
        quote! { .initial_width(#w) }
    } else {
        quote! {}
    };

    let col_id = quote! { ::chorale_core::ColumnId(#field_name_str) };

    let sortable_tokens = if sortable {
        quote! { .sortable() }
    } else {
        quote! {}
    };

    quote! {
        ::chorale_core::ColumnDef::new(
            #col_id,
            #header_str,
            |r: &Self| #accessor,
        )
        #sortable_tokens
        #filter_tokens
        #render_tokens
        #align_tokens
        #width_tokens
    }
}

/// Emit `.render_kind(...)` for an explicit `#[chorale(render = "...")]`
/// attribute, or nothing (leaving the `RenderKind::Text` default).
fn build_render(cfg: &FieldConfig) -> TokenStream2 {
    let Some(render) = &cfg.render_override else {
        return quote! {};
    };
    let render_kind = match render {
        RenderOverride::Text => quote! { ::chorale_core::RenderKind::Text },
        RenderOverride::Number => quote! { ::chorale_core::RenderKind::Number },
        RenderOverride::Currency(code) => quote! {
            ::chorale_core::RenderKind::Currency(::chorale_core::CurrencyCode(#code))
        },
        RenderOverride::Date => quote! { ::chorale_core::RenderKind::Date },
        RenderOverride::DateTime => quote! { ::chorale_core::RenderKind::DateTime },
        RenderOverride::Boolean => quote! { ::chorale_core::RenderKind::Boolean },
    };
    quote! { .render_kind(#render_kind) }
}

fn build_accessor(field_name: &syn::Ident, kind: InferredKind) -> TokenStream2 {
    match kind {
        InferredKind::Text => quote! {
            ::chorale_core::CellValue::Text(r.#field_name.clone())
        },
        InferredKind::Integer => quote! {
            ::chorale_core::CellValue::Integer(r.#field_name as i64)
        },
        InferredKind::Float => quote! {
            ::chorale_core::CellValue::Float(r.#field_name as f64)
        },
        InferredKind::Boolean => quote! {
            ::chorale_core::CellValue::Boolean(r.#field_name)
        },
        InferredKind::Date => quote! {
            ::chorale_core::CellValue::Date(r.#field_name)
        },
        InferredKind::OptionText => quote! {
            r.#field_name.as_ref().map_or(
                ::chorale_core::CellValue::Empty,
                |v| ::chorale_core::CellValue::Text(v.clone()),
            )
        },
        InferredKind::OptionInteger => quote! {
            r.#field_name.map_or(
                ::chorale_core::CellValue::Empty,
                |v| ::chorale_core::CellValue::Integer(v as i64),
            )
        },
        InferredKind::OptionFloat => quote! {
            r.#field_name.map_or(
                ::chorale_core::CellValue::Empty,
                |v| ::chorale_core::CellValue::Float(v as f64),
            )
        },
        InferredKind::OptionBoolean => quote! {
            r.#field_name.map_or(
                ::chorale_core::CellValue::Empty,
                ::chorale_core::CellValue::Boolean,
            )
        },
        InferredKind::OptionDate => quote! {
            r.#field_name.map_or(
                ::chorale_core::CellValue::Empty,
                ::chorale_core::CellValue::Date,
            )
        },
        InferredKind::OptionDisplay => quote! {
            r.#field_name.as_ref().map_or(
                ::chorale_core::CellValue::Empty,
                |v| ::chorale_core::CellValue::Text(v.to_string()),
            )
        },
        InferredKind::Display => quote! {
            ::chorale_core::CellValue::Text(r.#field_name.to_string())
        },
    }
}

fn build_filter(
    cfg: &FieldConfig,
    kind: InferredKind,
    field_name: &syn::Ident,
    mode: ColumnsMode,
) -> TokenStream2 {
    let filter = if let Some(fo) = &cfg.filter_override {
        match fo {
            FilterOverride::None => return quote! {},
            FilterOverride::Text => quote! {
                ::chorale_core::FilterKind::Text
            },
            FilterOverride::Boolean => quote! {
                ::chorale_core::FilterKind::Boolean
            },
            FilterOverride::DateRange => quote! {
                ::chorale_core::FilterKind::DateRange
            },
            FilterOverride::MultiSelect(opts) => {
                if opts.is_empty() && mode == ColumnsMode::RowsAware {
                    // Data-aware path: populate the options from the sorted
                    // distinct stringified values in `rows`, capped at 50.
                    return build_rows_aware_multi_select(kind, field_name);
                }
                quote! {
                    ::chorale_core::FilterKind::MultiSelect {
                        options: vec![#(#opts.to_owned()),*],
                    }
                }
            }
        }
    } else {
        // Inferred
        match kind {
            InferredKind::Text | InferredKind::OptionText => quote! {
                ::chorale_core::FilterKind::Text
            },
            InferredKind::Boolean | InferredKind::OptionBoolean => quote! {
                ::chorale_core::FilterKind::Boolean
            },
            InferredKind::Date | InferredKind::OptionDate => quote! {
                ::chorale_core::FilterKind::DateRange
            },
            InferredKind::Integer
            | InferredKind::OptionInteger
            | InferredKind::Float
            | InferredKind::OptionFloat => {
                if mode == ColumnsMode::RowsAware {
                    // Data-aware path: real min/max folded over `rows`.
                    return build_rows_aware_numeric_range(kind, field_name);
                }
                static_numeric_range(kind)
            }
            // Display: no filter by default.
            _ => return quote! {},
        }
    };
    quote! { .filter(#filter) }
}

/// Static-default `FilterKind::NumericRange` tokens for a numeric kind.
/// These are the bounds `chorale_columns()` has always emitted; the
/// rows-aware path falls back to them when `rows` is empty.
fn static_numeric_range(kind: InferredKind) -> TokenStream2 {
    match kind {
        InferredKind::Integer | InferredKind::OptionInteger => quote! {
            ::chorale_core::FilterKind::NumericRange {
                min: 0.0,
                max: 1_000_000.0,
                step: 1_000.0,
            }
        },
        InferredKind::Float | InferredKind::OptionFloat => quote! {
            ::chorale_core::FilterKind::NumericRange {
                min: 0.0,
                max: 100.0,
                step: 0.1,
            }
        },
        _ => unreachable!("static_numeric_range called for non-numeric kind"),
    }
}

/// Emit `.filter(...)` tokens that compute `NumericRange` bounds at runtime by
/// folding over the in-scope `rows: &[Self]` slice.
///
/// Step policy: `(max - min) / 100`, snapped DOWN to the nearest power of 10
/// so the slider step is a round number (e.g. range `0..=87_000` → raw 870 →
/// step 100). Integer columns clamp the step to at least `1.0`. When the
/// range is degenerate (`max == min`) the kind's static default step is used,
/// and when `rows` is empty (or holds no finite values) the entire static
/// default range is used, so the emitted range is never inverted.
fn build_rows_aware_numeric_range(kind: InferredKind, field_name: &syn::Ident) -> TokenStream2 {
    let value_expr = match kind {
        InferredKind::Integer | InferredKind::Float => quote! {
            ::core::option::Option::Some(__row.#field_name as f64)
        },
        InferredKind::OptionInteger | InferredKind::OptionFloat => quote! {
            __row.#field_name.map(|__v| __v as f64)
        },
        _ => unreachable!("build_rows_aware_numeric_range called for non-numeric kind"),
    };

    let is_integer = matches!(kind, InferredKind::Integer | InferredKind::OptionInteger);
    // Integer sliders should never step by less than a whole unit.
    let step_expr = if is_integer {
        quote! { __snapped.max(1.0) }
    } else {
        quote! { __snapped }
    };
    let (default_min, default_max, default_step) = if is_integer {
        (0.0f64, 1_000_000.0f64, 1_000.0f64)
    } else {
        (0.0f64, 100.0f64, 0.1f64)
    };

    quote! {
        .filter({
            let mut __min = f64::INFINITY;
            let mut __max = f64::NEG_INFINITY;
            for __row in rows {
                if let ::core::option::Option::Some(__v) = #value_expr {
                    if __v.is_finite() {
                        __min = __min.min(__v);
                        __max = __max.max(__v);
                    }
                }
            }
            if __min.is_finite() && __max.is_finite() {
                let __step = if __max > __min {
                    let __snapped =
                        10f64.powf(((__max - __min) / 100.0).log10().floor());
                    #step_expr
                } else {
                    #default_step
                };
                ::chorale_core::FilterKind::NumericRange {
                    min: __min,
                    max: __max,
                    step: __step,
                }
            } else {
                // Empty rows (or no finite values): static defaults.
                ::chorale_core::FilterKind::NumericRange {
                    min: #default_min,
                    max: #default_max,
                    step: #default_step,
                }
            }
        })
    }
}

/// Emit `.filter(...)` tokens that collect the sorted distinct stringified
/// values of this field from the in-scope `rows: &[Self]` slice, capped at the
/// first 50 in sort order. `None` / `Empty` values are excluded.
fn build_rows_aware_multi_select(kind: InferredKind, field_name: &syn::Ident) -> TokenStream2 {
    let string_expr = match kind {
        InferredKind::Text => quote! {
            ::core::option::Option::Some(__row.#field_name.clone())
        },
        InferredKind::OptionText => quote! {
            __row.#field_name.clone()
        },
        InferredKind::OptionInteger
        | InferredKind::OptionFloat
        | InferredKind::OptionBoolean
        | InferredKind::OptionDate
        | InferredKind::OptionDisplay => quote! {
            __row.#field_name.as_ref().map(|__v| __v.to_string())
        },
        _ => quote! {
            ::core::option::Option::Some(__row.#field_name.to_string())
        },
    };

    quote! {
        .filter({
            let mut __distinct =
                ::std::collections::BTreeSet::<::std::string::String>::new();
            for __row in rows {
                if let ::core::option::Option::Some(__s) = #string_expr {
                    __distinct.insert(__s);
                }
            }
            ::chorale_core::FilterKind::MultiSelect {
                options: __distinct.into_iter().take(50).collect(),
            }
        })
    }
}

fn build_align(cfg: &FieldConfig, kind: InferredKind) -> TokenStream2 {
    let align = if let Some(a) = &cfg.align_override {
        match a.as_str() {
            "Left" | "left" => quote! { ::chorale_core::Alignment::Left },
            "Center" | "center" => quote! { ::chorale_core::Alignment::Center },
            "Right" | "right" => quote! { ::chorale_core::Alignment::Right },
            _ => return quote! {},
        }
    } else {
        // Right-align numeric columns by default.
        if kind.is_numeric() {
            quote! { ::chorale_core::Alignment::Right }
        } else {
            return quote! {};
        }
    };
    quote! { .alignment(#align) }
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

fn snake_to_title(s: &str) -> String {
    s.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------------------------------------------------------------------------
// Tests (compile-time expansion is tested via trybuild/integration tests)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::snake_to_title;

    #[test]
    fn snake_to_title_basic() {
        assert_eq!(snake_to_title("due_date"), "Due Date");
        assert_eq!(snake_to_title("customer"), "Customer");
        assert_eq!(snake_to_title("first_last_name"), "First Last Name");
        assert_eq!(snake_to_title("id"), "Id");
    }

    #[test]
    fn snake_to_title_empty() {
        assert_eq!(snake_to_title(""), "");
    }
}
