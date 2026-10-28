//! Unit, scale and origin markers declared with `exact_units::unit!` / `exact_units::scale!` (INT-13) and
//! `exact_units::origin!` (STD-64).
//!
//! The walker reads such an invocation as rustc does: a unit struct per line, with the line's attributes. Each struct
//! also gets a `#[ww_marker(..)]` attribute holding its line, from which its definition (dimension, scale, symbol)
//! goes into the API model. The grammar is the one documented at the macros; a line the macros don't accept is an
//! error here too. An `origin!` declares one struct, `<attrs> <vis> struct <Name> { <keyed fields> }`; its fields
//! are kept as they are in `#[ww_marker(origin, ..)]`.

use super::crate_walker::{CrateContext, Scratch};
use super::ty::convert_ty;
use anyhow::{Context, Result, anyhow, bail};
use exact_units::{CountedOwned, DimOwned, Irrational, Magnitude, Offset, ProvenanceOwned};
use proc_macro2::TokenStream;
use syn::parse::{Parse, ParseStream};
use syn::{
    Attribute, Ident, Item, LitFloat, LitInt, LitStr, Token, Visibility, bracketed, parenthesized,
};
use ww_self::units::{marker_name_of, origin_of, scale_of, unit_of};
use ww_self::{
    ApiBundleOwned, MarkerDefOwned, MarkerRefOwned, OriginDefOwned, ScaleDefOwned, UnitDefOwned,
};

/// Name of the attribute a struct made from a `unit!` / `scale!` line carries, with the line after its `=`.
pub(crate) const MARKER_ATTR: &str = "ww_marker";

/// The macro an invocation's path names: `unit!`, `exact_units::unit!`, `ww_si::scale!`, ...
fn marker_macro(item: &syn::ItemMacro) -> Option<MacroKind> {
    match item.mac.path.segments.last()?.ident.to_string().as_str() {
        "unit" => Some(MacroKind::Unit),
        "scale" => Some(MacroKind::Scale),
        "origin" => Some(MacroKind::Origin),
        _ => None,
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) enum MacroKind {
    Unit,
    Scale,
    Origin,
}

/// Replaces every `unit!` / `scale!` / `origin!` invocation in `items` (and in inline modules) with the structs it declares.
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
        unit: syn::Type,
        symbol: String,
        offset: syn::Expr,
        // boxed: rarely given, and they would make the variant far larger than `Above`
        step: Option<Box<syn::Expr>>,
        reference: Option<Box<syn::Type>>,
        modulus: Option<Box<syn::Type>>,
    },
    /// `above: Parent, offset: ..`.
    Above {
        parent: syn::Type,
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
            let unit = input.parse()?;
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
            let reference = match reference {
                Some(r) if !last_ident_is(r, "UnitZero") => {
                    let (ty, bundle) = resolve(r)?;
                    let name = marker_name_of(&ty, &bundle).context("the origin's reference")?;
                    Some(MarkerRefOwned {
                        crate_id: name.crate_id,
                        name: name.name,
                    })
                }
                _ => None,
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
                unit: UnitDefOwned {
                    symbol: unit.to_string(),
                    dim: unit.dim,
                    magnitude: unit.magnitude,
                    // a unit expression has no provenance of its own yet: origins are over defined units (K, °)
                    provenance: ProvenanceOwned::Defined,
                },
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
    Unit(UnitLine),
    Scale(ScaleLine),
    /// A hand-written `impl Unit` / `impl FixedScale` (the trait's name).
    Hand(String),
    /// The keyed fields of an `origin!`: `unit: Kelvin, symbol: "°C", offset: Offset::decimal(27315, -2)` or
    /// `above: Celsius, offset: ..`.
    Origin(Box<OriginFields>),
}

/// The definition of a struct made from a marker line, `None` for any other struct.
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
            "hand" => Ok(MarkerLineDef::Hand(input.parse::<Ident>()?.to_string())),
            "origin" => Ok(MarkerLineDef::Origin(Box::new(input.parse()?))),
            _ => Err(syn::Error::new(kind.span(), "unit, scale, hand or origin")),
        }
    })?;
    Ok(Some(def))
}

