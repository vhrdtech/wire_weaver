//! The head+tail encoding (SW-46): every fixed-width field at a constant offset in a head, the variable parts
//! one after another in a tail. What it is and why is in the docs of `shrink_wrap::head_tail`.
//!
//! The head is walked with the cursor of the fixed layout (a const byte expression and the bits used after it).
//! A list or string puts its `u16` count there, a nested type its own head (byte-aligned), an `Option` or `Result`
//! only its flag (a field of its own already). Reading is two passes over the fields: the head first (loads at
//! constant offsets), then the tail in field order, through the `Tail` cursor.

use crate::ast::item_enum::{Fields, Variant};
use crate::ast::item_struct::Field;
use crate::ast::repr::Repr;
use crate::ast::ty::{ArrayLen, Type};
use crate::codegen::fixed_layout::{
    At, Bits, Item, Kind, Prim, c_field, classify as fixed_classify, flag_local, lit, lit_str,
    local, read_simple, repr_bytes, snake, struct_items, unraw, variant_items, write_simple,
};
use crate::codegen::util::CratePath;
use proc_macro2::{Ident, Literal, TokenStream};
use quote::{ToTokens, format_ident, quote};

/// A payload in the tail: of an `Option` or `Result`.
#[derive(Clone)]
enum Payload {
    Prim(Prim),
    Array(ArrayLen, Prim),
    /// A `HeadTail` type: with `'i` for code, with `'static` for consts.
    Nested(TokenStream, TokenStream),
}

#[derive(Clone)]
enum HKind {
    /// A number, an array of numbers, a bit field or a flag: as in the fixed layout.
    Head(Kind),
    /// A `HeadTail` type: its head here, its tail in the tail. With `'i` and with `'static`.
    Nested(TokenStream, TokenStream),
    /// `List<'i, T>`: a `u16` count here, the items in the tail. The item type.
    List(TokenStream),
    /// `&str`: a `u16` count here, the bytes in the tail.
    Str,
    Option(Payload),
    Result(Payload, Payload),
}

