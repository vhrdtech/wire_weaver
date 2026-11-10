//! Unit, scale and origin markers declared with `exact_units::unit!` / `exact_units::scale!` (INT-13),
//! `exact_units::origin!` (STD-64), and quantity markers declared with `exact_units::quantity!` (UNIT-8).
//!
//! The walker reads such an invocation as rustc does: a unit struct per line, with the line's attributes. Each struct
//! also gets a `#[ww_marker(..)]` attribute holding its line, from which its definition (dimension, scale, symbol)
//! goes into the API model. The grammar is the one documented at the macros; a line the macros don't accept is an
//! error here too. An `origin!` declares one struct, `<attrs> <vis> struct <Name> { <keyed fields> }`; its fields
//! are kept as they are in `#[ww_marker(origin, ..)]`. A `quantity!` line is `= [kind] <dimension>[, <character>]`
//! for a root or `: [kind] <Parent>[, <character>]` for a child, kept with its `=` or `:`. A struct with an
//! `impl exact_units::Reference` beside it (PT-4, STD-108) gets `#[ww_marker(reference, "<NAME>", <HOW>)]`, the
//! impl's `NAME` and `HOW` constants as written.

use super::crate_walker::{CrateContext, Scratch};
use super::ty::convert_ty;
use anyhow::{Context, Result, anyhow, bail};
use exact_units::codec::{
    CodeSpan, CodecDefOwned, CodecShapeOwned, CurveDefOwned, CurveForm, ExpBase, Referred, Segment,
};
use exact_units::{
    BaseExpOwned, Character, DimOwned, DimWireOwned, Exp, Fraction, MAX_TRUE_BASES, Magnitude,
    MarkerIdOwned, Offset, Prefix, PrefixPolicy, PrefixSet, ProvenanceOwned, QuantityDefOwned,
    Symbols, System, SystemDef, SystemDefOwned, UnitDefOwned, UnitNames, UnitNamesOwned,
    systems::Si,
};
use proc_macro2::TokenStream;
use syn::parse::{Parse, ParseStream};
use syn::{Attribute, Ident, Item, LitInt, LitStr, Token, Visibility, bracketed, parenthesized};
use ww_self::units::{
    origin_of, quantity_of, reference_of, scale_of, system_of, unit_of, unit_zero,
};
use ww_self::{
    ApiBundleOwned, MarkerDefOwned, OriginDefOwned, ReferenceDefOwned, ReferenceHow, ScaleDefOwned,
    TimeCounting, TimeScaleDef, TimeZero,
};

/// Name of the attribute a struct made from a `unit!` / `scale!` line carries, with the line after its `=`.
pub(crate) const MARKER_ATTR: &str = "ww_marker";

