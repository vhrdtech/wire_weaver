//! The fixed-layout fast path (SW-44, SW-45): for a type whose fields sit at offsets known at compile time,
//! offset consts per field, `from_fixed` / `to_fixed` straight on the slice (no `BufReader` / `BufWriter`), and the
//! matching C reader as text. The bytes are the ones `ser_shrink_wrap` writes. What qualifies and why is in the
//! docs of `shrink_wrap::fixed_layout`.
//!
//! The layout is walked with a cursor: a byte offset (a const expression, nested types are sized by the compiler)
//! and a number of bits used after it (known here). Byte-aligned fields round the bits up to the next byte, as
//! `BufWriter` aligns; bit fields add to them; a `Nibble` first rounds them to 4.

use crate::ast::item_enum::{Fields, Variant};
use crate::ast::item_struct::{Field, StructKind};
use crate::ast::object_size::ObjectSize;
use crate::ast::repr::Repr;
use crate::ast::ty::{ArrayLen, Type};
use crate::codegen::util::CratePath;
use proc_macro2::{Ident, Literal, Span, TokenStream};
use quote::{format_ident, quote};
use syn::LitStr;

#[derive(Clone, Copy, Debug)]
pub(crate) enum Prim {
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
    F32,
    F64,
}

impl Prim {
    pub(crate) fn of(ty: &Type) -> Option<Prim> {
        Some(match ty {
            Type::U8 => Prim::U8,
            Type::U16 => Prim::U16,
            Type::U32 => Prim::U32,
            Type::U64 => Prim::U64,
            Type::I8 => Prim::I8,
            Type::I16 => Prim::I16,
            Type::I32 => Prim::I32,
            Type::I64 => Prim::I64,
            Type::F32 => Prim::F32,
            Type::F64 => Prim::F64,
            _ => return None,
        })
    }

    pub(crate) fn bytes(self) -> usize {
        match self {
            Prim::U8 | Prim::I8 => 1,
            Prim::U16 | Prim::I16 => 2,
            Prim::U32 | Prim::I32 | Prim::F32 => 4,
            Prim::U64 | Prim::I64 | Prim::F64 => 8,
        }
    }

    pub(crate) fn rust(self) -> TokenStream {
        match self {
            Prim::U8 => quote! { u8 },
            Prim::U16 => quote! { u16 },
            Prim::U32 => quote! { u32 },
            Prim::U64 => quote! { u64 },
            Prim::I8 => quote! { i8 },
            Prim::I16 => quote! { i16 },
            Prim::I32 => quote! { i32 },
            Prim::I64 => quote! { i64 },
            Prim::F32 => quote! { f32 },
            Prim::F64 => quote! { f64 },
        }
    }

    pub(crate) fn c_ty(self) -> &'static str {
        match self {
            Prim::U8 => "uint8_t",
            Prim::U16 => "uint16_t",
            Prim::U32 => "uint32_t",
            Prim::U64 => "uint64_t",
            Prim::I8 => "int8_t",
            Prim::I16 => "int16_t",
            Prim::I32 => "int32_t",
            Prim::I64 => "int64_t",
            Prim::F32 => "float",
            Prim::F64 => "double",
        }
    }

    /// C expression reading the value at `ptr` (C text).
    pub(crate) fn c_load(self, ptr: &str) -> String {
        match self {
            Prim::U8 => format!("sw_fixed_u8({ptr})"),
            Prim::U16 => format!("sw_fixed_u16({ptr})"),
            Prim::U32 => format!("sw_fixed_u32({ptr})"),
            Prim::U64 => format!("sw_fixed_u64({ptr})"),
            Prim::I8 => format!("(int8_t)sw_fixed_u8({ptr})"),
            Prim::I16 => format!("(int16_t)sw_fixed_u16({ptr})"),
            Prim::I32 => format!("(int32_t)sw_fixed_u32({ptr})"),
            Prim::I64 => format!("(int64_t)sw_fixed_u64({ptr})"),
            Prim::F32 => format!("sw_fixed_f32({ptr})"),
            Prim::F64 => format!("sw_fixed_f64({ptr})"),
        }
    }
}

/// A bit field.
#[derive(Clone, Debug)]
pub(crate) enum Bits {
    Bool,
    Nibble,
    /// `U1`..`U32`, its type name.
    Unsigned(Ident),
    /// `I2`..`I32`, its type name.
    Signed(Ident),
}

#[derive(Clone, Debug)]
pub(crate) enum Kind {
    Prim(Prim),
    Array(ArrayLen, Prim),
    /// Another fixed-layout type, `FixedLayout` is checked by the compiler.
    Nested(TokenStream),
    Bits(u32, Bits),
    /// The presence flag of the `Option` (or `Result`) field of this name: one bit.
    Flag(Ident),
    Option(Box<Kind>),
    Result(Box<Kind>, Box<Kind>),
}

impl Kind {
    fn is_bits(&self) -> bool {
        matches!(self, Kind::Bits(..) | Kind::Flag(_))
    }
}

/// How `ty` is laid out, or why it has no fixed layout and what to use instead.
pub(crate) fn classify(ty: &Type) -> Result<Kind, String> {
    if let Some(prim) = Prim::of(ty) {
        return Ok(Kind::Prim(prim));
    }
    let why = match ty {
        Type::Array(len, inner) => {
            return match Prim::of(inner) {
                Some(prim) => Ok(Kind::Array(len.clone(), prim)),
                None => Err(
                    "an array of bit fields or of other types has no fixed layout yet. Use [u8; N] (or u16..u64), \
                     or a fixed_layout struct per element written out as fields"
                        .into(),
                ),
            };
        }
        Type::Bool => return Ok(Kind::Bits(1, Bits::Bool)),
        Type::Nibble => return Ok(Kind::Bits(4, Bits::Nibble)),
        Type::IsSome(target) | Type::IsOk(target) => return Ok(Kind::Flag(target.clone())),
        Type::External(path, has_lifetime) => {
            let name = path.segments.last().map(|s| s.to_string()).unwrap_or_default();
            let signed = name.starts_with(['I', 'i']);
            let bits = name
                .strip_prefix(['U', 'u', 'I', 'i'])
                .and_then(|n| n.parse::<u32>().ok());
            match bits {
                Some(n) if path.args.is_empty() && n <= 32 => {
                    let ident = Ident::new(&name, Span::call_site());
                    return Ok(Kind::Bits(
                        n,
                        if signed {
                            Bits::Signed(ident)
                        } else {
                            Bits::Unsigned(ident)
                        },
                    ));
                }
                Some(_) if path.args.is_empty() => format!(
                    "`{name}` is wider than 32 bits, the bit fields of the fast path are read through a 32-bit \
                     window. Use u64"
                ),
                _ if *has_lifetime => format!(
                    "`{name}` borrows from the buffer; a fixed layout holds plain values only. Use [u8; N] for bytes"
                ),
                _ => return Ok(Kind::Nested(path.tokens(None))),
            }
        }
        Type::Option(..) | Type::Result(..) => {
            let payloads: Vec<&Type> = match ty {
                Type::Option(_, inner_ty) => vec![inner_ty],
                Type::Result(_, ok_err) => vec![&ok_err.0, &ok_err.1],
                _ => unreachable!(),
            };
            let mut kinds = Vec::new();
            for payload in payloads {
                let kind = classify(payload)?;
                if kind.is_bits() || matches!(kind, Kind::Option(_) | Kind::Result(..)) {
                    return Err(
                        "the payload of an Option or Result must start on a byte, as it does when it is a number, \
                         an array or a fixed_layout type. For a bit field, use a U2 (or two bools): one bit for \
                         the flag, one for the value"
                            .into(),
                    );
                }
                kinds.push(kind);
            }
            let mut kinds = kinds.into_iter();
            let first = Box::new(kinds.next().expect("one payload"));
            return Ok(match kinds.next() {
                Some(err) => Kind::Result(first, Box::new(err)),
                None => Kind::Option(first),
            });
        }
        Type::U128 | Type::I128 => {
            "128-bit numbers are not supported by the fast path (no 128-bit loads on small CPUs). Use two u64".into()
        }
        Type::UNib32
        | Type::UVlq32
        | Type::UVlq64
        | Type::ULeb32
        | Type::ULeb64
        | Type::ULeb128
        | Type::ILeb32
        | Type::ILeb64
        | Type::ILeb128 => "a variable-length number takes more bytes for larger values, so the fields after it \
                            have no fixed offset. Use u32 / u64, or U1..U32 for fewer bits"
            .into(),
        Type::String | Type::Vec(_) | Type::MaxLen(..) => "a string or list has a length known only at run time, \
                                                           so the fields after it have no fixed offset. Use [u8; N] \
                                                           (or [u16; N]..) with a count field"
            .into(),
        Type::RefBox(_) => "RefBox holds a value of any length. Use the fixed_layout type itself".into(),
        Type::TailSize(_) => {
            "TailSize stores the length of the rest of the value; a fixed layout has a known length \
             (ENCODED_LEN). Remove it"
                .into()
        }
        Type::Tuple(_) => "tuples are not supported by the fast path. Use a fixed_layout struct".into(),
        Type::Range(_) | Type::RangeInclusive(_) => {
            "ranges are not supported by the fast path. Use a fixed_layout struct with start and end".into()
        }
        _ => "this type has no fixed layout".into(),
    };
    Err(why)
}

