//! The head+tail encoding (SW-46): every fixed-width field at a constant offset in a head, the variable parts
//! one after another in a tail. What it is and why is in the docs of `shrink_wrap::head_tail`.
//!
//! The head is walked with the cursor of the fixed layout (a const byte expression and the bits used after it).
//! A list or string puts its `u16` count there, a nested type its own head (byte-aligned), an `Option` or `Result`
//! only its flag (a field of its own already). Reading is two passes over the fields: the head first (loads at
//! constant offsets), then the tail in field order, through the `Tail` cursor.

use crate::ast::item_enum::{Fields, Variant};
use crate::ast::item_struct::{Field, StructKind};
use crate::ast::repr::Repr;
use crate::ast::ty::{ArrayLen, Type};
use crate::codegen::fixed_layout::{
    At, Bits, CExtra, CView, Item, Kind, Prim, article, c_field, c_line, c_name_of,
    classify as fixed_classify, flag_local, lit, lit_str, local, read_simple, repr_bytes, snake,
    struct_items, unraw, variant_items, write_simple,
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
/// The number type of a list's items when they are numbers (`List<'i, u16>`), else `None` (fixed-layout items).
fn list_prim(item: &TokenStream) -> Option<Prim> {
    Some(match item.to_string().as_str() {
        "u8" => Prim::U8,
        "u16" => Prim::U16,
        "u32" => Prim::U32,
        "u64" => Prim::U64,
        "i8" => Prim::I8,
        "i16" => Prim::I16,
        "i32" => Prim::I32,
        "i64" => Prim::I64,
        "f32" => Prim::F32,
        "f64" => Prim::F64,
        _ => return None,
    })
}

/// Bit offset of the flag of the Option or Result field `name` of `head`.
fn flag_at(head: &Head, name: &str) -> TokenStream {
    head.placed
        .iter()
        .find_map(|f| match (&f.kind, &f.at) {
            (HKind::Head(Kind::Flag(t)), Some(At::Bit(off))) if unraw(t) == name => {
                Some(off.clone())
            }
            _ => None,
        })
        .expect("an Option or Result has a flag")
}

/// The view members of an Option's or Result's payload: `(member, payload)`.
fn payload_members<'p>(name: &str, kind: &'p HKind) -> Vec<(String, &'p Payload)> {
    match kind {
        HKind::Option(p) => vec![(name.to_string(), p)],
        HKind::Result(ok, err) => vec![(format!("{name}_ok"), ok), (format!("{name}_err"), err)],
        _ => vec![],
    }
}

