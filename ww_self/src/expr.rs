//! Values and calls written as Rust expressions (TOOL-9): `Config { mode: Mode::Fast, gains: [1.0, 2.0] }` for a value
//! of a known type, `motor.set_speed(1.5, Dir::Cw)` for a call. Parsed with `syn`, turned into a [ValueOwned] by the
//! type: a literal `5` becomes whatever the field is (`u16`, `ub3`, `f32`), out of range is an error naming the field.
//!
//! What is understood: `true`/`false`; integer, float, byte (`b'a'`) literals, negative ones, any suffix; strings;
//! `[a, b]`, `[x; n]`, `vec![..]` and `b"bytes"` for arrays, `Vec`s and `Series`; tuples; struct literals
//! (`Name { a: 1, .. Default::default() }`, `Name(1, 2)`, `Name`); enum variants (`Mode::Fast`, `Fast`,
//! `Shape::Circle(5)`, `Shape::Rect { w: 1, h: 2 }`); `Some(x)`, `None`, `Ok(x)`, `Err(e)`; `Box::new(x)` or just `x`;
//! ranges `3..10` and `3..=10`. Parentheses anywhere.

use crate::value::{numeric_base, signed_value, unsigned_value};
use crate::{
    ApiBundleOwned, FieldOwned, FieldsOwned, FieldsValueOwned, ItemEnumOwned, ItemStructOwned,
    TypeOwned, ValueOwned,
};
use anyhow::{Context, Result, anyhow, bail};
use quote::ToTokens;
use shrink_wrap::{Nibble, UNib32, UVlq32, UVlq64};
use syn::punctuated::Punctuated;
use syn::{Expr, Lit, Member, Token, UnOp};
use ww_numeric::{NumericBaseType, NumericValue};
/// The `syn` and `quote` the expressions are parsed with, for a [Hook] to match on.
pub use {quote, syn};

/// Parse `text`, a Rust expression, as a value of type `ty`.
pub fn parse_value(text: &str, ty: &TypeOwned, api_bundle: &ApiBundleOwned) -> Result<ValueOwned> {
    let expr: Expr = syn::parse_str(text).map_err(|e| anyhow!("not a Rust expression: {e}"))?;
    value_from_expr(&expr, ty, api_bundle)
}

/// A value of type `ty` from a parsed expression.
pub fn value_from_expr(
    expr: &Expr,
    ty: &TypeOwned,
    api_bundle: &ApiBundleOwned,
) -> Result<ValueOwned> {
    Conv {
        api_bundle,
        hook: None,
    }
    .value(expr, ty)
}

/// Values a tool reads its own way, asked first for every value inside the expression: `None` leaves it to the usual
/// reading. `wire_weaver_client::expr` reads `ww_si` quantities with it (`q!(3300 mV)`).
pub type Hook<'h> = &'h dyn Fn(&Expr, &TypeOwned, &ApiBundleOwned) -> Option<Result<ValueOwned>>;

/// [parse_value] with a [Hook].
pub fn parse_value_with(
    text: &str,
    ty: &TypeOwned,
    api_bundle: &ApiBundleOwned,
    hook: Hook,
) -> Result<ValueOwned> {
    let expr: Expr = syn::parse_str(text).map_err(|e| anyhow!("not a Rust expression: {e}"))?;
    value_from_expr_with(&expr, ty, api_bundle, hook)
}

/// [value_from_expr] with a [Hook].
pub fn value_from_expr_with(
    expr: &Expr,
    ty: &TypeOwned,
    api_bundle: &ApiBundleOwned,
    hook: Hook,
) -> Result<ValueOwned> {
    Conv {
        api_bundle,
        hook: Some(hook),
    }
    .value(expr, ty)
}