/// The type with the lifetime of the value (`'i`), and with `'static` for consts.
fn nested(path: &crate::ast::path::Path, has_lifetime: bool) -> (TokenStream, TokenStream) {
    if has_lifetime {
        (
            path.tokens(Some(quote! { 'i })),
            path.tokens(Some(quote! { 'static })),
        )
    } else {
        (path.tokens(None), path.tokens(None))
    }
}

fn classify_payload(ty: &Type) -> Result<Payload, String> {
    if let Type::External(path, has_lifetime) = ty {
        let name = path
            .segments
            .last()
            .map(|s| s.to_string())
            .unwrap_or_default();
        if name != "List" && !matches!(fixed_classify(ty), Ok(Kind::Bits(..))) {
            let (ty_i, ty_s) = nested(path, *has_lifetime);
            return Ok(Payload::Nested(ty_i, ty_s));
        }
    }
    match fixed_classify(ty) {
        Ok(Kind::Prim(p)) => Ok(Payload::Prim(p)),
        Ok(Kind::Array(len, p)) => Ok(Payload::Array(len, p)),
        _ => Err("the payload of an Option or Result in a head_tail type must be a number, an array of numbers or \
                  a head_tail or fixed_layout type (a bool: use a U2, or two bools)"
            .into()),
    }
}

fn classify(ty: &Type) -> Result<HKind, String> {
    match ty {
        Type::String => Ok(HKind::Str),
        Type::Vec(_) => Err("a list in a head_tail type is a shrink_wrap::head_tail::List<'i, T> (count in the head, \
                             items in the tail), not Vec or RefVec"
            .into()),
        Type::Option(_, inner) => Ok(HKind::Option(classify_payload(inner)?)),
        Type::Result(_, ok_err) => Ok(HKind::Result(
            classify_payload(&ok_err.0)?,
            classify_payload(&ok_err.1)?,
        )),
        Type::External(path, has_lifetime) => {
            let name = path.segments.last().map(|s| s.to_string()).unwrap_or_default();
            if name == "List" {
                let item = path.args.iter().rev().find_map(|a| match a {
                    syn::GenericArgument::Type(t) => Some(t.to_token_stream()),
                    _ => None,
                });
                return match item {
                    Some(item) => Ok(HKind::List(item)),
                    None => Err("expected List<'i, T>".into()),
                };
            }
            match fixed_classify(ty) {
                Ok(kind @ Kind::Bits(..)) => Ok(HKind::Head(kind)),
                Err(why) if !has_lifetime && path.args.is_empty() && why.contains("wider than 32") => Err(why),
                _ => {
                    let (ty_i, ty_s) = nested(path, *has_lifetime);
                    Ok(HKind::Nested(ty_i, ty_s))
                }
            }
        }
        _ => match fixed_classify(ty) {
            Ok(kind @ (Kind::Prim(_) | Kind::Array(..) | Kind::Bits(..) | Kind::Flag(_))) => Ok(HKind::Head(kind)),
            Ok(_) => unreachable!("handled above"),
            Err(why) => Err(why),
        },
    }
}

struct Placed<'a> {
    item: Item<'a>,
    kind: HKind,
    /// Where its head part starts; `None` for an Option or Result (only its flag is in the head).
    at: Option<At>,
    /// Bit fields: the bytes they touch, `(first, last)`.
    bytes: Option<(TokenStream, TokenStream)>,
}

/// The head of a struct or of one enum variant.
struct Head<'a> {
    placed: Vec<Placed<'a>>,
    consts: Vec<TokenStream>,
    asserts: Vec<TokenStream>,
    len: TokenStream,
}

/// Lays out the head of `items`; consts are named `{prefix}{FIELD}_..` on `ty_s` (the type with `'static`).
fn head<'a>(
    ty_s: &TokenStream,
    prefix: &str,
    where_: &str,
    items: Vec<Item<'a>>,
    cp: &CratePath,
) -> Result<Head<'a>, String> {
    let ht = cp.item(quote! { head_tail::HeadTail });
    let fixed = cp.item(quote! { fixed_layout::FixedLayout });
    let mut placed = Vec::new();
    let mut consts = Vec::new();
    let mut asserts = Vec::new();
    let mut base = quote! { 0 };
    let mut bits = 0usize;
    for item in items {
        let name = item.name.clone();
        let kind =
            classify(item.ty).map_err(|why| format!("head_tail: {where_}field `{name}`: {why}"))?;
        if item.has_default {
            return Err(format!(
                "head_tail: {where_}field `{name}`: #[default] is for evolvable types; a head_tail type has one \
                 shape. Remove it"
            ));
        }
        let upper = name.to_uppercase();
        let byte_field = |len: TokenStream,
                          base: &mut TokenStream,
                          bits: &mut usize,
                          consts: &mut Vec<TokenStream>| {
            let off_name = format_ident!("{prefix}{upper}_OFFSET");
            let doc = lit_str(&format!(" Offset of `{name}` in the head, in bytes."));
            let skip = lit(bits.div_ceil(8));
            consts.push(quote! { #[doc = #doc] pub const #off_name: usize = #base + #skip; });
            let at = quote! { <#ty_s>::#off_name };
            *base = quote! { (#at + #len) };
            *bits = 0;
            At::Byte(at)
        };
        let (at, bytes) = match &kind {
            HKind::Head(Kind::Bits(..) | Kind::Flag(_)) => {
                let (n, nibble) = match &kind {
                    HKind::Head(Kind::Bits(n, b)) => (
                        *n as usize,
                        matches!(b, crate::codegen::fixed_layout::Bits::Nibble),
                    ),
                    _ => (1, false),
                };
                if nibble {
                    bits = bits.div_ceil(4) * 4;
                }
                let off_name = format_ident!("{prefix}{upper}_BIT_OFFSET");
                let bits_name = format_ident!("{prefix}{upper}_BITS");
                let doc = lit_str(&format!(
                    " Bit offset of `{name}` in the head, from the most significant bit of its first byte."
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
                let last = lit((bits + n - 1) / 8);
                bits += n;
                (
                    Some(At::Bit(quote! { <#ty_s>::#off_name })),
                    Some((quote! { #base + #first }, quote! { #base + #last })),
                )
            }
            HKind::Head(Kind::Prim(p)) => {
                let n = p.bytes();
                (
                    Some(byte_field(quote! { #n }, &mut base, &mut bits, &mut consts)),
                    None,
                )
            }
            HKind::Head(Kind::Array(len, p)) => {
                let n = p.bytes();
                (
                    Some(byte_field(
                        quote! { ((#len) * #n) },
                        &mut base,
                        &mut bits,
                        &mut consts,
                    )),
                    None,
                )
            }
            HKind::Head(_) => unreachable!("classify gives no other head kinds"),
            HKind::List(item_ty) => {
                let msg = lit_str(&format!(
                    "head_tail: {where_}field `{name}`: the items of a List must have one length (a number, or a \
                     fixed_layout type whose SAME_LEN is true): they are read at i * ENCODED_LEN"
                ));
                asserts.push(
                    quote! { const _: () = ::core::assert!(<#item_ty as #fixed>::SAME_LEN, #msg); },
                );
                (
                    Some(byte_field(quote! { 2 }, &mut base, &mut bits, &mut consts)),
                    None,
                )
            }
            HKind::Str => (
                Some(byte_field(quote! { 2 }, &mut base, &mut bits, &mut consts)),
                None,
            ),
            HKind::Nested(_, ty_s_n) => (
                Some(byte_field(
                    quote! { <#ty_s_n as #ht<'static>>::HEAD_LEN },
                    &mut base,
                    &mut bits,
                    &mut consts,
                )),
                None,
            ),
            HKind::Option(_) | HKind::Result(..) => (None, None),
        };
        placed.push(Placed {
            item,
            kind,
            at,
            bytes,
        });
    }
    let end = lit(bits.div_ceil(8));
    Ok(Head {
        placed,
        consts,
        asserts,
        len: quote! { #base + #end },
    })
}

fn byte_at(p: &Placed) -> TokenStream {
    match &p.at {
        Some(At::Byte(off)) => off.clone(),
        _ => unreachable!("a byte-aligned head part"),
    }
}

/// Local holding the count of a list or string while reading.
fn count_local(name: &str) -> Ident {
    format_ident!("__n_{}", name)
}

fn payload_read(p: &Payload, view: bool, cp: &CratePath) -> TokenStream {
    let ht = cp.item(quote! { head_tail::HeadTail });
    let take = cp.item(quote! { fixed_layout::take });
    match p {
        Payload::Prim(prim) => {
            let n = prim.bytes();
            let ty = prim.rust();
            quote! { <#ty>::from_le_bytes(*tail.take_array::<#n>()?) }
        }
        Payload::Array(len, Prim::U8) => quote! { *tail.take_array::<{ #len }>()? },
        Payload::Array(len, prim) => {
            let n = prim.bytes();
            let ty = prim.rust();
            quote! {{
                let raw = tail.take_array::<{ (#len) * #n }>()?;
                ::core::array::from_fn(|i| <#ty>::from_le_bytes(#take::<#n>(raw, i * #n)))
            }}
        }
        Payload::Nested(ty_i, _) if view => quote! {{
            let h = tail.take(<#ty_i as #ht<'i>>::HEAD_LEN)?;
            <#ty_i as #ht<'i>>::view_ht(h, tail)?
        }},
        Payload::Nested(ty_i, _) => quote! {{
            let h = tail.take(<#ty_i as #ht<'i>>::HEAD_LEN)?;
            <#ty_i as #ht<'i>>::read_ht(h, tail)?
        }},
    }
}

fn payload_write(p: &Payload, v: &TokenStream, cp: &CratePath) -> TokenStream {
    let ht = cp.item(quote! { head_tail::HeadTail });
    match p {
        Payload::Prim(prim) => {
            let n = prim.bytes();
            quote! { tail.put(#n)?.copy_from_slice(&(*#v).to_le_bytes()); }
        }
        Payload::Array(len, Prim::U8) => quote! { tail.put(#len)?.copy_from_slice(&(*#v)[..]); },
        Payload::Array(len, prim) => {
            let n = prim.bytes();
            quote! {
                let out = tail.put((#len) * #n)?;
                for (i, x) in (*#v).iter().enumerate() {
                    out[i * #n..(i + 1) * #n].copy_from_slice(&x.to_le_bytes());
                }
            }
        }
        Payload::Nested(ty_i, _) => quote! {
            let h = tail.put(<#ty_i as #ht<'_>>::HEAD_LEN)?;
            #ht::write_ht(#v, h, tail)?;
        },
    }
}

fn payload_len(p: &Payload, v: &TokenStream, cp: &CratePath) -> TokenStream {
    let ht = cp.item(quote! { head_tail::HeadTail });
    match p {
        Payload::Prim(prim) => {
            let n = prim.bytes();
            quote! { #n }
        }
        Payload::Array(len, prim) => {
            let n = prim.bytes();
            quote! { ((#len) * #n) }
        }
        Payload::Nested(ty_i, _) => quote! { <#ty_i as #ht<'_>>::HEAD_LEN + #ht::tail_len(#v) },
    }
}

/// Reads of a head and its tail: `let` statements, one local per field, from `b` (the head, an array) and `tail`.
/// For a view (`view`), of the head only what the tail needs (counts, flags), and nested types as views.
fn reads(head: &Head, view: bool, cp: &CratePath) -> Vec<TokenStream> {
    let ht = cp.item(quote! { head_tail::HeadTail });
    let le = cp.item(quote! { fixed_layout::Le });
    let at = cp.item(quote! { fixed_layout::at });
    let list = cp.item(quote! { head_tail::List });
    let fixed = cp.item(quote! { fixed_layout::FixedLayout });
    let error = cp.error();
    let mut out = Vec::new();
    // the head
    for p in &head.placed {
        let local = local(&p.item.name);
        match (&p.kind, &p.at) {
            (HKind::Head(kind), Some(at)) => {
                if !view || matches!(kind, Kind::Flag(_)) {
                    out.push(read_simple(kind, at, &local, cp).expect("a head kind"));
                }
            }
            (HKind::List(_) | HKind::Str, _) => {
                let off = byte_at(p);
                let n = count_local(&p.item.name);
                out.push(quote! { let #n = <u16 as #le>::ld(#at::<2>(b, #off)) as usize; });
            }
            _ => {}
        }
    }
    // the tail, in field order
    for p in &head.placed {
        let local = local(&p.item.name);
        let n = count_local(&p.item.name);
        let stmt = match &p.kind {
            HKind::Head(_) => continue,
            HKind::List(item) => quote! {
                let #local = #list::wire(tail.take_items(#n, <#item as #fixed>::ENCODED_LEN)?, #n);
            },
            HKind::Str => quote! {
                let #local = ::core::str::from_utf8(tail.take(#n)?).map_err(|_| #error::MalformedUtf8)?;
            },
            HKind::Nested(ty_i, _) => {
                let off = byte_at(p);
                let f = if view {
                    quote! { view_ht }
                } else {
                    quote! { read_ht }
                };
                quote! { let #local = <#ty_i as #ht<'i>>::#f(&b[#off..], tail)?; }
            }
            HKind::Option(payload) => {
                let flag = flag_local(&p.item.name);
                let read = payload_read(payload, view, cp);
                quote! { let #local = if #flag { Some(#read) } else { None }; }
            }
            HKind::Result(ok, err) => {
                let flag = flag_local(&p.item.name);
                let (ok, err) = (payload_read(ok, view, cp), payload_read(err, view, cp));
                quote! { let #local = if #flag { Ok(#ok) } else { Err(#err) }; }
            }
        };
        out.push(stmt);
    }
    out
}

type Access<'f> = &'f dyn Fn(&str) -> TokenStream;

/// Writes of a head (into `b`, an array) and its tail (through `tail`).
fn writes(head: &Head, access: Access, cp: &CratePath) -> Vec<TokenStream> {
    let ht = cp.item(quote! { head_tail::HeadTail });
    let put_bits = cp.item(quote! { fixed_layout::put_bits });
    let count = cp.item(quote! { head_tail::count });
    let mut out = Vec::new();
    // bit fields are or-ed into their bytes: clear the head once (`b` is the head, an array)
    if head.placed.iter().any(|p| p.bytes.is_some()) {
        out.push(quote! { b.fill(0); });
    }
    for p in &head.placed {
        let acc = access(&p.item.name);
        match (&p.kind, &p.at) {
            (HKind::Head(Kind::Flag(target)), Some(At::Bit(off))) => {
                let target_name = unraw(target);
                let target_acc = access(&target_name);
                let is_option = head
                    .placed
                    .iter()
                    .any(|t| t.item.name == target_name && matches!(t.kind, HKind::Option(_)));
                let set = if is_option {
                    quote! { (*#target_acc).is_some() }
                } else {
                    quote! { (*#target_acc).is_ok() }
                };
                out.push(quote! { #put_bits(b, #off, 1, #set as u32); });
            }
            (HKind::Head(kind), Some(at)) => {
                out.push(write_simple(kind, at, &acc, cp).expect("a head kind"));
            }
            (HKind::List(_) | HKind::Str, _) => {
                let off = byte_at(p);
                out.push(quote! { b[#off..#off + 2].copy_from_slice(&#count((*#acc).len())?); });
            }
            _ => {}
        }
    }
    for p in &head.placed {
        let acc = access(&p.item.name);
        let fixed = cp.item(quote! { fixed_layout::FixedLayout });
        let stmt = match &p.kind {
            HKind::Head(_) => continue,
            HKind::List(item) => quote! {
                (*#acc).write_items(tail.put((*#acc).len() * <#item as #fixed>::ENCODED_LEN)?)?;
            },
            HKind::Str => quote! { tail.put((*#acc).len())?.copy_from_slice((*#acc).as_bytes()); },
            HKind::Nested(ty_i, _) => {
                let off = byte_at(p);
                quote! { <#ty_i as #ht<'_>>::write_ht(#acc, &mut b[#off..], tail)?; }
            }
            HKind::Option(payload) => {
                let write = payload_write(payload, &quote! { v }, cp);
                quote! { if let Some(v) = #acc { #write } }
            }
            HKind::Result(ok, err) => {
                let (ok, err) = (
                    payload_write(ok, &quote! { v }, cp),
                    payload_write(err, &quote! { v }, cp),
                );
                quote! { match #acc { Ok(v) => { #ok } Err(v) => { #err } } }
            }
        };
        out.push(stmt);
    }
    out
}

/// Bytes the tail of the value takes.
fn tail_len(head: &Head, access: Access, cp: &CratePath) -> TokenStream {
    let ht = cp.item(quote! { head_tail::HeadTail });
    let fixed = cp.item(quote! { fixed_layout::FixedLayout });
    let mut parts = vec![quote! { 0 }];
    for p in &head.placed {
        let acc = access(&p.item.name);
        parts.push(match &p.kind {
            HKind::Head(_) => continue,
            HKind::List(item) => quote! { (*#acc).len() * <#item as #fixed>::ENCODED_LEN },
            HKind::Str => quote! { (*#acc).len() },
            HKind::Nested(ty_i, _) => quote! { <#ty_i as #ht<'_>>::tail_len(#acc) },
            HKind::Option(payload) => {
                let len = payload_len(payload, &quote! { v }, cp);
                quote! { match #acc { Some(v) => #len, None => 0 } }
            }
            HKind::Result(ok, err) => {
                let (ok, err) = (
                    payload_len(ok, &quote! { v }, cp),
                    payload_len(err, &quote! { v }, cp),
                );
                quote! { match #acc { Ok(v) => #ok, Err(v) => #err } }
            }
        });
    }
    quote! { #(#parts)+* }
}

fn construct_fields(head: &Head, named: bool) -> Vec<TokenStream> {
    head.placed
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

/// `writeln!(out, fmt, args..)?;` for the C text.
fn cw(fmt: &str, args: &[TokenStream]) -> TokenStream {
    let fmt = lit_str(fmt);
    quote! { ::core::writeln!(out, #fmt #(, #args)*)?; }
}

/// C text of a head: the defines and readers of its fields (`p` points at the head), `c_name` the snake name of
/// the struct or `{enum}_{variant}`.
fn c_head(
    ty_s: &TokenStream,
    prefix: &str,
    c_name: &str,
    head: &Head,
    cp: &CratePath,
) -> Vec<TokenStream> {
    let ht = cp.item(quote! { head_tail::HeadTail });
    let upper = c_name.to_uppercase();
    let ty_ident = format_ident!("__HtSelf");
    let mut out = Vec::new();
    for p in &head.placed {
        let name = &p.item.name;
        let fu = name.to_uppercase();
        let off_const = format_ident!("{prefix}{fu}_OFFSET");
        match &p.kind {
            HKind::Head(kind) => out.extend(c_field(&ty_ident, prefix, c_name, name, kind)),
            HKind::List(_) | HKind::Str => {
                out.push(cw(
                    &format!("#define {{}}{upper}_{fu}_OFFSET {{}}u"),
                    &[quote! { up }, quote! { <#ty_s>::#off_const }],
                ));
                out.push(cw(
                    &format!(
                        "static inline uint16_t {{}}{c_name}_{name}_count(const uint8_t *p) {{{{ return \
                         sw_fixed_u16(p + {{}}u); }}}}"
                    ),
                    &[quote! { prefix }, quote! { <#ty_s>::#off_const }],
                ));
            }
            HKind::Nested(_, ty_n) => {
                out.push(cw(
                    &format!(
                        "/* {name}: a {{}}{{}} head */\n#define {{}}{upper}_{fu}_OFFSET {{}}u"
                    ),
                    &[
                        quote! { prefix },
                        quote! { <#ty_n as #ht<'static>>::C_NAME },
                        quote! { up },
                        quote! { <#ty_s>::#off_const },
                    ],
                ));
                out.push(cw(
                    &format!(
                        "static inline const uint8_t *{{}}{c_name}_{name}(const uint8_t *p) {{{{ return p + {{}}u; }}}}"
                    ),
                    &[quote! { prefix }, quote! { <#ty_s>::#off_const }],
                ));
            }
            HKind::Option(_) | HKind::Result(..) => {}
        }
    }
    // c_field names the consts on `__HtSelf`
    vec![quote! {{
        #[allow(non_camel_case_types)]
        type #ty_ident = #ty_s;
        #(#out)*
    }}]
}

/// Bytes of a payload in C, a Rust expression.
fn c_payload_len(p: &Payload, cp: &CratePath) -> TokenStream {
    let ht = cp.item(quote! { head_tail::HeadTail });
    match p {
        Payload::Prim(prim) => {
            let n = prim.bytes();
            quote! { #n }
        }
        Payload::Array(len, prim) => {
            let n = prim.bytes();
            quote! { ((#len) * #n) }
        }
        Payload::Nested(_, ty_s) => quote! { <#ty_s as #ht<'static>>::HEAD_LEN },
    }
}

/// Members of the C tail struct for the fields of `head`, names starting with `m`.
fn c_members(m: &str, head: &Head, cp: &CratePath) -> Vec<TokenStream> {
    let ht = cp.item(quote! { head_tail::HeadTail });
    let mut out = Vec::new();
    for p in &head.placed {
        let name = &p.item.name;
        let nested_member = |ty_s: &TokenStream, member: String| {
            let text = format!("    {{}}{{}}_tail_t {member};");
            let text = lit_str(&text);
            quote! {
                if <#ty_s as #ht<'static>>::C_TAIL {
                    ::core::writeln!(out, #text, prefix, <#ty_s as #ht<'static>>::C_NAME)?;
                }
            }
        };
        match &p.kind {
            HKind::Head(_) => {}
            HKind::List(_) => out.push(cw(
                &format!("    const uint8_t *{m}{name}; /* the items */"),
                &[],
            )),
            HKind::Str => out.push(cw(
                &format!("    const uint8_t *{m}{name}; /* the bytes, not 0-terminated */"),
                &[],
            )),
            HKind::Nested(_, ty_s) => out.push(nested_member(ty_s, format!("{m}{name}"))),
            HKind::Option(payload) | HKind::Result(payload, _) => {
                let what = match &p.kind {
                    HKind::Result(..) => "the Ok or Err payload, by the flag",
                    _ => "the payload (NULL for None)",
                };
                out.push(cw(
                    &format!("    const uint8_t *{m}{name}; /* {what} */"),
                    &[],
                ));
                let payloads: Vec<&Payload> = match &p.kind {
                    HKind::Result(ok, err) => vec![ok, err],
                    _ => vec![payload],
                };
                for (i, pl) in payloads.into_iter().enumerate() {
                    if let Payload::Nested(_, ty_s) = pl {
                        let suffix = match (&p.kind, i) {
                            (HKind::Result(..), 0) => "_ok_tail",
                            (HKind::Result(..), _) => "_err_tail",
                            _ => "_tail",
                        };
                        out.push(nested_member(ty_s, format!("{m}{name}{suffix}")));
                    }
                }
            }
        }
    }
    out
}

/// Body of the C tail walker for the fields of `head`: `h` points at the head, `m` starts the member names,
/// `c_name` names the head readers.
fn c_walk(h: &str, m: &str, c_name: &str, head: &Head, cp: &CratePath) -> Vec<TokenStream> {
    let ht = cp.item(quote! { head_tail::HeadTail });
    let fixed = cp.item(quote! { fixed_layout::FixedLayout });
    let mut out = Vec::new();
    for p in &head.placed {
        let name = &p.item.name;
        // a nested value's tail: walk it when the type has a walker, else it must be all head
        // `at` is C text with `{}` for each of `at_args`
        let nested_walk = |ty_s: &TokenStream,
                           at: String,
                           at_args: Vec<TokenStream>,
                           member: String| {
            let walk = lit_str(&format!(
                "    if ({{}}{{}}_tail({at}, t, end, &tl->{member}) != 0) {{{{ return -1; }}}}"
            ));
            let err = lit_str(&format!(
                "#error \"{name}: a fixed_layout type of variable length has no C tail walker\""
            ));
            quote! {
                if <#ty_s as #ht<'static>>::C_TAIL {
                    ::core::writeln!(out, #walk, prefix, <#ty_s as #ht<'static>>::C_NAME #(, #at_args)*)?;
                } else if <#ty_s as #ht<'static>>::HEAD_LEN == 0 {
                    ::core::writeln!(out, #err)?;
                }
            }
        };
        match &p.kind {
            HKind::Head(_) => {}
            HKind::List(item) => out.push(cw(
                &format!(
                    "    tl->{m}{name} = sw_ht_take(t, end, (size_t){{}}{c_name}_{name}_count({h}) * {{}}u);\n    \
                     if (tl->{m}{name} == NULL) {{{{ return -1; }}}}"
                ),
                &[quote! { prefix }, quote! { <#item as #fixed>::ENCODED_LEN }],
            )),
            HKind::Str => out.push(cw(
                &format!(
                    "    tl->{m}{name} = sw_ht_take(t, end, (size_t){{}}{c_name}_{name}_count({h}));\n    \
                     if (tl->{m}{name} == NULL) {{{{ return -1; }}}}"
                ),
                &[quote! { prefix }],
            )),
            HKind::Nested(_, ty_s) => {
                let off = byte_at(p);
                out.push(nested_walk(ty_s, format!("{h} + {{}}u"), vec![off], format!("{m}{name}")));
            }
            HKind::Option(payload) | HKind::Result(payload, _) => {
                let flag = format!("{c_name}_{name}_flag");
                let payloads: Vec<(&Payload, &str)> = match &p.kind {
                    HKind::Result(ok, err) => vec![(ok, "_ok_tail"), (err, "_err_tail")],
                    _ => vec![(payload, "_tail")],
                };
                let mut arms = Vec::new();
                for (pl, suffix) in &payloads {
                    let len = c_payload_len(pl, cp);
                    let mut arm = vec![cw(
                        &format!(
                            "        tl->{m}{name} = sw_ht_take(t, end, {{}}u);\n        if (tl->{m}{name} == NULL) \
                             {{{{ return -1; }}}}"
                        ),
                        &[len],
                    )];
                    if let Payload::Nested(_, ty_s) = pl {
                        arm.push(nested_walk(ty_s, format!("tl->{m}{name}"), vec![], format!("{m}{name}{suffix}")));
                    }
                    arms.push(arm);
                }
                let open = lit_str(&format!("    if ({{}}{flag}({h})) {{{{"));
                let first = &arms[0];
                let rest = match arms.get(1) {
                    Some(second) => {
                        let else_ = lit_str("    }} else {{");
                        quote! { ::core::writeln!(out, #else_)?; #(#second)* }
                    }
                    None => {
                        let else_ = cw(&format!("    }}}} else {{{{\n        tl->{m}{name} = NULL;"), &[]);
                        quote! { #else_ }
                    }
                };
                let close = lit_str("    }}");
                out.push(quote! {
                    ::core::writeln!(out, #open, prefix)?;
                    #(#first)*
                    #rest
                    ::core::writeln!(out, #close)?;
                });
            }
        }
    }
    out
}

/// What the derive sees of the type: its ident, with `'i` and with `'static`.
pub(crate) struct Names {
    pub(crate) ident: Ident,
    pub(crate) has_lifetime: bool,
}

impl Names {
    fn ty_i(&self) -> TokenStream {
        let ident = &self.ident;
        if self.has_lifetime {
            quote! { #ident<'i> }
        } else {
            quote! { #ident }
        }
    }

    fn ty_s(&self) -> TokenStream {
        let ident = &self.ident;
        if self.has_lifetime {
            quote! { #ident<'static> }
        } else {
            quote! { #ident }
        }
    }
}

/// The inherent fns every head_tail type gets, on top of `HeadTail`.
fn inherent(names: &Names, cp: &CratePath) -> TokenStream {
    let error = cp.error();
    let ht = cp.item(quote! { head_tail::HeadTail });
    let read = cp.item(quote! { head_tail::read });
    let write = cp.item(quote! { head_tail::write });
    let view_fn = cp.item(quote! { head_tail::view });
    let ty_i = names.ty_i();
    let ty_s = names.ty_s();
    quote! {
        impl<'i> #ty_i {
            /// Reads the value at the start of `buf`: its head, then its tail.
            #[inline]
            pub fn from_head_tail(buf: &'i [u8]) -> Result<Self, #error> {
                #read::<Self>(buf)
            }

            /// Writes the value at the start of `buf`, returns its length, [Self::head_tail_len].
            #[inline]
            pub fn to_head_tail(&self, buf: &mut [u8]) -> Result<usize, #error> {
                #write(self, buf)
            }

            /// Views the value at the start of `buf` (SW-52): checks the lengths and walks the tail, but reads a
            /// field of the head only when its accessor is called. Same bytes as [Self::from_head_tail].
            #[inline]
            pub fn view(buf: &'i [u8]) -> Result<<Self as #ht<'i>>::View, #error> {
                #view_fn::<Self>(buf)
            }

            /// Length of the value on the wire: the head and the tail.
            #[inline]
            pub fn head_tail_len(&self) -> usize {
                <#ty_s>::HEAD_LEN + <Self as #ht<'i>>::tail_len(self)
            }
        }
    }
}

/// A `head_tail` struct.
pub(crate) fn ht_struct(
    names: &Names,
    fields: &[Field],
    cp: &CratePath,
) -> Result<TokenStream, String> {
    let ident = &names.ident;
    let ty_i = names.ty_i();
    let ty_s = names.ty_s();
    let head = head(&ty_s, "", "", struct_items(fields), cp)?;
    let error = cp.error();
    let ht = cp.item(quote! { head_tail::HeadTail });
    let tail_ty = cp.item(quote! { head_tail::Tail });
    let tail_wr = cp.item(quote! { head_tail::TailWr });
    let upper_ty = cp.item(quote! { fixed_layout::Upper });

    let reads = reads(&head, false, cp);
    let construct = construct_fields(&head, true);
    let view_ident = format_ident!("{}View", ident);
    let view_def = view_struct(
        &quote! {},
        &view_ident,
        &format!(" A [{ident}] on the wire, see [{ident}::view]."),
        &head,
        quote! { { <#ty_s>::HEAD_LEN } },
        cp,
    );
    let view_reads = self::reads(&head, true, cp);
    let view_construct = view_construct(&head);
    let access = |name: &str| {
        let field = format_ident!("{}", name);
        quote! { (&self.#field) }
    };
    let writes = writes(&head, &access, cp);
    let tail_len = tail_len(&head, &access, cp);
    let Head {
        consts,
        asserts,
        len,
        ..
    } = &head;

    let c_name = snake(&ident.to_string());
    let c_upper = c_name.to_uppercase();
    let c_title = cw(
        &format!(
            "\n/* {ident}: a head of {{}} bytes, then its tail */\n#define {{}}{c_upper}_HEAD_LEN {{}}u"
        ),
        &[
            quote! { <#ty_s>::HEAD_LEN },
            quote! { up },
            quote! { <#ty_s>::HEAD_LEN },
        ],
    );
    let c_head = c_head(&ty_s, "", &c_name, &head, cp);
    let mut c_members = c_members("", &head, cp);
    c_members.insert(
        0,
        cw(
            "typedef struct {{\n    char none; /* C has no empty structs */",
            &[],
        ),
    );
    c_members.push(cw(
        &format!("}}}} {{}}{c_name}_tail_t;"),
        &[quote! { prefix }],
    ));
    let c_open = cw(
        &format!(
            "/* walks the tail of a {ident} whose head is at h and tail at *t (ending at end): fills tl and moves *t \
             past it; 0, or -1 when the bytes are short */\nstatic inline int {{}}{c_name}_tail(const uint8_t *h, \
             const uint8_t **t, const uint8_t *end, {{}}{c_name}_tail_t *tl) {{{{\n    (void)h;\n    (void)t;\n    \
             (void)end;\n    (void)tl;"
        ),
        &[quote! { prefix }, quote! { prefix }],
    );
    let c_walk = c_walk("h", "", &c_name, &head, cp);
    let c_close = cw("    return 0;\n}}", &[]);
    let c_name_lit = lit_str(&c_name);
    let unused = quote! { let _ = (&b, &tail); };

    Ok(quote! {
        #(#asserts)*

        impl #ty_s {
            #(#consts)*
            /// Bytes of the head.
            pub const HEAD_LEN: usize = #len;
        }

        #view_def

        #[allow(clippy::identity_op)]
        impl<'i> #ht<'i> for #ty_i {
            const HEAD_LEN: usize = <#ty_s>::HEAD_LEN;
            const C_NAME: &'static str = #c_name_lit;
            const C_TAIL: bool = true;

            #[inline]
            fn read_ht(head: &'i [u8], tail: &mut #tail_ty<'i>) -> Result<Self, #error> {
                let Some(b) = head.first_chunk::<{ <#ty_s>::HEAD_LEN }>() else {
                    return Err(#error::OutOfBoundsReadRawSlice);
                };
                #unused
                #(#reads)*
                Ok(#ident { #(#construct),* })
            }

            #[inline]
            fn write_ht(&self, head: &mut [u8], tail: &mut #tail_wr) -> Result<(), #error> {
                let Some(b) = head.first_chunk_mut::<{ <#ty_s>::HEAD_LEN }>() else {
                    return Err(#error::OutOfBoundsWriteRawSlice);
                };
                #unused
                #(#writes)*
                Ok(())
            }

            #[inline]
            fn tail_len(&self) -> usize {
                #tail_len
            }

            type View = #view_ident<'i>;

            #[inline]
            fn view_ht(head: &'i [u8], tail: &mut #tail_ty<'i>) -> Result<Self::View, #error> {
                let Some(b) = head.first_chunk::<{ <#ty_s>::HEAD_LEN }>() else {
                    return Err(#error::OutOfBoundsReadRawSlice);
                };
                #unused
                #(#view_reads)*
                Ok(#view_ident { __head: b, #(#view_construct),* })
            }

            fn write_c_ht(prefix: &str, out: &mut dyn ::core::fmt::Write) -> ::core::fmt::Result {
                let up = #upper_ty(prefix);
                #c_title
                #(#c_head)*
                #(#c_members)*
                #c_open
                #(#c_walk)*
                #c_close
                Ok(())
            }
        }
    }
    .into_iter()
    .chain(inherent(names, cp))
    .collect())
}

/// A `head_tail` enum: the tag is the head, the variant's head and tail the tail.
pub(crate) fn ht_enum(
    names: &Names,
    repr: Repr,
    variants: &[Variant],
    cp: &CratePath,
) -> Result<TokenStream, String> {
    let Some((tag_len, tag_prim)) = repr_bytes(repr) else {
        return Err("head_tail: the tag must be byte-aligned: use ww_repr = u8, u16 or u32".into());
    };
    let ident = &names.ident;
    let ty_i = names.ty_i();
    let ty_s = names.ty_s();
    let error = cp.error();
    let ht = cp.item(quote! { head_tail::HeadTail });
    let tail_ty = cp.item(quote! { head_tail::Tail });
    let tail_wr = cp.item(quote! { head_tail::TailWr });
    let upper_ty = cp.item(quote! { fixed_layout::Upper });
    let tag_ty = tag_prim.rust();
    let c_name = snake(&ident.to_string());
    let c_upper = c_name.to_uppercase();

    let mut defs = Vec::new();
    let mut asserts = Vec::new();
    let view_ident = format_ident!("{}View", ident);
    let view_has_lt = variants.iter().any(|v| !matches!(v.fields, Fields::Unit));
    let view_ty = if view_has_lt {
        quote! { #view_ident<'i> }
    } else {
        quote! { #view_ident }
    };
    let mut view_variants = Vec::new();
    let mut view_structs = Vec::new();
    let mut view_arms = Vec::new();
    let mut writers = Vec::new();
    let mut read_arms = Vec::new();
    let mut write_arms = Vec::new();
    let mut len_arms = Vec::new();
    let mut c_heads = Vec::new();
    let mut c_members = Vec::new();
    let mut c_cases = Vec::new();
    for v in variants {
        // a variant compiled out (SW-54) keeps its tag: it is just unknown to this build, like a newer one
        let cfg = v.cfg_attrs();
        let vs = snake(&v.ident.to_string());
        let vu = vs.to_uppercase();
        let items = variant_items(v).map_err(|e| format!("head_tail: {e}"))?;
        let head = head(
            &ty_s,
            &format!("{vu}_"),
            &format!("variant `{}`, ", v.ident),
            items,
            cp,
        )?;
        let tag_name = format_ident!("{}_TAG", vu);
        let head_name = format_ident!("{}_HEAD_LEN", vu);
        let disc = Literal::u32_unsuffixed(v.discriminant);
        let doc_tag = lit_str(&format!(" Discriminant of `{}`.", v.ident));
        let doc_head = lit_str(&format!(
            " Bytes of the head of `{}`, the start of the tail.",
            v.ident
        ));
        let Head {
            consts,
            asserts: a,
            len,
            ..
        } = &head;
        asserts.extend(a.iter().map(|a| quote! { #cfg #a }));
        defs.push(quote! {
            #cfg
            impl #ty_s {
                #[doc = #doc_tag]
                pub const #tag_name: #tag_ty = #disc;
                #[doc = #doc_head]
                pub const #head_name: usize = #len;
                #(#consts)*
            }
        });

        let v_ident = &v.ident;
        let named = matches!(v.fields, Fields::Named(_));
        let bindings: Vec<(String, Ident)> = head
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
        let field_idents: Vec<&Ident> = head
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
        let construct = construct_fields(&head, named);
        let construct = match &v.fields {
            Fields::Named(_) => quote! { #ident::#v_ident { #(#construct),* } },
            Fields::Unnamed(_) => quote! { #ident::#v_ident ( #(#construct),* ) },
            Fields::Unit => quote! { #ident::#v_ident },
        };
        let reads = reads(&head, false, cp);
        let writes = writes(&head, &access, cp);
        let len = tail_len(&head, &access, cp);
        read_arms.push(quote! {
            #cfg
            #disc => {
                let b = tail.take_array::<{ <#ty_s>::#head_name }>()?;
                let _ = &b;
                #(#reads)*
                Ok(#construct)
            }
        });
        write_arms.push(quote! {
            #cfg
            #pattern => {
                tag.copy_from_slice(&<#ty_s>::#tag_name.to_le_bytes());
                let Some(b) = tail.put(<#ty_s>::#head_name)?.first_chunk_mut::<{ <#ty_s>::#head_name }>() else {
                    return Err(#error::OutOfBoundsWriteRawSlice);
                };
                let _ = &b;
                #(#writes)*
            }
        });
        len_arms.push(quote! { #cfg #pattern => <#ty_s>::#head_name + #len, });

        // the view of the variant (SW-52): nothing, the view of its one nested field, or a view struct
        let view_reads = self::reads(&head, true, cp);
        let single_nested = match (&v.fields, head.placed.as_slice()) {
            (Fields::Unnamed(_), [p]) => match &p.kind {
                HKind::Nested(ty_i, _) => Some((ty_i.clone(), local(&p.item.name))),
                _ => None,
            },
            _ => None,
        };
        let construct_view = if matches!(v.fields, Fields::Unit) {
            view_variants.push(quote! { #cfg #v_ident });
            quote! { #view_ident::#v_ident }
        } else if let Some((ty_n, l)) = single_nested {
            view_variants.push(quote! { #cfg #v_ident(<#ty_n as #ht<'i>>::View) });
            quote! { #view_ident::#v_ident(#l) }
        } else {
            let vs_ident = format_ident!("{}{}View", ident, v_ident);
            view_structs.push(view_struct(
                &cfg,
                &vs_ident,
                &format!(
                    " The variant `{v_ident}` of a [{ident}] on the wire, see [{ident}::view]."
                ),
                &head,
                quote! { { <#ty_s>::#head_name } },
                cp,
            ));
            view_variants.push(quote! { #cfg #v_ident(#vs_ident<'i>) });
            let fields = view_construct(&head);
            quote! { #view_ident::#v_ident(#vs_ident { __head: b, #(#fields),* }) }
        };
        view_arms.push(quote! {
            #cfg
            #disc => {
                let b = tail.take_array::<{ <#ty_s>::#head_name }>()?;
                let _ = &b;
                #(#view_reads)*
                Ok(#construct_view)
            }
        });
        writers.push(writer(names, v, &vs, &head, tag_len, cp));

        let vc = format!("{c_name}_{vs}");
        let mut v_heads = Vec::new();
        v_heads.push(cw(
            &format!("#define {{}}{c_upper}_{vu} {{}}u\n#define {{}}{c_upper}_{vu}_HEAD_LEN {{}}u"),
            &[
                quote! { up },
                quote! { <#ty_s>::#tag_name },
                quote! { up },
                quote! { <#ty_s>::#head_name },
            ],
        ));
        v_heads.extend(c_head(&ty_s, &format!("{vu}_"), &vc, &head, cp));
        c_heads.push(quote! { #cfg { #(#v_heads)* } });
        let v_members = c_members_of(&format!("{vs}_"), &head, cp);
        c_members.push(quote! { #cfg { #(#v_members)* } });
        let mut case = vec![cw(
            "    case {}u:\n        vh = sw_ht_take(t, end, {}u);\n        if (vh == NULL) {{ return -1; }}\n        \
             tl->head = vh;",
            &[
                quote! { <#ty_s>::#tag_name },
                quote! { <#ty_s>::#head_name },
            ],
        )];
        case.extend(c_walk("vh", &format!("{vs}_"), &vc, &head, cp));
        case.push(cw("        return 0;", &[]));
        c_cases.push(quote! { #cfg { #(#case)* } });
    }

    let c_title = cw(
        &format!(
            "\n/* {ident}: a {tag_len}-byte tag (the head), then the head and tail of its variant */\n#define \
             {{}}{c_upper}_TAG_LEN {tag_len}u\nstatic inline {} {{}}{c_name}_tag(const uint8_t *p) {{{{ return {}; }}}}",
            tag_prim.c_ty(),
            tag_prim.c_load("p")
        ),
        &[quote! { up }, quote! { prefix }],
    );
    let c_struct_open = cw(
        "typedef struct {{\n    const uint8_t *head; /* the head of the variant */",
        &[],
    );
    let c_struct_close = cw(&format!("}}}} {{}}{c_name}_tail_t;"), &[quote! { prefix }]);
    let c_open = cw(
        &format!(
            "/* walks the tail of a {ident} whose tag is at h and tail at *t (ending at end): fills tl and moves *t \
             past it; 0, or -1 when the bytes are short or the tag unknown */\nstatic inline int \
             {{}}{c_name}_tail(const uint8_t *h, const uint8_t **t, const uint8_t *end, {{}}{c_name}_tail_t *tl) \
             {{{{\n    const uint8_t *vh;\n    switch ({{}}{c_name}_tag(h)) {{{{"
        ),
        &[quote! { prefix }, quote! { prefix }, quote! { prefix }],
    );
    let c_close = cw("    default:\n        return -1;\n    }}\n}}", &[]);
    let c_name_lit = lit_str(&c_name);

    let view_doc = lit_str(&format!(
        " A [{ident}] on the wire: the view of its variant, see [{ident}::view]."
    ));
    let view_lt = if view_has_lt {
        quote! { <'i> }
    } else {
        quote! {}
    };

    Ok(quote! {
        #(#asserts)*

        #[doc = #view_doc]
        pub enum #view_ident #view_lt {
            #(#view_variants),*
        }
        #(#view_structs)*
        #(#writers)*

        impl #ty_s {
            /// Bytes of the discriminant: the head.
            pub const TAG_LEN: usize = #tag_len;
            /// Bytes of the head.
            pub const HEAD_LEN: usize = #tag_len;
        }
        #(#defs)*

        #[allow(clippy::identity_op)]
        impl<'i> #ht<'i> for #ty_i {
            const HEAD_LEN: usize = #tag_len;
            const C_NAME: &'static str = #c_name_lit;
            const C_TAIL: bool = true;

            #[inline]
            fn read_ht(head: &'i [u8], tail: &mut #tail_ty<'i>) -> Result<Self, #error> {
                let Some(tag) = head.first_chunk::<#tag_len>() else {
                    return Err(#error::OutOfBoundsReadRawSlice);
                };
                match <#tag_ty>::from_le_bytes(*tag) {
                    #(#read_arms)*
                    _ => Err(#error::EnumFutureVersionOrMalformedData),
                }
            }

            #[inline]
            fn write_ht(&self, head: &mut [u8], tail: &mut #tail_wr) -> Result<(), #error> {
                let Some(tag) = head.first_chunk_mut::<#tag_len>() else {
                    return Err(#error::OutOfBoundsWriteRawSlice);
                };
                match self {
                    #(#write_arms)*
                }
                Ok(())
            }

            #[inline]
            fn tail_len(&self) -> usize {
                match self {
                    #(#len_arms)*
                }
            }

            type View = #view_ty;

            #[inline]
            fn view_ht(head: &'i [u8], tail: &mut #tail_ty<'i>) -> Result<Self::View, #error> {
                let Some(tag) = head.first_chunk::<#tag_len>() else {
                    return Err(#error::OutOfBoundsReadRawSlice);
                };
                match <#tag_ty>::from_le_bytes(*tag) {
                    #(#view_arms)*
                    _ => Err(#error::EnumFutureVersionOrMalformedData),
                }
            }

            fn write_c_ht(prefix: &str, out: &mut dyn ::core::fmt::Write) -> ::core::fmt::Result {
                let up = #upper_ty(prefix);
                #c_title
                #(#c_heads)*
                #c_struct_open
                #(#c_members)*
                #c_struct_close
                #c_open
                #(#c_cases)*
                #c_close
                Ok(())
            }
        }
    }
    .into_iter()
    .chain(inherent(names, cp))
    .collect())
}

fn c_members_of(m: &str, head: &Head, cp: &CratePath) -> Vec<TokenStream> {
    c_members(m, head, cp)
}

/// Rust type of a payload: as declared, or as a view holds it (`view`).
fn payload_ty(p: &Payload, view: bool, cp: &CratePath) -> TokenStream {
    let ht = cp.item(quote! { head_tail::HeadTail });
    match p {
        Payload::Prim(prim) => prim.rust(),
        Payload::Array(len, prim) => {
            let t = prim.rust();
            quote! { [#t; #len] }
        }
        Payload::Nested(ty_i, _) if view => quote! { <#ty_i as #ht<'i>>::View },
        Payload::Nested(ty_i, _) => ty_i.clone(),
    }
}

/// Rust type of a field: as declared, or as a view holds it (`view`); `None` for a flag.
fn field_ty(kind: &HKind, view: bool, cp: &CratePath) -> Option<TokenStream> {
    let ht = cp.item(quote! { head_tail::HeadTail });
    let list = cp.item(quote! { head_tail::List });
    Some(match kind {
        HKind::Head(Kind::Prim(p)) => p.rust(),
        HKind::Head(Kind::Array(len, p)) => {
            let t = p.rust();
            quote! { [#t; #len] }
        }
        HKind::Head(Kind::Bits(_, bits)) => match bits {
            Bits::Bool => quote! { bool },
            Bits::Nibble => cp.item(quote! { Nibble }),
            Bits::Unsigned(t) | Bits::Signed(t) => quote! { #t },
        },
        HKind::Head(_) => return None,
        HKind::List(item) => quote! { #list<'i, #item> },
        HKind::Str => quote! { &'i str },
        HKind::Nested(ty_i, _) if view => quote! { <#ty_i as #ht<'i>>::View },
        HKind::Nested(ty_i, _) => ty_i.clone(),
        HKind::Option(p) => {
            let t = payload_ty(p, view, cp);
            quote! { Option<#t> }
        }
        HKind::Result(ok, err) => {
            let (ok, err) = (payload_ty(ok, view, cp), payload_ty(err, view, cp));
            quote! { Result<#ok, #err> }
        }
    })
}

/// A view holds this field as a view of a nested type (handed out by reference), not as a `Copy` value.
fn holds_view(kind: &HKind) -> bool {
    let nested = |p: &Payload| matches!(p, Payload::Nested(..));
    match kind {
        HKind::Nested(..) => true,
        HKind::Option(p) => nested(p),
        HKind::Result(ok, err) => nested(ok) || nested(err),
        _ => false,
    }
}

/// The fields of a view struct, from the locals of `reads(.., true, ..)`: everything but the head's numbers.
fn view_construct(head: &Head) -> Vec<TokenStream> {
    head.placed
        .iter()
        .filter(|p| !matches!(p.kind, HKind::Head(_)))
        .filter_map(|p| {
            let ident = p.item.ident.as_ref()?;
            let local = local(&p.item.name);
            Some(quote! { #ident: #local })
        })
        .collect()
}

/// The view of a head (a struct, or an enum variant with fields, SW-52): a reference to the head, whose numbers
/// and bit fields its accessors read at their offsets (the code of `read_ht`, one field at a time), and the parts
/// of the tail, found when the view was made. Always `pub`: it is the `View` of a public trait.
fn view_struct(
    cfg: &TokenStream,
    view_ident: &Ident,
    doc: &str,
    head: &Head,
    head_len: TokenStream,
    cp: &CratePath,
) -> TokenStream {
    let doc = lit_str(doc);
    let mut fields = Vec::new();
    let mut accessors = Vec::new();
    for p in &head.placed {
        let Some(ident) = &p.item.ident else { continue };
        let Some(ty) = field_ty(&p.kind, true, cp) else {
            continue;
        };
        let local = local(&p.item.name);
        let name = &p.item.name;
        match (&p.kind, &p.at) {
            (HKind::Head(kind), Some(at)) => {
                let read = read_simple(kind, at, &local, cp).expect("a head kind");
                let doc = lit_str(&format!(" `{name}`, read from the head."));
                accessors.push(quote! {
                    #[doc = #doc]
                    #[inline]
                    pub fn #ident(&self) -> #ty {
                        let b = self.__head;
                        #read
                        #local
                    }
                });
            }
            _ if holds_view(&p.kind) => {
                let doc = lit_str(&format!(" `{name}`."));
                fields.push(quote! { #ident: #ty });
                accessors.push(quote! {
                    #[doc = #doc]
                    #[inline]
                    pub fn #ident(&self) -> &#ty {
                        &self.#ident
                    }
                });
            }
            _ => {
                let doc = lit_str(&format!(" `{name}`."));
                fields.push(quote! { #ident: #ty });
                accessors.push(quote! {
                    #[doc = #doc]
                    #[inline]
                    pub fn #ident(&self) -> #ty {
                        self.#ident
                    }
                });
            }
        }
    }
    quote! {
        #cfg
        #[doc = #doc]
        pub struct #view_ident<'i> {
            __head: &'i [u8; #head_len],
            #(#fields),*
        }

        #cfg
        #[allow(clippy::identity_op)]
        impl<'i> #view_ident<'i> {
            #(#accessors)*
        }
    }
}

/// `Enum::write_<variant>(buf, fields..)` (SW-52): writes the variant from its fields into `buf` without making a
/// value of the enum first; the code of `write_ht` for that variant.
fn writer(
    names: &Names,
    v: &Variant,
    vs: &str,
    head: &Head,
    tag_len: usize,
    cp: &CratePath,
) -> TokenStream {
    let error = cp.error();
    let tail_wr = cp.item(quote! { head_tail::TailWr });
    let ty_i = names.ty_i();
    let ty_s = names.ty_s();
    let ident = &names.ident;
    let v_ident = &v.ident;
    let cfg = v.cfg_attrs();
    let vu = vs.to_uppercase();
    let tag_name = format_ident!("{}_TAG", vu);
    let head_name = format_ident!("{}_HEAD_LEN", vu);
    let mut params = Vec::new();
    let mut moves = Vec::new();
    for p in &head.placed {
        let Some(param) = &p.item.ident else { continue };
        let Some(ty) = field_ty(&p.kind, false, cp) else {
            continue;
        };
        let w = format_ident!("__w_{}", p.item.name);
        params.push(quote! { #param: #ty });
        moves.push(quote! { let #w = #param; });
    }
    let access = |name: &str| {
        let w = format_ident!("__w_{}", name);
        quote! { (&#w) }
    };
    let writes = writes(head, &access, cp);
    // the buffer's name must not be a field's
    let buf = if head
        .placed
        .iter()
        .any(|p| p.item.ident.as_ref().is_some_and(|i| i == "buf"))
    {
        format_ident!("__buf")
    } else {
        format_ident!("buf")
    };
    let name = format_ident!("write_{}", vs);
    let doc = lit_str(&format!(
        " Writes `{ident}::{v_ident}` with these fields at the start of `{buf}`, returns its length: the same bytes as \
         [{ident}::to_head_tail], without making the value first."
    ));
    quote! {
        #cfg
        impl<'i> #ty_i {
            #[doc = #doc]
            #[allow(clippy::too_many_arguments, clippy::identity_op)]
            pub fn #name(#buf: &mut [u8], #(#params),*) -> Result<usize, #error> {
                #(#moves)*
                let Some((tag, rest)) = #buf.split_first_chunk_mut::<#tag_len>() else {
                    return Err(#error::OutOfBoundsWriteRawSlice);
                };
                *tag = <#ty_s>::#tag_name.to_le_bytes();
                let mut tail_wr = #tail_wr::new(rest);
                let tail = &mut tail_wr;
                let Some(b) = tail.put(<#ty_s>::#head_name)?.first_chunk_mut::<{ <#ty_s>::#head_name }>() else {
                    return Err(#error::OutOfBoundsWriteRawSlice);
                };
                let _ = &b;
                #(#writes)*
                Ok(#tag_len + tail.pos())
            }
        }
    }
}
