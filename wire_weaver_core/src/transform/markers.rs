//! Unit and scale markers declared with `exact_units::unit!` / `exact_units::scale!` (INT-13).
//!
//! The walker reads such an invocation as rustc does: a unit struct per line, with the line's attributes. Each struct
//! also gets a `#[ww_marker(..)]` attribute holding its line, from which its definition (dimension, scale, symbol)
//! goes into the API model. The grammar is the one documented at the macros; a line the macros don't accept is an
//! error here too.

use anyhow::{Context, Result};
use proc_macro2::TokenStream;
use syn::parse::{Parse, ParseStream};
use syn::{
    Attribute, Ident, Item, LitFloat, LitInt, LitStr, Token, Visibility, bracketed, parenthesized,
};

/// Name of the attribute a struct made from a `unit!` / `scale!` line carries, with the line after its `=`.
pub(crate) const MARKER_ATTR: &str = "ww_marker";

/// The macro an invocation's path names: `unit!`, `exact_units::unit!`, `ww_si::scale!`, ...
fn marker_macro(item: &syn::ItemMacro) -> Option<MacroKind> {
    match item.mac.path.segments.last()?.ident.to_string().as_str() {
        "unit" => Some(MacroKind::Unit),
        "scale" => Some(MacroKind::Scale),
        _ => None,
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) enum MacroKind {
    Unit,
    Scale,
}

/// Replaces every `unit!` / `scale!` invocation in `items` (and in inline modules) with the structs it declares.
pub(crate) fn expand_marker_macros(items: &mut Vec<Item>) -> Result<()> {
    let mut expanded = Vec::with_capacity(items.len());
    for mut item in items.drain(..) {
        match &mut item {
            Item::Macro(item_macro) if item_macro.ident.is_none() => {
                if let Some(kind) = marker_macro(item_macro) {
                    let lines = parse_invocation(kind, item_macro.mac.tokens.clone())
                        .with_context(|| format!("in {kind:?} marker declarations"))?;
                    expanded.extend(
                        lines
                            .into_iter()
                            .map(|line| Item::Struct(line.to_struct(kind))),
                    );
                    continue;
                }
            }
            Item::Mod(item_mod) => {
                if let Some((_, items)) = &mut item_mod.content {
                    expand_marker_macros(items)?;
                }
            }
            _ => {}
        }
        expanded.push(item);
    }
    *items = expanded;
    Ok(())
}

fn parse_invocation(kind: MacroKind, tokens: TokenStream) -> Result<Vec<MarkerLine>> {
    let parser = |input: ParseStream| {
        // exact_units' own table: `@table <attrs> <vis> static ALL;`, a static, not a marker
        if input.peek(Token![@]) {
            input.parse::<Token![@]>()?;
            input.parse::<Ident>()?;
            Attribute::parse_outer(input)?;
            input.parse::<Visibility>()?;
            input.parse::<Token![static]>()?;
            input.parse::<Ident>()?;
            input.parse::<Token![;]>()?;
        }
        let mut lines = Vec::new();
        while !input.is_empty() {
            lines.push(MarkerLine::parse(kind, input)?);
        }
        Ok(lines)
    };
    Ok(syn::parse::Parser::parse2(parser, tokens)?)
}

/// One line: `<attrs> <vis> struct <Name> = <def>;`.
pub(crate) struct MarkerLine {
    attrs: Vec<Attribute>,
    vis: Visibility,
    ident: Ident,
    def: TokenStream,
}

impl MarkerLine {
    fn parse(kind: MacroKind, input: ParseStream) -> syn::Result<Self> {
        let attrs = Attribute::parse_outer(input)?;
        let vis = input.parse()?;
        input.parse::<Token![struct]>()?;
        let ident = input.parse()?;
        input.parse::<Token![=]>()?;
        let mut def = TokenStream::new();
        while !input.peek(Token![;]) {
            if input.is_empty() {
                return Err(input.error("expected `;` at the end of the line"));
            }
            def.extend([input.parse::<proc_macro2::TokenTree>()?]);
        }
        input.parse::<Token![;]>()?;
        // checked here, so that a wrong line is an error even when its marker isn't used
        match kind {
            MacroKind::Unit => {
                syn::parse2::<UnitLine>(def.clone())?;
            }
            MacroKind::Scale => {
                syn::parse2::<ScaleLine>(def.clone())?;
            }
        }
        Ok(MarkerLine {
            attrs,
            vis,
            ident,
            def,
        })
    }

    fn to_struct(&self, kind: MacroKind) -> syn::ItemStruct {
        let MarkerLine {
            attrs,
            vis,
            ident,
            def,
        } = self;
        let attr = Ident::new(MARKER_ATTR, ident.span());
        let kind = match kind {
            MacroKind::Unit => quote::quote!(unit),
            MacroKind::Scale => quote::quote!(scale),
        };
        syn::parse_quote! {
            #(#attrs)*
            #[#attr(#kind, #def)]
            #vis struct #ident;
        }
    }
}

/// What a `#[ww_marker(..)]` attribute says.
pub(crate) enum MarkerLineDef {
    Unit(UnitLine),
    Scale(ScaleLine),
}

/// The definition of a struct made from a marker line, `None` for any other struct.
#[allow(dead_code)] // read by the model conversion
pub(crate) fn marker_line(attrs: &[Attribute]) -> Result<Option<MarkerLineDef>> {
    let Some(attr) = attrs.iter().find(|a| a.path().is_ident(MARKER_ATTR)) else {
        return Ok(None);
    };
    let def = attr.parse_args_with(|input: ParseStream| {
        let kind: Ident = input.parse()?;
        input.parse::<Token![,]>()?;
        match kind.to_string().as_str() {
            "unit" => Ok(MarkerLineDef::Unit(input.parse()?)),
            "scale" => Ok(MarkerLineDef::Scale(input.parse()?)),
            _ => Err(syn::Error::new(kind.span(), "unit or scale")),
        }
    })?;
    Ok(Some(def))
}

/// `"<symbol>", <dimension>[, <scale>]` of a `unit!` line.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct UnitLine {
    pub(crate) symbol: String,
    pub(crate) dim: DimLine,
    pub(crate) scale: UnitScaleLine,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DimLine {
    /// `[s 1, m -2]`: exponents by base name (`s`, `m`, `kg`, `a`, `k`, `mol`, `cd`, `rad`, `bit`).
    Exps(Vec<(String, i8)>),
    /// `counted`: a counted base named by the crate and the marker.
    Counted,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum UnitScaleLine {
    Coherent,
    Pow10(i8),
    Affine { factor: f64, offset: f64 },
    Exact(MagnitudeLine),
}

/// `num / den, pow10 a, pow2 b, pi c`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MagnitudeLine {
    pub(crate) num: u64,
    pub(crate) den: u64,
    pub(crate) pow10: i8,
    pub(crate) pow2: i8,
    pub(crate) pi: i8,
}

/// `"<symbol>", <factor>[, prefix <Prefix>]` of a `scale!` line.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ScaleLine {
    pub(crate) symbol: String,
    pub(crate) factor: ScaleFactorLine,
    pub(crate) prefix: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ScaleFactorLine {
    Pow10(i8),
    Pow2(i8),
    Exact(MagnitudeLine),
}

const BASES: [&str; 9] = ["s", "m", "kg", "a", "k", "mol", "cd", "rad", "bit"];

impl Parse for UnitLine {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let symbol = input.parse::<LitStr>()?.value();
        input.parse::<Token![,]>()?;
        let dim = if input.peek(syn::token::Bracket) {
            let content;
            bracketed!(content in input);
            let mut exps = Vec::new();
            while !content.is_empty() {
                let base: Ident = content.parse()?;
                if !BASES.contains(&base.to_string().as_str()) {
                    return Err(syn::Error::new(
                        base.span(),
                        format!("a base of a dimension is one of {}", BASES.join(", ")),
                    ));
                }
                let e = parse_i8(&content)?;
                if !(-8..=7).contains(&e) {
                    return Err(syn::Error::new(
                        base.span(),
                        "exponent of a dimension out of -8..=7",
                    ));
                }
                exps.push((base.to_string(), e));
                if !content.is_empty() {
                    content.parse::<Token![,]>()?;
                }
            }
            DimLine::Exps(exps)
        } else {
            let counted: Ident = input.parse()?;
            if counted != "counted" {
                return Err(syn::Error::new(
                    counted.span(),
                    "a dimension is `[<base> <exponent>, ..]` or `counted`",
                ));
            }
            DimLine::Counted
        };
        let scale = if input.is_empty() {
            UnitScaleLine::Coherent
        } else {
            input.parse::<Token![,]>()?;
            let kind: Ident = input.parse()?;
            match kind.to_string().as_str() {
                "Pow10" => UnitScaleLine::Pow10(in_parens(input, parse_i8)?),
                "Exact" => UnitScaleLine::Exact(in_parens(input, MagnitudeLine::parse)?),
                "Affine" => {
                    let content;
                    syn::braced!(content in input);
                    let field = |name: &str| -> syn::Result<f64> {
                        let ident: Ident = content.parse()?;
                        if ident != name {
                            return Err(syn::Error::new(
                                ident.span(),
                                format!("expected `{name}`"),
                            ));
                        }
                        content.parse::<Token![:]>()?;
                        let value = parse_f64(&content)?;
                        if !content.is_empty() {
                            content.parse::<Token![,]>()?;
                        }
                        Ok(value)
                    };
                    let factor = field("factor")?;
                    let offset = field("offset")?;
                    UnitScaleLine::Affine { factor, offset }
                }
                _ => {
                    return Err(syn::Error::new(
                        kind.span(),
                        "a unit's scale is Pow10(e), Exact(num / den, ..) or Affine { factor, offset }",
                    ));
                }
            }
        };
        if !input.is_empty() {
            return Err(input.error("unexpected tokens after the scale"));
        }
        Ok(UnitLine { symbol, dim, scale })
    }
}