/// `value` of type `ty` written as a Rust expression [parse_value] reads back to the same value: struct and enum names
/// from the type, `Name { a: 1 }`, `Mode::Fast`, `Some(..)`, `[..]`, `3..10`. Fails for what a literal can't write (a
/// NaN or an infinite float, an enum variant the type doesn't have) and where the value doesn't match the type.
pub fn to_expr(value: &ValueOwned, ty: &TypeOwned, api_bundle: &ApiBundleOwned) -> Result<String> {
    Print { api_bundle }.value(value, ty)
}

/// A call written as a Rust expression, see [parse_call].
#[derive(Debug)]
pub enum Call {
    /// `motor.set_speed(1.5, Dir::Cw)`: a method with its arguments, each still to be read by its type.
    Method { path: String, args: Vec<Expr> },
    /// `motor.limit = 3`: a property written.
    Write { path: String, value: Expr },
    /// `motor.limit`, `bank[0].level`: a resource by its path (a property read, a stream, ...).
    Path { path: String },
}

/// `motor.set_speed(1.5)`, `motor.limit = 3` or `bank[0].level`: resource paths in the syntax of
/// `DynResource::path()`, arguments left for the method's types ([value_from_expr]).
pub fn parse_call(text: &str) -> Result<Call> {
    let expr: Expr = syn::parse_str(text).map_err(|e| anyhow!("not a Rust expression: {e}"))?;
    Ok(match unwrap(&expr) {
        Expr::MethodCall(call) => Call::Method {
            path: format!("{}.{}", resource_path(&call.receiver)?, call.method),
            args: call.args.iter().cloned().collect(),
        },
        Expr::Call(call) => Call::Method {
            path: resource_path(&call.func)?,
            args: call.args.iter().cloned().collect(),
        },
        Expr::Assign(assign) => Call::Write {
            path: resource_path(&assign.left)?,
            value: (*assign.right).clone(),
        },
        other => Call::Path {
            path: resource_path(other)?,
        },
    })
}

/// `motor`, `motor.limit`, `bank[0].level`.
fn resource_path(expr: &Expr) -> Result<String> {
    Ok(match unwrap(expr) {
        Expr::Path(p) if p.qself.is_none() && p.path.segments.len() == 1 => {
            p.path.segments[0].ident.to_string()
        }
        Expr::Field(field) => match &field.member {
            Member::Named(name) => format!("{}.{name}", resource_path(&field.base)?),
            Member::Unnamed(_) => bail!("`{}`: expected a name after `.`", text(expr)),
        },
        Expr::Index(index) => {
            let Expr::Lit(syn::ExprLit {
                lit: Lit::Int(i), ..
            }) = unwrap(&index.index)
            else {
                bail!("`{}`: an index must be a number", text(expr));
            };
            format!("{}[{}]", resource_path(&index.expr)?, i.base10_digits())
        }
        _ => bail!("`{}` is not a resource path", text(expr)),
    })
}

struct Conv<'a> {
    api_bundle: &'a ApiBundleOwned,
    hook: Option<Hook<'a>>,
}