/// The macro an invocation's path names: `unit!`, `exact_units::unit!`, `ww_si::scale!`, ...
fn marker_macro(item: &syn::ItemMacro) -> Option<MacroKind> {
    match item.mac.path.segments.last()?.ident.to_string().as_str() {
        "unit" => Some(MacroKind::Unit),
        "scale" => Some(MacroKind::Scale),
        "origin" => Some(MacroKind::Origin),
        "quantity" => Some(MacroKind::Quantity),
        _ => None,
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) enum MacroKind {
    Unit,
    Scale,
    Origin,
    Quantity,
}

/// Replaces every `unit!` / `scale!` / `origin!` / `quantity!` invocation in `items` (and in inline modules) with the structs it declares.
pub(crate) fn expand_marker_macros(items: &mut Vec<Item>) -> Result<()> {
    let mut expanded = Vec::with_capacity(items.len());
    for mut item in items.drain(..) {
        match &mut item {
            Item::Macro(item_macro) if item_macro.ident.is_none() => {
                if let Some(MacroKind::Origin) = marker_macro(item_macro) {
                    let origin: OriginDecl = syn::parse2(item_macro.mac.tokens.clone())
                        .context("in an origin! declaration")?;
                    expanded.push(Item::Struct(origin.to_struct()));
                    continue;
                }
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
    mark_hand_written(&mut expanded);
    *items = expanded;
    Ok(())
}

/// Marks each struct with a hand-written `impl Unit for X { const DIM .. }`, `impl FixedScale for X { const SCALE
/// .. }` or `impl Origin for X { const OFFSET .. }` next to it with `#[ww_marker(hand, <trait>)]`: the walker can't read such a definition, and says so when the
/// marker is used, instead of leaving it unitless.
fn mark_hand_written(items: &mut [Item]) {
    let mut hand = Vec::new();
    let mut references = Vec::new();
    let mut codecs = Vec::new();
    let mut generic_codecs = Vec::new();
    // `const X: [T; N] = [..];` of this module: a codec's `DEF` names its tables by them (`def_list(&X)`)
    let tables: Vec<(Ident, syn::Expr)> = items
        .iter()
        .filter_map(|item| match item {
            Item::Const(c) if matches!(&*c.expr, syn::Expr::Array(_)) => {
                Some((c.ident.clone(), (*c.expr).clone()))
            }
            _ => None,
        })
        .collect();
    for item in items.iter() {
        let Item::Impl(item_impl) = item else {
            continue;
        };
        let Some((trait_path, _)) = &item_impl.trait_ else {
            continue;
        };
        let Some(trait_name) = trait_path.segments.last().map(|s| s.ident.to_string()) else {
            continue;
        };
        // `impl<Q, C> Codec for Transfer<Q, C>`: its `DEF` is one per instance (`..Q::DEF`), no definition of the
        // struct, refused where an API uses it (STD-112)
        if trait_name == "Codec"
            && !item_impl.generics.params.is_empty()
            && let syn::Type::Path(self_ty) = &*item_impl.self_ty
            && let Some(name) = self_ty.path.segments.last()
        {
            generic_codecs.push(name.ident.clone());
            continue;
        }
        if trait_name == "Codec"
            && let syn::Type::Path(self_ty) = &*item_impl.self_ty
            && let Some(name) = self_ty.path.segments.last()
            && let Some(def) = item_impl.items.iter().find_map(|i| match i {
                syn::ImplItem::Const(c) if c.ident == "DEF" => Some(c.expr.clone()),
                _ => None,
            })
        {
            let mut def = def;
            inline_tables(&mut def, &tables);
            codecs.push((name.ident.clone(), def));
            continue;
        }
        if trait_name == "Reference"
            && let syn::Type::Path(self_ty) = &*item_impl.self_ty
            && let Some(name) = self_ty.path.segments.last()
        {
            let konst = |k: &str| {
                item_impl.items.iter().find_map(|i| match i {
                    syn::ImplItem::Const(c) if c.ident == k => Some(c.expr.clone()),
                    _ => None,
                })
            };
            references.push((name.ident.clone(), konst("NAME"), konst("HOW")));
            continue;
        }
        let konst = match trait_name.as_str() {
            "Unit" => "DIM",
            "FixedScale" => "SCALE",
            "Origin" => "OFFSET",
            _ => continue,
        };
        let defines = item_impl
            .items
            .iter()
            .any(|i| matches!(i, syn::ImplItem::Const(c) if c.ident == konst));
        if let (true, syn::Type::Path(self_ty)) = (defines, &*item_impl.self_ty)
            && let Some(name) = self_ty.path.segments.last()
        {
            hand.push((
                name.ident.clone(),
                Ident::new(&trait_name, name.ident.span()),
            ));
        }
    }
    for item in items.iter_mut() {
        if let Item::Struct(item_struct) = item
            && !item_struct
                .attrs
                .iter()
                .any(|a| a.path().is_ident(MARKER_ATTR))
            && let Some((_, trait_name)) = hand.iter().find(|(name, _)| *name == item_struct.ident)
        {
            let attr = Ident::new(MARKER_ATTR, item_struct.ident.span());
            item_struct
                .attrs
                .push(syn::parse_quote!(#[#attr(hand, #trait_name)]));
        }
        if let Item::Struct(item_struct) = item
            && !item_struct
                .attrs
                .iter()
                .any(|a| a.path().is_ident(MARKER_ATTR))
            && let Some((_, name, how)) = references
                .iter()
                .find(|(ident, _, _)| *ident == item_struct.ident)
        {
            let attr = Ident::new(MARKER_ATTR, item_struct.ident.span());
            // a missing constant is refused when the marker is used ([MarkerLineDef::Reference])
            let name = name
                .clone()
                .unwrap_or_else(|| syn::parse_quote!(missing_name));
            let how = how
                .clone()
                .unwrap_or_else(|| syn::parse_quote!(missing_how));
            item_struct
                .attrs
                .push(syn::parse_quote!(#[#attr(reference, #name, #how)]));
        }
        if let Item::Struct(item_struct) = item
            && !item_struct
                .attrs
                .iter()
                .any(|a| a.path().is_ident(MARKER_ATTR))
            && let Some((_, def)) = codecs.iter().find(|(ident, _)| *ident == item_struct.ident)
        {
            let attr = Ident::new(MARKER_ATTR, item_struct.ident.span());
            item_struct
                .attrs
                .push(syn::parse_quote!(#[#attr(codec, #def)]));
        }
        if let Item::Struct(item_struct) = item
            && !item_struct
                .attrs
                .iter()
                .any(|a| a.path().is_ident(MARKER_ATTR))
            && generic_codecs.contains(&item_struct.ident)
        {
            let attr = Ident::new(MARKER_ATTR, item_struct.ident.span());
            item_struct
                .attrs
                .push(syn::parse_quote!(#[#attr(generic_codec)]));
        }
    }
}

/// Replaces each table a codec's `DEF` names, `def_list(&X)` with `X` a `const X: [T; N] = [..]` of the same module,
/// by its literal, so the `DEF` reads alone once it is a `#[ww_marker(codec, ..)]`: the walker evaluates no `const`
/// code, and a table computed by one stays a name that [def_list_items] refuses.
fn inline_tables(e: &mut syn::Expr, tables: &[(Ident, syn::Expr)]) {
    match e {
        syn::Expr::Path(p) => {
            if let Some(ident) = p.path.get_ident()
                && let Some((_, table)) = tables.iter().find(|(name, _)| name == ident)
            {
                *e = table.clone();
            }
        }
        syn::Expr::Reference(r) => inline_tables(&mut r.expr, tables),
        syn::Expr::Call(c) => c.args.iter_mut().for_each(|a| inline_tables(a, tables)),
        syn::Expr::Struct(s) => s
            .fields
            .iter_mut()
            .for_each(|f| inline_tables(&mut f.expr, tables)),
        _ => {}
    }
}

/// The field `name` of a `DEF`'s struct expression (`CodecDef { .. }`, `CodecShape::Offset { .. }`).
fn field<'a>(s: &'a syn::ExprStruct, name: &str) -> Result<&'a syn::Expr> {
    s.fields
        .iter()
        .find(|f| matches!(&f.member, syn::Member::Named(i) if i == name))
        .map(|f| &f.expr)
        .ok_or_else(|| anyhow!("`{}`: no `{name}: ..`", quote::quote!(#s)))
}

/// The elements of a list as `exact_units` writes one in a `DEF`: `def_list(&[..])`, the slice `&[..]` alone, or
/// either naming a table of the same module that [inline_tables] put in.
fn def_list_items(e: &syn::Expr) -> Result<impl Iterator<Item = &syn::Expr>> {
    let list = match e {
        syn::Expr::Call(c)
            if c.args.len() == 1
                && matches!(&*c.func, syn::Expr::Path(p)
                    if p.path.segments.last().is_some_and(|s| s.ident == "def_list")) =>
        {
            &c.args[0]
        }
        other => other,
    };
    let syn::Expr::Reference(r) = list else {
        bail!("`{}`: def_list(&[..])", quote::quote!(#e));
    };
    match &*r.expr {
        syn::Expr::Array(a) => Ok(a.elems.iter()),
        syn::Expr::Path(p) => bail!(
            "`{}`: `{}` is no `const` array literal of the codec's module; the walker evaluates no `const` code, \
             so a table is written out",
            quote::quote!(#e),
            quote::quote!(#p)
        ),
        _ => bail!("`{}`: def_list(&[..])", quote::quote!(#e)),
    }
}

/// The `shape` of an `impl Codec`'s `DEF`: `CodecShape::MuLaw`, `ALaw`, `Table(def_list(&[..]))`, `Offset { zero }`
/// or `MantissaExponent { radix, exponents, mantissas, mantissa_offset, exponent_zero, exponent_step }`, each number
/// an integer expression [int_expr] reads.
fn codec_shape(def: &syn::Expr) -> Result<CodecShapeOwned> {
    let syn::Expr::Struct(s) = def else {
        bail!("DEF `{}`: CodecDef {{ id, shape }}", quote::quote!(#def));
    };
    let shape = field(s, "shape").with_context(|| format!("DEF `{}`", quote::quote!(#def)))?;
    let last = |path: &syn::Path| path.segments.last().map(|s| s.ident.to_string());
    let read = || -> Result<CodecShapeOwned> {
        match shape {
            syn::Expr::Path(p) => match last(&p.path).as_deref() {
                Some("MuLaw") => Ok(CodecShapeOwned::MuLaw),
                Some("ALaw") => Ok(CodecShapeOwned::ALaw),
                _ => bail!("not a CodecShape"),
            },
            syn::Expr::Call(c)
                if c.args.len() == 1
                    && matches!(&*c.func, syn::Expr::Path(p) if last(&p.path).as_deref() == Some("Table")) =>
            {
                let table = def_list_items(&c.args[0])?
                    .map(int_expr)
                    .collect::<Result<Vec<_>>>()?;
                Ok(CodecShapeOwned::Table(table))
            }
            syn::Expr::Struct(f) => match last(&f.path).as_deref() {
                Some("Offset") => Ok(CodecShapeOwned::Offset {
                    zero: int_expr(field(f, "zero")?)?,
                }),
                Some("MantissaExponent") => Ok(CodecShapeOwned::MantissaExponent {
                    radix: int_expr(field(f, "radix")?)?,
                    exponents: int_expr(field(f, "exponents")?)?,
                    mantissas: def_list_items(field(f, "mantissas")?)?
                        .map(int_expr)
                        .collect::<Result<Vec<_>>>()?,
                    mantissa_offset: int_expr(field(f, "mantissa_offset")?)?,
                    exponent_zero: int_expr(field(f, "exponent_zero")?)?,
                    exponent_step: int_expr(field(f, "exponent_step")?)?,
                }),
                _ => bail!("not a CodecShape"),
            },
            _ => bail!("not a CodecShape"),
        }
    };
    read().with_context(|| {
        format!(
            "shape `{}`: the walker reads MuLaw, ALaw, Table(def_list(&[..])), Offset {{ zero }} and \
             MantissaExponent {{ .. }}",
            quote::quote!(#shape)
        )
    })
}

/// The `spare` codes of an `impl Codec`'s `DEF`, as `exact_units` writes them: `def_list(&[])`,
/// `def_list(&[CodeSpan::one(0x7F)])`, `def_list(&[CodeSpan { low: 0x07FE, high: 0x0802 }])` or a same-module table
/// of them, each code an integer [int_expr] reads. Checked as `Coded::SPARE` checks them: every span ascending.
fn codec_spare(def: &syn::Expr) -> Result<Vec<CodeSpan>> {
    let syn::Expr::Struct(s) = def else {
        bail!(
            "DEF `{}`: CodecDef {{ id, shape, spare }}",
            quote::quote!(#def)
        );
    };
    let Some(spare) = s
        .fields
        .iter()
        .find(|f| matches!(&f.member, syn::Member::Named(i) if i == "spare"))
        .map(|f| &f.expr)
    else {
        bail!("DEF `{}`: no `spare: ..`", quote::quote!(#def));
    };
    def_list_items(spare)
        .with_context(|| {
            format!(
                "spare `{}`: def_list(&[CodeSpan, ..])",
                quote::quote!(#spare)
            )
        })?
        .map(|e| {
            let span = code_span(e).with_context(|| format!("spare `{}`", quote::quote!(#e)))?;
            if span.low > span.high {
                bail!(
                    "spare `{}`: its low code is above its high code",
                    quote::quote!(#e)
                );
            }
            Ok(span)
        })
        .collect()
}

/// The `codes` of an `impl Codec`'s `DEF`: every code its `Raw` holds, as `exact_units` writes it,
/// `CodeSpan::all::<Raw>()`.
fn codec_codes(def: &syn::Expr) -> Result<CodeSpan> {
    let syn::Expr::Struct(s) = def else {
        bail!(
            "DEF `{}`: CodecDef {{ id, shape, spare, codes }}",
            quote::quote!(#def)
        );
    };
    let Some(codes) = s
        .fields
        .iter()
        .find(|f| matches!(&f.member, syn::Member::Named(i) if i == "codes"))
        .map(|f| &f.expr)
    else {
        bail!("DEF `{}`: no `codes: ..`", quote::quote!(#def));
    };
    let span = code_span(codes).with_context(|| format!("codes `{}`", quote::quote!(#codes)))?;
    if span.low > span.high {
        bail!(
            "codes `{}`: its low code is above its high code",
            quote::quote!(#codes)
        );
    }
    Ok(span)
}

/// The `nominal` of an `impl Codec`'s `DEF`: `None`, or `Some(CodeSpan ..)` as [codec_codes] reads a `CodeSpan`.
fn codec_nominal(def: &syn::Expr) -> Result<Option<CodeSpan>> {
    let syn::Expr::Struct(s) = def else {
        bail!(
            "DEF `{}`: CodecDef {{ id, shape, spare, codes, nominal }}",
            quote::quote!(#def)
        );
    };
    let Some(nominal) = s
        .fields
        .iter()
        .find(|f| matches!(&f.member, syn::Member::Named(i) if i == "nominal"))
        .map(|f| &f.expr)
    else {
        bail!("DEF `{}`: no `nominal: ..`", quote::quote!(#def));
    };
    match nominal {
        syn::Expr::Path(p) if p.path.is_ident("None") => Ok(None),
        syn::Expr::Call(c)
            if c.args.len() == 1
                && matches!(&*c.func, syn::Expr::Path(p) if p.path.is_ident("Some")) =>
        {
            Ok(Some(code_span(&c.args[0]).with_context(|| {
                format!("nominal `{}`", quote::quote!(#nominal))
            })?))
        }
        _ => bail!(
            "nominal `{}`: None or Some(CodeSpan ..)",
            quote::quote!(#nominal)
        ),
    }
}

/// The `curve` of an `impl Codec`'s `DEF`: `None`, or `Some(CurveDef { id, referred, unit, scale, low, high,
/// segments })`, every `Segment`'s `CurveForm` (`Affine`, `Power`, `Exp`, `RationalPower`) and `ExpBase` (`E`,
/// `Exactly`) a literal, every number a [fraction_lit].
fn codec_curve(def: &syn::Expr) -> Result<Option<CurveDefOwned>> {
    let syn::Expr::Struct(s) = def else {
        bail!(
            "DEF `{}`: CodecDef {{ id, shape, spare, codes, nominal, curve }}",
            quote::quote!(#def)
        );
    };
    let curve = field(s, "curve").with_context(|| format!("DEF `{}`", quote::quote!(#def)))?;
    match curve {
        syn::Expr::Path(p) if p.path.is_ident("None") => Ok(None),
        syn::Expr::Call(c)
            if c.args.len() == 1
                && matches!(&*c.func, syn::Expr::Path(p) if p.path.is_ident("Some")) =>
        {
            curve_def_lit(&c.args[0])
                .with_context(|| format!("curve `{}`", quote::quote!(#curve)))
                .map(Some)
        }
        _ => bail!(
            "curve `{}`: None or Some(CurveDef {{ .. }})",
            quote::quote!(#curve)
        ),
    }
}

/// A `CurveDef { id, referred, unit, scale, low, high, segments }` literal, `segments` a `def_list(&[Segment, ..])`
/// (or a same-module table [def_list_items] already inlined).
fn curve_def_lit(e: &syn::Expr) -> Result<CurveDefOwned> {
    let syn::Expr::Struct(s) = e else {
        bail!(
            "`{}` is not CurveDef {{ id, referred, unit, scale, low, high, segments }}",
            quote::quote!(#e)
        );
    };
    let id = marker_id_lit(field(s, "id")?)?;
    let referred = referred_lit(field(s, "referred")?)?;
    let unit = marker_id_lit(field(s, "unit")?)?;
    let scale = fraction_lit(field(s, "scale")?)?;
    let low = fraction_lit(field(s, "low")?)?;
    let high = fraction_lit(field(s, "high")?)?;
    let segments = def_list_items(field(s, "segments")?)?
        .map(segment_lit)
        .collect::<Result<Vec<_>>>()?;
    Ok(CurveDefOwned {
        id,
        referred,
        unit,
        scale,
        low,
        high,
        segments,
    })
}

/// A `Segment { from, form }` literal.
fn segment_lit(e: &syn::Expr) -> Result<Segment> {
    let syn::Expr::Struct(s) = e else {
        bail!("`{}` is not Segment {{ from, form }}", quote::quote!(#e));
    };
    let from = fraction_lit(field(s, "from")?)?;
    let form = curve_form_lit(field(s, "form")?)?;
    Ok(Segment { from, form })
}

/// One `CurveForm` variant literal: `Affine { gain, offset }`, `Power { gain, scale, shift, exponent, offset }`,
/// `Exp { gain, base, scale, shift, offset }` or `RationalPower { m1, m2, c1, c2, c3 }`, every parameter a
/// [fraction_lit].
fn curve_form_lit(e: &syn::Expr) -> Result<CurveForm> {
    let syn::Expr::Struct(s) = e else {
        bail!(
            "`{}` is not a CurveForm variant (Affine, Power, Exp or RationalPower)",
            quote::quote!(#e)
        );
    };
    let frac = |name: &str| -> Result<Fraction> { fraction_lit(field(s, name)?) };
    let last = |path: &syn::Path| path.segments.last().map(|s| s.ident.to_string());
    match last(&s.path).as_deref() {
        Some("Affine") => Ok(CurveForm::Affine {
            gain: frac("gain")?,
            offset: frac("offset")?,
        }),
        Some("Power") => Ok(CurveForm::Power {
            gain: frac("gain")?,
            scale: frac("scale")?,
            shift: frac("shift")?,
            exponent: frac("exponent")?,
            offset: frac("offset")?,
        }),
        Some("Exp") => Ok(CurveForm::Exp {
            gain: frac("gain")?,
            base: exp_base_lit(field(s, "base")?)?,
            scale: frac("scale")?,
            shift: frac("shift")?,
            offset: frac("offset")?,
        }),
        Some("RationalPower") => Ok(CurveForm::RationalPower {
            m1: frac("m1")?,
            m2: frac("m2")?,
            c1: frac("c1")?,
            c2: frac("c2")?,
            c3: frac("c3")?,
        }),
        _ => bail!(
            "`{}` is not a CurveForm variant (Affine, Power, Exp or RationalPower)",
            quote::quote!(#e)
        ),
    }
}

/// `ExpBase::E` or `ExpBase::Exactly(<fraction>)`.
fn exp_base_lit(e: &syn::Expr) -> Result<ExpBase> {
    match e {
        syn::Expr::Path(p) if p.path.segments.last().is_some_and(|s| s.ident == "E") => {
            Ok(ExpBase::E)
        }
        syn::Expr::Call(c)
            if c.args.len() == 1
                && matches!(&*c.func, syn::Expr::Path(p) if p.path.segments.last().is_some_and(|s| s.ident == "Exactly")) =>
        {
            Ok(ExpBase::Exactly(fraction_lit(&c.args[0])?))
        }
        _ => bail!("`{}` is not ExpBase::E or ExpBase::Exactly(..)", quote::quote!(#e)),
    }
}

/// `Referred::Scene` or `Referred::Display`, by the last segment of its path.
fn referred_lit(e: &syn::Expr) -> Result<Referred> {
    let syn::Expr::Path(p) = e else {
        bail!(
            "`{}` is not Referred::Scene or Referred::Display",
            quote::quote!(#e)
        );
    };
    match p.path.segments.last().map(|s| s.ident.to_string()).as_deref() {
        Some("Scene") => Ok(Referred::Scene),
        Some("Display") => Ok(Referred::Display),
        _ => bail!(
            "`{}` is not Referred::Scene or Referred::Display",
            quote::quote!(#e)
        ),
    }
}

/// A `Fraction` literal: `Fraction::ZERO`, `Fraction::ONE`, `Fraction::int(n)` or `Fraction::new(num, den)`, every
/// number an [int_expr].
fn fraction_lit(e: &syn::Expr) -> Result<Fraction> {
    let call_name = |c: &syn::ExprCall| match &*c.func {
        syn::Expr::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
        _ => None,
    };
    let not_a_fraction = || -> anyhow::Error {
        anyhow!(
            "`{}` is not Fraction::ZERO, Fraction::ONE, Fraction::int(n) or Fraction::new(num, den)",
            quote::quote!(#e)
        )
    };
    match e {
        syn::Expr::Path(p) => match p.path.segments.last().map(|s| s.ident.to_string()).as_deref() {
            Some("ZERO") => Ok(Fraction::ZERO),
            Some("ONE") => Ok(Fraction::ONE),
            _ => Err(not_a_fraction()),
        },
        syn::Expr::Call(c) => match (call_name(c).as_deref(), c.args.len()) {
            (Some("int"), 1) => Ok(Fraction::int(int_expr(&c.args[0])?)),
            (Some("new"), 2) => {
                let den = u128::try_from(int_expr(&c.args[1])?)
                    .map_err(|_| anyhow!("Fraction::new(.., den): den doesn't fit a u128"))?;
                Ok(Fraction::new(int_expr(&c.args[0])?, den))
            }
            _ => Err(not_a_fraction()),
        },
        _ => Err(not_a_fraction()),
    }
}

/// A `MarkerId { crate_name: "..", name: ".." }` literal, both string literals.
fn marker_id_lit(e: &syn::Expr) -> Result<MarkerIdOwned> {
    let syn::Expr::Struct(s) = e else {
        bail!(
            "`{}` is not MarkerId {{ crate_name, name }}",
            quote::quote!(#e)
        );
    };
    let str_field = |name: &str| -> Result<String> {
        match field(s, name)? {
            syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(lit),
                ..
            }) => Ok(lit.value()),
            other => bail!("MarkerId {{ .. }}: `{name}` is `{}`, not a string literal", quote::quote!(#other)),
        }
    };
    Ok(MarkerIdOwned {
        crate_name: str_field("crate_name")?,
        name: str_field("name")?,
    })
}

/// A `CodeSpan::one(code)`, `CodeSpan { low, high }` or `CodeSpan::all::<T>()` for an integer `T`.
fn code_span(e: &syn::Expr) -> Result<CodeSpan> {
    match e {
        syn::Expr::Call(c)
            if c.args.is_empty()
                && matches!(&*c.func, syn::Expr::Path(p)
                    if p.path.segments.len() >= 2
                        && p.path.segments[p.path.segments.len() - 2].ident == "CodeSpan"
                        && p.path.segments[p.path.segments.len() - 1].ident == "all") =>
        {
            let syn::Expr::Path(p) = &*c.func else {
                unreachable!()
            };
            let args = match &p.path.segments.last().unwrap().arguments {
                syn::PathArguments::AngleBracketed(a) => &a.args,
                _ => bail!("CodeSpan::all::<T>(): no `T`"),
            };
            let Some(syn::GenericArgument::Type(syn::Type::Path(t))) = args.first() else {
                bail!("CodeSpan::all::<T>(): no `T`");
            };
            let Some(ty) = t.path.segments.last().map(|s| s.ident.to_string()) else {
                bail!("CodeSpan::all::<T>(): no `T`");
            };
            all_of(&ty).ok_or_else(|| {
                anyhow!(
                    "CodeSpan::all::<{ty}>(): the walker knows u8, u16, u32, u64, i8, i16, i32, i64"
                )
            })
        }
        syn::Expr::Call(c)
            if c.args.len() == 1
                && matches!(&*c.func, syn::Expr::Path(p)
                    if p.path.segments.len() >= 2
                        && p.path.segments[p.path.segments.len() - 2].ident == "CodeSpan"
                        && p.path.segments[p.path.segments.len() - 1].ident == "one") =>
        {
            Ok(CodeSpan::one(int_expr(&c.args[0])?))
        }
        syn::Expr::Struct(s)
            if s.path
                .segments
                .last()
                .is_some_and(|s| s.ident == "CodeSpan") =>
        {
            let field = |name: &str| {
                s.fields
                    .iter()
                    .find(|f| matches!(&f.member, syn::Member::Named(i) if i == name))
                    .map(|f| int_expr(&f.expr))
                    .unwrap_or_else(|| bail!("no `{name}: ..`"))
            };
            Ok(CodeSpan {
                low: field("low")?,
                high: field("high")?,
            })
        }
        _ => bail!("not CodeSpan::one(code), CodeSpan {{ low, high }} or CodeSpan::all::<T>()"),
    }
}

/// `CodeSpan::all::<T>()` for the integer storages the walker knows, `T::MIN..=T::MAX` as `i128`.
fn all_of(ty: &str) -> Option<CodeSpan> {
    let (low, high) = match ty {
        "u8" => (u8::MIN.into(), u8::MAX.into()),
        "u16" => (u16::MIN.into(), u16::MAX.into()),
        "u32" => (u32::MIN.into(), u32::MAX.into()),
        "u64" => (u64::MIN.into(), u64::MAX.into()),
        "i8" => (i8::MIN.into(), i8::MAX.into()),
        "i16" => (i16::MIN.into(), i16::MAX.into()),
        "i32" => (i32::MIN.into(), i32::MAX.into()),
        "i64" => (i64::MIN.into(), i64::MAX.into()),
        _ => return None,
    };
    Some(CodeSpan { low, high })
}

/// `exact_units::ReferenceHow` as an `impl Reference`'s `HOW` writes it: `ReferenceHow::Exact`,
/// `ReferenceHow::RunTime` or `ReferenceHow::TimeScale(TimeScaleDef { counting: TimeCounting::EverySecond, zero:
/// TimeZero::Agreed })`, by the last segment of each path.
fn reference_how(e: &syn::Expr) -> Result<ReferenceHow> {
    let last = |p: &syn::Path| p.segments.last().map(|s| s.ident.to_string());
    let field = |s: &syn::ExprStruct, name: &str| -> Result<String> {
        s.fields
            .iter()
            .find(|f| matches!(&f.member, syn::Member::Named(i) if i == name))
            .and_then(|f| match &f.expr {
                syn::Expr::Path(p) => last(&p.path),
                _ => None,
            })
            .ok_or_else(|| anyhow!("`{}`: no `{name}: <path>`", quote::quote!(#s)))
    };
    match e {
        syn::Expr::Path(p) => match last(&p.path).as_deref() {
            Some("Exact") => Ok(ReferenceHow::Exact),
            Some("RunTime") => Ok(ReferenceHow::RunTime),
            _ => bail!(
                "HOW `{}`: Exact, RunTime or TimeScale(..)",
                quote::quote!(#e)
            ),
        },
        syn::Expr::Call(c)
            if matches!(&*c.func, syn::Expr::Path(p) if last(&p.path).as_deref() == Some("TimeScale"))
                && c.args.len() == 1 =>
        {
            let syn::Expr::Struct(s) = &c.args[0] else {
                bail!("`{}`: TimeScale(TimeScaleDef {{ .. }})", quote::quote!(#e));
            };
            let counting = match field(s, "counting")?.as_str() {
                "EverySecond" => TimeCounting::EverySecond,
                "PosixDays" => TimeCounting::PosixDays,
                other => bail!("counting `{other}`: EverySecond or PosixDays"),
            };
            let zero = match field(s, "zero")?.as_str() {
                "Agreed" => TimeZero::Agreed,
                "RunTime" => TimeZero::RunTime,
                other => bail!("zero `{other}`: Agreed or RunTime"),
            };
            Ok(ReferenceHow::TimeScale(TimeScaleDef { counting, zero }))
        }
        _ => bail!(
            "HOW `{}`: Exact, RunTime or TimeScale(..)",
            quote::quote!(#e)
        ),
    }
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

/// One line: `<attrs> <vis> struct <Name> = <def>;`, for a `quantity!` line also `: <def>` (the `=` or `:` is kept in
/// `def`).
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
        let mut def = TokenStream::new();
        if kind == MacroKind::Quantity && (input.peek(Token![:]) || input.peek(Token![=])) {
            def.extend([input.parse::<proc_macro2::TokenTree>()?]);
        } else {
            input.parse::<Token![=]>()?;
        }
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
            MacroKind::Quantity => {
                syn::parse2::<QuantityLine>(def.clone())?;
            }
            MacroKind::Origin => unreachable!("an origin! is one OriginDecl"),
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
            MacroKind::Quantity => quote::quote!(quantity),
            MacroKind::Origin => unreachable!("an origin! is one OriginDecl"),
        };
        syn::parse_quote! {
            #(#attrs)*
            #[#attr(#kind, #def)]
            #vis struct #ident;
        }
    }
}

/// `<attrs> <vis> struct <Name> { <keyed fields> }`: the body of an `origin!`, one zero-size origin marker.
struct OriginDecl {
    attrs: Vec<Attribute>,
    vis: Visibility,
    ident: Ident,
    fields: TokenStream,
}

impl Parse for OriginDecl {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let attrs = Attribute::parse_outer(input)?;
        let vis = input.parse()?;
        input.parse::<Token![struct]>()?;
        let ident = input.parse()?;
        let body;
        syn::braced!(body in input);
        let fields = body.parse()?;
        Ok(OriginDecl {
            attrs,
            vis,
            ident,
            fields,
        })
    }
}

impl OriginDecl {
    fn to_struct(&self) -> syn::ItemStruct {
        let OriginDecl {
            attrs,
            vis,
            ident,
            fields,
        } = self;
        let attr = Ident::new(MARKER_ATTR, ident.span());
        syn::parse_quote! {
            #(#attrs)*
            #[#attr(origin, #fields)]
            #vis struct #ident;
        }
    }
}

/// The fields of an `origin!`, in the order the macro takes them.
pub(crate) enum OriginFields {
    /// From the unit's zero, or from a reference.
    Unit {
        // boxed: a `syn::Type` is over 200 bytes, and would make the variant far larger than `Above`
        unit: Box<syn::Type>,
        symbol: String,
        offset: syn::Expr,
        // boxed: rarely given, and they would make the variant far larger than `Above`
        step: Option<Box<syn::Expr>>,
        reference: Option<Box<syn::Type>>,
        modulus: Option<Box<syn::Type>>,
    },
    /// `above: Parent, offset: ..`.
    Above {
        parent: Box<syn::Type>,
        offset: syn::Expr,
    },
}

impl Parse for OriginFields {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        fn key(input: ParseStream, name: &str) -> syn::Result<bool> {
            let fork = input.fork();
            match fork.parse::<Ident>() {
                Ok(ident) if ident == name => {
                    input.parse::<Ident>()?;
                    input.parse::<Token![:]>()?;
                    Ok(true)
                }
                _ => Ok(false),
            }
        }
        fn comma(input: ParseStream) -> syn::Result<()> {
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
            Ok(())
        }
        let fields = if key(input, "above")? {
            let parent = input.parse()?;
            input.parse::<Token![,]>()?;
            if !key(input, "offset")? {
                return Err(input.error("`offset: ..` after `above`"));
            }
            let offset = input.parse()?;
            comma(input)?;
            OriginFields::Above { parent, offset }
        } else {
            if !key(input, "unit")? {
                return Err(input.error("`unit: ..` or `above: ..`"));
            }
            let unit = Box::new(input.parse()?);
            input.parse::<Token![,]>()?;
            if !key(input, "symbol")? {
                return Err(input.error("`symbol: \"..\"` after `unit`"));
            }
            let symbol = input.parse::<LitStr>()?.value();
            input.parse::<Token![,]>()?;
            if !key(input, "offset")? {
                return Err(input.error("`offset: ..` after `symbol`"));
            }
            let offset = input.parse()?;
            comma(input)?;
            let step = if key(input, "step")? {
                let step = input.parse()?;
                comma(input)?;
                Some(Box::new(step))
            } else {
                None
            };
            let reference = if key(input, "reference")? {
                let reference = input.parse()?;
                comma(input)?;
                Some(Box::new(reference))
            } else {
                None
            };
            let modulus = if key(input, "modulus")? {
                let modulus = input.parse()?;
                comma(input)?;
                Some(Box::new(modulus))
            } else {
                None
            };
            OriginFields::Unit {
                unit,
                symbol,
                offset,
                step,
                reference,
                modulus,
            }
        };
        if !input.is_empty() {
            return Err(input.error("step, reference and modulus, in that order, after offset"));
        }
        Ok(fields)
    }
}

/// The definition of an `origin!` with `fields`, in `current_crate`: its unit, reference and modulus (or its parent)
/// converted as types and resolved through their own definitions, so the model carries the origin whole.
pub(crate) fn origin_def(
    fields: &OriginFields,
    current_crate: &CrateContext,
    scratch: &mut Scratch,
) -> Result<OriginDefOwned> {
    let mut resolve = |ty: &syn::Type| -> Result<(ww_self::TypeOwned, ApiBundleOwned)> {
        let ty = convert_ty(ty, current_crate, scratch)?;
        Ok((ty, scratch.root_bundle.to_bundle()))
    };
    match fields {
        OriginFields::Unit {
            unit,
            symbol,
            offset,
            step,
            reference,
            modulus,
        } => {
            let (unit_ty, bundle) = resolve(unit)?;
            let unit = unit_of(&unit_ty, &bundle).context("the origin's unit")?;
            let Some(unit) = unit.named().cloned() else {
                bail!(
                    "the origin's unit {unit} is not a named unit: declare one of its dimension with                      `exact_units::unit!` and use it"
                );
            };
            let reference = match reference {
                Some(r) if !last_ident_is(r, "UnitZero") => {
                    let (ty, bundle) = resolve(r)?;
                    reference_of(&ty, &bundle).context("the origin's reference")?
                }
                _ => unit_zero(),
            };
            let modulus = match modulus {
                Some(m) if !last_ident_is(m, "Linear") => {
                    let period = modulo_arg(m).ok_or_else(|| {
                        anyhow!("modulus `{}`: Linear or Modulo<S>", quote::quote!(#m))
                    })?;
                    let (ty, bundle) = resolve(period)?;
                    Some(scale_of(&ty, &bundle).context("the origin's modulus")?)
                }
                _ => None,
            };
            let offset = offset_value(offset)?;
            Ok(OriginDefOwned {
                // set by marker_def, which knows the origin's crate and name
                id: MarkerIdOwned {
                    crate_name: String::new(),
                    name: String::new(),
                },
                unit,
                symbol: symbol.clone(),
                reference,
                offset,
                modulus,
                step: step
                    .as_ref()
                    .map(|s| step_value(s))
                    .transpose()?
                    .unwrap_or(Magnitude::ONE),
                shown: offset,
            })
        }
        OriginFields::Above { parent, offset } => {
            let (ty, bundle) = resolve(parent)?;
            let mut def = origin_of(&ty, &bundle).context("the origin it is above")?;
            def.offset = def
                .offset
                .checked_add(offset_value(offset)?)
                .ok_or_else(|| {
                    anyhow!("the offsets don't add up to a fraction of 64-bit integers")
                })?;
            Ok(def)
        }
    }
}

fn last_ident_is(ty: &syn::Type, name: &str) -> bool {
    matches!(ty, syn::Type::Path(p) if p.path.segments.last().is_some_and(|s| s.ident == name))
}

/// `S` of `Modulo<S>`.
fn modulo_arg(ty: &syn::Type) -> Option<&syn::Type> {
    let syn::Type::Path(p) = ty else {
        return None;
    };
    let last = p.path.segments.last()?;
    if last.ident != "Modulo" {
        return None;
    }
    match &last.arguments {
        syn::PathArguments::AngleBracketed(a) if a.args.len() == 1 => match &a.args[0] {
            syn::GenericArgument::Type(t) => Some(t),
            _ => None,
        },
        _ => None,
    }
}

/// The value of an `origin!`'s `offset`: `Offset::ZERO`, `Offset::int(n)`, `Offset::ratio(num, den)` or
/// `Offset::decimal(num, exp)` with integer literals, as `exact_units` computes it.
pub(crate) fn offset_value(e: &syn::Expr) -> Result<Offset> {
    let (name, args) = const_call(e, "Offset")?;
    let wrong = || {
        anyhow!(
            "offset `{}`: clients read only Offset::ZERO, Offset::int(n), Offset::ratio(num, den) and \
             Offset::decimal(num, exp) with integer literals",
            quote::quote!(#e)
        )
    };
    let int = |i: usize| args.get(i).copied().ok_or_else(wrong).and_then(int_lit);
    let o = match (name.as_str(), args.len()) {
        ("ZERO", 0) => Offset::ZERO,
        ("int", 1) => ratio_offset(int(0)?, 1)?,
        ("ratio", 2) => ratio_offset(
            int(0)?,
            u64::try_from(int(1)?).map_err(|_| anyhow!("an offset's denominator is positive"))?,
        )?,
        ("decimal", 2) => {
            let exp =
                i8::try_from(int(1)?).map_err(|_| anyhow!("decimal exponent out of range"))?;
            let p = Magnitude::checked_pow10(exp)
                .ok_or_else(|| anyhow!("decimal exponent out of range"))?;
            let o = ratio_offset(int(0)?, 1)?;
            match o.magnitude() {
                Some(m) => Offset::new(
                    o.is_negative(),
                    m.checked_mul(p).map_err(|e| anyhow!("offset: {e:?}"))?,
                ),
                None => o,
            }
        }
        _ => return Err(wrong()),
    };
    Ok(o)
}

/// The value of an `origin!`'s `step`: `Magnitude::ONE`, `Magnitude::ratio(num, den)` or `Magnitude::pow10(e)`.
pub(crate) fn step_value(e: &syn::Expr) -> Result<Magnitude> {
    let (name, args) = const_call(e, "Magnitude")?;
    let wrong = || {
        anyhow!(
            "step `{}`: clients read only Magnitude::ONE, Magnitude::ratio(num, den) and Magnitude::pow10(e) with \
             integer literals",
            quote::quote!(#e)
        )
    };
    let int = |i: usize| args.get(i).copied().ok_or_else(wrong).and_then(int_lit);
    let positive =
        |i: usize| u64::try_from(int(i)?).map_err(|_| anyhow!("a step is a positive ratio"));
    match (name.as_str(), args.len()) {
        ("ONE", 0) => Ok(Magnitude::ONE),
        ("ratio", 2) => {
            Magnitude::checked_ratio(positive(0)?, positive(1)?).map_err(|e| anyhow!("step: {e:?}"))
        }
        ("pow10", 1) => i8::try_from(int(0)?)
            .ok()
            .and_then(Magnitude::checked_pow10)
            .ok_or_else(|| anyhow!("step exponent out of range")),
        _ => Err(wrong()),
    }
}

fn ratio_offset(num: i128, den: u64) -> Result<Offset> {
    if num == 0 {
        return Ok(Offset::ZERO);
    }
    let n = u64::try_from(num.unsigned_abs())
        .map_err(|_| anyhow!("offset {num} doesn't fit 64 bits"))?;
    Ok(Offset::new(
        num < 0,
        Magnitude::checked_ratio(n, den).map_err(|e| anyhow!("offset {num}/{den}: {e:?}"))?,
    ))
}

/// `Ty::NAME` or `Ty::name(args..)` (with any path before `Ty`): the name and the arguments.
fn const_call<'a>(e: &'a syn::Expr, ty: &str) -> Result<(String, Vec<&'a syn::Expr>)> {
    let (path, args) = match e {
        syn::Expr::Path(p) => (&p.path, vec![]),
        syn::Expr::Call(c) => match &*c.func {
            syn::Expr::Path(p) => (&p.path, c.args.iter().collect()),
            _ => bail!("`{}` is not `{ty}::..`", quote::quote!(#e)),
        },
        _ => bail!("`{}` is not `{ty}::..`", quote::quote!(#e)),
    };
    let n = path.segments.len();
    if n < 2 || path.segments[n - 2].ident != ty {
        bail!("`{}` is not `{ty}::..`", quote::quote!(#e));
    }
    Ok((path.segments[n - 1].ident.to_string(), args))
}

/// An integer as a `DEF` writes one: a literal, negated, in parentheses, or `+`, `-`, `*` and `<<` of such
/// (`1 << 11`), in `i128` and checked, the arithmetic `const` items use for their numbers. Nothing else: a name or
/// a call is refused, the walker evaluates no `const` code.
fn int_expr(e: &syn::Expr) -> Result<i128> {
    match e {
        syn::Expr::Paren(p) => int_expr(&p.expr),
        syn::Expr::Group(g) => int_expr(&g.expr),
        syn::Expr::Unary(syn::ExprUnary {
            op: syn::UnOp::Neg(_),
            expr,
            ..
        }) => int_expr(expr)?
            .checked_neg()
            .ok_or_else(|| anyhow!("`{}` overflows i128", quote::quote!(#e))),
        syn::Expr::Binary(b) => {
            let (l, r) = (int_expr(&b.left)?, int_expr(&b.right)?);
            match b.op {
                syn::BinOp::Add(_) => l.checked_add(r),
                syn::BinOp::Sub(_) => l.checked_sub(r),
                syn::BinOp::Mul(_) => l.checked_mul(r),
                // checked_shl refuses only a shift past the width: a bit shifted out is refused by shifting back
                syn::BinOp::Shl(_) => u32::try_from(r)
                    .ok()
                    .and_then(|r| l.checked_shl(r).filter(|v| v >> r == l)),
                _ => bail!(
                    "`{}`: the walker reads +, -, * and << of integers",
                    quote::quote!(#e)
                ),
            }
            .ok_or_else(|| anyhow!("`{}` overflows i128", quote::quote!(#e)))
        }
        _ => int_lit(e),
    }
}

/// An integer literal, possibly negated.
fn int_lit(e: &syn::Expr) -> Result<i128> {
    match e {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Int(i),
            ..
        }) => Ok(i.base10_parse()?),
        syn::Expr::Unary(syn::ExprUnary {
            op: syn::UnOp::Neg(_),
            expr,
            ..
        }) => int_lit(expr).map(|v| -v),
        _ => bail!("`{}` is not an integer literal", quote::quote!(#e)),
    }
}

/// What a `#[ww_marker(..)]` attribute says.
pub(crate) enum MarkerLineDef {
    Unit(Box<UnitLine>),
    Scale(ScaleLine),
    /// A hand-written `impl Unit` / `impl FixedScale` (the trait's name).
    Hand(String),
    /// The keyed fields of an `origin!`: `unit: Kelvin, symbol: "°C", offset: Offset::decimal(27315, -2)` or
    /// `above: Celsius, offset: ..`.
    Origin(Box<OriginFields>),
    /// A `quantity!` line.
    Quantity(QuantityLine),
    /// An `impl Reference`'s `NAME` and `HOW` constants, as written (STD-108).
    Reference(Box<(syn::Expr, syn::Expr)>),
    /// An `impl Codec`'s `DEF` constant, as written (LVL-6).
    Codec(Box<syn::Expr>),
    /// A generic `impl<..> Codec`, whose `DEF` is one per instance (`Transfer<Q, C>`): no definition of the struct
    /// alone, an instance an API uses is refused ([generic_codec_error], STD-112).
    GenericCodec,
}

/// Whether `attrs` mark a generic codec ([MarkerLineDef::GenericCodec]).
pub(crate) fn is_generic_codec(attrs: &[Attribute]) -> bool {
    matches!(marker_line(attrs), Ok(Some(MarkerLineDef::GenericCodec)))
}

/// The error for an instance of the generic codec `ident` used in an API: its definition is one per instance,
/// which the API model can't hold yet.
pub(crate) fn generic_codec_error(ident: &syn::Ident) -> anyhow::Error {
    anyhow!(
        "{ident}<..> is a generic codec: its `impl<..> Codec` gives a definition per instance (`CodecDef {{ .., \
         ..Q::DEF }}`), which the API model can't hold yet (STD-112 generic-codec-markers); use a codec with \
         a fixed definition"
    )
}

/// The definition of a struct made from a marker line, `None` for any other struct.
pub(crate) fn marker_line(attrs: &[Attribute]) -> Result<Option<MarkerLineDef>> {
    let Some(attr) = attrs.iter().find(|a| a.path().is_ident(MARKER_ATTR)) else {
        return Ok(None);
    };
    let def = attr.parse_args_with(|input: ParseStream| {
        let kind: Ident = input.parse()?;
        if kind == "generic_codec" {
            return Ok(MarkerLineDef::GenericCodec);
        }
        input.parse::<Token![,]>()?;
        match kind.to_string().as_str() {
            "unit" => Ok(MarkerLineDef::Unit(Box::new(input.parse()?))),
            "scale" => Ok(MarkerLineDef::Scale(input.parse()?)),
            "hand" => Ok(MarkerLineDef::Hand(input.parse::<Ident>()?.to_string())),
            "origin" => Ok(MarkerLineDef::Origin(Box::new(input.parse()?))),
            "quantity" => Ok(MarkerLineDef::Quantity(input.parse()?)),
            "codec" => Ok(MarkerLineDef::Codec(Box::new(input.parse()?))),
            "reference" => {
                let name = input.parse()?;
                input.parse::<Token![,]>()?;
                Ok(MarkerLineDef::Reference(Box::new((name, input.parse()?))))
            }
            _ => Err(syn::Error::new(
                kind.span(),
                "unit, scale, hand, origin, quantity, reference, codec or generic_codec",
            )),
        }
    })?;
    Ok(Some(def))
}

/// `exact_units`' own generic markers, whose meaning is fixed by their name and generic arguments.
static BUILT_IN: [(&str, MarkerDefOwned); 20] = [
    ("Per", MarkerDefOwned::Per),
    ("Times", MarkerDefOwned::Times),
    ("Sqrt", MarkerDefOwned::Sqrt),
    ("Pow10", MarkerDefOwned::Pow10),
    ("Pow2", MarkerDefOwned::Pow2),
    ("Rational", MarkerDefOwned::Rational),
    ("FullScale", MarkerDefOwned::FullScale),
    ("FullScaleMax", MarkerDefOwned::FullScaleMax),
    ("Unbounded", MarkerDefOwned::Unbounded),
    ("Range", MarkerDefOwned::Range),
    ("RangeU64", MarkerDefOwned::RangeU64),
    ("RangeDec", MarkerDefOwned::RangeDec),
    ("Angle", MarkerDefOwned::Angle),
    ("Wrapping", MarkerDefOwned::Wrapping),
    ("Saturating", MarkerDefOwned::Saturating),
    ("Product", MarkerDefOwned::Product),
    ("Quotient", MarkerDefOwned::Quotient),
    ("Of", MarkerDefOwned::Of),
    ("MaybeNan", MarkerDefOwned::MaybeNan),
    ("Finite", MarkerDefOwned::Finite),
];

/// Resolves what a marker line names elsewhere, as types of the crate it is declared in.
pub(crate) trait MarkerRefs {
    /// The definition of an `origin!` with `fields`, its id left empty ([origin_def]).
    fn origin(&mut self, fields: &OriginFields) -> Result<OriginDefOwned>;
    /// The definition of the quantity marker `ty` names (a unit line's `quantity X`, a quantity's parent), the
    /// marker converted so that the bundle lists it.
    fn quantity(&mut self, ty: &syn::Type) -> Result<QuantityDefOwned>;
    /// The definition of the system marker `ty` names (before a dimension's colon, UNIT-9), the marker converted so
    /// that the bundle lists it.
    fn system(&mut self, ty: &syn::Type) -> Result<SystemDefOwned>;
}

/// [MarkerRefs] in `current_crate`: the types converted into the root bundle and resolved through their own
/// definitions.
pub(crate) struct CrateRefs<'a, 'b> {
    pub(crate) current_crate: &'a CrateContext,
    pub(crate) scratch: &'b mut Scratch,
}

impl MarkerRefs for CrateRefs<'_, '_> {
    fn origin(&mut self, fields: &OriginFields) -> Result<OriginDefOwned> {
        origin_def(fields, self.current_crate, self.scratch)
    }

    fn quantity(&mut self, ty: &syn::Type) -> Result<QuantityDefOwned> {
        let ty = convert_ty(ty, self.current_crate, self.scratch)?;
        quantity_of(&ty, &self.scratch.root_bundle.to_bundle())
    }

    fn system(&mut self, ty: &syn::Type) -> Result<SystemDefOwned> {
        let ty = convert_ty(ty, self.current_crate, self.scratch)?;
        system_of(&ty, &self.scratch.root_bundle.to_bundle())
    }
}

/// The crate whose markers [BUILT_IN] names.
const EXACT_UNITS: &str = "exact_units";

/// What `item_struct` of crate `crate_name` (as Rust names it) means as a marker ([ItemStructOwned::marker]), `None`
/// for any other struct. A marker with a hand-written `impl Unit` / `impl FixedScale` is an error naming it.
///
/// [ItemStructOwned::marker]: ww_self::ItemStructOwned::marker
///
/// What a marker names elsewhere is resolved through `refs`: an `origin!`'s unit, reference, modulus and parent
/// ([origin_def]), a unit line's quantity and a quantity's parent ([MarkerRefs::quantity]).
pub(crate) fn marker_def(
    item_struct: &syn::ItemStruct,
    crate_name: &str,
    refs: &mut dyn MarkerRefs,
) -> Result<Option<MarkerDefOwned>> {
    let crate_name = crate_name.replace('-', "_");
    let ident = item_struct.ident.to_string();
    let id = || MarkerIdOwned {
        crate_name: crate_name.clone(),
        name: ident.clone(),
    };
    if crate_name == EXACT_UNITS
        && let Some((_, def)) = BUILT_IN.iter().find(|(name, _)| *name == ident)
    {
        return Ok(Some(def.clone()));
    }
    if crate_name == EXACT_UNITS && SYSTEMS.contains(&ident.as_str()) {
        return Ok(Some(MarkerDefOwned::System(
            SystemDefOwned::try_from(*builtin_system(&ident)?)
                .map_err(|e| anyhow!("system {ident}: {e:?}"))?,
        )));
    }
    let Some(line) = marker_line(&item_struct.attrs)? else {
        return Ok(None);
    };
    let def = match line {
        MarkerLineDef::Unit(line) => {
            let dim = line
                .dim
                .to_owned(&crate_name, &ident)
                .with_context(|| format!("unit marker {ident}"))?;
            line.dim
                .list_system(refs)
                .with_context(|| format!("the system of unit marker {ident}"))?;
            let scale = match &line.scale {
                UnitScaleLine::Coherent => Magnitude::ONE,
                UnitScaleLine::Pow10(e) => magnitude(&MagnitudeLine {
                    num: 1,
                    den: 1,
                    pow10: *e,
                    pow2: 0,
                    pi: 0,
                    ln2: 0,
                    ln10: 0,
                    mu0_x2: 0,
                    g_x2: 0,
                    m_u_x2: 0,
                })
                .with_context(|| format!("unit marker {ident}"))?,
                UnitScaleLine::Exact(m) => {
                    magnitude(m).with_context(|| format!("unit marker {ident}"))?
                }
            };
            let names = line.names.to_owned(&line.symbol, &line.dim, &line.scale);
            let quantity = match &line.quantity {
                Some(q) => {
                    let q = refs
                        .quantity(q)
                        .with_context(|| format!("the quantity of unit marker {ident}"))?;
                    if !q.dim.as_dim().same(dim.as_dim()) {
                        bail!(
                            "unit marker {ident} of dimension {} measures {}, of dimension {}",
                            dim.as_dim(),
                            q.id.name,
                            q.dim.as_dim()
                        );
                    }
                    Some(q.id)
                }
                None => None,
            };
            let def = UnitDefOwned {
                id: id(),
                names,
                dim,
                quantity,
                scale,
                provenance: match line.provenance {
                    Some((source, year)) => ProvenanceOwned::Conventional { source, year },
                    None => ProvenanceOwned::Defined,
                },
            };
            if line.dim == DimLine::Base(None) {
                MarkerDefOwned::Base(def)
            } else {
                MarkerDefOwned::Unit(def)
            }
        }
        MarkerLineDef::Scale(line) => {
            let m = match &line.factor {
                ScaleFactorLine::Pow10(e) => MagnitudeLine {
                    num: 1,
                    den: 1,
                    pow10: *e,
                    pow2: 0,
                    pi: 0,
                    ln2: 0,
                    ln10: 0,
                    mu0_x2: 0,
                    g_x2: 0,
                    m_u_x2: 0,
                },
                ScaleFactorLine::Pow2(e) => MagnitudeLine {
                    num: 1,
                    den: 1,
                    pow10: 0,
                    pow2: *e,
                    pi: 0,
                    ln2: 0,
                    ln10: 0,
                    mu0_x2: 0,
                    g_x2: 0,
                    m_u_x2: 0,
                },
                ScaleFactorLine::Exact(m) => m.clone(),
            };
            MarkerDefOwned::Scale(ScaleDefOwned {
                id: id(),
                symbol: line.symbol,
                magnitude: magnitude(&m).with_context(|| format!("scale marker {ident}"))?,
            })
        }
        MarkerLineDef::Origin(fields) => MarkerDefOwned::Origin(OriginDefOwned {
            id: id(),
            ..refs
                .origin(&fields)
                .with_context(|| format!("origin marker {ident}"))?
        }),
        MarkerLineDef::Quantity(line) => {
            let (parent, dim, is_kind, character) = match line {
                QuantityLine::Root {
                    is_kind,
                    dim,
                    character,
                } => {
                    dim.list_system(refs)
                        .with_context(|| format!("the system of quantity marker {ident}"))?;
                    let dim = dim
                        .to_owned(&crate_name, &ident)
                        .with_context(|| format!("quantity marker {ident}"))?;
                    (None, dim, is_kind, character)
                }
                QuantityLine::Child {
                    is_kind,
                    parent,
                    character,
                } => {
                    let parent = refs
                        .quantity(&parent)
                        .with_context(|| format!("the parent of quantity marker {ident}"))?;
                    (Some(parent.id), parent.dim, is_kind, character)
                }
            };
            MarkerDefOwned::Quantity(QuantityDefOwned {
                id: id(),
                parent,
                dim,
                is_kind,
                character,
            })
        }
        MarkerLineDef::Reference(line) => {
            let (name, how) = *line;
            let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(name),
                ..
            }) = name
            else {
                bail!(
                    "reference marker {ident}: its `impl Reference` names it with a string literal, \
                     `const NAME: &'static str = \"..\"`"
                );
            };
            MarkerDefOwned::Reference(ReferenceDefOwned {
                id: id(),
                name: name.value(),
                how: reference_how(&how).with_context(|| format!("reference marker {ident}"))?,
            })
        }
        MarkerLineDef::Codec(def) => MarkerDefOwned::Codec(CodecDefOwned {
            id: id(),
            shape: codec_shape(&def).with_context(|| format!("codec marker {ident}"))?,
            spare: codec_spare(&def).with_context(|| format!("codec marker {ident}"))?,
            codes: codec_codes(&def).with_context(|| format!("codec marker {ident}"))?,
            nominal: codec_nominal(&def).with_context(|| format!("codec marker {ident}"))?,
            curve: codec_curve(&def).with_context(|| format!("codec marker {ident}"))?,
        }),
        // the struct alone, a generic definition: no marker of its own, an instance is refused in `convert_user_ty`
        MarkerLineDef::GenericCodec => return Ok(None),
        MarkerLineDef::Hand(trait_name) => bail!(
            "marker {ident} of crate {crate_name} has a hand-written `impl {trait_name}`, which clients can't know: \
             declare it with `exact_units::{}!` instead",
            match trait_name.as_str() {
                "Unit" => "unit",
                "Origin" => "origin",
                _ => "scale",
            }
        ),
    };
    Ok(Some(def))
}

/// The exact magnitude a `num / den, pow10 a, pow2 b, pi c, ln2 d, ln10 e, mu0 f, g h` line means.
fn magnitude(m: &MagnitudeLine) -> Result<Magnitude> {
    let x2 = |e: i8| {
        e.checked_mul(2)
            .ok_or_else(|| anyhow!("exponent {e} out of range"))
    };
    let powers = Magnitude::from_parts_x2(
        x2(m.pow10)?,
        x2(m.pow2)?,
        Symbols {
            pi: x2(m.pi)?,
            ln2: x2(m.ln2)?,
            ln10: x2(m.ln10)?,
            mu0: m.mu0_x2,
            g: m.g_x2,
            m_u: m.m_u_x2,
        },
        [],
    )
    .map_err(|e| anyhow!("{e:?}"))?;
    Magnitude::checked_ratio(m.num, m.den)
        .and_then(|r| r.checked_mul(powers))
        .map_err(|e| anyhow!("{} / {} · {powers:?}: {e:?}", m.num, m.den))
}

/// `"<symbol>" [(<names>)], <dimension>[ quantity <Quantity>][, <scale>]` of a `unit!` line.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct UnitLine {
    pub(crate) symbol: String,
    /// The names in parentheses after the symbol (exact_units IO-9), all left out when there are none.
    pub(crate) names: NamesLine,
    pub(crate) dim: DimLine,
    /// `quantity crate::quantities::Resistance` after the dimension: the quantity the unit measures (UNIT-8), a
    /// path to a quantity marker.
    pub(crate) quantity: Option<syn::Type>,
    pub(crate) scale: UnitScaleLine,
    /// `conventional("<source>", <year>)` after the scale: a value a source prints, not a definition.
    pub(crate) provenance: Option<(String, u16)>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DimLine {
    /// `[s 1, m -2, rad 1, kg 1/2]`: exponents by base name, in halves (`s`, `m`, `kg`, `a`, `k`, `mol`, `cd`, and
    /// `rad`, `bit`, the reserved spellings of exact_units' distinguished bases `Radian` and `Bit`). With a system
    /// before a colon (`[Gaussian: g 1/2, cm -1/2, s -1]`, exact_units UNIT-9), the true bases are that system's, by
    /// their symbols.
    Exps {
        /// The system marker named before the colon, SI's syntax without one.
        system: Option<String>,
        exps: Vec<(String, Exp)>,
    },
    /// `base` or `base(Other)`: a distinguished base of its own, named by the crate and the marker, or, with
    /// `(Other)`, the base of another marker of this crate (`Decibel`'s `base(Neper)` is a scaled unit of `Neper`'s
    /// base, so a `Decibel` converts into a `Neper` by scale alone).
    Base(Option<String>),
}

impl DimLine {
    /// Lists the system the line names before its colon in the bundle through `refs`, when it isn't SI: the marker
    /// the name resolves to in the crate must be exact_units' system of that name.
    fn list_system(&self, refs: &mut dyn MarkerRefs) -> Result<()> {
        let DimLine::Exps {
            system: Some(name), ..
        } = self
        else {
            return Ok(());
        };
        let builtin = builtin_system(name)?;
        if builtin.is_si() {
            return Ok(());
        }
        let ident = Ident::new(name, proc_macro2::Span::call_site());
        let def = refs.system(&syn::parse_quote!(#ident))?;
        if def.id.as_id() != builtin.id {
            bail!(
                "{name} names {}::{}, not exact_units' system",
                def.id.crate_name,
                def.id.name
            );
        }
        Ok(())
    }

    /// The dimension of the line, declared in crate `crate_name` by the marker `ident`.
    fn to_owned(&self, crate_name: &str, ident: &str) -> Result<DimOwned> {
        let id = |name: &str| MarkerIdOwned {
            crate_name: crate_name.to_string(),
            name: name.to_string(),
        };
        let wire = match self {
            DimLine::Base(base) => DimWireOwned {
                system: MarkerIdOwned::from(Si::ID),
                exps: vec![Exp::ZERO; MAX_TRUE_BASES],
                bases: vec![BaseExpOwned {
                    base: id(base.as_deref().unwrap_or(ident)),
                    exp: Exp::ONE,
                }],
            },
            DimLine::Exps { system, exps } => {
                let (def, names) = match system {
                    Some(name) => {
                        let def = builtin_system(name)?;
                        (def, def.bases.iter().filter_map(Result::ok).collect())
                    }
                    None => (Si::DEF, TRUE_BASES.to_vec()),
                };
                let mut wire = DimWireOwned {
                    system: MarkerIdOwned::from(def.id),
                    exps: vec![Exp::ZERO; names.len()],
                    bases: Vec::new(),
                };
                for (name, e) in exps {
                    let reserved = |name: &str| MarkerIdOwned {
                        crate_name: EXACT_UNITS.to_string(),
                        name: name.to_string(),
                    };
                    let slot = match name.as_str() {
                        "rad" | "bit" => {
                            let base = reserved(if name == "rad" { "Radian" } else { "Bit" });
                            if wire.bases.iter().any(|b| b.base == base) {
                                bail!("the base {name} twice in a dimension");
                            }
                            wire.bases.push(BaseExpOwned { base, exp: *e });
                            continue;
                        }
                        other => names.iter().position(|b| *b == other).ok_or_else(|| {
                            anyhow!("{other} is no base of a dimension of {}", def.id.name)
                        })?,
                    };
                    let Some(exp) = wire.exps.get_mut(slot) else {
                        bail!("{name} is no base of a dimension");
                    };
                    if !exp.is_zero() {
                        bail!("the base {name} twice in a dimension");
                    }
                    *exp = *e;
                }
                wire.bases.retain(|b| !b.exp.is_zero());
                wire.bases.sort();
                wire
            }
        };
        DimOwned::try_from(wire).map_err(|_| anyhow!("dimension of {ident} isn't canonical"))
    }
}

/// `ascii "..", long ".." [plural ".."], aliases ["..", ..], ucum "..", prefixes ..` in the parentheses after a
/// unit's symbol, each at most once, in any order.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct NamesLine {
    pub(crate) ascii: Option<String>,
    /// The singular and the plural (the singular and an `s` when the line has no `plural`).
    pub(crate) long: Option<(String, String)>,
    pub(crate) aliases: Vec<String>,
    pub(crate) ucum: Option<String>,
    pub(crate) prefixes: Option<PrefixPolicy>,
}

impl NamesLine {
    /// The names a line with `symbol`, `dim` and `scale` stands for, as `unit!` builds them: without `prefixes`, a
    /// coherent unit takes the SI prefixes and a scaled one or a base none, and a compound symbol none as a whole.
    fn to_owned(&self, symbol: &str, dim: &DimLine, scale: &UnitScaleLine) -> UnitNamesOwned {
        let default = match (dim, scale) {
            (DimLine::Base(_), _) => PrefixPolicy::None,
            (DimLine::Exps { .. }, UnitScaleLine::Coherent) => PrefixPolicy::Decimal,
            (DimLine::Exps { .. }, _) => PrefixPolicy::None,
        };
        let base = UnitNames::new(symbol, default);
        let aliases = self.aliases.join("|");
        let names = UnitNames {
            ascii: self.ascii.as_deref().or(base.ascii),
            long: self.long.as_ref().map_or(base.long, |(l, _)| l),
            plural: self.long.as_ref().map_or(base.plural, |(_, p)| p),
            aliases: &aliases,
            ucum: self.ucum.as_deref().or(base.ucum),
            prefixes: self.prefixes.unwrap_or(base.prefixes),
            ..base
        };
        names.into()
    }
}

impl Parse for NamesLine {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut names = NamesLine::default();
        let mut seen = Vec::new();
        while !input.is_empty() {
            let item: Ident = input.parse()?;
            let word = item.to_string();
            if seen.contains(&word) {
                return Err(syn::Error::new(
                    item.span(),
                    format!("{word} twice in a unit's names"),
                ));
            }
            match word.as_str() {
                "ascii" => names.ascii = Some(input.parse::<LitStr>()?.value()),
                "long" => {
                    let long = input.parse::<LitStr>()?.value();
                    let plural = if input.peek(Ident) {
                        let plural: Ident = input.parse()?;
                        if plural != "plural" {
                            return Err(syn::Error::new(plural.span(), "expected `plural \"..\"`"));
                        }
                        input.parse::<LitStr>()?.value()
                    } else {
                        format!("{long}s")
                    };
                    names.long = Some((long, plural));
                }
                "aliases" => {
                    let content;
                    bracketed!(content in input);
                    let list = content
                        .parse_terminated(|i: ParseStream| i.parse::<LitStr>(), Token![,])?;
                    names.aliases = list.iter().map(LitStr::value).collect();
                }
                "ucum" => names.ucum = Some(input.parse::<LitStr>()?.value()),
                "prefixes" => {
                    let policy: Ident = input.parse()?;
                    names.prefixes = Some(match policy.to_string().as_str() {
                        "none" => PrefixPolicy::None,
                        "decimal" => PrefixPolicy::Decimal,
                        "decimal_and_binary" => PrefixPolicy::DecimalAndBinary,
                        "only" => {
                            let content;
                            bracketed!(content in input);
                            let list = content.parse_terminated(Ident::parse, Token![,])?;
                            let mut prefixes = Vec::new();
                            for p in &list {
                                match Prefix::from_name(&p.to_string())
                                    .filter(|p| PrefixSet::bit(*p).is_some())
                                {
                                    Some(prefix) => prefixes.push(prefix),
                                    None => {
                                        return Err(syn::Error::new(
                                            p.span(),
                                            "a named prefix: Milli, Kilo, Kibi, ..",
                                        ));
                                    }
                                }
                            }
                            PrefixPolicy::Only(PrefixSet::of(&prefixes))
                        }
                        _ => {
                            return Err(syn::Error::new(
                                policy.span(),
                                "prefixes none, decimal, decimal_and_binary or only [Milli, ..]",
                            ));
                        }
                    });
                }
                _ => {
                    return Err(syn::Error::new(
                        item.span(),
                        "a unit's names are ascii \"..\", long \"..\" [plural \"..\"], aliases [\"..\", ..], \
                         ucum \"..\", prefixes none | decimal | decimal_and_binary | only [Milli, ..]",
                    ));
                }
            }
            seen.push(word);
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }
        Ok(names)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum UnitScaleLine {
    Coherent,
    Pow10(i8),
    Exact(MagnitudeLine),
}

/// `num / den, pow10 a, pow2 b, pi c, ln2 d, ln10 e, mu0 f, g h, m_u i`: the measured constants μ₀, G and m_u stay
/// symbols (exact_units NUM-10, NUM-32), their exponent whole or a half (`mu0 1 / 2`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MagnitudeLine {
    pub(crate) num: u64,
    pub(crate) den: u64,
    pub(crate) pow10: i8,
    pub(crate) pow2: i8,
    pub(crate) pi: i8,
    pub(crate) ln2: i8,
    pub(crate) ln10: i8,
    /// Twice the exponent of μ₀.
    pub(crate) mu0_x2: i8,
    /// Twice the exponent of G.
    pub(crate) g_x2: i8,
    /// Twice the exponent of m_u, the atomic mass constant.
    pub(crate) m_u_x2: i8,
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

/// The true bases of an SI dimension as a `unit!` line without a system spells them, in the order of
/// [DimOwned::exps].
const TRUE_BASES: [&str; MAX_TRUE_BASES] = ["s", "m", "kg", "a", "k", "mol", "cd"];

/// The systems a `unit!` or `quantity!` line may name before its bracket: exact_units' own, by the marker's name.
// TODO(UNIT-9): a user's system, once it is declared by a line the walker reads (a `system!`)
const SYSTEMS: [&str; 3] = ["Si", "Natural", "Gaussian"];

/// The definition of the built-in system `name` ([SYSTEMS]).
fn builtin_system(name: &str) -> Result<&'static SystemDef<'static>> {
    let def = SYSTEMS
        .contains(&name)
        .then(|| {
            exact_units::systems::builtin(exact_units::MarkerId {
                crate_name: EXACT_UNITS,
                name,
            })
        })
        .flatten();
    def.ok_or_else(|| {
        anyhow!(
            "{name} is not a system the walker knows: a dimension names one of {} before its colon",
            SYSTEMS.join(", ")
        )
    })
}

impl Parse for UnitLine {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let symbol = input.parse::<LitStr>()?.value();
        let names = if input.peek(syn::token::Paren) {
            let content;
            parenthesized!(content in input);
            content.parse()?
        } else {
            NamesLine::default()
        };
        input.parse::<Token![,]>()?;
        let dim = if input.peek(syn::token::Bracket) {
            let content;
            bracketed!(content in input);
            parse_exps(&content)?
        } else {
            let word: Ident = input.parse()?;
            if word != "base" {
                return Err(syn::Error::new(
                    word.span(),
                    "a dimension is `[<base> <exponent>, ..]`, `base` or `base(<Other>)`",
                ));
            }
            let base = if input.peek(syn::token::Paren) {
                let content;
                parenthesized!(content in input);
                Some(content.parse::<Ident>()?.to_string())
            } else {
                None
            };
            DimLine::Base(base)
        };
        let quantity = if input.peek(Ident) {
            let word: Ident = input.parse()?;
            if word != "quantity" {
                return Err(syn::Error::new(
                    word.span(),
                    "after the dimension only `quantity <Quantity>`",
                ));
            }
            Some(type_path(input)?)
        } else {
            None
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
                    return Err(syn::Error::new(
                        kind.span(),
                        "a unit has no offset: a scale with a zero of its own is an origin of points \
                         (`exact_units::origin!`, °C is `exact_units::origins::Celsius`)",
                    ));
                }
                _ => {
                    return Err(syn::Error::new(
                        kind.span(),
                        "a unit's scale is Pow10(e) or Exact(num / den, ..)",
                    ));
                }
            }
        };
        let mut provenance = None;
        if !input.is_empty() {
            input.parse::<Token![,]>()?;
            let kind: Ident = input.parse()?;
            if kind != "conventional" {
                return Err(syn::Error::new(
                    kind.span(),
                    "after the scale only `conventional(\"<source>\", <year>)`",
                ));
            }
            let content;
            syn::parenthesized!(content in input);
            let source = content.parse::<LitStr>()?.value();
            content.parse::<Token![,]>()?;
            let year = content.parse::<syn::LitInt>()?.base10_parse()?;
            provenance = Some((source, year));
        }
        if !input.is_empty() {
            return Err(input.error("unexpected tokens after the scale"));
        }
        Ok(UnitLine {
            symbol,
            names,
            dim,
            quantity,
            scale,
            provenance,
        })
    }
}

/// The exponents in the brackets of a dimension: `s 1, m -2, rad 1, kg 1/2`, or with a system before a colon,
/// `Gaussian: g 1/2, cm -1/2, s -1`.
fn parse_exps(input: ParseStream) -> syn::Result<DimLine> {
    let system = if input.peek(Ident) && input.peek2(Token![:]) && !input.peek2(Token![::]) {
        let ident: Ident = input.parse()?;
        input.parse::<Token![:]>()?;
        builtin_system(&ident.to_string()).map_err(|e| syn::Error::new(ident.span(), e))?;
        Some(ident.to_string())
    } else {
        None
    };
    let names: Vec<&str> = match &system {
        Some(name) => builtin_system(name)
            .map_err(|e| input.error(e))?
            .bases
            .iter()
            .filter_map(Result::ok)
            .collect(),
        None => TRUE_BASES.to_vec(),
    };
    let mut exps = Vec::new();
    while !input.is_empty() {
        let base: Ident = input.parse()?;
        let name = base.to_string();
        if !names.contains(&name.as_str()) && name != "rad" && name != "bit" {
            return Err(syn::Error::new(
                base.span(),
                format!(
                    "a base of a dimension is one of {}, rad or bit",
                    names.join(", ")
                ),
            ));
        }
        if exps.iter().any(|(b, _)| *b == name) {
            return Err(syn::Error::new(base.span(), "a base twice in a dimension"));
        }
        let n = parse_i8(input)?;
        let e = if input.peek(Token![/]) {
            input.parse::<Token![/]>()?;
            let two: LitInt = input.parse()?;
            if two.base10_parse::<u8>()? != 2 {
                return Err(syn::Error::new(
                    two.span(),
                    "an exponent of a dimension is whole or a half: n or n/2",
                ));
            }
            Exp::from_halves(n)
        } else {
            Exp::checked_int(n).ok_or_else(|| {
                syn::Error::new(base.span(), "exponent of a dimension out of -64..=63")
            })?
        };
        exps.push((name, e));
        if !input.is_empty() {
            input.parse::<Token![,]>()?;
        }
    }
    Ok(DimLine::Exps { system, exps })
}

/// A path to a marker, as a type: `crate::quantities::Resistance`, `Length`.
fn type_path(input: ParseStream) -> syn::Result<syn::Type> {
    let path = input.call(syn::Path::parse_mod_style)?;
    Ok(syn::parse_quote!(#path))
}

/// `= [kind] [<exponents>][, <character>]` (a root) or `: [kind] <Parent>[, <character>]` (a child) of a
/// `quantity!` line.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum QuantityLine {
    /// A root, with its dimension.
    Root {
        is_kind: bool,
        dim: DimLine,
        character: Character,
    },
    /// A child of `parent`, of its dimension.
    Child {
        is_kind: bool,
        parent: Box<syn::Type>,
        character: Character,
    },
}

impl Parse for QuantityLine {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let root = input.peek(Token![=]);
        if root {
            input.parse::<Token![=]>()?;
        } else {
            input.parse::<Token![:]>()?;
        }
        let is_kind = input.peek(Ident) && input.fork().parse::<Ident>()? == "kind";
        if is_kind {
            input.parse::<Ident>()?;
        }
        let mut line = match root {
            true if input.peek(syn::token::Bracket) => {
                let content;
                bracketed!(content in input);
                QuantityLine::Root {
                    is_kind,
                    dim: parse_exps(&content)?,
                    character: Character::Scalar,
                }
            }
            true => {
                return Err(input.error("a root quantity's dimension is `[<base> <exponent>, ..]`"));
            }
            false => QuantityLine::Child {
                is_kind,
                parent: Box::new(type_path(input)?),
                character: Character::Scalar,
            },
        };
        if !input.is_empty() {
            input.parse::<Token![,]>()?;
            let word: Ident = input.parse()?;
            let (QuantityLine::Root { character, .. } | QuantityLine::Child { character, .. }) =
                &mut line;
            *character = match word.to_string().as_str() {
                "complex" => Character::Complex,
                "vector" => Character::Vector,
                _ => {
                    return Err(syn::Error::new(
                        word.span(),
                        "a quantity's character is `complex` or `vector` (scalar without)",
                    ));
                }
            };
        }
        if !input.is_empty() {
            return Err(input.error("unexpected tokens after the quantity's character"));
        }
        Ok(line)
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
            ln2: 0,
            ln10: 0,
            mu0_x2: 0,
            g_x2: 0,
            m_u_x2: 0,
        };
        let mut next = 0;
        while input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
            if input.is_empty() {
                break;
            }
            let name: Ident = input.parse()?;
            let order = ["pow10", "pow2", "pi", "ln2", "ln10", "mu0", "g", "m_u"];
            let Some(pos) = order.iter().position(|o| name == o).filter(|p| *p >= next) else {
                return Err(syn::Error::new(
                    name.span(),
                    "after `num / den` come `pow10 e`, `pow2 e`, `pi e`, `ln2 e`, `ln10 e`, `mu0 e`, `g e`, `m_u e`, \
                     each at most once and in this order",
                ));
            };
            next = pos + 1;
            let e = parse_i8(input)?;
            match pos {
                0 => m.pow10 = e,
                1 => m.pow2 = e,
                2 => m.pi = e,
                3 => m.ln2 = e,
                4 => m.ln10 = e,
                5 => m.mu0_x2 = symbol_x2(input, e)?,
                6 => m.g_x2 = symbol_x2(input, e)?,
                _ => m.m_u_x2 = symbol_x2(input, e)?,
            }
        }
        Ok(m)
    }
}

/// Twice the exponent `e` of a measured symbol, or `e` itself after `/ 2`: a half power (`mu0 1 / 2`).
fn symbol_x2(input: ParseStream, e: i8) -> syn::Result<i8> {
    if !input.peek(Token![/]) {
        return e
            .checked_mul(2)
            .ok_or_else(|| input.error("exponent out of range"));
    }
    input.parse::<Token![/]>()?;
    let two: LitInt = input.parse()?;
    if two.base10_parse::<u8>()? != 2 {
        return Err(syn::Error::new(
            two.span(),
            "a symbol's exponent is whole or a half: `/ 2`",
        ));
    }
    Ok(e)
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

#[cfg(test)]
mod tests {
    use super::*;
    use exact_units::{FixedScale, Origin, origins, prefix};
    use shrink_wrap::SerializeShrinkWrapOwned;
    use std::collections::BTreeMap;
    use ww_self::{TypeLocationOwned, TypeOwned};

    /// Every marker of `exact_units` as the walker reads it from source, by name.
    fn exact_units_markers() -> BTreeMap<String, MarkerDefOwned> {
        // the source of the exact_units this crate is built with: it lives in its own repository
        let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let root = crate::dependency_dir(here, "exact_units").unwrap();
        let bundle = crate::transform::snapshot::load_crate(&root).unwrap();
        bundle
            .types
            .iter()
            .filter_map(|t| match t {
                TypeLocationOwned::InLine {
                    ty: TypeOwned::Struct(s),
                    ..
                } => Some((s.ident.clone(), s.marker()?.clone())),
                _ => None,
            })
            .collect()
    }

    fn dim_bytes(dim: &DimOwned) -> Vec<u8> {
        dim.to_ww_bytes_owned().unwrap()
    }

    /// The walked definition of a unit against the compiled one, field by field, so a mismatch names its field.
    fn assert_unit_def(walked: &UnitDefOwned, compiled: &exact_units::UnitDef<'_>, name: &str) {
        let compiled = UnitDefOwned::from(*compiled);
        assert_eq!(walked.id, compiled.id, "{name}: id");
        assert_eq!(walked.names, compiled.names, "{name}: names");
        assert_eq!(
            dim_bytes(&walked.dim),
            dim_bytes(&compiled.dim),
            "{name}: dim {:?} vs {:?}",
            walked.dim,
            compiled.dim
        );
        assert_eq!(walked.quantity, compiled.quantity, "{name}: quantity");
        assert_eq!(walked.scale, compiled.scale, "{name}: scale");
        assert_eq!(walked.provenance, compiled.provenance, "{name}: provenance");
        assert_eq!(*walked, compiled, "{name}");
    }

    /// INT-13, UNIT-7: the walker's reading of each `unit!` / `scale!` / `origin!` line is the definition rustc
    /// compiled from it (`UnitDef`, `FixedScale::ID` and `SCALE`, `Origin::DEF`), so the grammar here and the macros
    /// can't drift apart.
    #[test]
    fn exact_units_definitions_match_the_compiled_consts() {
        let mut markers = exact_units_markers();
        let walked_bases: std::collections::BTreeSet<String> = markers
            .iter()
            .filter(|(_, def)| matches!(def, MarkerDefOwned::Base(_)))
            .map(|(name, _)| name.clone())
            .collect();
        // UNIT-9: the units of Natural and Gaussian, their dimensions in their own systems
        for expr in exact_units::units::ALL
            .iter()
            .chain(exact_units::units::CUSTOMARY)
            .chain(exact_units::systems::natural::ALL)
            .chain(exact_units::systems::gaussian::ALL)
        {
            let Some(compiled) = expr.def() else {
                panic!("units::ALL holds named units")
            };
            let name = compiled.id.name;
            assert_eq!(compiled.id.crate_name, EXACT_UNITS, "{name}");
            let marker = markers.remove(name);
            let Some(def) = marker.as_ref().and_then(MarkerDefOwned::unit_def) else {
                panic!("{name}: no unit definition in the model");
            };
            assert_unit_def(def, compiled, name);
        }
        macro_rules! scales {
            ($($name:ident)*) => {
                [$((stringify!($name), <prefix::$name as FixedScale>::DEF)),*]
            };
        }
        for (name, compiled) in scales!(
            Quecto Ronto Yocto Zepto Atto Femto Pico Nano Micro Milli Centi Deci One Deca Hecto Kilo Mega Giga Tera
            Peta Exa Zetta Yotta Ronna Quetta Kibi Mebi Gibi Tebi
        )
        .into_iter()
        .chain([("TwoPi", <origins::TwoPi as FixedScale>::DEF)])
        {
            let Some(MarkerDefOwned::Scale(def)) = markers.remove(name) else {
                panic!("{name}: no scale definition in the model");
            };
            assert_eq!(
                (
                    Some(def.id.as_id()),
                    def.symbol.as_str(),
                    def.magnitude
                ),
                (compiled.id, compiled.symbol, compiled.scale),
                "{name}"
            );
        }
        // `q!`'s dimensionless unit (`q!(5 s^-1)` is `Per<Unitless, Second>`), in no catalogue and behind exact_units'
        // feature `q`: its line is `unit! { pub struct Unitless = "", []; }`
        let unitless = markers.remove("Unitless");
        let Some(def) = unitless.as_ref().and_then(MarkerDefOwned::unit_def) else {
            panic!("Unitless: no unit definition in the model");
        };
        assert_eq!(
            def.id.as_id(),
            exact_units::MarkerId {
                crate_name: EXACT_UNITS,
                name: "Unitless"
            }
        );
        assert_eq!(def.names.symbol, "", "Unitless");
        assert_eq!(def.dim, DimOwned::from(exact_units::Dim::ONE), "Unitless");
        assert_eq!(def.scale, Magnitude::ONE, "Unitless");
        macro_rules! origins {
            ($($name:ident)*) => {
                [$((stringify!($name), <origins::$name as Origin>::DEF)),*]
            };
        }
        for (name, compiled) in origins!(
            AbsoluteZero Celsius Rankine Fahrenheit Vacuum Gauge AboveStandardAtmosphere UnixEpoch NtpEpoch TaiEpoch1958 GpsEpoch PtpEpoch
            DeviceBoot
        ) {
            let Some(MarkerDefOwned::Origin(def)) = markers.remove(name) else {
                panic!("{name}: no origin definition in the model");
            };
            assert_eq!(def.id.as_id(), compiled.id, "{name}");
            let Some(unit) = compiled.unit.def() else {
                panic!("{name}: a named unit");
            };
            assert_unit_def(&def.unit, unit, name);
            assert_eq!(def.symbol, compiled.symbol, "{name}");
            assert_eq!(
                def.reference,
                ww_self::ReferenceDefOwned::from(&compiled.reference),
                "{name}"
            );
            assert_eq!(
                (def.offset, def.modulus, def.step, def.shown),
                (
                    compiled.offset,
                    compiled.modulus,
                    compiled.step,
                    compiled.shown
                ),
                "{name}"
            );
        }
        // UNIT-8: each `quantity!` line, its parent by id
        for compiled in exact_units::quantities::ALL
            .iter()
            .chain(exact_units::systems::gaussian::quantities::ALL)
        {
            let name = compiled.id.name;
            let Some(MarkerDefOwned::Quantity(def)) = markers.remove(name) else {
                panic!("{name}: no quantity definition in the model");
            };
            let compiled = QuantityDefOwned::from(**compiled);
            assert_eq!(def.parent, compiled.parent, "{name}: parent");
            assert_eq!(dim_bytes(&def.dim), dim_bytes(&compiled.dim), "{name}: dim");
            assert_eq!(def, compiled, "{name}");
        }
        for (name, def) in BUILT_IN.iter().cloned() {
            assert_eq!(markers.remove(name), Some(def), "{name}");
        }
        // UNIT-9: each system marker carries its definition whole
        for compiled in [
            exact_units::systems::Si::DEF,
            exact_units::systems::Natural::DEF,
            exact_units::systems::Gaussian::DEF,
        ] {
            let name = compiled.id.name;
            let Some(MarkerDefOwned::System(def)) = markers.remove(name) else {
                panic!("{name}: no system definition in the model");
            };
            assert_eq!(def, SystemDefOwned::try_from(*compiled).unwrap(), "{name}");
        }
        // PT-4, STD-108: each `impl Reference`, its NAME and HOW as compiled
        for compiled in [
            <exact_units::UnitZero as exact_units::Reference>::DEF,
            <exact_units::AmbientPressure as exact_units::Reference>::DEF,
            <origins::TaiScale as exact_units::Reference>::DEF,
            <origins::PosixScale as exact_units::Reference>::DEF,
            <origins::DeviceMonotonic as exact_units::Reference>::DEF,
        ] {
            let name = compiled.id.name;
            let Some(MarkerDefOwned::Reference(def)) = markers.remove(name) else {
                panic!("{name}: no reference definition in the model");
            };
            assert_eq!(def, ReferenceDefOwned::from(&compiled), "{name}");
        }
        // LVL-6: each `impl Codec`, its DEF as compiled, spare codes included
        for compiled in [
            <exact_units::codec::MuLaw as exact_units::codec::Codec>::DEF,
            <exact_units::codec::MuLawSingleZero as exact_units::codec::Codec>::DEF,
            // the shapes with parameters, their tables written out in the codec's module
            <exact_units::codec::CanFdDlc as exact_units::codec::Codec>::DEF,
            <exact_units::codec::J2kStepSize as exact_units::codec::Codec>::DEF,
            <exact_units::codec::HevcLevelScale as exact_units::codec::Codec>::DEF,
            <exact_units::codec::HrdBitRate as exact_units::codec::Codec>::DEF,
            <exact_units::codec::HrdCpbSize as exact_units::codec::Codec>::DEF,
            <exact_units::codec::video::Narrow8 as exact_units::codec::Codec>::DEF,
            <exact_units::codec::video::NarrowChroma8 as exact_units::codec::Codec>::DEF,
            <exact_units::codec::video::Narrow10 as exact_units::codec::Codec>::DEF,
            <exact_units::codec::video::NarrowChroma10 as exact_units::codec::Codec>::DEF,
            <exact_units::codec::video::Narrow12 as exact_units::codec::Codec>::DEF,
            <exact_units::codec::video::NarrowChroma12 as exact_units::codec::Codec>::DEF,
            <exact_units::codec::video::Full10 as exact_units::codec::Codec>::DEF,
            <exact_units::codec::video::FullChroma10 as exact_units::codec::Codec>::DEF,
            <exact_units::codec::video::Full12 as exact_units::codec::Codec>::DEF,
            <exact_units::codec::video::FullChroma12 as exact_units::codec::Codec>::DEF,
            <exact_units::exposure::ApertureLabel as exact_units::codec::Codec>::DEF,
            <exact_units::exposure::ShutterLabel as exact_units::codec::Codec>::DEF,
        ] {
            let name = compiled.id.name;
            let Some(MarkerDefOwned::Codec(def)) = markers.remove(name) else {
                panic!("{name}: no codec definition in the model");
            };
            // the compiled definition through its own bytes, read back as the model's owned form
            let mut buf = [0u8; 1024];
            let bytes = shrink_wrap::SerializeShrinkWrap::to_ww_bytes(&compiled, &mut buf).unwrap();
            let compiled =
                <CodecDefOwned as shrink_wrap::DeserializeShrinkWrapOwned>::from_ww_bytes_owned(
                    bytes,
                )
                .unwrap();
            assert_eq!(def, compiled, "{name}");
        }
        // UNIT-10: exactly the `base` lines are `Base`, each declaring the base its id names; `base(Other)` is none
        macro_rules! bases {
            ($($name:ident)*) => {
                [$((stringify!($name), <exact_units::units::$name as exact_units::Unit>::BASE)),*]
            };
        }
        let mut walked_bases: Vec<_> = walked_bases.into_iter().collect();
        walked_bases.sort();
        let mut compiled_bases = bases!(
            Radian Neper NeperLoss Bit Item Sample Frame Packet Line Row Record Entry Cycle Block Sector Chunk Char Code
            Argument Boot Buffer Channel Event Fifo Filter Operation Pin Pulse Scan Segment Session Step Tick TimeQuantum
            Month Interface Retry Request
        );
        compiled_bases.sort_by_key(|(name, _)| *name);
        assert_eq!(
            walked_bases,
            compiled_bases
                .iter()
                .map(|(n, _)| n.to_string())
                .collect::<Vec<_>>()
        );
        for (name, base) in compiled_bases {
            assert_eq!(
                base.map(|b| b.id),
                Some(exact_units::MarkerId {
                    crate_name: EXACT_UNITS,
                    name
                })
            );
        }
        for (name, base) in bases!(Decibel Bel DecibelLoss Absorbance OpticalDepth Volt) {
            assert_eq!(base, None, "{name}");
        }
        // a marker added to exact_units is checked here too
        assert!(markers.is_empty(), "not checked: {markers:?}");
    }

    fn expand(src: &str) -> Vec<Item> {
        let mut file = syn::parse_file(src).unwrap();
        expand_marker_macros(&mut file.items).unwrap();
        file.items
    }

    /// Resolves nothing: the lines tested name no origin parts and no quantities.
    struct NoRefs;

    impl MarkerRefs for NoRefs {
        fn origin(&mut self, _: &OriginFields) -> Result<OriginDefOwned> {
            bail!("no origins here")
        }

        fn quantity(&mut self, _: &syn::Type) -> Result<QuantityDefOwned> {
            bail!("no quantities here")
        }

        fn system(&mut self, _: &syn::Type) -> Result<SystemDefOwned> {
            bail!("no systems here")
        }
    }

    #[test]
    fn origin_fields_and_values() {
        let fields: OriginFields = syn::parse_str(
            "unit: Kelvin, symbol: \"°F\", offset: Offset::ratio(45967, 180), step: Magnitude::ratio(5, 9)",
        )
        .unwrap();
        let OriginFields::Unit {
            symbol,
            offset,
            step: Some(step),
            reference: None,
            modulus: None,
            ..
        } = fields
        else {
            panic!()
        };
        assert_eq!(symbol, "°F");
        assert_eq!(offset_value(&offset).unwrap(), Offset::ratio(45967, 180));
        assert_eq!(step_value(&step).unwrap(), Magnitude::ratio(5, 9));
        let e = |s: &str| syn::parse_str::<syn::Expr>(s).unwrap();
        for (expr, value) in [
            ("Offset::ZERO", Offset::ZERO),
            ("exact_units::Offset::int(-40)", Offset::int(-40)),
            (
                "$crate::Offset::decimal(-27315, -2)",
                Offset::decimal(-27315, -2),
            ),
            ("Offset::decimal(0, 3)", Offset::ZERO),
        ] {
            let expr = expr.replace("$crate::", "crate::");
            assert_eq!(offset_value(&e(&expr)).unwrap(), value, "{expr}");
        }
        assert_eq!(
            step_value(&e("Magnitude::pow10(-3)")).unwrap(),
            Magnitude::pow10(-3)
        );
        assert_eq!(step_value(&e("Magnitude::ONE")).unwrap(), Magnitude::ONE);
        let err = offset_value(&e("Offset::int(25).add(Offset::ZERO)")).unwrap_err();
        assert!(err.to_string().contains("is not `Offset::..`"), "{err}");
        let err = offset_value(&e("Offset::int(N)")).unwrap_err();
        assert!(err.to_string().contains("not an integer literal"), "{err}");
        let err = step_value(&e("Magnitude::ratio(-1, 9)")).unwrap_err();
        assert!(err.to_string().contains("positive"), "{err}");
        let above: OriginFields =
            syn::parse_str("above: Celsius, offset: Offset::int(25),").unwrap();
        assert!(matches!(above, OriginFields::Above { .. }));
        let wrong = syn::parse_str::<OriginFields>("unit: Kelvin, offset: Offset::ZERO");
        assert!(wrong.is_err());
        let wrong = syn::parse_str::<OriginFields>(
            "unit: Kelvin, symbol: \"K\", offset: Offset::ZERO, modulus: Linear, step: Magnitude::ONE",
        );
        assert!(wrong.is_err());
    }

    #[test]
    fn origin_becomes_its_unit_struct() {
        let items = expand(
            r#"
            crate::origin! {
                /// Degrees Celsius.
                pub struct Celsius { unit: Kelvin, symbol: "°C", offset: Offset::decimal(27315, -2) }
            }
            mod inner {
                origin! { pub(crate) struct Lsm { above: Celsius, offset: Offset::int(25) } }
            }
            "#,
        );
        let Item::Struct(celsius) = &items[0] else {
            panic!()
        };
        assert_eq!(celsius.ident, "Celsius");
        assert!(matches!(celsius.fields, syn::Fields::Unit));
        assert!(celsius.attrs.iter().any(|a| a.path().is_ident("doc")));
        let Some(MarkerLineDef::Origin(fields)) = marker_line(&celsius.attrs).unwrap() else {
            panic!()
        };
        let OriginFields::Unit { symbol, offset, .. } = *fields else {
            panic!()
        };
        assert_eq!(symbol, "°C");
        assert_eq!(offset_value(&offset).unwrap(), Offset::decimal(27315, -2));
        let Item::Mod(inner) = &items[1] else {
            panic!()
        };
        assert!(
            matches!(&inner.content.as_ref().unwrap().1[0], Item::Struct(s) if s.ident == "Lsm")
        );
    }

    #[test]
    fn unit_lines_become_structs_with_their_definition() {
        let items = expand(
            r#"
            exact_units::unit! {
                /// Revolutions per minute.
                pub struct Rpm = "rpm", [rad 1, s -1], Exact(1 / 30, pi 1);
                #[cfg(feature = "x")]
                pub struct Bucket = "bucket", base;
                pub struct InHg = "inHg", [kg 1, m -1, s -2], Exact(338638 / 100), conventional("NIST SP 811", 2008);
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
            *line,
            UnitLine {
                symbol: "rpm".into(),
                names: NamesLine::default(),
                dim: DimLine::Exps {
                    system: None,
                    exps: vec![("rad".into(), Exp::ONE), ("s".into(), Exp::int(-1))]
                },
                quantity: None,
                scale: UnitScaleLine::Exact(MagnitudeLine {
                    num: 1,
                    den: 30,
                    pow10: 0,
                    pow2: 0,
                    pi: 1,
                    ln2: 0,
                    ln10: 0,
                    mu0_x2: 0,
                    g_x2: 0,
                    m_u_x2: 0,
                }),
                provenance: None,
            }
        );
        let Item::Struct(bucket) = &items[1] else {
            panic!()
        };
        assert!(bucket.attrs.iter().any(|a| a.path().is_ident("cfg")));
        let Some(MarkerLineDef::Unit(line)) = marker_line(&bucket.attrs).unwrap() else {
            panic!()
        };
        assert_eq!(line.dim, DimLine::Base(None));
        let Item::Struct(in_hg) = &items[2] else {
            panic!()
        };
        let Some(MarkerLineDef::Unit(line)) = marker_line(&in_hg.attrs).unwrap() else {
            panic!()
        };
        assert_eq!(line.provenance, Some(("NIST SP 811".into(), 2008)));
    }

    /// UNIT-10 and IO-9: a line's names, half exponents, the reserved `rad` and `bit`, `base` and `base(Other)`
    /// become the definition `unit!` compiles from the same line.
    #[test]
    fn names_halves_and_bases() {
        mod compiled {
            exact_units::unit! {
                pub struct NoiseDensity = "V/√Hz" (long "volt per root hertz" plural "volts per root hertz",
                    aliases ["V/rtHz"], ascii "V/sqrtHz"), [kg 1, m 2, s -5/2, a -1];
                pub struct Pixel = "px" (long "pixel", prefixes only [Kilo, Mega]), base;
                pub struct HalfPixel = "hpx", base(Pixel), Exact(1 / 2);
                pub struct ByteRate = "B/s" (ucum "By/s"), [bit 1, s -1, rad 1], Exact(8 / 1);
            }
        }
        use exact_units::Unit;
        let items = expand(
            r#"
            exact_units::unit! {
                pub struct NoiseDensity = "V/√Hz" (long "volt per root hertz" plural "volts per root hertz",
                    aliases ["V/rtHz"], ascii "V/sqrtHz"), [kg 1, m 2, s -5/2, a -1];
                pub struct Pixel = "px" (long "pixel", prefixes only [Kilo, Mega]), base;
                pub struct HalfPixel = "hpx", base(Pixel), Exact(1 / 2);
                pub struct ByteRate = "B/s" (ucum "By/s"), [bit 1, s -1, rad 1], Exact(8 / 1);
            }
            "#,
        );
        let walked: Vec<_> = items
            .iter()
            .map(|item| {
                let Item::Struct(s) = item else { panic!() };
                marker_def(s, "my_api", &mut NoRefs).unwrap().unwrap()
            })
            .collect();
        let compiled_crate = env!("CARGO_CRATE_NAME");
        let with_crate = |def: &UnitDefOwned| {
            let mut def = def.clone();
            def.id.crate_name = compiled_crate.into();
            let mut wire = DimWireOwned::from(def.dim.clone());
            for b in &mut wire.bases {
                if b.base.crate_name == "my_api" {
                    b.base.crate_name = compiled_crate.into();
                }
            }
            def.dim = DimOwned::try_from(wire).unwrap();
            def
        };
        let defs = [
            compiled::NoiseDensity::EXPR,
            compiled::Pixel::EXPR,
            compiled::HalfPixel::EXPR,
            compiled::ByteRate::EXPR,
        ];
        for (walked, compiled) in walked.iter().zip(defs) {
            let def = walked.unit_def().unwrap();
            let compiled = compiled.def().unwrap();
            assert_unit_def(&with_crate(def), compiled, compiled.id.name);
        }
        assert!(matches!(walked[1], MarkerDefOwned::Base(_)));
        assert_eq!(compiled::Pixel::BASE.map(|b| b.id.name), Some("Pixel"));
        for (i, walked) in walked.iter().enumerate().filter(|(i, _)| *i != 1) {
            assert!(matches!(walked, MarkerDefOwned::Unit(_)), "{i}");
        }
        assert_eq!(compiled::HalfPixel::BASE, None);
        assert_eq!(
            walked[0].unit_def().unwrap().dim.to_string(),
            "kg·m²·s⁻⁵ᐟ²·A⁻¹"
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

    /// A marker whose definition only rustc knows is an error when used, never a struct without a unit.
    /// STD-108: an `impl Reference` beside a struct makes it a reference marker, its NAME and HOW read as written.
    #[test]
    fn a_reference_impl_is_read() {
        let items = expand(
            r#"
            pub struct IndexPulse;
            impl exact_units::Reference for IndexPulse {
                const ID: MarkerId<'static> = MarkerId { crate_name: "my_api", name: "IndexPulse" };
                const NAME: &'static str = "index pulse";
                const HOW: ReferenceHow = ReferenceHow::RunTime;
            }
            pub struct BoardClock;
            impl Reference for BoardClock {
                const ID: MarkerId<'static> = MarkerId { crate_name: "my_api", name: "BoardClock" };
                const NAME: &'static str = "board clock";
                const HOW: ReferenceHow = exact_units::ReferenceHow::TimeScale(TimeScaleDef {
                    counting: TimeCounting::EverySecond,
                    zero: TimeZero::RunTime,
                });
            }
            pub struct Named;
            impl Reference for Named {
                const NAME: &'static str = NAME;
                const HOW: ReferenceHow = ReferenceHow::Exact;
            }
            "#,
        );
        let structs: Vec<_> = items
            .iter()
            .filter_map(|i| match i {
                Item::Struct(s) => Some(s),
                _ => None,
            })
            .collect();
        let def = |i: usize| marker_def(structs[i], "my-api", &mut NoRefs);
        assert_eq!(
            def(0).unwrap(),
            Some(MarkerDefOwned::Reference(ReferenceDefOwned {
                id: MarkerIdOwned {
                    crate_name: "my_api".into(),
                    name: "IndexPulse".into()
                },
                name: "index pulse".into(),
                how: ReferenceHow::RunTime,
            }))
        );
        let Some(MarkerDefOwned::Reference(clock)) = def(1).unwrap() else {
            panic!("a reference");
        };
        assert_eq!(
            clock.how,
            ReferenceHow::TimeScale(TimeScaleDef {
                counting: TimeCounting::EverySecond,
                zero: TimeZero::RunTime
            })
        );
        let err = def(2).unwrap_err().to_string();
        assert!(
            err.contains("Named") && err.contains("string literal"),
            "{err}"
        );
    }

    #[test]
    fn a_hand_written_impl_is_an_error_naming_the_marker() {
        let items = expand(
            r#"
            pub struct Rpm;
            impl exact_units::Unit for Rpm {
                const DIM: Dim<'static> = Dim::ONE;
                const EXPR: &'static UnitExpr = &UnitExpr::Pow(..);
            }
            pub struct Lsb;
            impl FixedScale for Lsb {
                const SCALE: Magnitude = Magnitude::ONE;
            }
            /// A trait of its own named Unit, without DIM: not a marker
            pub struct Other;
            impl Unit for Other {
                fn name() -> &'static str { "other" }
            }
            "#,
        );
        let structs: Vec<_> = items
            .iter()
            .filter_map(|i| match i {
                Item::Struct(s) => Some(s),
                _ => None,
            })
            .collect();
        let err = marker_def(structs[0], "my-api", &mut NoRefs)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("Rpm") && err.contains("my_api") && err.contains("unit!"),
            "{err}"
        );
        let err = marker_def(structs[1], "my_api", &mut NoRefs)
            .unwrap_err()
            .to_string();
        assert!(err.contains("Lsb") && err.contains("scale!"), "{err}");
        assert!(
            marker_def(structs[2], "my_api", &mut NoRefs)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn a_wrong_line_is_an_error() {
        for src in [
            r#"unit! { pub struct A = "a", [q 1]; }"#,
            r#"unit! { pub struct A = "a", [s 64]; }"#,
            r#"unit! { pub struct A = "a", [s 1/3]; }"#,
            r#"unit! { pub struct A = "a", [s 1, s 2]; }"#,
            r#"unit! { pub struct A = "a", [rad 1, rad 1]; }"#,
            r#"unit! { pub struct A = "a", counted; }"#,
            r#"unit! { pub struct A = "a" (lng "a"), [s 1]; }"#,
            r#"unit! { pub struct A = "a" (ucum "a", ucum "b"), [s 1]; }"#,
            r#"unit! { pub struct A = "a" (prefixes only [Pow10]), [s 1]; }"#,
            r#"unit! { pub struct A = "a", [s 1], Exact(1 / 2, pi 1, pow10 2); }"#,
            r#"unit! { pub struct A = "a", [s 1], Pow2(1); }"#,
            r#"scale! { pub struct A = "a", Factor(2.0); }"#,
            r#"unit! { pub struct A = "a", [s 1], Factor(2.0); }"#,
            r#"unit! { pub struct A = "a", [s 1] knd "x"; }"#,
            r#"unit! { pub struct A = "a", [s 1] kind "x"; }"#,
            r#"unit! { pub struct A = "a", [s 1] quantity "x"; }"#,
            r#"quantity! { pub struct A = [q 1]; }"#,
            r#"quantity! { pub struct A = kind; }"#,
            r#"quantity! { pub struct A = Length; }"#,
            r#"quantity! { pub struct A: [m 1]; }"#,
            r#"quantity! { pub struct A: Length, scalar; }"#,
            r#"quantity! { pub struct A = [m 1], complex vector; }"#,
            r#"quantity! { pub struct A; }"#,
            r#"unit! { pub struct A = "a", [s 1] }"#,
            r#"unit! { pub struct DegC = "°C", [k 1], Affine { factor: 1.0, offset: 273.15 }; }"#,
        ] {
            let mut file = syn::parse_file(src).unwrap();
            assert!(expand_marker_macros(&mut file.items).is_err(), "{src}");
        }
    }

    /// A codec's shape with parameters, as exact_units writes them: a `Table` of a same-module `const` array, an
    /// `Offset`, a `MantissaExponent` with `<<` and negative numbers. A table computed by `const` code stays a name,
    /// refused with what to do; so is a number the walker would have to evaluate.
    #[test]
    fn codec_shapes_with_parameters() {
        let def_of = |src: &str| -> syn::Expr {
            let items = expand(src);
            let Some(Item::Struct(s)) = items.iter().find(|i| matches!(i, Item::Struct(_))) else {
                panic!("no struct");
            };
            match marker_line(&s.attrs).unwrap() {
                Some(MarkerLineDef::Codec(def)) => *def,
                _ => panic!("no codec marker"),
            }
        };
        let codec = |table: &str, shape: &str| {
            format!(
                "pub struct C;\n{table}\nimpl Codec for C {{ const DEF: CodecDef<'static> = CodecDef {{ \
                 id: MarkerId {{ crate_name: \"c\", name: \"C\" }}, shape: {shape}, \
                 spare: def_list(&SPARE), codes: CodeSpan::all::<u8>(), nominal: None, curve: None }}; }}\n\
                 const SPARE: [CodeSpan; 2] = [CodeSpan::one(0x1_FFFF), CodeSpan {{ low: 16 << 32, high: (1 << 40) - 1 }}];"
            )
        };
        let def = def_of(&codec(
            "const TABLE: [i128; 4] = [0, -6, 12, 64];",
            "CodecShape::Table(def_list(&TABLE))",
        ));
        assert_eq!(
            codec_shape(&def).unwrap(),
            CodecShapeOwned::Table(vec![0, -6, 12, 64])
        );
        assert_eq!(
            codec_spare(&def).unwrap(),
            vec![
                CodeSpan::one(0x1_FFFF),
                CodeSpan {
                    low: 16 << 32,
                    high: (1 << 40) - 1
                }
            ]
        );
        let def = def_of(&codec("", "CodecShape::Offset { zero: 2048 }"));
        assert_eq!(
            codec_shape(&def).unwrap(),
            CodecShapeOwned::Offset { zero: 2048 }
        );
        let def = def_of(&codec(
            "const LEVEL: [i128; 2] = [40, 45];",
            "CodecShape::MantissaExponent { radix: 1 << 11, exponents: (32), mantissas: def_list(&LEVEL), \
             mantissa_offset: 2 * 1024 + 0, exponent_zero: 31, exponent_step: -1 }",
        ));
        assert_eq!(
            codec_shape(&def).unwrap(),
            CodecShapeOwned::MantissaExponent {
                radix: 2048,
                exponents: 32,
                mantissas: vec![40, 45],
                mantissa_offset: 2048,
                exponent_zero: 31,
                exponent_step: -1,
            }
        );
        // a table computed by const code is no literal: named, with what to do
        let def = def_of(&codec(
            "const TABLE: [i128; 2] = { let t = [0; 2]; t };",
            "CodecShape::Table(def_list(&TABLE))",
        ));
        let err = format!("{:#}", codec_shape(&def).unwrap_err());
        assert!(
            err.contains("TABLE") && err.contains("written out"),
            "{err}"
        );
        // numbers the walker would have to evaluate, or that overflow
        for shape in [
            "CodecShape::Offset { zero: ZERO }",
            "CodecShape::Offset { zero: f(1) }",
            "CodecShape::Offset { zero: 1 << 200 }",
            "CodecShape::Offset { zero: 1 << 127 }",
            "CodecShape::Offset { zero: 7 / 2 }",
            "CodecShape::Gamma { g: 2 }",
        ] {
            assert!(codec_shape(&def_of(&codec("", shape))).is_err(), "{shape}");
        }
    }

    /// LVL-6: a codec's spare codes as exact_units writes them, one code or a span; a span upside down is refused.
    #[test]
    fn codec_spare_forms() {
        let def = |spare: &str| -> syn::Expr {
            syn::parse_str(&format!(
                "CodecDef {{ id: MarkerId {{ crate_name: \"c\", name: \"X\" }}, shape: CodecShape::MuLaw, spare: {spare} }}"
            ))
            .unwrap()
        };
        assert_eq!(codec_spare(&def("def_list(&[])")).unwrap(), vec![]);
        assert_eq!(
            codec_spare(&def("def_list(&[CodeSpan::one(0x7F)])")).unwrap(),
            vec![CodeSpan::one(0x7F)]
        );
        assert_eq!(
            codec_spare(&def(
                "exact_units::system::def_list(&[CodeSpan { low: 0x07FE, high: 0x0802 }, CodeSpan::one(-1)])"
            ))
            .unwrap(),
            vec![
                CodeSpan {
                    low: 0x07FE,
                    high: 0x0802
                },
                CodeSpan::one(-1)
            ]
        );
        assert!(codec_spare(&def("def_list(&[CodeSpan { low: 2, high: 1 }])")).is_err());
        assert!(codec_spare(&def("SPARE")).is_err());
    }
}