/// One field to lay out: its name (consts, C, locals), its Rust ident (`None` for a flag), its type, `#[default]`.
pub(crate) struct Item<'a> {
    pub(crate) name: String,
    pub(crate) ident: Option<Ident>,
    pub(crate) ty: &'a Type,
    pub(crate) has_default: bool,
}

pub(crate) fn struct_items(fields: &[Field]) -> Vec<Item<'_>> {
    fields
        .iter()
        .map(|f| {
            let flag = matches!(f.ty, Type::IsSome(_) | Type::IsOk(_));
            let name = match &f.ty {
                Type::IsSome(target) | Type::IsOk(target) => format!("{}_flag", unraw(target)),
                _ => unraw(&f.ident),
            };
            Item {
                name,
                ident: (!flag).then(|| f.ident.clone()),
                ty: &f.ty,
                has_default: f.default.is_some(),
            }
        })
        .collect()
}

pub(crate) fn variant_items(v: &Variant) -> Result<Vec<Item<'_>>, String> {
    Ok(match &v.fields {
        Fields::Named(named) => struct_items(named),
        Fields::Unnamed(unnamed) => {
            if unnamed
                .iter()
                .any(|ty| matches!(ty, Type::IsSome(_) | Type::IsOk(_)))
            {
                return Err(format!(
                    "variant `{}`: an Option or Result in a tuple variant has no name for its flag. Name the \
                     fields of the variant",
                    v.ident
                ));
            }
            unnamed
                .iter()
                .enumerate()
                .map(|(i, ty)| Item {
                    name: format!("{i}"),
                    ident: Some(format_ident!("_{}", i)),
                    ty,
                    has_default: false,
                })
                .collect()
        }
        Fields::Unit => vec![],
    })
}

pub(crate) fn unraw(ident: &Ident) -> String {
    let s = ident.to_string();
    s.strip_prefix("r#").map(str::to_string).unwrap_or(s)
}

/// `PlanHeader` -> `plan_header`.
pub(crate) fn snake(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i != 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

pub(crate) fn lit_str(s: &str) -> LitStr {
    LitStr::new(s, Span::call_site())
}

pub(crate) fn lit(n: usize) -> Literal {
    Literal::usize_unsuffixed(n)
}

/// Where a field starts.
#[derive(Clone)]
pub(crate) enum At {
    /// Byte offset, a const path.
    Byte(TokenStream),
    /// Bit offset from the start of the value, a const path.
    Bit(TokenStream),
}

struct Placed<'a> {
    item: Item<'a>,
    kind: Kind,
    at: At,
    /// Bit fields: the bytes they touch, as const expressions `(first, last)`.
    bytes: Option<(TokenStream, TokenStream)>,
}

/// The laid out fields of a struct or of one enum variant.
struct Layout<'a> {
    placed: Vec<Placed<'a>>,
    consts: Vec<TokenStream>,
    asserts: Vec<TokenStream>,
    /// Bytes of everything before the variable last field, or of the whole value when there is none.
    head_len: TokenStream,
    /// The longest encoding.
    max_len: TokenStream,
    /// The shortest encoding.
    min_len: TokenStream,
    /// Every value has the same length.
    same_len: TokenStream,
    leads_with_bits: TokenStream,
    trails_with_bits: TokenStream,
    /// The last field has a variable length (an Option, a Result, a nested type of variable length).
    tail: bool,
}