impl Conv<'_> {
    fn value(&self, expr: &Expr, ty: &TypeOwned) -> Result<ValueOwned> {
        let expr = unwrap(expr);
        if let Some(hook) = self.hook
            && let Some(value) = hook(expr, ty, self.api_bundle)
        {
            return value;
        }
        let mismatch = || {
            anyhow!(
                "expected {}, got `{}`",
                crate::value::type_name(ty, self.api_bundle),
                text(expr)
            )
        };
        Ok(match ty {
            TypeOwned::Instance { .. } | TypeOwned::Param(_) => {
                return Err(crate::value::not_instantiated(ty));
            }
            TypeOwned::OutOfLine { type_idx } => {
                return self.value(expr, self.api_bundle.get_ty(type_idx.0)?.0);
            }
            TypeOwned::Bool => match expr {
                Expr::Lit(syn::ExprLit {
                    lit: Lit::Bool(b), ..
                }) => ValueOwned::Bool(b.value()),
                _ => return Err(mismatch()),
            },
            TypeOwned::NumericAny(any) => {
                ValueOwned::Numeric(numeric(expr, numeric_base(any)).map_err(|e| match e {
                    NumError::Shape => mismatch(),
                    NumError::Other(e) => e,
                })?)
            }
            TypeOwned::Flag => bail!("a flag is not written as a value"),
            TypeOwned::String => match expr {
                Expr::Lit(syn::ExprLit {
                    lit: Lit::Str(s), ..
                }) => ValueOwned::String(s.value()),
                _ => return Err(mismatch()),
            },
            TypeOwned::MaxLen { ty, .. } => return self.value(expr, ty),
            TypeOwned::Box(ty) => return self.value(box_new(expr), ty),
            TypeOwned::Vec(item) | TypeOwned::Series(item) => {
                ValueOwned::Vec(self.items(expr, item).ok_or_else(mismatch)??)
            }
            TypeOwned::Array { ty: item, .. } => {
                ValueOwned::Array(self.items(expr, item).ok_or_else(mismatch)??)
            }
            TypeOwned::Tuple(types) => {
                let elems: Vec<&Expr> = match expr {
                    Expr::Tuple(t) => t.elems.iter().collect(),
                    _ => return Err(mismatch()),
                };
                if elems.len() != types.len() {
                    bail!("expected {} elements, got {}", types.len(), elems.len());
                }
                ValueOwned::Tuple(
                    elems
                        .iter()
                        .zip(types)
                        .enumerate()
                        .map(|(i, (e, ty))| self.value(e, ty).with_context(|| format!("[{i}]")))
                        .collect::<Result<_>>()?,
                )
            }
            TypeOwned::Struct(def) => self.strukt(expr, def)?,
            TypeOwned::Enum(def) => self.enumeration(expr, def)?,
            TypeOwned::Option { some_ty } => match ctor(expr) {
                Some(("None", None)) => ValueOwned::Option(None),
                Some(("Some", Some([inner]))) => {
                    ValueOwned::Option(Some(Box::new(self.value(inner, some_ty).context("Some")?)))
                }
                _ => return Err(mismatch()),
            },
            TypeOwned::Result { ok_ty, err_ty } => match ctor(expr) {
                Some(("Ok", Some([inner]))) => {
                    ValueOwned::Result(Ok(Box::new(self.value(inner, ok_ty).context("Ok")?)))
                }
                Some(("Err", Some([inner]))) => {
                    ValueOwned::Result(Err(Box::new(self.value(inner, err_ty).context("Err")?)))
                }
                _ => return Err(mismatch()),
            },
            TypeOwned::Range(base) | TypeOwned::RangeInclusive(base) => {
                let closed = matches!(ty, TypeOwned::RangeInclusive(_));
                let Expr::Range(range) = expr else {
                    return Err(mismatch());
                };
                if matches!(range.limits, syn::RangeLimits::Closed(_)) != closed {
                    return Err(mismatch());
                }
                let (Some(start), Some(end)) = (&range.start, &range.end) else {
                    bail!("`{}`: a range needs both ends", text(expr));
                };
                let num = |e: &Expr, end: &str| {
                    numeric(e, base).map_err(|e| match e {
                        NumError::Shape => mismatch(),
                        NumError::Other(e) => e.context(end.to_string()),
                    })
                };
                let (start, end) = (num(start, "start")?, num(end, "end")?);
                if closed {
                    ValueOwned::RangeInclusive(start..=end)
                } else {
                    ValueOwned::Range(start..end)
                }
            }
        })
    }

    /// Items of an array, `Vec` or `Series`: `None` when `expr` is not a list.
    fn items(&self, expr: &Expr, item: &TypeOwned) -> Option<Result<Vec<ValueOwned>>> {
        let one = |i: usize, e: &Expr| self.value(e, item).with_context(|| format!("[{i}]"));
        Some(match expr {
            Expr::Array(a) => a.elems.iter().enumerate().map(|(i, e)| one(i, e)).collect(),
            Expr::Repeat(r) => (|| {
                let n = int_lit(&r.len)
                    .ok_or_else(|| anyhow!("`{}`: length must be a number", text(&r.len)))?;
                let n = usize::try_from(n).map_err(|_| anyhow!("length {n} is out of range"))?;
                let v = one(0, &r.expr)?;
                Ok(vec![v; n])
            })(),
            Expr::Macro(m) if m.mac.path.is_ident("vec") => (|| {
                let elems = m
                    .mac
                    .parse_body_with(Punctuated::<Expr, Token![,]>::parse_terminated)
                    .map_err(|e| anyhow!("vec![..]: {e}"))?;
                elems.iter().enumerate().map(|(i, e)| one(i, e)).collect()
            })(),
            // `0xa102`, as `ww` shows bytes
            Expr::Lit(syn::ExprLit {
                lit: Lit::Int(i), ..
            }) if self.is_u8(item) && i.to_string().starts_with("0x") => {
                hex_bytes(&i.to_string()[2..]).map(|bytes| {
                    bytes
                        .into_iter()
                        .map(|b| ValueOwned::Numeric(NumericValue::U8(b)))
                        .collect()
                })
            }
            Expr::Lit(syn::ExprLit {
                lit: Lit::ByteStr(b),
                ..
            }) => b
                .value()
                .iter()
                .enumerate()
                .map(|(i, byte)| {
                    let lit: Expr =
                        syn::parse_str(&byte.to_string()).expect("a number is an expression");
                    one(i, &lit)
                })
                .collect(),
            _ => return None,
        })
    }

    fn is_u8(&self, ty: &TypeOwned) -> bool {
        matches!(ty.get_in_line(self.api_bundle), Ok(TypeOwned::NumericAny(n)) if *numeric_base(n) == NumericBaseType::U8)
    }

    fn strukt(&self, expr: &Expr, def: &ItemStructOwned) -> Result<ValueOwned> {
        let (path, shape) = shape(expr)
            .ok_or_else(|| anyhow!("expected struct {}, got `{}`", def.ident, text(expr)))?;
        let name = path.last().map(String::as_str).unwrap_or_default();
        if name != plain_name(&def.ident) && name != "Self" {
            bail!("expected struct {}, got `{}`", def.ident, text(expr));
        }
        Ok(ValueOwned::Struct {
            fields: self.fields(shape, &def.fields, &def.ident)?,
        })
    }

    fn enumeration(&self, expr: &Expr, def: &ItemEnumOwned) -> Result<ValueOwned> {
        let names = || crate::value::variant_names(def);
        let (path, shape) = shape(expr).ok_or_else(|| {
            anyhow!(
                "expected a variant of {} ({}), got `{}`",
                def.ident,
                names(),
                text(expr)
            )
        })?;
        let (variant, prefix) = match path.as_slice() {
            [variant] => (variant.as_str(), None),
            [.., prefix, variant] => (variant.as_str(), Some(prefix.as_str())),
            [] => bail!("expected a variant of {}", def.ident),
        };
        if let Some(prefix) = prefix
            && prefix != plain_name(&def.ident)
            && prefix != "Self"
        {
            bail!("expected a variant of {}, got `{}`", def.ident, text(expr));
        }
        let v = def
            .variants
            .iter()
            .find(|v| v.ident == variant)
            .ok_or_else(|| {
                anyhow!(
                    "{} has no variant {variant}, expected one of: {}",
                    def.ident,
                    names()
                )
            })?;
        Ok(ValueOwned::Enum {
            variant: v.ident.clone(),
            fields: self.fields(shape, &v.fields, &format!("{}::{}", def.ident, v.ident))?,
        })
    }

    /// Fields of a struct or variant `what` from the shape it was written in.
    fn fields(&self, shape: Shape, defs: &FieldsOwned, what: &str) -> Result<FieldsValueOwned> {
        Ok(match (shape, defs) {
            (Shape::Unit, FieldsOwned::Unit) => FieldsValueOwned::Unit,
            (Shape::Named { fields, rest }, FieldsOwned::Named(defs)) => {
                let defs: Vec<&FieldOwned> = defs.iter().filter(|d| !d.is_flag()).collect();
                let mut values = vec![];
                for field in fields {
                    let Member::Named(name) = &field.member else {
                        bail!("{what} has named fields");
                    };
                    let name = name.to_string();
                    if field.colon_token.is_none() {
                        bail!("{what}: write `{name}: value`");
                    }
                    let def = defs
                        .iter()
                        .find(|d| d.ident.as_deref() == Some(name.as_str()))
                        .ok_or_else(|| {
                            anyhow!(
                                "{what} has no field `{name}`, it has: {}",
                                field_list(&defs)
                            )
                        })?;
                    values.push((
                        name.clone(),
                        self.value(&field.expr, &def.ty).context(name)?,
                    ));
                }
                if rest {
                    for def in &defs {
                        let name = def.ident.clone().unwrap_or_default();
                        if !values.iter().any(|(n, _)| *n == name) {
                            values.push((name, ValueOwned::default(&def.ty, self.api_bundle)?));
                        }
                    }
                }
                FieldsValueOwned::Named(values)
            }
            (Shape::Tuple(args), FieldsOwned::Unnamed(defs)) => {
                if args.len() != defs.len() {
                    bail!("{what} has {} fields, got {}", defs.len(), args.len());
                }
                FieldsValueOwned::Unnamed(
                    args.iter()
                        .zip(defs)
                        .enumerate()
                        .map(|(i, (e, d))| self.value(e, &d.ty).with_context(|| i.to_string()))
                        .collect::<Result<_>>()?,
                )
            }
            (_, FieldsOwned::Unit) => bail!("{what} has no fields"),
            (_, FieldsOwned::Named(defs)) => {
                let defs: Vec<&FieldOwned> = defs.iter().filter(|d| !d.is_flag()).collect();
                bail!(
                    "{what} has named fields: {what} {{ {} }}",
                    field_list(&defs)
                )
            }
            (_, FieldsOwned::Unnamed(defs)) => {
                bail!("{what} has {} unnamed fields: {what}(..)", defs.len())
            }
        })
    }
}