impl Parse for ScaleLine {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let symbol = input.parse::<LitStr>()?.value();
        input.parse::<Token![,]>()?;
        let kind: Ident = input.parse()?;
        let factor = match kind.to_string().as_str() {
            "Pow10" => ScaleFactorLine::Pow10(in_parens(input, parse_i8)?),
            "Pow2" => ScaleFactorLine::Pow2(in_parens(input, parse_i8)?),
            "Exact" => ScaleFactorLine::Exact(in_parens(input, MagnitudeLine::parse)?),
            _ => {
                return Err(syn::Error::new(
                    kind.span(),
                    "a scale is Pow10(e), Pow2(e) or Exact(num / den, ..)",
                ));
            }
        };
        let prefix = if input.is_empty() {
            None
        } else {
            input.parse::<Token![,]>()?;
            let word: Ident = input.parse()?;
            if word != "prefix" {
                return Err(syn::Error::new(word.span(), "expected `prefix <Prefix>`"));
            }
            Some(input.parse::<Ident>()?.to_string())
        };
        if !input.is_empty() {
            return Err(input.error("unexpected tokens after the scale"));
        }
        Ok(ScaleLine {
            symbol,
            factor,
            prefix,
        })
    }
}

impl Parse for MagnitudeLine {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let num = input.parse::<LitInt>()?.base10_parse()?;
        input.parse::<Token![/]>()?;
        let den = input.parse::<LitInt>()?.base10_parse()?;
        let mut m = MagnitudeLine {
            num,
            den,
            pow10: 0,
            pow2: 0,
            pi: 0,
        };
        let mut next = 0;
        while input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
            if input.is_empty() {
                break;
            }
            let name: Ident = input.parse()?;
            let order = ["pow10", "pow2", "pi"];
            let Some(pos) = order.iter().position(|o| name == o).filter(|p| *p >= next) else {
                return Err(syn::Error::new(
                    name.span(),
                    "after `num / den` come `pow10 e`, `pow2 e`, `pi e`, each at most once and in this order",
                ));
            };
            next = pos + 1;
            let e = parse_i8(input)?;
            match pos {
                0 => m.pow10 = e,
                1 => m.pow2 = e,
                _ => m.pi = e,
            }
        }
        Ok(m)
    }
}