/// `exact_units`' own generic markers, whose meaning is fixed by their name and generic arguments.
const BUILT_IN: [(&str, MarkerDefOwned); 15] = [
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
];

/// The crate whose markers [BUILT_IN] names.
const EXACT_UNITS: &str = "exact_units";

/// What `item_struct` of crate `crate_name` (as Rust names it) means as a marker ([ItemStructOwned::marker]), `None`
/// for any other struct. A marker with a hand-written `impl Unit` / `impl FixedScale` is an error naming it.
///
/// [ItemStructOwned::marker]: ww_self::ItemStructOwned::marker
///
/// An `origin!` is resolved by `origin`: its unit, reference, modulus and parent are types to convert, see
/// [origin_def].
pub(crate) fn marker_def(
    item_struct: &syn::ItemStruct,
    crate_name: &str,
    origin: &mut dyn FnMut(&OriginFields) -> Result<OriginDefOwned>,
) -> Result<Option<MarkerDefOwned>> {
    let crate_name = crate_name.replace('-', "_");
    let ident = item_struct.ident.to_string();
    if crate_name == EXACT_UNITS
        && let Some((_, def)) = BUILT_IN.iter().find(|(name, _)| *name == ident)
    {
        return Ok(Some(def.clone()));
    }
    let Some(line) = marker_line(&item_struct.attrs)? else {
        return Ok(None);
    };
    let def = match line {
        MarkerLineDef::Unit(line) => {
            let dim = match &line.dim {
                DimLine::Exps(exps) => {
                    let mut dim = DimOwned {
                        s: 0,
                        m: 0,
                        kg: 0,
                        a: 0,
                        k: 0,
                        mol: 0,
                        cd: 0,
                        rad: 0,
                        bit: 0,
                        root: false,
                        counted: [None, None, None, None],
                    };
                    for (base, e) in exps {
                        let slot = match base.as_str() {
                            "s" => &mut dim.s,
                            "m" => &mut dim.m,
                            "kg" => &mut dim.kg,
                            "a" => &mut dim.a,
                            "k" => &mut dim.k,
                            "mol" => &mut dim.mol,
                            "cd" => &mut dim.cd,
                            "rad" => &mut dim.rad,
                            _ => &mut dim.bit,
                        };
                        *slot = *e;
                    }
                    dim
                }
                DimLine::Counted => DimOwned {
                    s: 0,
                    m: 0,
                    kg: 0,
                    a: 0,
                    k: 0,
                    mol: 0,
                    cd: 0,
                    rad: 0,
                    bit: 0,
                    root: false,
                    counted: [
                        Some(CountedOwned {
                            crate_name: crate_name.clone(),
                            name: ident.clone(),
                            exp: 1,
                        }),
                        None,
                        None,
                        None,
                    ],
                },
            };
            let magnitude = match &line.scale {
                UnitScaleLine::Coherent => Magnitude::ONE,
                UnitScaleLine::Pow10(e) => magnitude(&MagnitudeLine {
                    num: 1,
                    den: 1,
                    pow10: *e,
                    pow2: 0,
                    pi: 0,
                })
                .with_context(|| format!("unit marker {ident}"))?,
                UnitScaleLine::Exact(m) => {
                    magnitude(m).with_context(|| format!("unit marker {ident}"))?
                }
                // TODO(STD-74): °C over K goes to origins; the model carries the offset as exact_units::Offset then
                UnitScaleLine::Affine { .. } => return Ok(None),
            };
            let provenance = match line.provenance {
                Some((source, year)) => ProvenanceOwned::Conventional { source, year },
                None => ProvenanceOwned::Defined,
            };
            MarkerDefOwned::Unit(UnitDefOwned {
                symbol: line.symbol,
                dim,
                magnitude,
                provenance,
            })
        }
        MarkerLineDef::Scale(line) => {
            let m = match &line.factor {
                ScaleFactorLine::Pow10(e) => MagnitudeLine {
                    num: 1,
                    den: 1,
                    pow10: *e,
                    pow2: 0,
                    pi: 0,
                },
                ScaleFactorLine::Pow2(e) => MagnitudeLine {
                    num: 1,
                    den: 1,
                    pow10: 0,
                    pow2: *e,
                    pi: 0,
                },
                ScaleFactorLine::Exact(m) => m.clone(),
            };
            MarkerDefOwned::Scale(ScaleDefOwned {
                symbol: line.symbol,
                magnitude: magnitude(&m).with_context(|| format!("scale marker {ident}"))?,
            })
        }
        MarkerLineDef::Origin(fields) => MarkerDefOwned::Origin(
            origin(&fields).with_context(|| format!("origin marker {ident}"))?,
        ),
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

/// The exact magnitude a `num / den, pow10 a, pow2 b, pi c` line means.
fn magnitude(m: &MagnitudeLine) -> Result<Magnitude> {
    let x2 = |e: i8| {
        e.checked_mul(2)
            .ok_or_else(|| anyhow!("exponent {e} out of range"))
    };
    let powers = Magnitude::from_parts_x2(
        x2(m.pow10)?,
        x2(m.pow2)?,
        Irrational {
            pi: x2(m.pi)?,
            ln2: 0,
            ln10: 0,
        },
        [],
    )
    .map_err(|e| anyhow!("{e:?}"))?;
    Magnitude::checked_ratio(m.num, m.den)
        .and_then(|r| r.checked_mul(powers))
        .map_err(|e| anyhow!("{} / {} · {powers:?}: {e:?}", m.num, m.den))
}

/// `"<symbol>", <dimension>[, <scale>]` of a `unit!` line.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct UnitLine {
    pub(crate) symbol: String,
    pub(crate) dim: DimLine,
    pub(crate) scale: UnitScaleLine,
    /// `conventional("<source>", <year>)` after the scale: a value a source prints, not a definition.
    pub(crate) provenance: Option<(String, u16)>,
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
            dim,
            scale,
            provenance,
        })
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
    use exact_units::{FixedScale, Origin, Scale, UnitExpr, origins, prefix};
    use shrink_wrap::{SerializeShrinkWrap, SerializeShrinkWrapOwned};
    use std::collections::BTreeMap;
    use ww_self::{TypeLocationOwned, TypeOwned};

    /// Every marker of `exact_units` as the walker reads it from source, by name.
    fn exact_units_markers() -> BTreeMap<String, MarkerDefOwned> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../exact_units");
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

    /// INT-13: the walker's reading of each `unit!` / `scale!` line is the definition rustc compiled from it, so the
    /// grammar here and the macros can't drift apart.
    #[test]
    fn exact_units_definitions_match_the_compiled_consts() {
        let mut markers = exact_units_markers();
        for expr in exact_units::units::ALL
            .iter()
            .chain(exact_units::units::CUSTOMARY)
        {
            let UnitExpr::Named {
                name,
                symbol,
                dim,
                scale,
                provenance,
            } = expr
            else {
                panic!("units::ALL holds named units")
            };
            let magnitude = match scale {
                Scale::Coherent => Magnitude::ONE,
                Scale::Pow10(e) => Magnitude::pow10(*e),
                Scale::Exact(m) => *m,
                // TODO(STD-74): °C, not in the model until origins carry its offset
                Scale::Affine { .. } => {
                    assert!(!markers.contains_key(*name), "{name}");
                    continue;
                }
            };
            let Some(MarkerDefOwned::Unit(def)) = markers.remove(*name) else {
                panic!("{name}: no unit definition in the model");
            };
            assert_eq!(def.symbol, *symbol, "{name}");
            assert_eq!(
                dim_bytes(&def.dim),
                dim.to_ww_bytes(&mut [0; 64]).unwrap(),
                "{name}: {:?} vs {dim:?}",
                def.dim
            );
            assert_eq!(def.magnitude, magnitude, "{name}");
            let compiled = match provenance {
                exact_units::Provenance::Defined => ProvenanceOwned::Defined,
                exact_units::Provenance::Conventional { source, year } => {
                    ProvenanceOwned::Conventional {
                        source: source.to_string(),
                        year: *year,
                    }
                }
                exact_units::Provenance::Unknown(_) => {
                    panic!("{name}: a compiled unit is never unknown")
                }
            };
            assert_eq!(def.provenance, compiled, "{name}");
        }
        macro_rules! scales {
            ($($name:ident)*) => {
                [$((stringify!($name), <prefix::$name as FixedScale>::SYMBOL, <prefix::$name as FixedScale>::SCALE)),*]
            };
        }
        for (name, symbol, magnitude) in scales!(
            Quecto Ronto Yocto Zepto Atto Femto Pico Nano Micro Milli Centi Deci One Deca Hecto Kilo Mega Giga Tera
            Peta Exa Zetta Yotta Ronna Quetta Kibi Mebi Gibi Tebi
        ) {
            let Some(MarkerDefOwned::Scale(def)) = markers.remove(name) else {
                panic!("{name}: no scale definition in the model");
            };
            assert_eq!(
                (def.symbol.as_str(), def.magnitude),
                (symbol, magnitude),
                "{name}"
            );
        }
        let Some(MarkerDefOwned::Scale(two_pi)) = markers.remove("TwoPi") else {
            panic!("TwoPi: no scale definition in the model");
        };
        assert_eq!(two_pi.magnitude, <origins::TwoPi as FixedScale>::SCALE);
        macro_rules! origins {
            ($($name:ident)*) => {
                [$((stringify!($name), <origins::$name as Origin>::DEF)),*]
            };
        }
        for (name, compiled) in origins!(
            AbsoluteZero Celsius Rankine Fahrenheit Vacuum Gauge AboveStandardAtmosphere
        ) {
            let Some(MarkerDefOwned::Origin(def)) = markers.remove(name) else {
                panic!("{name}: no origin definition in the model");
            };
            let UnitExpr::Named {
                symbol, dim, scale, ..
            } = compiled.unit
            else {
                panic!("{name}: a named unit");
            };
            assert_eq!(def.unit.symbol, *symbol, "{name}");
            assert_eq!(
                dim_bytes(&def.unit.dim),
                dim.to_ww_bytes(&mut [0; 64]).unwrap(),
                "{name}"
            );
            assert_eq!(*scale, Scale::Coherent, "{name}");
            assert_eq!(def.unit.magnitude, Magnitude::ONE, "{name}");
            assert_eq!(def.symbol, compiled.symbol, "{name}");
            assert_eq!(
                def.reference.is_some(),
                !compiled.reference.is_empty(),
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
        for (name, def) in BUILT_IN {
            assert_eq!(markers.remove(name), Some(def), "{name}");
        }
        // a marker added to exact_units is checked here too
        assert!(markers.is_empty(), "not checked: {markers:?}");
    }

    fn expand(src: &str) -> Vec<Item> {
        let mut file = syn::parse_file(src).unwrap();
        expand_marker_macros(&mut file.items).unwrap();
        file.items
    }

    fn no_origin(_: &OriginFields) -> Result<OriginDefOwned> {
        bail!("no origins here")
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
                pub struct Bucket = "bucket", counted;
                pub struct DegC = "°C", [k 1], Affine { factor: 1.0, offset: 273.15 };
                pub struct InHg = "inHg", [kg 1, m -1, s -2], Exact(338638 / 100), conventional("NIST SP 811", 2008);
            }
            "#,
        );
        assert_eq!(items.len(), 4);
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
        assert_eq!(line.provenance, None);
        let Item::Struct(in_hg) = &items[3] else {
            panic!()
        };
        let Some(MarkerLineDef::Unit(line)) = marker_line(&in_hg.attrs).unwrap() else {
            panic!()
        };
        assert_eq!(line.provenance, Some(("NIST SP 811".into(), 2008)));
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
        let err = marker_def(structs[0], "my-api", &mut no_origin)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("Rpm") && err.contains("my_api") && err.contains("unit!"),
            "{err}"
        );
        let err = marker_def(structs[1], "my_api", &mut no_origin)
            .unwrap_err()
            .to_string();
        assert!(err.contains("Lsb") && err.contains("scale!"), "{err}");
        assert!(
            marker_def(structs[2], "my_api", &mut no_origin)
                .unwrap()
                .is_none()
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