/// How a struct or variant was written, after its path.
enum Shape<'e> {
    Unit,
    Tuple(Vec<&'e Expr>),
    Named {
        fields: Vec<&'e syn::FieldValue>,
        /// `..Default::default()`: missing fields take their default.
        rest: bool,
    },
}

/// The path (`Shape::Rect` as `["Shape", "Rect"]`) and the fields of a struct literal, call or plain path.
fn shape(expr: &Expr) -> Option<(Vec<String>, Shape<'_>)> {
    let names = |path: &syn::Path| path.segments.iter().map(|s| s.ident.to_string()).collect();
    Some(match expr {
        Expr::Path(p) => (names(&p.path), Shape::Unit),
        Expr::Call(call) => match unwrap(&call.func) {
            Expr::Path(p) => (names(&p.path), Shape::Tuple(call.args.iter().collect())),
            _ => return None,
        },
        Expr::Struct(s) => (
            names(&s.path),
            Shape::Named {
                fields: s.fields.iter().collect(),
                rest: s.dot2_token.is_some(),
            },
        ),
        _ => return None,
    })
}

/// `Some(x)` as `("Some", Some([x]))`, `None` as `("None", None)`.
fn ctor(expr: &Expr) -> Option<(&str, Option<[&Expr; 1]>)> {
    match expr {
        Expr::Path(p) if p.path.segments.len() == 1 => {
            let name = p.path.segments[0].ident.to_string();
            (name == "None").then_some(("None", None))
        }
        Expr::Call(call) if call.args.len() == 1 => {
            let Expr::Path(p) = unwrap(&call.func) else {
                return None;
            };
            let name = p.path.segments.last()?.ident.to_string();
            let name = ["Some", "Ok", "Err"].into_iter().find(|n| *n == name)?;
            Some((name, Some([&call.args[0]])))
        }
        _ => None,
    }
}

