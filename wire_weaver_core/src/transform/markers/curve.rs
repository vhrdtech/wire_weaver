//! Transfer curve markers (an `impl exact_units::codec::Curve`, STD-112): the `CurveDef` of the impl's `DEF`, read
//! from source.
//!
//! A curve's `DEF` names its numbers by constants (`Pq::M1`, a module's `const PQ_FORM: CurveForm = ..`) and
//! computes some with `Fraction`'s checked `const fn`s (`Hlg::A.checked_div_int(3)`, unwrapped by a helper of the
//! module, `known(..)`). [ConstScope::inline] puts the constants of the module in, so the `DEF` reads alone once it
//! is a `#[ww_marker(curve, ..)]`, and [curve_def] runs the `Fraction` arithmetic with `exact_units`' own
//! `Fraction`, the code the compiled `DEF` ran: exact, never a float. A call of anything else is refused naming it.

use anyhow::{Result, anyhow, bail};
use exact_units::codec::{CurveDefOwned, CurveForm, ExpBase, Referred, Segment};
use exact_units::{Fraction, MarkerIdOwned};
use syn::{Ident, Item, visit_mut::VisitMut};

use super::{def_list_items, field, int_expr};

/// Deepest nesting of constants [ConstScope::inline] follows: a constant naming itself stops there and stays a name,
/// which [curve_def] refuses.
const MAX_INLINE_DEPTH: usize = 16;

/// The constants of a module a curve's `DEF` may name: `const X: T = ..` of the module (`X`), `const N: T = ..` of
/// an inherent `impl Ty` (`Ty::N`, `Self::` in it written out), and the module's unwrap helpers.
pub(super) struct ConstScope {
    consts: Vec<(Vec<String>, syn::Expr)>,
    /// `const fn f(x: Option<T>) -> T { match x { Some(v) => v, None => panic!(..) } }`: a call is `x.unwrap()`.
    unwraps: Vec<Ident>,
}

impl ConstScope {
    pub(super) fn of(items: &[Item]) -> Self {
        let mut consts = Vec::new();
        let mut unwraps = Vec::new();
        for item in items {
            match item {
                Item::Const(c) => consts.push((vec![c.ident.to_string()], (*c.expr).clone())),
                Item::Impl(i) if i.trait_.is_none() && i.generics.params.is_empty() => {
                    let syn::Type::Path(ty) = &*i.self_ty else {
                        continue;
                    };
                    let Some(ty) = ty.path.get_ident() else {
                        continue;
                    };
                    for c in i.items.iter().filter_map(|i| match i {
                        syn::ImplItem::Const(c) => Some(c),
                        _ => None,
                    }) {
                        let mut expr = c.expr.clone();
                        SelfIs(ty).visit_expr_mut(&mut expr);
                        consts.push((vec![ty.to_string(), c.ident.to_string()], expr));
                    }
                }
                Item::Fn(f) if is_unwrap_helper(f) => unwraps.push(f.sig.ident.clone()),
                _ => {}
            }
        }
        ConstScope { consts, unwraps }
    }

    /// `e` with every constant of the scope it names put in, `Self::` standing for `self_ty`.
    pub(super) fn inline(&self, e: &mut syn::Expr, self_ty: &Ident) {
        SelfIs(self_ty).visit_expr_mut(e);
        Inline {
            scope: self,
            depth: 0,
        }
        .visit_expr_mut(e);
    }
}

/// Writes `Self::X` as `Ty::X`.
struct SelfIs<'a>(&'a Ident);

impl VisitMut for SelfIs<'_> {
    fn visit_path_mut(&mut self, p: &mut syn::Path) {
        if p.segments.len() >= 2 && p.segments[0].ident == "Self" {
            p.segments[0].ident = self.0.clone();
        }
        syn::visit_mut::visit_path_mut(self, p);
    }
}

struct Inline<'a> {
    scope: &'a ConstScope,
    depth: usize,
}