/// The C view of a head and its tail (a `head_tail` struct, or an enum variant with fields), named
/// `${pre}{vc}_view_t`: its typedef with a member per variable part, the walker `${pre}{vc}__walk` that checks the
/// tail and fills them, and a reader per field. `prefix` starts the names of the Rust consts.
fn c_view(
    ty_s: &TokenStream,
    prefix: &str,
    vc: &str,
    what: &str,
    made_by: &str,
    head: &Head,
    cp: &CratePath,
) -> Vec<TokenStream> {
    let ht = cp.item(quote! { head_tail::HeadTail });
    let fixed = cp.item(quote! { fixed_layout::FixedLayout });
    let ty_ident = format_ident!("__HtSelf");
    let upper = vc.to_uppercase();
    let view_t = format!("${{pre}}{vc}_view_t");
    let param = format!("const {view_t} *v");
    let off_of = |p: &Placed| {
        let c = format_ident!("{prefix}{}_OFFSET", p.item.name.to_uppercase());
        quote! { <#ty_s>::#c }
    };
    // a nested type: its C name and its head length
    let nested = |ty: &TokenStream| {
        vec![
            ("n", quote! { <#ty as #ht<'static>>::C_NAME }),
            ("hl", quote! { <#ty as #ht<'static>>::HEAD_LEN }),
        ]
    };
    // checks a nested value whose head is at `at`, into the member `m`: walks the tail of a head_tail type, checks
    // a fixed-layout one, which is all head (one of variable length has no C reader in a head_tail type)
    let check_nested = |ty: &TokenStream,
                        at: &str,
                        at_args: &[(&'static str, TokenStream)],
                        m: &str,
                        ind: &str| {
        let mut args = nested(ty);
        args.extend(at_args.iter().cloned());
        let walk = c_line(
            &format!("{ind}if (${{pre}}${{n}}__walk({at}, t, end, &v->{m}) != 0) {{ return -1; }}"),
            &args,
        );
        let check = c_line(
            &format!(
                "{ind}if (${{pre}}${{n}}_view({at}, ${{hl}}u, &v->{m}) != 0) {{ return -1; }}"
            ),
            &args,
        );
        let err = c_line(
            &format!(
                "#error \"{m}: a fixed_layout type of variable length has no C reader in a head_tail type\""
            ),
            &[],
        );
        quote! {
            if <#ty as #ht<'static>>::C_TAIL {
                #walk
            } else if <#ty as #ht<'static>>::HEAD_LEN == 0 {
                #err
            } else {
                #check
            }
        }
    };

    let mut members = Vec::new();
    let mut walk = Vec::new();
    let mut readers = Vec::new();
    for p in &head.placed {
        let field = &p.item.name;
        // the member of a tuple field `0` is `_0`
        let member = if field.starts_with(|c: char| c.is_ascii_digit()) {
            format!("_{field}")
        } else {
            field.clone()
        };
        let name = &member;
        let fu = field.to_uppercase();
        let func = format!("${{pre}}{vc}_{field}");
        match &p.kind {
            HKind::Head(kind) => readers.extend(c_field(
                &ty_ident,
                prefix,
                vc,
                &CView {
                    param: &param,
                    p: "v->p",
                    len: "v->len",
                },
                field,
                kind,
                CExtra::default(),
                cp,
            )),
            HKind::List(_) | HKind::Str => {
                let off = off_of(p);
                let (what, isz) = match &p.kind {
                    HKind::List(item) => ("the items", quote! { <#item as #fixed>::ENCODED_LEN }),
                    _ => ("the bytes, not 0-terminated", quote! { 1usize }),
                };
                members.push(c_line(
                    &format!("    const uint8_t *{name}; /* {what} */"),
                    &[],
                ));
                walk.push(c_line(
                    &format!(
                        "    v->{name} = sw_ht_take_items(t, end, (size_t)sw_fixed_u16(h + ${{off}}u), ${{isz}}u);\n    \
                         if (v->{name} == NULL) {{ return -1; }}"
                    ),
                    &[("off", off.clone()), ("isz", isz.clone())],
                ));
                readers.push(c_line(
                    &format!(
                        "#define ${{up}}{upper}_{fu}_OFFSET ${{off}}u\n\
                         static inline uint16_t {func}_count({param}) {{ return sw_fixed_u16(v->p + ${{off}}u); }}"
                    ),
                    &[("off", off.clone())],
                ));
                let item = match &p.kind {
                    HKind::List(item) => Some(item),
                    _ => None,
                };
                let text = match item.map(|i| (i, list_prim(i))) {
                    None | Some((_, Some(Prim::U8))) => {
                        format!(
                            "static inline const uint8_t *{func}({param}) {{ return v->{name}; }}"
                        )
                    }
                    Some((_, Some(prim))) => format!(
                        "/* item i, 0 past the end */\nstatic inline {} {func}({param}, size_t i) {{\n    \
                         if (i >= (size_t){func}_count(v)) {{ return 0; }}\n    return {};\n}}",
                        prim.c_ty(),
                        prim.c_load(&format!("v->{name} + {}u * i", prim.bytes()))
                    ),
                    Some((_, None)) => format!(
                        "/* checks item i and makes *out its view; 0, or -1 (*out empty) past the end or when it is \
                         malformed */\nSW_NODISCARD static inline int {func}({param}, size_t i, ${{pre}}${{n}}_view_t \
                         *out) {{\n    if (i >= (size_t){func}_count(v)) {{ out->p = NULL; out->len = 0u; return -1; }}\n    \
                         return ${{pre}}${{n}}_view(v->{name} + ${{isz}}u * i, ${{isz}}u, out);\n}}"
                    ),
                };
                let mut args = vec![("isz", isz)];
                if let Some(item) = item {
                    args.push(("n", c_name_of(item, cp)));
                }
                readers.push(c_line(&text, &args));
            }
            HKind::Nested(_, ty) => {
                let off = off_of(p);
                members.push(c_line(
                    &format!("    ${{pre}}${{n}}_view_t {name};"),
                    &nested(ty),
                ));
                walk.push(check_nested(
                    ty,
                    "h + ${off}u",
                    &[("off", off.clone())],
                    name,
                    "    ",
                ));
                let mut args = nested(ty);
                args.push(("off", off));
                readers.push(c_line(
                    &format!(
                        "/* {field}: a ${{pre}}${{n}} */\n#define ${{up}}{upper}_{fu}_OFFSET ${{off}}u\n\
                         static inline const ${{pre}}${{n}}_view_t *{func}({param}) {{ return &v->{name}; }}"
                    ),
                    &args,
                ));
            }
            HKind::Option(_) | HKind::Result(..) => {
                let flag = flag_at(head, field);
                let pm = payload_members(name, &p.kind);
                for (m, pl) in &pm {
                    match pl {
                        Payload::Nested(_, ty) => {
                            members.push(c_line(
                                &format!(
                                    "    ${{pre}}${{n}}_view_t {m}; /* p NULL when not there */"
                                ),
                                &nested(ty),
                            ));
                        }
                        _ => members.push(c_line(
                            &format!(
                                "    const uint8_t *{m}; /* the payload, NULL when not there */"
                            ),
                            &[],
                        )),
                    }
                }
                let take = |m: &str, pl: &Payload| -> TokenStream {
                    match pl {
                        Payload::Nested(_, ty) => {
                            let open = c_line(
                                "        {\n            const uint8_t *ph = sw_ht_take(t, end, ${hl}u);\n            \
                                 if (ph == NULL) { return -1; }",
                                &nested(ty),
                            );
                            let w = check_nested(ty, "ph", &[], m, "            ");
                            let close = c_line("        }", &[]);
                            quote! { #open #w #close }
                        }
                        other => c_line(
                            &format!(
                                "        v->{m} = sw_ht_take(t, end, ${{len}}u);\n        if (v->{m} == NULL) {{ return -1; }}"
                            ),
                            &[("len", c_payload_len(other))],
                        ),
                    }
                };
                let absent = |m: &str, pl: &Payload| match pl {
                    Payload::Nested(..) => c_line(
                        &format!("        v->{m}.p = NULL;\n        v->{m}.len = 0u;"),
                        &[],
                    ),
                    _ => c_line(&format!("        v->{m} = NULL;"), &[]),
                };
                walk.push(c_line(
                    "    if (sw_fixed_bits(h, ${flag}u, 1u) != 0u) {",
                    &[("flag", flag)],
                ));
                let (first, second) = (&pm[0], pm.get(1));
                walk.push(take(&first.0, first.1));
                if let Some(s) = second {
                    walk.push(absent(&s.0, s.1));
                }
                walk.push(c_line("    } else {", &[]));
                walk.push(absent(&first.0, first.1));
                if let Some(s) = second {
                    walk.push(take(&s.0, s.1));
                }
                walk.push(c_line("    }", &[]));
                for (m, pl) in &pm {
                    let func = format!("${{pre}}{vc}_{m}");
                    let open = format!("SW_NODISCARD static inline int {func}({param}");
                    let doc = "/* sets *out and returns 0, or returns -1 when it is not there */";
                    let (text, args) = match pl {
                        Payload::Prim(prim) => (
                            format!(
                                "{doc}\n{open}, {} *out) {{\n    if (v->{m} == NULL) {{ *out = 0; return -1; }}\n    \
                                 *out = {};\n    return 0;\n}}",
                                prim.c_ty(),
                                prim.c_load(&format!("v->{m}"))
                            ),
                            vec![],
                        ),
                        Payload::Array(_, Prim::U8) => (
                            format!(
                                "{doc}\n{open}, const uint8_t **out) {{\n    *out = v->{m};\n    \
                                 return v->{m} == NULL ? -1 : 0;\n}}"
                            ),
                            vec![],
                        ),
                        Payload::Array(len, prim) => (
                            format!(
                                "/* sets *out to item i and returns 0, or returns -1 (*out 0) when it is not there or i \
                                 is past the end */\n{open}, size_t i, {} *out) {{\n    \
                                 if (v->{m} == NULL || i >= ${{count}}u) {{ *out = 0; return -1; }}\n    \
                                 *out = {};\n    return 0;\n}}",
                                prim.c_ty(),
                                prim.c_load(&format!("v->{m} + {}u * i", prim.bytes()))
                            ),
                            vec![("count", quote! { (#len) })],
                        ),
                        Payload::Nested(_, ty) => (
                            format!(
                                "/* sets *out to its view and returns 0, or returns -1 (*out NULL) when it is not there \
                                 */\n{open}, const ${{pre}}${{n}}_view_t **out) {{\n    \
                                 if (v->{m}.p == NULL) {{ *out = NULL; return -1; }}\n    *out = &v->{m};\n    return 0;\n}}"
                            ),
                            nested(ty),
                        ),
                    };
                    readers.push(c_line(&text, &args));
                }
            }
        }
    }

    let mut out = vec![c_line(
        &format!(
            "/* a checked view of {what}: its head at p, len bytes in all (head and tail), and its variable parts; \
             made by {made_by} */\ntypedef struct {{\n    \
             const uint8_t *p;\n    size_t len;"
        ),
        &[],
    )];
    out.extend(members);
    out.push(c_line(&format!("}} {view_t};"), &[]));
    out.push(c_line(
        &format!(
            "/* checks the tail of {what} whose head (already checked) is at h and tail at *t (ending at end), fills *v \
             and moves *t past it; 0, or -1 when the bytes are short or malformed. For the views of the types that \
             hold it */\nSW_NODISCARD static inline int ${{pre}}{vc}__walk(const uint8_t *h, const uint8_t **t, \
             const uint8_t *end, {view_t} *v) {{\n    const uint8_t *t0 = *t;\n    (void)h;\n    (void)end;"
        ),
        &[],
    ));
    out.extend(walk);
    out.push(c_line(
        "    v->p = h;\n    v->len = ${hl}u + (size_t)(*t - t0);\n    return 0;\n}",
        &[("hl", head.len.clone())],
    ));
    out.push(quote! {{
        #[allow(non_camel_case_types)]
        type #ty_ident = #ty_s;
        #(#readers)*
    }});
    out
}

/// Bytes of a number or array payload in C, a Rust expression.
fn c_payload_len(p: &Payload) -> TokenStream {
    match p {
        Payload::Prim(prim) => {
            let n = prim.bytes();
            quote! { #n }
        }
        Payload::Array(len, prim) => {
            let n = prim.bytes();
            quote! { ((#len) * #n) }
        }
        Payload::Nested(..) => unreachable!("a nested payload is checked by its type"),
    }
}

/// The checked entry of a top-level value: `${pre}{c_name}_view(buf, len, v)`.
fn c_entry(c_name: &str, what: &str, head_len: TokenStream) -> TokenStream {
    c_line(
        &format!(
            "/* checks that the len bytes at buf start with {what} and makes *v its view (v->len: its length); 0, or \
             -1 (*v zeroed) when they are too short or malformed */\nSW_NODISCARD static inline int \
             ${{pre}}{c_name}_view(const uint8_t *buf, size_t len, ${{pre}}{c_name}_view_t *v) {{\n    \
             const uint8_t *t;\n    memset(v, 0, sizeof *v);\n    if (buf == NULL || len < ${{hl}}u) {{ return -1; }}\n    \
             t = buf + ${{hl}}u;\n    if (${{pre}}{c_name}__walk(buf, &t, buf + len, v) != 0) {{\n        \
             memset(v, 0, sizeof *v);\n        return -1;\n    }}\n    return 0;\n}}"
        ),
        &[("hl", head_len)],
    )
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
    kind: StructKind,
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
    let construct = match kind {
        StructKind::Tuple => {
            let fields = construct_fields(&head, false);
            quote! { #ident ( #(#fields),* ) }
        }
        _ => {
            let fields = construct_fields(&head, true);
            quote! { #ident { #(#fields),* } }
        }
    };
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
        let field = kind.member(&format_ident!("{}", name));
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
    let mut c_text = vec![c_line(
        &format!(
            "\n/* {ident}: a head of ${{hl}} bytes, then its tail */\n#define ${{up}}{c_upper}_HEAD_LEN ${{hl}}u"
        ),
        &[("hl", quote! { <#ty_s>::HEAD_LEN })],
    )];
    c_text.extend(c_view(
        &ty_s,
        "",
        &c_name,
        &article(&ident.to_string()),
        &format!("${{pre}}{c_name}_view() or by the view of the value that holds it"),
        &head,
        cp,
    ));
    c_text.push(c_entry(
        &c_name,
        &article(&ident.to_string()),
        quote! { <#ty_s>::HEAD_LEN },
    ));
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
                Ok(#construct)
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
                #(#c_text)*
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
    let mut c_as = Vec::new();
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
        let tag_arg = ("tag", quote! { <#ty_s>::#tag_name });
        let hl_arg = ("hl", quote! { <#ty_s>::#head_name });
        let mut v_text = vec![c_line(
            &format!(
                "#define ${{up}}{c_upper}_{vu} ${{tag}}u\n#define ${{up}}{c_upper}_{vu}_HEAD_LEN ${{hl}}u"
            ),
            &[tag_arg.clone(), hl_arg.clone()],
        )];
        let mut case = vec![c_line("    case ${tag}u:", std::slice::from_ref(&tag_arg))];
        if head.placed.is_empty() {
            case.push(c_line(
                "        if (sw_ht_take(t, end, ${hl}u) == NULL) { return -1; }",
                &[hl_arg],
            ));
        } else {
            v_text.extend(c_view(
                &ty_s,
                &format!("{vu}_"),
                &vc,
                &article(&format!("{ident}::{}", v.ident)),
                &format!("${{pre}}{c_name}_as_{vs}()"),
                &head,
                cp,
            ));
            let member = c_line(&format!("        ${{pre}}{vc}_view_t {vs};"), &[]);
            c_members.push(quote! { #cfg #member });
            let as_fn = c_line(
                &format!(
                    "/* makes *out the view of v's {} variant and returns 0, or returns -1 (*out NULL) for another \
                     tag */\nSW_NODISCARD static inline int ${{pre}}{c_name}_as_{vs}(const ${{pre}}{c_name}_view_t *v, \
                     const ${{pre}}{vc}_view_t **out) {{\n    if (${{pre}}{c_name}_tag(v) != ${{tag}}u) {{ *out = NULL; \
                     return -1; }}\n    *out = &v->u.{vs};\n    return 0;\n}}",
                    v.ident
                ),
                &[tag_arg],
            );
            c_as.push(quote! { #cfg #as_fn });
            case.push(c_line(
                &format!(
                    "        {{\n            const uint8_t *vh = sw_ht_take(t, end, ${{hl}}u);\n            \
                     if (vh == NULL) {{ return -1; }}\n            \
                     if (${{pre}}{vc}__walk(vh, t, end, &v->u.{vs}) != 0) {{ return -1; }}\n        }}"
                ),
                &[hl_arg],
            ));
        }
        case.push(c_line("        break;", &[]));
        c_heads.push(quote! { #cfg { #(#v_text)* } });
        c_cases.push(quote! { #cfg { #(#case)* } });
    }

    let c_title = c_line(
        &format!(
            "\n/* {ident}: a {tag_len}-byte tag (the head), then the head and tail of its variant */\n\
             #define ${{up}}{c_upper}_TAG_LEN {tag_len}u"
        ),
        &[],
    );
    let c_struct_open = c_line(
        &format!(
            "/* a checked view of {}: its tag at p, len bytes in all, the view of its variant in u; made by \
             ${{pre}}{c_name}_view() or by the view of the value that holds it */\ntypedef struct {{\n    \
             const uint8_t *p;\n    size_t len;\n    union {{\n        char none; /* for variants without fields */",
            article(&ident.to_string())
        ),
        &[],
    );
    let c_struct_close = c_line(
        &format!(
            "    }} u;\n}} ${{pre}}{c_name}_view_t;\nstatic inline {} ${{pre}}{c_name}_tag(const ${{pre}}{c_name}_view_t *v) \
             {{ return {}; }}",
            tag_prim.c_ty(),
            tag_prim.c_load("v->p")
        ),
        &[],
    );
    let c_open = c_line(
        &format!(
            "/* checks the variant of {} whose tag (already checked) is at h, and its tail at *t (ending at \
             end), fills *v and moves *t past it; 0, or -1 when the bytes are short or the tag unknown. For the \
             views of the types that hold it */\nSW_NODISCARD static inline int ${{pre}}{c_name}__walk(const uint8_t \
             *h, const uint8_t **t, const uint8_t *end, ${{pre}}{c_name}_view_t *v) {{\n    const uint8_t *t0 = *t;\n    \
             switch ({}) {{",
            article(&ident.to_string()),
            tag_prim.c_load("h")
        ),
        &[],
    );
    let c_close = c_line(
        &format!(
            "    default:\n        return -1;\n    }}\n    v->p = h;\n    v->len = {tag_len}u + (size_t)(*t - t0);\n    \
             return 0;\n}}"
        ),
        &[],
    );
    let c_entry = c_entry(
        &c_name,
        &format!("{} of a known tag", article(&ident.to_string())),
        quote! { #tag_len },
    );
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
                #(#c_as)*
                #c_open
                #(#c_cases)*
                #c_close
                #c_entry
                Ok(())
            }
        }
    }
    .into_iter()
    .chain(inherent(names, cp))
    .collect())
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