/// `x` of `Box::new(x)`, else `expr` itself.
fn box_new(expr: &Expr) -> &Expr {
    if let Expr::Call(call) = expr
        && call.args.len() == 1
        && let Expr::Path(p) = unwrap(&call.func)
        && p.path.segments.len() == 2
        && p.path.segments[0].ident == "Box"
        && p.path.segments[1].ident == "new"
    {
        return unwrap(&call.args[0]);
    }
    expr
}

/// Without parentheses and invisible groups.
fn unwrap(expr: &Expr) -> &Expr {
    match expr {
        Expr::Paren(p) => unwrap(&p.expr),
        Expr::Group(g) => unwrap(&g.expr),
        _ => expr,
    }
}

fn text(expr: &Expr) -> String {
    expr.to_token_stream().to_string()
}

fn field_list(defs: &[&FieldOwned]) -> String {
    defs.iter()
        .filter_map(|d| d.ident.as_deref())
        .collect::<Vec<_>>()
        .join(", ")
}

enum NumError {
    /// Not a number at all.
    Shape,
    Other(anyhow::Error),
}

impl From<anyhow::Error> for NumError {
    fn from(e: anyhow::Error) -> Self {
        NumError::Other(e)
    }
}

/// An integer literal, possibly negative.
fn int_lit(expr: &Expr) -> Option<i128> {
    match unwrap(expr) {
        Expr::Lit(syn::ExprLit {
            lit: Lit::Int(i), ..
        }) => i.base10_parse().ok(),
        Expr::Lit(syn::ExprLit {
            lit: Lit::Byte(b), ..
        }) => Some(b.value() as i128),
        // the magnitude as u128: `-170141183460469231731687303715884105728` is i128::MIN
        Expr::Unary(u) if matches!(u.op, UnOp::Neg(_)) => match unwrap(&u.expr) {
            Expr::Lit(syn::ExprLit {
                lit: Lit::Int(i), ..
            }) => {
                let magnitude: u128 = i.base10_parse().ok()?;
                0i128.checked_sub_unsigned(magnitude)
            }
            other => int_lit(other).and_then(i128::checked_neg),
        },
        _ => None,
    }
}