fn in_parens<T>(input: ParseStream, f: fn(ParseStream) -> syn::Result<T>) -> syn::Result<T> {
    let content;
    parenthesized!(content in input);
    let value = f(&content)?;
    if !content.is_empty() {
        return Err(content.error("unexpected tokens"));
    }
    Ok(value)
}

fn parse_i8(input: ParseStream) -> syn::Result<i8> {
    let neg = input.parse::<Option<Token![-]>>()?.is_some();
    let lit: LitInt = input.parse()?;
    let v: i16 = lit.base10_parse()?;
    let v = if neg { -v } else { v };
    i8::try_from(v).map_err(|_| syn::Error::new(lit.span(), "out of i8"))
}

fn parse_f64(input: ParseStream) -> syn::Result<f64> {
    let neg = input.parse::<Option<Token![-]>>()?.is_some();
    let v: f64 = if input.peek(LitFloat) {
        input.parse::<LitFloat>()?.base10_parse()?
    } else {
        let lit: LitInt = input.parse()?;
        lit.base10_parse::<f64>()
            .map_err(|_| syn::Error::new(lit.span(), "a number"))?
    };
    Ok(if neg { -v } else { v })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expand(src: &str) -> Vec<Item> {
        let mut file = syn::parse_file(src).unwrap();
        expand_marker_macros(&mut file.items).unwrap();
        file.items
    }

    #[test]
    fn unit_lines_become_structs_with_their_definition() {
        let items = expand(
            r#"
            exact_units::unit! {
                /// Revolutions per minute.
                pub struct Rpm = "rpm", [rad 1, s -1], Exact(1 / 30, pi 1);
                #[cfg(feature = "x")]
                pub struct Bucket = "bucket", counted;
                pub struct DegC = "°C", [k 1], Affine { factor: 1.0, offset: 273.15 };
            }
            "#,
        );
        assert_eq!(items.len(), 3);
        let Item::Struct(rpm) = &items[0] else {
            panic!()
        };
        assert_eq!(rpm.ident, "Rpm");
        assert!(matches!(rpm.fields, syn::Fields::Unit));
        let Some(MarkerLineDef::Unit(line)) = marker_line(&rpm.attrs).unwrap() else {
            panic!()
        };
        assert_eq!(
            line,
            UnitLine {
                symbol: "rpm".into(),
                dim: DimLine::Exps(vec![("rad".into(), 1), ("s".into(), -1)]),
                scale: UnitScaleLine::Exact(MagnitudeLine {
                    num: 1,
                    den: 30,
                    pow10: 0,
                    pow2: 0,
                    pi: 1
                }),
            }
        );
        let Item::Struct(bucket) = &items[1] else {
            panic!()
        };
        assert!(bucket.attrs.iter().any(|a| a.path().is_ident("cfg")));
        let Some(MarkerLineDef::Unit(line)) = marker_line(&bucket.attrs).unwrap() else {
            panic!()
        };
        assert_eq!(line.dim, DimLine::Counted);
        let Item::Struct(deg_c) = &items[2] else {
            panic!()
        };
        let Some(MarkerLineDef::Unit(line)) = marker_line(&deg_c.attrs).unwrap() else {
            panic!()
        };
        assert_eq!(
            line.scale,
            UnitScaleLine::Affine {
                factor: 1.0,
                offset: 273.15
            }
        );
    }

    #[test]
    fn table_header_and_scale_lines() {
        let items = expand(
            r#"
            mod inner {
                crate::unit! {
                    @table
                    /// All.
                    pub static ALL;
                    pub struct Second = "s", [s 1];
                }
            }
            crate::scale! {
                pub struct Milli = "m", Pow10(-3), prefix Milli;
                pub struct Steps = "", Exact(70 / 1000);
            }
            "#,
        );
        let Item::Mod(inner) = &items[0] else {
            panic!()
        };
        assert_eq!(inner.content.as_ref().unwrap().1.len(), 1);
        let Item::Struct(milli) = &items[1] else {
            panic!()
        };
        let Some(MarkerLineDef::Scale(line)) = marker_line(&milli.attrs).unwrap() else {
            panic!()
        };
        assert_eq!(
            line,
            ScaleLine {
                symbol: "m".into(),
                factor: ScaleFactorLine::Pow10(-3),
                prefix: Some("Milli".into())
            }
        );
    }

    #[test]
    fn a_wrong_line_is_an_error() {
        for src in [
            r#"unit! { pub struct A = "a", [q 1]; }"#,
            r#"unit! { pub struct A = "a", [s 9]; }"#,
            r#"unit! { pub struct A = "a", [s 1], Exact(1 / 2, pi 1, pow10 2); }"#,
            r#"unit! { pub struct A = "a", [s 1], Pow2(1); }"#,
            r#"scale! { pub struct A = "a", Factor(2.0); }"#,
            r#"unit! { pub struct A = "a", [s 1], Factor(2.0); }"#,
            r#"unit! { pub struct A = "a", [s 1] }"#,
        ] {
            let mut file = syn::parse_file(src).unwrap();
            assert!(expand_marker_macros(&mut file.items).is_err(), "{src}");
        }
    }
}