impl VisitMut for Inline<'_> {
    fn visit_expr_mut(&mut self, e: &mut syn::Expr) {
        if self.depth < MAX_INLINE_DEPTH {
            match e {
                syn::Expr::Path(p) if p.qself.is_none() => {
                    let path: Vec<String> = p
                        .path
                        .segments
                        .iter()
                        .map(|s| s.ident.to_string())
                        .collect();
                    if let Some((_, value)) = self.scope.consts.iter().find(|(k, _)| *k == path) {
                        *e = value.clone();
                        self.depth += 1;
                        self.visit_expr_mut(e);
                        self.depth -= 1;
                        return;
                    }
                }
                syn::Expr::Call(c)
                    if c.args.len() == 1
                        && matches!(&*c.func, syn::Expr::Path(f)
                            if f.path.get_ident().is_some_and(|f| self.scope.unwraps.contains(f))) =>
                {
                    let arg = &c.args[0];
                    *e = syn::parse_quote!((#arg).unwrap());
                }
                _ => {}
            }
        }
        syn::visit_mut::visit_expr_mut(self, e);
    }
}

/// Whether `f` is `const fn f(x: Option<T>) -> T { match x { Some(v) => v, None => panic!(..) } }`, an unwrap a
/// `const` can call.
fn is_unwrap_helper(f: &syn::ItemFn) -> bool {
    if f.sig.constness.is_none() || f.sig.inputs.len() != 1 || !f.sig.generics.params.is_empty() {
        return false;
    }
    let Some(syn::FnArg::Typed(arg)) = f.sig.inputs.first() else {
        return false;
    };
    let syn::Pat::Ident(param) = &*arg.pat else {
        return false;
    };
    let [syn::Stmt::Expr(syn::Expr::Match(m), None)] = f.block.stmts.as_slice() else {
        return false;
    };
    if !matches!(&*m.expr, syn::Expr::Path(p) if p.path.is_ident(&param.ident)) || m.arms.len() != 2
    {
        return false;
    }
    m.arms.iter().all(|arm| match (&arm.pat, &*arm.body) {
        (syn::Pat::TupleStruct(t), syn::Expr::Path(body))
            if t.path.is_ident("Some") && t.elems.len() == 1 =>
        {
            matches!(&t.elems[0], syn::Pat::Ident(v) if body.path.is_ident(&v.ident))
        }
        (syn::Pat::Ident(none), syn::Expr::Macro(mac)) => {
            none.ident == "None" && mac.mac.path.is_ident("panic")
        }
        (syn::Pat::Path(none), syn::Expr::Macro(mac)) => {
            none.path.is_ident("None") && mac.mac.path.is_ident("panic")
        }
        _ => false,
    })
}

/// The definition an `impl Curve`'s `DEF` gives, its constants [inlined](ConstScope::inline): `CurveDef { id,
/// referred, unit, scale, low, high, segments }`.
pub(super) fn curve_def(def: &syn::Expr) -> Result<CurveDefOwned> {
    let syn::Expr::Struct(s) = def else {
        bail!(
            "DEF `{}`: CurveDef {{ id, referred, unit, scale, low, high, segments }}",
            quote::quote!(#def)
        );
    };
    let segments = def_list_items(field(s, "segments")?)?
        .map(segment)
        .collect::<Result<Vec<_>>>()?;
    Ok(CurveDefOwned {
        id: marker_id(field(s, "id")?)?,
        referred: referred(field(s, "referred")?)?,
        unit: marker_id(field(s, "unit")?)?,
        scale: fraction(field(s, "scale")?)?,
        low: fraction(field(s, "low")?)?,
        high: fraction(field(s, "high")?)?,
        segments,
    })
}

/// `MarkerId { crate_name: "..", name: ".." }`.
fn marker_id(e: &syn::Expr) -> Result<MarkerIdOwned> {
    let syn::Expr::Struct(s) = e else {
        bail!(
            "`{}`: MarkerId {{ crate_name: \"..\", name: \"..\" }}",
            quote::quote!(#e)
        );
    };
    let str_of = |name: &str| match field(s, name)? {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(v),
            ..
        }) => Ok(v.value()),
        other => bail!("`{}`: a string literal", quote::quote!(#other)),
    };
    Ok(MarkerIdOwned {
        crate_name: str_of("crate_name")?,
        name: str_of("name")?,
    })
}

/// The last segment of a path expression: `Display` of `Referred::Display`.
fn last_ident(e: &syn::Expr) -> Option<String> {
    match e {
        syn::Expr::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
        _ => None,
    }
}

fn referred(e: &syn::Expr) -> Result<Referred> {
    match last_ident(e).as_deref() {
        Some("Scene") => Ok(Referred::Scene),
        Some("Display") => Ok(Referred::Display),
        _ => bail!(
            "referred `{}`: Referred::Scene or Referred::Display",
            quote::quote!(#e)
        ),
    }
}

/// `Segment { from, form }`.
fn segment(e: &syn::Expr) -> Result<Segment> {
    let syn::Expr::Struct(s) = e else {
        bail!("`{}`: Segment {{ from, form }}", quote::quote!(#e));
    };
    Ok(Segment {
        from: fraction(field(s, "from")?)?,
        form: form(field(s, "form")?)?,
    })
}

/// A `CurveForm::Affine { .. }`, `Power { .. }`, `Exp { .. }` or `RationalPower { .. }`.
fn form(e: &syn::Expr) -> Result<CurveForm> {
    let syn::Expr::Struct(s) = e else {
        bail!(
            "form `{}`: CurveForm::Affine, Power, Exp or RationalPower {{ .. }}",
            quote::quote!(#e)
        );
    };
    let f = |name: &str| fraction(field(s, name)?);
    let variant = s.path.segments.last().map(|s| s.ident.to_string());
    Ok(match variant.as_deref() {
        Some("Affine") => CurveForm::Affine {
            gain: f("gain")?,
            offset: f("offset")?,
        },
        Some("Power") => CurveForm::Power {
            gain: f("gain")?,
            scale: f("scale")?,
            shift: f("shift")?,
            exponent: f("exponent")?,
            offset: f("offset")?,
        },
        Some("Exp") => CurveForm::Exp {
            gain: f("gain")?,
            base: exp_base(field(s, "base")?)?,
            scale: f("scale")?,
            shift: f("shift")?,
            offset: f("offset")?,
        },
        Some("RationalPower") => CurveForm::RationalPower {
            m1: f("m1")?,
            m2: f("m2")?,
            c1: f("c1")?,
            c2: f("c2")?,
            c3: f("c3")?,
        },
        _ => bail!(
            "form `{}`: CurveForm::Affine, Power, Exp or RationalPower",
            quote::quote!(#e)
        ),
    })
}

/// `ExpBase::E` or `ExpBase::Exactly(fraction)`.
fn exp_base(e: &syn::Expr) -> Result<ExpBase> {
    match e {
        syn::Expr::Call(c)
            if c.args.len() == 1 && last_ident(&c.func).as_deref() == Some("Exactly") =>
        {
            Ok(ExpBase::Exactly(fraction(&c.args[0])?))
        }
        _ if last_ident(e).as_deref() == Some("E") => Ok(ExpBase::E),
        _ => bail!(
            "base `{}`: ExpBase::E or ExpBase::Exactly(..)",
            quote::quote!(#e)
        ),
    }
}

/// A `Fraction` as a curve's `DEF` writes one: `Fraction::new(num, den)`, `Fraction::int(n)`, `Fraction::ZERO`,
/// `Fraction::ONE`, or an `.unwrap()` of [checked](checked) arithmetic on such; integers as [int_expr] reads them.
fn fraction(e: &syn::Expr) -> Result<Fraction> {
    let refuse = || {
        anyhow!(
            "`{}`: the walker reads Fraction::new(num, den), ::int(n), ::ZERO, ::ONE and Fraction's checked \
             arithmetic, unwrapped",
            quote::quote!(#e)
        )
    };
    match e {
        syn::Expr::Paren(p) => fraction(&p.expr),
        syn::Expr::Group(g) => fraction(&g.expr),
        syn::Expr::MethodCall(m) if m.method == "unwrap" && m.args.is_empty() => {
            checked(&m.receiver)?.ok_or_else(|| anyhow!("`{}` overflows", quote::quote!(#e)))
        }
        syn::Expr::Path(p) if p.path.segments.len() >= 2 => {
            let n = p.path.segments.len();
            if p.path.segments[n - 2].ident != "Fraction" {
                return Err(refuse());
            }
            match p.path.segments[n - 1].ident.to_string().as_str() {
                "ZERO" => Ok(Fraction::ZERO),
                "ONE" => Ok(Fraction::ONE),
                _ => Err(refuse()),
            }
        }
        syn::Expr::Call(c) => {
            let syn::Expr::Path(p) = &*c.func else {
                return Err(refuse());
            };
            let n = p.path.segments.len();
            if n < 2 || p.path.segments[n - 2].ident != "Fraction" {
                return Err(refuse());
            }
            match (
                p.path.segments[n - 1].ident.to_string().as_str(),
                c.args.len(),
            ) {
                ("new", 2) => {
                    let num = int_expr(&c.args[0])?;
                    let den = u128::try_from(int_expr(&c.args[1])?)
                        .map_err(|_| anyhow!("`{}`: a negative denominator", quote::quote!(#e)))?;
                    Fraction::checked_new(num, den)
                        .ok_or_else(|| anyhow!("`{}` is no Fraction", quote::quote!(#e)))
                }
                ("int", 1) => Ok(Fraction::int(int_expr(&c.args[0])?)),
                _ => Err(refuse()),
            }
        }
        _ => Err(refuse()),
    }
}

/// `Fraction`'s checked arithmetic, `None` where it overflows: `a.checked_add(b)`, `checked_sub`, `checked_mul`,
/// `checked_div`, `checked_mul_int(n)`, `checked_div_int(n)` and `checked_recip()`.
fn checked(e: &syn::Expr) -> Result<Option<Fraction>> {
    let syn::Expr::MethodCall(m) = e else {
        if let syn::Expr::Paren(p) = e {
            return checked(&p.expr);
        }
        bail!(
            "`{}`: Fraction's checked arithmetic, `a.checked_add(b)` ..",
            quote::quote!(#e)
        );
    };
    let a = fraction(&m.receiver)?;
    let args: Vec<_> = m.args.iter().collect();
    Ok(match (m.method.to_string().as_str(), args.as_slice()) {
        ("checked_add", [b]) => a.checked_add(fraction(b)?),
        ("checked_sub", [b]) => a.checked_sub(fraction(b)?),
        ("checked_mul", [b]) => a.checked_mul(fraction(b)?),
        ("checked_div", [b]) => a.checked_div(fraction(b)?),
        ("checked_mul_int", [n]) => a.checked_mul_int(int_expr(n)?),
        ("checked_div_int", [n]) => a.checked_div_int(int_expr(n)?),
        ("checked_recip", []) => a.checked_recip(),
        _ => bail!(
            "`{}`: the walker runs Fraction's checked_add, _sub, _mul, _div, _mul_int, _div_int and _recip",
            quote::quote!(#e)
        ),
    })
}