/// A float literal (or an integer one), possibly negative.
fn float_lit(expr: &Expr) -> Option<f64> {
    match unwrap(expr) {
        Expr::Lit(syn::ExprLit {
            lit: Lit::Float(f), ..
        }) => f.base10_parse().ok(),
        Expr::Unary(u) if matches!(u.op, UnOp::Neg(_)) => float_lit(&u.expr).map(|f| -f),
        e => int_lit(e).map(|i| i as f64),
    }
}

/// A number of type `base` from a literal, range checked.
fn numeric(expr: &Expr, base: &NumericBaseType) -> Result<NumericValue, NumError> {
    use NumericBaseType as B;
    use NumericValue as N;
    if let B::F32 | B::F64 = base {
        let f = float_lit(expr).ok_or(NumError::Shape)?;
        return Ok(match base {
            B::F32 => N::F32(f as f32),
            _ => N::F64(f),
        });
    }
    // u128 above i128::MAX is the one integer that doesn't fit
    if let B::U128 = base
        && let Expr::Lit(syn::ExprLit {
            lit: Lit::Int(i), ..
        }) = unwrap(expr)
        && let Ok(v) = i.base10_parse::<u128>()
    {
        return Ok(N::U128(v));
    }
    let i = int_lit(expr).ok_or(NumError::Shape)?;
    let range = || anyhow!("{i} is out of range for {}", base_name(base));
    macro_rules! fit {
        ($t:ty) => {
            <$t>::try_from(i).map_err(|_| range())?
        };
    }
    Ok(match base {
        B::Nibble => N::Nibble(Nibble::new(fit!(u8)).ok_or_else(range)?),
        B::U8 => N::U8(fit!(u8)),
        B::U16 => N::U16(fit!(u16)),
        B::U32 => N::U32(fit!(u32)),
        B::UNib32 => N::UNib32(UNib32(fit!(u32))),
        B::U64 => N::U64(fit!(u64)),
        B::U128 => N::U128(fit!(u128)),
        B::I8 => N::I8(fit!(i8)),
        B::I16 => N::I16(fit!(i16)),
        B::I32 => N::I32(fit!(i32)),
        B::I64 => N::I64(fit!(i64)),
        B::I128 => N::I128(i),
        B::UVlq32 => N::UVlq32(UVlq32(fit!(u32))),
        B::UVlq64 => N::UVlq64(UVlq64(fit!(u64))),
        B::UB(bits) => {
            let v = fit!(u64);
            if bits.0 < 64 && v >> bits.0 != 0 {
                return Err(range().into());
            }
            unsigned_value(bits.0, v)
        }
        B::IB(bits) => {
            let v = fit!(i64);
            let half = 1i64 << (bits.0 - 1);
            if bits.0 < 64 && !(-half..half).contains(&v) {
                return Err(range().into());
            }
            signed_value(bits.0, v)
        }
        other => return Err(anyhow!("{other:?} numbers can't be written yet").into()),
    })
}