/// Lays out `items` from byte `start`; consts are named `{prefix}{FIELD}_..` on `ty_ident`. `None`: no fixed
/// layout and none required.
fn layout<'a>(
    ty_ident: &Ident,
    prefix: &str,
    where_: &str,
    items: Vec<Item<'a>>,
    start: TokenStream,
    required: bool,
    cp: &CratePath,
) -> Result<Option<Layout<'a>>, String> {
    let fixed = cp.item(quote! { fixed_layout::FixedLayout });
    let mut placed = Vec::new();
    let mut consts = Vec::new();
    let mut asserts = Vec::new();
    // the cursor: a byte offset and the bits used after it
    let mut base = start;
    let mut bits = 0usize;
    // a nested type just before, whose last bits a bit field here would share
    let mut after_nested: Option<(TokenStream, String)> = None;
    let mut tail = false;
    let mut head_len = None;
    let n_items = items.len();
    let mut leads_with_bits = quote! { false };
    let mut trails_with_bits = quote! { false };
    let mut tail_max = quote! { 0 };
    let mut tail_min = quote! { 0 };
    let mut tail_same = quote! { true };

    for (i, item) in items.into_iter().enumerate() {
        let name = item.name.clone();
        let fail = |why: String| -> Result<Option<Layout<'a>>, String> {
            if required {
                Err(format!("fixed_layout: {where_}field `{name}`: {why}"))
            } else {
                Ok(None)
            }
        };
        let kind = match classify(item.ty) {
            Ok(kind) => kind,
            Err(why) => return fail(why),
        };
        if item.has_default {
            return fail(
                "#[default] is for evolvable types reading data written before the field existed; a fixed \
                 layout has one shape. Remove it"
                    .into(),
            );
        }
        let has_nested = matches!(&kind, Kind::Nested(_))
            || matches!(&kind, Kind::Option(p) if matches!(**p, Kind::Nested(_)))
            || matches!(&kind, Kind::Result(o, e) if matches!(**o, Kind::Nested(_)) || matches!(**e, Kind::Nested(_)));
        if has_nested && !required {
            // the macro cannot see inside a type defined elsewhere: only with the directive
            return Ok(None);
        }
        let last = i + 1 == n_items;
        let upper = name.to_uppercase();
        if i == 0 {
            leads_with_bits = match &kind {
                k if k.is_bits() => quote! { true },
                Kind::Nested(ty) => quote! { <#ty as #fixed>::LEADS_WITH_BITS },
                _ => quote! { false },
            };
        }
        match &kind {
            Kind::Bits(..) | Kind::Flag(_) => {
                let (n, nibble) = match &kind {
                    Kind::Bits(n, b) => (*n as usize, matches!(b, Bits::Nibble)),
                    _ => (1, false),
                };
                if let Some((ty, prev)) = after_nested.take() {
                    let msg = lit_str(&format!(
                        "fixed_layout: {where_}field `{name}` is a bit field right after `{prev}`, which ends with \
                         bit fields: shrink_wrap would pack them into one byte, where no fixed offset describes \
                         both. Put a byte-aligned field between them, or end `{prev}` with one"
                    ));
                    asserts.push(quote! { const _: () = ::core::assert!(!<#ty as #fixed>::TRAILS_WITH_BITS, #msg); });
                }
                if nibble {
                    bits = bits.div_ceil(4) * 4;
                }
                let off_name = format_ident!("{prefix}{upper}_BIT_OFFSET");
                let bits_name = format_ident!("{prefix}{upper}_BITS");
                let doc = lit_str(&format!(
                    " Bit offset of `{name}`, from the most significant bit of the first byte."
                ));
                let at_bits = lit(bits);
                let n_lit = lit(n);
                consts.push(quote! {
                    #[doc = #doc]
                    pub const #off_name: usize = #base * 8 + #at_bits;
                    #[doc = " Bits of the field."]
                    pub const #bits_name: usize = #n_lit;
                });
                let first = lit(bits / 8);
                let lastb = lit((bits + n - 1) / 8);
                placed.push(Placed {
                    bytes: Some((quote! { #base + #first }, quote! { #base + #lastb })),
                    item,
                    kind,
                    at: At::Bit(quote! { #ty_ident::#off_name }),
                });
                bits += n;
                if last {
                    trails_with_bits = quote! { true };
                }
            }
            Kind::Prim(_) | Kind::Array(..) | Kind::Nested(_) => {
                let off_name = format_ident!("{prefix}{upper}_OFFSET");
                let doc = lit_str(&format!(" Offset of `{name}` in bytes."));
                let skip = lit(bits.div_ceil(8));
                consts.push(quote! { #[doc = #doc] pub const #off_name: usize = #base + #skip; });
                let at = quote! { #ty_ident::#off_name };
                after_nested = None;
                let len = match &kind {
                    Kind::Prim(p) => {
                        let n = p.bytes();
                        quote! { #n }
                    }
                    Kind::Array(len, p) => {
                        let n = p.bytes();
                        quote! { ((#len) * #n) }
                    }
                    Kind::Nested(ty) => {
                        if !bits.is_multiple_of(8) {
                            let msg = lit_str(&format!(
                                "fixed_layout: {where_}field `{name}` starts with bit fields after bit fields of \
                                 this type: shrink_wrap would pack them into one byte, where no fixed offset \
                                 describes both. Put a byte-aligned field before it, or start its type with one"
                            ));
                            asserts.push(
                                quote! { const _: () = ::core::assert!(!<#ty as #fixed>::LEADS_WITH_BITS, #msg); },
                            );
                        }
                        after_nested = Some((ty.clone(), name.clone()));
                        if last {
                            tail = true;
                            head_len = Some(at.clone());
                            tail_max = quote! { <#ty as #fixed>::ENCODED_LEN };
                            tail_min = quote! { <#ty as #fixed>::MIN_LEN };
                            tail_same = quote! { <#ty as #fixed>::SAME_LEN };
                            trails_with_bits = quote! { <#ty as #fixed>::TRAILS_WITH_BITS };
                        } else {
                            let msg = lit_str(&format!(
                                "fixed_layout: {where_}field `{name}` has a variable length (an enum whose \
                                 variants differ in length, or a type ending with an Option), so the fields after \
                                 it have no fixed offset. Make it the last field"
                            ));
                            asserts.push(quote! { const _: () = ::core::assert!(<#ty as #fixed>::SAME_LEN, #msg); });
                        }
                        quote! { <#ty as #fixed>::ENCODED_LEN }
                    }
                    _ => unreachable!(),
                };
                base = quote! { (#at + #len) };
                bits = 0;
                placed.push(Placed {
                    item,
                    kind,
                    at: At::Byte(at),
                    bytes: None,
                });
            }
            Kind::Option(_) | Kind::Result(..) => {
                if !last {
                    return fail(
                        "an Option or Result is on the wire only as far as its flag says (nothing for None, Ok or \
                         Err of different lengths), so the fields after it have no fixed offset. Make it the last \
                         field, or use a plain value with a separate bool"
                            .into(),
                    );
                }
                let off_name = format_ident!("{prefix}{upper}_OFFSET");
                let doc = lit_str(&format!(
                    " Offset of the payload of `{name}` in bytes, when its flag says it is there."
                ));
                let skip = lit(bits.div_ceil(8));
                consts.push(quote! { #[doc = #doc] pub const #off_name: usize = #base + #skip; });
                let at = quote! { #ty_ident::#off_name };
                tail = true;
                head_len = Some(at.clone());
                let (max, min) = match &kind {
                    Kind::Option(p) => (payload_max(p, cp), quote! { 0 }),
                    Kind::Result(o, e) => {
                        let (om, em) = (payload_max(o, cp), payload_max(e, cp));
                        let (on, en) = (payload_min(o, cp), payload_min(e, cp));
                        (
                            quote! { if #om > #em { #om } else { #em } },
                            quote! { if #on < #en { #on } else { #en } },
                        )
                    }
                    _ => unreachable!(),
                };
                tail_max = max;
                tail_min = min;
                tail_same = quote! { false };
                trails_with_bits = if !bits.is_multiple_of(8) {
                    quote! { true }
                } else {
                    quote! { false }
                };
                placed.push(Placed {
                    item,
                    kind,
                    at: At::Byte(at),
                    bytes: None,
                });
            }
        }
    }
    let (head_len, max_len, min_len, same_len) = if tail {
        let head = head_len.expect("a tail has a start");
        (
            head.clone(),
            quote! { #head + #tail_max },
            quote! { #head + #tail_min },
            tail_same,
        )
    } else {
        let end = lit(bits.div_ceil(8));
        let len = quote! { #base + #end };
        (len.clone(), len.clone(), len, quote! { true })
    };
    Ok(Some(Layout {
        placed,
        consts,
        asserts,
        head_len,
        max_len,
        min_len,
        same_len,
        leads_with_bits,
        trails_with_bits,
        tail,
    }))
}

fn payload_max(kind: &Kind, cp: &CratePath) -> TokenStream {
    let fixed = cp.item(quote! { fixed_layout::FixedLayout });
    match kind {
        Kind::Prim(p) => {
            let n = p.bytes();
            quote! { #n }
        }
        Kind::Array(len, p) => {
            let n = p.bytes();
            quote! { ((#len) * #n) }
        }
        Kind::Nested(ty) => quote! { <#ty as #fixed>::ENCODED_LEN },
        _ => unreachable!("payloads are byte-aligned"),
    }
}

fn payload_min(kind: &Kind, cp: &CratePath) -> TokenStream {
    let fixed = cp.item(quote! { fixed_layout::FixedLayout });
    match kind {
        Kind::Nested(ty) => quote! { <#ty as #fixed>::MIN_LEN },
        _ => payload_max(kind, cp),
    }
}

/// Local holding a field's value while reading.
pub(crate) fn local(name: &str) -> Ident {
    format_ident!("__f_{}", name)
}

/// Local holding a flag while reading, by the name of its Option or Result field.
pub(crate) fn flag_local(target: &str) -> Ident {
    format_ident!("__flag_{}", target)
}

/// Reads a byte-aligned payload at `off` of the whole buffer `buf`, checking its length.
fn read_payload(kind: &Kind, off: &TokenStream, cp: &CratePath) -> TokenStream {
    let take_at = cp.item(quote! { fixed_layout::take_at });
    let fixed = cp.item(quote! { fixed_layout::FixedLayout });
    let error = cp.error();
    match kind {
        Kind::Prim(p) => {
            let n = p.bytes();
            let ty = p.rust();
            quote! { <#ty>::from_le_bytes(#take_at::<#n>(buf, #off)?) }
        }
        Kind::Array(len, Prim::U8) => quote! { #take_at::<{ #len }>(buf, #off)? },
        Kind::Array(len, p) => {
            let n = p.bytes();
            let ty = p.rust();
            let take = cp.item(quote! { fixed_layout::take });
            quote! {{
                let raw = #take_at::<{ (#len) * #n }>(buf, #off)?;
                ::core::array::from_fn(|i| <#ty>::from_le_bytes(#take::<#n>(&raw, i * #n)))
            }}
        }
        Kind::Nested(ty) => quote! {
            <#ty as #fixed>::read_fixed(buf.get(#off..).ok_or(#error::OutOfBoundsReadRawSlice)?)?
        },
        _ => unreachable!("payloads are byte-aligned"),
    }
}

/// Writes a byte-aligned payload `v` (a reference) at `off` of `b`, which holds the whole value.
fn write_payload(kind: &Kind, v: &TokenStream, off: &TokenStream, cp: &CratePath) -> TokenStream {
    let fixed = cp.item(quote! { fixed_layout::FixedLayout });
    match kind {
        Kind::Prim(p) => {
            let n = p.bytes();
            quote! { b[#off..#off + #n].copy_from_slice(&(*#v).to_le_bytes()); }
        }
        Kind::Array(len, Prim::U8) => {
            quote! { b[#off..#off + (#len)].copy_from_slice(&(*#v)[..]); }
        }
        Kind::Array(_, p) => {
            let n = p.bytes();
            quote! {
                for (i, x) in (*#v).iter().enumerate() {
                    b[#off + i * #n..#off + (i + 1) * #n].copy_from_slice(&x.to_le_bytes());
                }
            }
        }
        Kind::Nested(ty) => quote! { <#ty as #fixed>::write_fixed(#v, &mut b[#off..])?; },
        _ => unreachable!("payloads are byte-aligned"),
    }
}

/// Length of a byte-aligned payload `v` (a reference).
fn payload_len(kind: &Kind, v: &TokenStream, cp: &CratePath) -> TokenStream {
    let fixed = cp.item(quote! { fixed_layout::FixedLayout });
    match kind {
        Kind::Nested(ty) => quote! { <#ty as #fixed>::fixed_len(#v) },
        other => payload_max(other, cp),
    }
}

/// Reads a number, array or bit field (`kind` at `at`) from `b` into the local `local`, flags into their flag
/// local; `None` for other kinds.
pub(crate) fn read_simple(
    kind: &Kind,
    at: &At,
    local: &Ident,
    cp: &CratePath,
) -> Option<TokenStream> {
    let take = cp.item(quote! { fixed_layout::take });
    let get_bits = cp.item(quote! { fixed_layout::get_bits });
    let nibble = cp.item(quote! { Nibble });
    Some(match (kind, at) {
        (Kind::Prim(prim), At::Byte(off)) => {
            let n = prim.bytes();
            let ty = prim.rust();
            let le = cp.item(quote! { fixed_layout::Le });
            let at = cp.item(quote! { fixed_layout::at });
            quote! { let #local = <#ty as #le>::ld(#at::<#n>(b, #off)); }
        }
        (Kind::Array(len, Prim::U8), At::Byte(off)) => {
            quote! { let #local = #take::<{ #len }>(b, #off); }
        }
        (Kind::Array(_, prim), At::Byte(off)) => {
            let n = prim.bytes();
            let ty = prim.rust();
            let le = cp.item(quote! { fixed_layout::Le });
            let at = cp.item(quote! { fixed_layout::at });
            quote! {
                let #local = ::core::array::from_fn(|i| <#ty as #le>::ld(#at::<#n>(b, #off + i * #n)));
            }
        }
        (Kind::Bits(n, bits), At::Bit(off)) => {
            let n = *n;
            let raw = quote! { #get_bits(b, #off, #n) };
            let value = match bits {
                Bits::Bool => quote! { #raw != 0 },
                Bits::Nibble => quote! { #nibble::new_masked(#raw as u8) },
                Bits::Unsigned(ty) | Bits::Signed(ty) => {
                    quote! { #ty::__from_wire(#raw as u64) }
                }
            };
            quote! { let #local = #value; }
        }
        (Kind::Flag(target), At::Bit(off)) => {
            let flag = flag_local(&unraw(target));
            quote! { let #flag = #get_bits(b, #off, 1) != 0; }
        }
        _ => return None,
    })
}

/// The reads of a layout: `let` statements, one local per field, from `b` (the head, an array) and `buf` (all).
fn reads(layout: &Layout, cp: &CratePath) -> Vec<TokenStream> {
    let fixed = cp.item(quote! { fixed_layout::FixedLayout });
    let error = cp.error();
    let mut out = Vec::new();
    for p in &layout.placed {
        let local = local(&p.item.name);
        if let Some(stmt) = read_simple(&p.kind, &p.at, &local, cp) {
            out.push(stmt);
            continue;
        }
        let stmt = match (&p.kind, &p.at) {
            (Kind::Nested(ty), At::Byte(off)) if layout.tail && is_last(layout, p) => quote! {
                let #local = <#ty as #fixed>::read_fixed(buf.get(#off..).ok_or(#error::OutOfBoundsReadRawSlice)?)?;
            },
            (Kind::Nested(ty), At::Byte(off)) => {
                quote! { let #local = <#ty as #fixed>::read_fixed(&b[#off..])?; }
            }
            (Kind::Option(payload), At::Byte(off)) => {
                let flag = flag_local(&p.item.name);
                let read = read_payload(payload, off, cp);
                quote! { let #local = if #flag { Some(#read) } else { None }; }
            }
            (Kind::Result(ok, err), At::Byte(off)) => {
                let flag = flag_local(&p.item.name);
                let (ok, err) = (read_payload(ok, off, cp), read_payload(err, off, cp));
                quote! { let #local = if #flag { Ok(#ok) } else { Err(#err) }; }
            }
            _ => unreachable!("bit fields have bit offsets, the rest byte offsets"),
        };
        out.push(stmt);
    }
    out
}

fn is_last(layout: &Layout, p: &Placed) -> bool {
    layout.placed.last().is_some_and(|l| core::ptr::eq(l, p))
}

/// The field values for the constructor: `ident: local` (named) or `local` (tuple).
fn construct_fields(layout: &Layout, named: bool) -> Vec<TokenStream> {
    layout
        .placed
        .iter()
        .filter_map(|p| {
            let ident = p.item.ident.as_ref()?;
            let local = local(&p.item.name);
            Some(if named {
                quote! { #ident: #local }
            } else {
                quote! { #local }
            })
        })
        .collect()
}

/// Reference to the field of this name, given how fields are reached (`self.x` or a binding).
type Access<'f> = &'f dyn Fn(&str) -> TokenStream;

/// Writes a number, array or bit field (`kind` at `at`, the value a reference `acc`) into `b`, whose bit field
/// bytes are cleared; `None` for other kinds (flags too: they need the field they are for).
pub(crate) fn write_simple(
    kind: &Kind,
    at: &At,
    acc: &TokenStream,
    cp: &CratePath,
) -> Option<TokenStream> {
    let put_bits = cp.item(quote! { fixed_layout::put_bits });
    Some(match (kind, at) {
        (Kind::Prim(prim), At::Byte(off)) => {
            let n = prim.bytes();
            let le = cp.item(quote! { fixed_layout::Le });
            let at_mut = cp.item(quote! { fixed_layout::at_mut });
            quote! { #le::st(*#acc, #at_mut::<#n>(b, #off)); }
        }
        (Kind::Array(len, Prim::U8), At::Byte(off)) => {
            quote! { b[#off..#off + (#len)].copy_from_slice(&(*#acc)[..]); }
        }
        (Kind::Array(_, prim), At::Byte(off)) => {
            let n = prim.bytes();
            let le = cp.item(quote! { fixed_layout::Le });
            let at_mut = cp.item(quote! { fixed_layout::at_mut });
            quote! {
                for (i, v) in (*#acc).iter().enumerate() {
                    #le::st(*v, #at_mut::<#n>(b, #off + i * #n));
                }
            }
        }
        (Kind::Bits(n, bits), At::Bit(off)) => {
            let n = *n;
            let raw = match bits {
                Bits::Bool => quote! { (*#acc) as u32 },
                Bits::Nibble => quote! { (*#acc).value() as u32 },
                Bits::Unsigned(_) | Bits::Signed(_) => quote! { (*#acc).__to_wire() as u32 },
            };
            quote! { #put_bits(b, #off, #n, #raw); }
        }
        _ => return None,
    })
}

/// The writes of a layout into `b` (the whole value, its length checked), bit field bytes cleared first.
fn writes(layout: &Layout, access: Access, cp: &CratePath) -> Vec<TokenStream> {
    let put_bits = cp.item(quote! { fixed_layout::put_bits });
    let fixed = cp.item(quote! { fixed_layout::FixedLayout });
    let mut out = Vec::new();
    for p in &layout.placed {
        if let Some((first, last)) = &p.bytes {
            // not `..=`: a RangeInclusive index stays a call at opt-level z
            out.push(quote! { b[#first..#last + 1].fill(0); });
        }
    }
    for p in &layout.placed {
        let acc = access(&p.item.name);
        if let Some(stmt) = write_simple(&p.kind, &p.at, &acc, cp) {
            out.push(stmt);
            continue;
        }
        let stmt = match (&p.kind, &p.at) {
            (Kind::Nested(ty), At::Byte(off)) => {
                quote! { <#ty as #fixed>::write_fixed(#acc, &mut b[#off..])?; }
            }
            (Kind::Flag(target), At::Bit(off)) => {
                let target_name = unraw(target);
                let target_acc = access(&target_name);
                let is_option = layout
                    .placed
                    .iter()
                    .any(|t| t.item.name == target_name && matches!(t.kind, Kind::Option(_)));
                let set = if is_option {
                    quote! { (*#target_acc).is_some() }
                } else {
                    quote! { (*#target_acc).is_ok() }
                };
                quote! { #put_bits(b, #off, 1, #set as u32); }
            }
            (Kind::Option(payload), At::Byte(off)) => {
                let write = write_payload(payload, &quote! { v }, off, cp);
                quote! { if let Some(v) = #acc { #write } }
            }
            (Kind::Result(ok, err), At::Byte(off)) => {
                let (ok, err) = (
                    write_payload(ok, &quote! { v }, off, cp),
                    write_payload(err, &quote! { v }, off, cp),
                );
                quote! { match #acc { Ok(v) => { #ok } Err(v) => { #err } } }
            }
            _ => unreachable!("bit fields have bit offsets, the rest byte offsets"),
        };
        out.push(stmt);
    }
    out
}

/// Length of the value: the head, plus what its variable last field takes.
fn len_expr(layout: &Layout, access: Access, cp: &CratePath) -> TokenStream {
    let head = &layout.head_len;
    let Some(last) = layout.placed.last().filter(|_| layout.tail) else {
        return head.clone();
    };
    let acc = access(&last.item.name);
    let tail = match &last.kind {
        Kind::Nested(_) => payload_len(&last.kind, &acc, cp),
        Kind::Option(p) => {
            let len = payload_len(p, &quote! { v }, cp);
            quote! { match #acc { Some(v) => #len, None => 0 } }
        }
        Kind::Result(o, e) => {
            let (ol, el) = (
                payload_len(o, &quote! { v }, cp),
                payload_len(e, &quote! { v }, cp),
            );
            quote! { match #acc { Ok(v) => #ol, Err(v) => #el } }
        }
        _ => unreachable!("only those can be a tail"),
    };
    quote! { #head + #tail }
}

/// `writeln!` calls emitting the C defines and readers of a layout; `c_name` is the snake name of the struct or
/// variant.
fn c_fields(ty_ident: &Ident, prefix: &str, c_name: &str, layout: &Layout) -> Vec<TokenStream> {
    let mut out = Vec::new();
    for p in &layout.placed {
        out.extend(c_field(ty_ident, prefix, c_name, &p.item.name, &p.kind));
    }
    out
}

/// The C defines and reader of the field `name` (`kind`), from the consts `{prefix}{NAME}_OFFSET` (or
/// `_BIT_OFFSET`) of `ty_ident`; `c_name` is the snake name of the struct or variant.
pub(crate) fn c_field(
    ty_ident: &Ident,
    prefix: &str,
    c_name: &str,
    name: &str,
    kind: &Kind,
) -> Vec<TokenStream> {
    let upper = c_name.to_uppercase();
    let mut out = Vec::new();
    {
        let fu = name.to_uppercase();
        let func = format!("{c_name}_{name}");
        match kind {
            Kind::Bits(..) | Kind::Flag(_) => {
                let n = match kind {
                    Kind::Bits(n, _) => *n,
                    _ => 1,
                };
                let off_const = format_ident!("{prefix}{fu}_BIT_OFFSET");
                let define = lit_str(&format!("#define {{}}{upper}_{fu}_BIT_OFFSET {{}}u"));
                let define_n = lit_str(&format!("#define {{}}{upper}_{fu}_BITS {n}u"));
                let signed = matches!(kind, Kind::Bits(_, Bits::Signed(_)));
                let text = if signed {
                    format!(
                        "static inline int32_t {{}}{func}(const uint8_t *p) {{{{ return sw_fixed_sbits(p, {{}}u, {n}u); }}}}"
                    )
                } else {
                    format!(
                        "static inline uint32_t {{}}{func}(const uint8_t *p) {{{{ return sw_fixed_bits(p, {{}}u, {n}u); }}}}"
                    )
                };
                let text = lit_str(&text);
                out.push(quote! {
                    ::core::writeln!(out, #define, up, #ty_ident::#off_const)?;
                    ::core::writeln!(out, #define_n, up)?;
                    ::core::writeln!(out, #text, prefix, #ty_ident::#off_const)?;
                });
            }
            kind => {
                let off_const = format_ident!("{prefix}{fu}_OFFSET");
                let define = lit_str(&format!("#define {{}}{upper}_{fu}_OFFSET {{}}u"));
                out.push(quote! { ::core::writeln!(out, #define, up, #ty_ident::#off_const)?; });
                let payloads: Vec<(String, &Kind)> = match kind {
                    Kind::Option(p) => vec![(func.clone(), p)],
                    Kind::Result(o, e) => {
                        vec![(format!("{func}_ok"), o), (format!("{func}_err"), e)]
                    }
                    other => vec![(func.clone(), other)],
                };
                for (func, kind) in payloads {
                    let text = match kind {
                        Kind::Prim(p) => format!(
                            "static inline {} {{}}{func}(const uint8_t *p) {{{{ return {}; }}}}",
                            p.c_ty(),
                            p.c_load("p + {}u")
                        ),
                        Kind::Array(_, Prim::U8) | Kind::Nested(_) => format!(
                            "static inline const uint8_t *{{}}{func}(const uint8_t *p) {{{{ return p + {{}}u; }}}}"
                        ),
                        Kind::Array(_, p) => format!(
                            "static inline {} {{}}{func}(const uint8_t *p, size_t i) {{{{ return {}; }}}}",
                            p.c_ty(),
                            p.c_load(&format!("p + {{}}u + {}u * i", p.bytes()))
                        ),
                        _ => unreachable!("payloads are byte-aligned"),
                    };
                    let text = lit_str(&text);
                    out.push(
                        quote! { ::core::writeln!(out, #text, prefix, #ty_ident::#off_const)?; },
                    );
                    if let Kind::Array(len, _) = kind {
                        let define = lit_str(&format!("#define {{}}{upper}_{fu}_COUNT {{}}u"));
                        out.push(quote! { ::core::writeln!(out, #define, up, #len)?; });
                    }
                }
            }
        }
    }
    out
}

/// Struct size assumption with `fixed_layout`: `sized` when none is given, `self_describing` when the last field
/// is an Option or Result (`last_is_flagged`).
pub(crate) fn struct_size_assumption(
    size_assumption: &mut Option<ObjectSize>,
    required: bool,
    last_is_flagged: bool,
) -> Result<bool, String> {
    match size_assumption {
        None if required => {
            *size_assumption = Some(if last_is_flagged {
                ObjectSize::SelfDescribing
            } else {
                ObjectSize::sized(0)
            });
            Ok(true)
        }
        Some(ObjectSize::Sized { .. })
        | Some(ObjectSize::UnsizedFinalStructure)
        | Some(ObjectSize::SelfDescribing) => Ok(true),
        _ => Ok(false),
    }
}

/// The fast path of a struct, `None` when it has no fixed layout and none is required.
pub(crate) fn fixed_struct(
    ident: &Ident,
    fields: &[Field],
    kind: StructKind,
    required: bool,
    cp: &CratePath,
) -> Result<Option<TokenStream>, String> {
    let Some(layout) = layout(
        ident,
        "",
        "",
        struct_items(fields),
        quote! { 0 },
        required,
        cp,
    )?
    else {
        return Ok(None);
    };
    let error = cp.error();
    let fixed = cp.item(quote! { fixed_layout::FixedLayout });
    let upper_ty = cp.item(quote! { fixed_layout::Upper });

    let Layout {
        consts,
        asserts,
        head_len,
        max_len,
        min_len,
        same_len,
        leads_with_bits,
        trails_with_bits,
        ..
    } = &layout;
    let reads = reads(&layout, cp);
    let construct = match kind {
        StructKind::Tuple => {
            let fields = construct_fields(&layout, false);
            quote! { #ident ( #(#fields),* ) }
        }
        _ => {
            let fields = construct_fields(&layout, true);
            quote! { #ident { #(#fields),* } }
        }
    };
    let access = |name: &str| {
        let field = kind.member(&format_ident!("{}", name));
        quote! { (&self.#field) }
    };
    let writes = writes(&layout, &access, cp);
    let len = len_expr(&layout, &access, cp);
    let (get_head, get_whole) = if layout.tail {
        (
            quote! { let Some(b) = buf.first_chunk::<{ #ident::HEAD_LEN }>() else {
                return Err(#error::OutOfBoundsReadRawSlice);
            }; },
            quote! {
                let len = #len;
                let Some(b) = buf.get_mut(..len) else {
                    return Err(#error::OutOfBoundsWriteRawSlice);
                };
            },
        )
    } else {
        (
            quote! { let Some(b) = buf.first_chunk::<{ #ident::ENCODED_LEN }>() else {
                return Err(#error::OutOfBoundsReadRawSlice);
            }; },
            quote! {
                let len = #ident::ENCODED_LEN;
                let Some(b) = buf.first_chunk_mut::<{ #ident::ENCODED_LEN }>() else {
                    return Err(#error::OutOfBoundsWriteRawSlice);
                };
            },
        )
    };

    let c_name = snake(&ident.to_string());
    let c_upper = c_name.to_uppercase();
    let c_head = lit_str(&format!("\n/* {ident}: {{}} to {{}} bytes */"));
    let c_len = lit_str(&format!("#define {{}}{c_upper}_LEN {{}}u"));
    let c_min = lit_str(&format!("#define {{}}{c_upper}_MIN_LEN {{}}u"));
    let c_fields = c_fields(ident, "", &c_name, &layout);
    let doc_len = lit_str(&format!(
        " Longest encoding of `{ident}` in bytes: the buffer that holds every value. One value takes \
         [fixed_len](Self::fixed_len) bytes, the same unless the last field has a variable length."
    ));

    let ht_impl = fixed_head_tail(ident, cp);
    Ok(Some(quote! {
        #(#asserts)*

        impl #ident {
            #(#consts)*
            #[doc = #doc_len]
            pub const ENCODED_LEN: usize = #max_len;
            /// Shortest encoding in bytes.
            pub const MIN_LEN: usize = #min_len;
            /// Every value has the length [Self::ENCODED_LEN].
            pub const SAME_LEN: bool = #same_len;
            #[doc(hidden)]
            pub const HEAD_LEN: usize = #head_len;

            /// Reads the value at the start of `buf`, without a `BufReader`: the same bytes
            /// `des_shrink_wrap` reads.
            #[inline]
            pub fn from_fixed(buf: &[u8]) -> Result<Self, #error> {
                #get_head
                #(#reads)*
                Ok(#construct)
            }

            /// Writes the value at the start of `buf`, without a `BufWriter`: the same bytes
            /// `ser_shrink_wrap` writes. Returns the length written, [Self::fixed_len].
            #[inline]
            pub fn to_fixed(&self, buf: &mut [u8]) -> Result<usize, #error> {
                #get_whole
                #(#writes)*
                Ok(len)
            }

            /// Length of this value on the wire, in bytes.
            #[inline]
            pub fn fixed_len(&self) -> usize {
                #len
            }
        }

        impl #fixed for #ident {
            const ENCODED_LEN: usize = #ident::ENCODED_LEN;
            const MIN_LEN: usize = #ident::MIN_LEN;
            const SAME_LEN: bool = #ident::SAME_LEN;
            const LEADS_WITH_BITS: bool = #leads_with_bits;
            const TRAILS_WITH_BITS: bool = #trails_with_bits;

            #[inline]
            fn read_fixed(buf: &[u8]) -> Result<Self, #error> {
                #ident::from_fixed(buf)
            }

            #[inline]
            fn write_fixed(&self, buf: &mut [u8]) -> Result<usize, #error> {
                self.to_fixed(buf)
            }

            fn fixed_len(&self) -> usize {
                #ident::fixed_len(self)
            }

            fn write_c(prefix: &str, out: &mut dyn ::core::fmt::Write) -> ::core::fmt::Result {
                let up = #upper_ty(prefix);
                ::core::writeln!(out, #c_head, #ident::MIN_LEN, #ident::ENCODED_LEN)?;
                ::core::writeln!(out, #c_len, up, #ident::ENCODED_LEN)?;
                ::core::writeln!(out, #c_min, up, #ident::MIN_LEN)?;
                #(#c_fields)*
                Ok(())
            }
        }

        #ht_impl
    }))
}

/// Byte width of a byte-aligned enum discriminant.
pub(crate) fn repr_bytes(repr: Repr) -> Option<(usize, Prim)> {
    match repr {
        Repr::U8 => Some((1, Prim::U8)),
        Repr::U16 => Some((2, Prim::U16)),
        Repr::U32 => Some((4, Prim::U32)),
        _ => None,
    }
}

/// Whether the enum can have a fixed layout at all, or why not (when required).
pub(crate) fn enum_eligible(
    size_assumption: &Option<ObjectSize>,
    repr: Repr,
    variants: &[Variant],
    has_future_version: bool,
    required: bool,
) -> Result<bool, String> {
    let fail = |why: &str| {
        if required {
            Err(format!("fixed_layout: {why}"))
        } else {
            Ok(false)
        }
    };
    if !matches!(
        size_assumption,
        Some(ObjectSize::Sized { .. }) | Some(ObjectSize::SelfDescribing)
    ) {
        return fail(
            "an enum must be sized (every variant the same length) or self_describing (the tag tells the \
             length); an evolvable enum may grow",
        );
    }
    if has_future_version {
        return fail(
            "a #[future_version] variant holds a discriminant of a newer version, whose length this one cannot \
             know. Remove it",
        );
    }
    if repr_bytes(repr).is_none() {
        return fail(
            "the tag must be byte-aligned so the variant's fields start on a byte: use ww_repr = u8, u16 or u32",
        );
    }
    if let Some(v) = variants.iter().find(|v| !v.cfg.is_empty()) {
        return fail(&format!(
            "variant `{}` has a #[cfg]: the layout would depend on the features. Remove the #[cfg]",
            v.ident
        ));
    }
    Ok(true)
}

/// The fast path of an enum, `None` when it has no fixed layout and none is required.
pub(crate) fn fixed_enum(
    ident: &Ident,
    repr: Repr,
    variants: &[Variant],
    required: bool,
    cp: &CratePath,
) -> Result<Option<TokenStream>, String> {
    let Some((tag_len, tag_prim)) = repr_bytes(repr) else {
        return Ok(None);
    };
    let mut layouts = Vec::new();
    for v in variants {
        let vu = snake(&v.ident.to_string()).to_uppercase();
        let items = match variant_items(v) {
            Ok(items) => items,
            Err(e) if required => return Err(format!("fixed_layout: {e}")),
            Err(_) => return Ok(None),
        };
        let where_ = format!("variant `{}`, ", v.ident);
        match layout(
            ident,
            &format!("{vu}_"),
            &where_,
            items,
            quote! { #tag_len },
            required,
            cp,
        )? {
            Some(layout) => layouts.push((v, layout)),
            None => return Ok(None),
        }
    }

    let error = cp.error();
    let fixed = cp.item(quote! { fixed_layout::FixedLayout });
    let upper_ty = cp.item(quote! { fixed_layout::Upper });
    let tag_ty = tag_prim.rust();
    let c_name = snake(&ident.to_string());
    let c_upper = c_name.to_uppercase();

    let mut defs = Vec::new();
    let mut asserts = Vec::new();
    let mut max_names = Vec::new();
    let mut min_names = Vec::new();
    let mut same_names = Vec::new();
    let mut trails = Vec::new();
    let mut read_arms = Vec::new();
    let mut write_arms = Vec::new();
    let mut len_arms = Vec::new();
    let mut c_text = Vec::new();
    let mut c_len_cases = Vec::new();
    let tag_tl = lit_str(&format!("#define {{}}{c_upper}_TAG_LEN {{}}u"));
    c_text.push(quote! { ::core::writeln!(out, #tag_tl, up, #tag_len)?; });
    let tag_fn = lit_str(&format!(
        "static inline {} {{}}{c_name}_tag(const uint8_t *p) {{{{ return {}; }}}}",
        tag_prim.c_ty(),
        tag_prim.c_load("p")
    ));
    c_text.push(quote! { ::core::writeln!(out, #tag_fn, prefix)?; });

    for (v, layout) in &layouts {
        let vs = snake(&v.ident.to_string());
        let vu = vs.to_uppercase();
        let tag_name = format_ident!("{}_TAG", vu);
        let len_name = format_ident!("{}_LEN", vu);
        let min_name = format_ident!("{}_MIN_LEN", vu);
        let head_name = format_ident!("{}_HEAD_LEN", vu);
        let same_name = format_ident!("{}_SAME_LEN", vu);
        let disc = Literal::u32_unsuffixed(v.discriminant);
        let Layout {
            consts,
            asserts: a,
            head_len,
            max_len,
            min_len,
            same_len,
            trails_with_bits,
            ..
        } = layout;
        asserts.extend(a.iter().cloned());
        let doc_tag = lit_str(&format!(" Discriminant of `{}`.", v.ident));
        let doc_len = lit_str(&format!(
            " Longest encoding of `{}` in bytes, tag included.",
            v.ident
        ));
        defs.push(quote! {
            #[doc = #doc_tag]
            pub const #tag_name: #tag_ty = #disc;
            #[doc = #doc_len]
            pub const #len_name: usize = #max_len;
            #[doc(hidden)]
            pub const #min_name: usize = #min_len;
            #[doc(hidden)]
            pub const #head_name: usize = #head_len;
            #[doc(hidden)]
            pub const #same_name: bool = #same_len;
            #(#consts)*
        });
        max_names.push(len_name.clone());
        min_names.push(min_name);
        same_names.push(same_name);
        trails.push(trails_with_bits.clone());

        let v_ident = &v.ident;
        let named = matches!(v.fields, Fields::Named(_));
        let bindings: Vec<(String, Ident)> = layout
            .placed
            .iter()
            .filter_map(|p| {
                p.item
                    .ident
                    .as_ref()
                    .map(|i| (p.item.name.clone(), format_ident!("__b_{}", unraw(i))))
            })
            .collect();
        let access = |name: &str| {
            let b = bindings
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, b)| b.clone())
                .unwrap_or_else(|| format_ident!("__b_{}", name));
            quote! { #b }
        };
        let field_idents: Vec<&Ident> = layout
            .placed
            .iter()
            .filter_map(|p| p.item.ident.as_ref())
            .collect();
        let binding_idents: Vec<&Ident> = bindings.iter().map(|(_, b)| b).collect();
        let pattern = match &v.fields {
            Fields::Named(_) => quote! { #ident::#v_ident { #(#field_idents: #binding_idents),* } },
            Fields::Unnamed(_) => quote! { #ident::#v_ident ( #(#binding_idents),* ) },
            Fields::Unit => quote! { #ident::#v_ident },
        };
        let construct = construct_fields(layout, named);
        let construct = match &v.fields {
            Fields::Named(_) => quote! { #ident::#v_ident { #(#construct),* } },
            Fields::Unnamed(_) => quote! { #ident::#v_ident ( #(#construct),* ) },
            Fields::Unit => quote! { #ident::#v_ident },
        };
        let reads = reads(layout, cp);
        let writes = writes(layout, &access, cp);
        let len = len_expr(layout, &access, cp);
        let unused_b = if layout.placed.is_empty() {
            quote! { let _ = b; }
        } else {
            quote! {}
        };
        let unused_buf = if layout.tail {
            quote! {}
        } else {
            quote! { let _ = &buf; }
        };
        read_arms.push(quote! {
            #disc => {
                let Some(b) = buf.first_chunk::<{ #ident::#head_name }>() else {
                    return Err(#error::OutOfBoundsReadRawSlice);
                };
                #unused_b
                #unused_buf
                #(#reads)*
                Ok(#construct)
            }
        });
        write_arms.push(quote! {
            #pattern => {
                let len = #len;
                let Some(b) = buf.get_mut(..len) else {
                    return Err(#error::OutOfBoundsWriteRawSlice);
                };
                b[..#tag_len].copy_from_slice(&#ident::#tag_name.to_le_bytes());
                #(#writes)*
                Ok(len)
            }
        });
        len_arms.push(quote! { #pattern => #len, });

        let c_tag = lit_str(&format!("#define {{}}{c_upper}_{vu} {{}}u"));
        let c_len = lit_str(&format!("#define {{}}{c_upper}_{vu}_LEN {{}}u"));
        c_text.push(quote! {
            ::core::writeln!(out, #c_tag, up, #ident::#tag_name)?;
            ::core::writeln!(out, #c_len, up, #ident::#len_name)?;
        });
        c_text.extend(c_fields(
            ident,
            &format!("{vu}_"),
            &format!("{c_name}_{vs}"),
            layout,
        ));
        let case = lit_str("    case {}u: return {}u;");
        c_len_cases
            .push(quote! { ::core::writeln!(out, #case, #ident::#tag_name, #ident::#head_name)?; });
    }

    let c_head = lit_str(&format!(
        "\n/* {ident}: a {tag_len}-byte tag, then the fields of its variant */"
    ));
    let c_len_fn = lit_str(&format!(
        "/* length of the value with this tag before a variable last field (the whole value when there is none), \
         0 for an unknown tag */\nstatic inline size_t {{}}{c_name}_len(uint32_t tag) {{{{\n    switch (tag) {{{{"
    ));
    let c_len_end = lit_str("    default: return 0u;\n    }}\n}}");
    let first_max = &max_names[0];
    let rest_max = &max_names[1..];
    let first_min = &min_names[0];
    let rest_min = &min_names[1..];

    let ht_impl = fixed_head_tail(ident, cp);
    Ok(Some(quote! {
        #(#asserts)*

        impl #ident {
            /// Bytes of the discriminant.
            pub const TAG_LEN: usize = #tag_len;
            #(#defs)*
            /// Longest encoding in bytes: the buffer that holds every value. One value takes
            /// [fixed_len](Self::fixed_len) bytes.
            pub const ENCODED_LEN: usize = {
                let mut max = #ident::#first_max;
                #(if #ident::#rest_max > max { max = #ident::#rest_max; })*
                max
            };
            /// Shortest encoding in bytes.
            pub const MIN_LEN: usize = {
                let mut min = #ident::#first_min;
                #(if #ident::#rest_min < min { min = #ident::#rest_min; })*
                min
            };
            /// Every value has the length [Self::ENCODED_LEN].
            pub const SAME_LEN: bool = true #(&& #ident::#same_names)* #(&& #ident::#rest_max == #ident::#first_max)*;

            /// Reads the value at the start of `buf`, without a `BufReader`: the same bytes
            /// `des_shrink_wrap` reads. Its length is [Self::fixed_len].
            #[inline]
            pub fn from_fixed(buf: &[u8]) -> Result<Self, #error> {
                let Some(tag) = buf.first_chunk::<#tag_len>() else {
                    return Err(#error::OutOfBoundsReadRawSlice);
                };
                match <#tag_ty>::from_le_bytes(*tag) {
                    #(#read_arms)*
                    _ => Err(#error::EnumFutureVersionOrMalformedData),
                }
            }

            /// Writes the value at the start of `buf`, without a `BufWriter`: the same bytes
            /// `ser_shrink_wrap` writes. Returns the length written.
            #[inline]
            pub fn to_fixed(&self, buf: &mut [u8]) -> Result<usize, #error> {
                match self {
                    #(#write_arms)*
                }
            }

            /// Length of this value on the wire, in bytes.
            #[inline]
            pub fn fixed_len(&self) -> usize {
                match self {
                    #(#len_arms)*
                }
            }
        }

        impl #fixed for #ident {
            const ENCODED_LEN: usize = #ident::ENCODED_LEN;
            const MIN_LEN: usize = #ident::MIN_LEN;
            const SAME_LEN: bool = #ident::SAME_LEN;
            const LEADS_WITH_BITS: bool = false;
            const TRAILS_WITH_BITS: bool = false #(|| #trails)*;

            #[inline]
            fn read_fixed(buf: &[u8]) -> Result<Self, #error> {
                #ident::from_fixed(buf)
            }

            #[inline]
            fn write_fixed(&self, buf: &mut [u8]) -> Result<usize, #error> {
                self.to_fixed(buf)
            }

            fn fixed_len(&self) -> usize {
                #ident::fixed_len(self)
            }

            fn write_c(prefix: &str, out: &mut dyn ::core::fmt::Write) -> ::core::fmt::Result {
                let up = #upper_ty(prefix);
                ::core::writeln!(out, #c_head)?;
                #(#c_text)*
                ::core::writeln!(out, #c_len_fn, prefix)?;
                #(#c_len_cases)*
                ::core::writeln!(out, #c_len_end)?;
                Ok(())
            }
        }

        #ht_impl
    }))
}

/// `HeadTail` for a fixed-layout type, so that `head_tail` types can hold it: one of a single length is all head,
/// any other all tail.
fn fixed_head_tail(ident: &Ident, cp: &CratePath) -> TokenStream {
    let error = cp.error();
    let ht = cp.item(quote! { head_tail::HeadTail });
    let tail = cp.item(quote! { head_tail::Tail });
    let tail_wr = cp.item(quote! { head_tail::TailWr });
    let fixed = cp.item(quote! { fixed_layout::FixedLayout });
    let c_name = lit_str(&snake(&ident.to_string()));
    quote! {
        impl<'i> #ht<'i> for #ident {
            const HEAD_LEN: usize = if #ident::SAME_LEN { #ident::ENCODED_LEN } else { 0 };
            const C_NAME: &'static str = #c_name;
            const C_TAIL: bool = false;

            #[inline]
            fn read_ht(head: &'i [u8], tail: &mut #tail<'i>) -> Result<Self, #error> {
                if #ident::SAME_LEN {
                    #ident::from_fixed(head)
                } else {
                    let v = #ident::from_fixed(tail.rest())?;
                    tail.take(v.fixed_len())?;
                    Ok(v)
                }
            }

            #[inline]
            fn write_ht(&self, head: &mut [u8], tail: &mut #tail_wr) -> Result<(), #error> {
                if #ident::SAME_LEN {
                    self.to_fixed(head)?;
                } else {
                    self.to_fixed(tail.put(self.fixed_len())?)?;
                }
                Ok(())
            }

            #[inline]
            fn tail_len(&self) -> usize {
                if #ident::SAME_LEN { 0 } else { self.fixed_len() }
            }

            type View = Self;

            #[inline]
            fn view_ht(head: &'i [u8], tail: &mut #tail<'i>) -> Result<Self, #error> {
                <Self as #ht<'i>>::read_ht(head, tail)
            }

            fn write_c_ht(prefix: &str, out: &mut dyn ::core::fmt::Write) -> ::core::fmt::Result {
                <#ident as #fixed>::write_c(prefix, out)
            }
        }
    }
}