/// `u8`, `ub4`, `ib10`.
fn base_name(base: &NumericBaseType) -> String {
    match base {
        NumericBaseType::UB(bits) => format!("ub{}", bits.0),
        NumericBaseType::IB(bits) => format!("ib{}", bits.0),
        other => format!("{other:?}").to_lowercase(),
    }
}

/// `a102` as `[0xa1, 0x02]`, `_` allowed between digits.
fn hex_bytes(digits: &str) -> Result<Vec<u8>> {
    let digits: String = digits.chars().filter(|c| *c != '_').collect();
    if !digits.len().is_multiple_of(2) {
        bail!("0x{digits}: bytes need an even number of hex digits");
    }
    (0..digits.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&digits[i..i + 2], 16).map_err(|_| anyhow!("0x{digits} is not hex"))
        })
        .collect()
}

struct Print<'a> {
    api_bundle: &'a ApiBundleOwned,
}

impl Print<'_> {
    fn value(&self, value: &ValueOwned, ty: &TypeOwned) -> Result<String> {
        let mismatch = || {
            anyhow!(
                "{value:?} is not a {}",
                crate::value::type_name(ty, self.api_bundle)
            )
        };
        Ok(match (ty, value) {
            (TypeOwned::Instance { .. } | TypeOwned::Param(_), _) => {
                return Err(crate::value::not_instantiated(ty));
            }
            (TypeOwned::OutOfLine { type_idx }, _) => {
                return self.value(value, self.api_bundle.get_ty(type_idx.0)?.0);
            }
            (TypeOwned::MaxLen { ty, .. } | TypeOwned::Box(ty), _) => return self.value(value, ty),
            (TypeOwned::Bool, ValueOwned::Bool(b)) => b.to_string(),
            (TypeOwned::NumericAny(_), ValueOwned::Numeric(n)) => number(n)?,
            (TypeOwned::String, ValueOwned::String(s)) => format!("{s:?}"),
            (
                TypeOwned::Vec(item) | TypeOwned::Series(item) | TypeOwned::Array { ty: item, .. },
                ValueOwned::Vec(items) | ValueOwned::Array(items),
            ) => format!("[{}]", self.list(items, |_| Some(item))?),
            (TypeOwned::Tuple(types), ValueOwned::Tuple(items)) => match items.len() {
                1 => format!("({},)", self.list(items, |i| types.get(i))?),
                _ => format!("({})", self.list(items, |i| types.get(i))?),
            },
            (TypeOwned::Struct(def), ValueOwned::Struct { fields }) => {
                format!(
                    "{}{}",
                    plain_name(&def.ident),
                    self.fields(fields, &def.fields)?
                )
            }
            (TypeOwned::Enum(def), ValueOwned::Enum { variant, fields }) => {
                let v = def
                    .variants
                    .iter()
                    .find(|v| v.ident == *variant)
                    .ok_or_else(|| anyhow!("{} has no variant {variant}", def.ident))?;
                format!(
                    "{}::{}{}",
                    plain_name(&def.ident),
                    v.ident,
                    self.fields(fields, &v.fields)?
                )
            }
            (TypeOwned::Option { .. }, ValueOwned::Option(None)) => "None".into(),
            (TypeOwned::Option { some_ty }, ValueOwned::Option(Some(v))) => {
                format!("Some({})", self.value(v, some_ty)?)
            }
            (TypeOwned::Result { ok_ty, .. }, ValueOwned::Result(Ok(v))) => {
                format!("Ok({})", self.value(v, ok_ty)?)
            }
            (TypeOwned::Result { err_ty, .. }, ValueOwned::Result(Err(v))) => {
                format!("Err({})", self.value(v, err_ty)?)
            }
            (TypeOwned::Range(_), ValueOwned::Range(r)) => {
                format!("{}..{}", number(&r.start)?, number(&r.end)?)
            }
            (TypeOwned::RangeInclusive(_), ValueOwned::RangeInclusive(r)) => {
                format!("{}..={}", number(r.start())?, number(r.end())?)
            }
            _ => return Err(mismatch()),
        })
    }

    fn list<'t>(
        &self,
        items: &[ValueOwned],
        ty: impl Fn(usize) -> Option<&'t TypeOwned>,
    ) -> Result<String> {
        let items = items
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let ty = ty(i).ok_or_else(|| anyhow!("more values than types"))?;
                self.value(v, ty).with_context(|| format!("[{i}]"))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(items.join(", "))
    }

    /// The fields after a struct's or variant's name: ` { a: 1 }`, `(1, 2)`, nothing.
    fn fields(&self, fields: &FieldsValueOwned, defs: &FieldsOwned) -> Result<String> {
        Ok(match (fields, defs) {
            (FieldsValueOwned::Unit, _) => String::new(),
            (FieldsValueOwned::Named(values), FieldsOwned::Named(defs)) => {
                let values = values
                    .iter()
                    .map(|(name, v)| {
                        let def = defs
                            .iter()
                            .find(|d| !d.is_flag() && d.ident.as_deref() == Some(name.as_str()))
                            .ok_or_else(|| anyhow!("no field `{name}`"))?;
                        Ok(format!(
                            "{name}: {}",
                            self.value(v, &def.ty).context(name.clone())?
                        ))
                    })
                    .collect::<Result<Vec<_>>>()?;
                format!(" {{ {} }}", values.join(", "))
            }
            (FieldsValueOwned::Unnamed(values), FieldsOwned::Unnamed(defs)) => {
                format!("({})", self.list(values, |i| defs.get(i).map(|d| &d.ty))?)
            }
            _ => bail!("fields don't match their definition"),
        })
    }
}

/// A number as a Rust literal: floats always with a point or an exponent, so they read back as floats.
fn number(n: &NumericValue) -> Result<String> {
    if n.is_float() {
        let f = n.to_f64();
        if !f.is_finite() {
            bail!("{f} has no literal");
        }
        let text = match n {
            NumericValue::F32(v) => format!("{v:?}"),
            _ => format!("{f:?}"),
        };
        return Ok(text);
    }
    if let NumericValue::U128(v) = n {
        return Ok(v.to_string());
    }
    n.to_i128()
        .map(|i| i.to_string())
        .ok_or_else(|| anyhow!("{n:?} is not a number a literal writes"))
}

/// `Timed` of a generic definition made concrete, `Timed<u16>`: the name a Rust literal writes.
fn plain_name(ident: &str) -> &str {
    ident.split('<').next().unwrap_or(ident)
}
