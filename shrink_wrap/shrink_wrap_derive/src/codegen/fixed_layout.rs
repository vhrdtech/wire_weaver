//! The fixed-layout fast path (SW-44): for a type whose encoding is a fixed little-endian byte layout, offset
//! consts per field, `from_fixed` / `to_fixed` straight on the slice (no `BufReader` / `BufWriter`), and the
//! matching C reader as text. The bytes are the ones `ser_shrink_wrap` writes.
//!
//! A type qualifies when every field is a byte-aligned fixed-width number (`u8`..`u64`, `i8`..`i64`, `f32`,
//! `f64`), an array of them, or (with the `fixed_layout` directive only) another fixed-layout type; a struct must
//! be `sized` or `final_structure`, an enum `sized` or `self_describing` with a `u8`, `u16` or `u32` `ww_repr`.
//! Detected types get the fast path silently; `fixed_layout` turns detection into a requirement and names the
//! field that breaks it.

use crate::ast::item_enum::{Fields, Variant};
use crate::ast::item_struct::Field;
use crate::ast::object_size::ObjectSize;
use crate::ast::repr::Repr;
use crate::ast::ty::{ArrayLen, Type};
use crate::codegen::util::CratePath;
use proc_macro2::{Ident, Span, TokenStream};
use quote::{format_ident, quote};
use syn::LitStr;

#[derive(Clone, Copy, Debug)]
enum Prim {
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
    fn of(ty: &Type) -> Option<Prim> {
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

    fn bytes(self) -> usize {
        match self {
            Prim::U8 | Prim::I8 => 1,
            Prim::U16 | Prim::I16 => 2,
            Prim::U32 | Prim::I32 | Prim::F32 => 4,
            Prim::U64 | Prim::I64 | Prim::F64 => 8,
        }
    }

    fn rust(self) -> TokenStream {
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

    fn c_ty(self) -> &'static str {
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
    fn c_load(self, ptr: &str) -> String {
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

#[derive(Clone, Debug)]
enum Kind {
    Prim(Prim),
    Array(ArrayLen, Prim),
    /// Another fixed-layout type, `FixedLayout` is checked by the compiler.
    Nested(TokenStream),
}

/// Why `ty` has no fixed byte layout, or how it is laid out.
fn classify(ty: &Type) -> Result<Kind, String> {
    if let Some(prim) = Prim::of(ty) {
        return Ok(Kind::Prim(prim));
    }
    let why = match ty {
        Type::Array(len, inner) => {
            return match Prim::of(inner) {
                Some(prim) => Ok(Kind::Array(len.clone(), prim)),
                None => Err(
                    "arrays may only hold fixed-width numbers (u8..u64, i8..i64, f32, f64)".into(),
                ),
            };
        }
        Type::External(path, has_lifetime) => {
            let name = path
                .segments
                .last()
                .map(|s| s.to_string())
                .unwrap_or_default();
            let bits = name
                .strip_prefix(['U', 'u', 'I', 'i'])
                .and_then(|n| n.parse::<u8>().ok());
            if path.args.is_empty() && bits.is_some() {
                format!("`{name}` is bit-packed")
            } else if *has_lifetime {
                format!("`{name}` borrows from the buffer")
            } else {
                return Ok(Kind::Nested(path.tokens(None)));
            }
        }
        Type::Bool => "bool is one bit, bit-packed".into(),
        Type::Nibble => "Nibble is four bits".into(),
        Type::U128 | Type::I128 => "128-bit numbers are not supported".into(),
        Type::UNib32
        | Type::UVlq32
        | Type::UVlq64
        | Type::ULeb32
        | Type::ULeb64
        | Type::ULeb128
        | Type::ILeb32
        | Type::ILeb64
        | Type::ILeb128 => "variable-length numbers have no fixed width".into(),
        Type::String | Type::Vec(_) | Type::MaxLen(..) => {
            "strings and lists have no fixed length".into()
        }
        Type::Option(..) | Type::IsSome(_) => "Option has a presence flag bit".into(),
        Type::Result(..) | Type::IsOk(_) => "Result has a flag bit".into(),
        Type::RefBox(_) => "RefBox has no fixed length".into(),
        Type::TailSize(_) => "TailSize is variable length".into(),
        Type::Tuple(_) => "tuples are not supported, use a struct".into(),
        Type::Range(_) | Type::RangeInclusive(_) => "ranges are not supported, use a struct".into(),
        _ => "not a fixed-width type".into(),
    };
    Err(why)
}

/// One field: its name for consts and C, how to reach it, how it is laid out.
struct FixedField {
    name: String,
    kind: Kind,
}

/// The fixed layout of a struct or of one enum variant, or why there is none.
fn layout_fields(
    fields: &[(String, &Type)],
    required: bool,
) -> Result<Option<Vec<FixedField>>, String> {
    let mut out = Vec::with_capacity(fields.len());
    for (name, ty) in fields {
        match classify(ty) {
            Ok(Kind::Nested(_)) if !required => return Ok(None),
            Ok(kind) => out.push(FixedField {
                name: name.clone(),
                kind,
            }),
            Err(why) if required => {
                return Err(format!("fixed_layout: field `{name}`: {why}"));
            }
            Err(_) => return Ok(None),
        }
    }
    Ok(Some(out))
}

fn struct_fields(fields: &[Field]) -> Vec<(String, &Type)> {
    fields.iter().map(|f| (unraw(&f.ident), &f.ty)).collect()
}

fn variant_fields(v: &Variant) -> Vec<(String, &Type)> {
    match &v.fields {
        Fields::Named(named) => struct_fields(named),
        Fields::Unnamed(unnamed) => unnamed
            .iter()
            .enumerate()
            .map(|(i, ty)| (format!("{i}"), ty))
            .collect(),
        Fields::Unit => vec![],
    }
}

fn unraw(ident: &Ident) -> String {
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

/// Struct size assumption with `fixed_layout`: `sized` when none is given. Errors for one that has no fixed end.
pub(crate) fn struct_size_assumption(
    size_assumption: &mut Option<ObjectSize>,
    required: bool,
) -> Result<bool, String> {
    match size_assumption {
        None if required => {
            *size_assumption = Some(ObjectSize::sized(0));
            Ok(true)
        }
        Some(ObjectSize::Sized { .. }) | Some(ObjectSize::UnsizedFinalStructure) => Ok(true),
        _ if required => Err(
            "fixed_layout: a struct must be sized or final_structure (or neither: sized is implied)".into(),
        ),
        _ => Ok(false),
    }
}

/// Byte width of a byte-aligned enum discriminant.
fn repr_bytes(repr: Repr) -> Option<(usize, Prim)> {
    match repr {
        Repr::U8 => Some((1, Prim::U8)),
        Repr::U16 => Some((2, Prim::U16)),
        Repr::U32 => Some((4, Prim::U32)),
        _ => None,
    }
}

fn lit_str(s: &str) -> LitStr {
    LitStr::new(s, Span::call_site())
}

/// Length of a field in bytes, as a const expression.
fn len_tokens(kind: &Kind, cp: &CratePath) -> TokenStream {
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
    }
}

/// Consts of a run of fields from `start`: `(defs, end)`, the offset const of each field being `prefix_FIELD_OFFSET`.
fn offset_consts(
    ty_ident: &Ident,
    const_prefix: &str,
    fields: &[FixedField],
    start: TokenStream,
    cp: &CratePath,
) -> (Vec<TokenStream>, Vec<Ident>, TokenStream) {
    let mut defs = Vec::new();
    let mut names = Vec::new();
    let mut at = start;
    for f in fields {
        let name = format_ident!("{}{}_OFFSET", const_prefix, f.name.to_uppercase());
        let doc = lit_str(&format!(" Offset of `{}` in bytes.", f.name));
        defs.push(quote! { #[doc = #doc] pub const #name: usize = #at; });
        let len = len_tokens(&f.kind, cp);
        at = quote! { #ty_ident::#name + #len };
        names.push(name);
    }
    (defs, names, at)
}

/// Expression reading field `f` at `off` of the byte array `b`.
fn read_expr(f: &FixedField, off: &TokenStream, cp: &CratePath) -> TokenStream {
    let take = cp.item(quote! { fixed_layout::take });
    let fixed = cp.item(quote! { fixed_layout::FixedLayout });
    match &f.kind {
        Kind::Prim(p) => {
            let n = p.bytes();
            let ty = p.rust();
            quote! { <#ty>::from_le_bytes(#take::<#n>(b, #off)) }
        }
        Kind::Array(len, Prim::U8) => quote! { #take::<{ #len }>(b, #off) },
        Kind::Array(_, p) => {
            let n = p.bytes();
            let ty = p.rust();
            quote! { ::core::array::from_fn(|i| <#ty>::from_le_bytes(#take::<#n>(b, #off + i * #n))) }
        }
        Kind::Nested(ty) => quote! { <#ty as #fixed>::read_fixed(&b[#off..])? },
    }
}

/// Statement writing `acc` (a reference to the field) at `off` of the byte array `b`.
fn write_stmt(f: &FixedField, acc: &TokenStream, off: &TokenStream, cp: &CratePath) -> TokenStream {
    let fixed = cp.item(quote! { fixed_layout::FixedLayout });
    match &f.kind {
        Kind::Prim(p) => {
            let n = p.bytes();
            quote! { b[#off..#off + #n].copy_from_slice(&(*#acc).to_le_bytes()); }
        }
        Kind::Array(len, Prim::U8) => {
            quote! { b[#off..#off + (#len)].copy_from_slice(&(*#acc)[..]); }
        }
        Kind::Array(_, p) => {
            let n = p.bytes();
            quote! {
                for (i, v) in (*#acc).iter().enumerate() {
                    b[#off + i * #n..#off + (i + 1) * #n].copy_from_slice(&v.to_le_bytes());
                }
            }
        }
        Kind::Nested(ty) => quote! { <#ty as #fixed>::write_fixed(#acc, &mut b[#off..])?; },
    }
}

/// `writeln!` calls emitting the C defines and readers of a run of fields; `c_name` is the snake name of the
/// struct or variant, `off_names` its offset consts.
fn c_fields(
    ty_ident: &Ident,
    c_name: &str,
    fields: &[FixedField],
    off_names: &[Ident],
) -> Vec<TokenStream> {
    let upper = c_name.to_uppercase();
    let mut out = Vec::new();
    for (f, off) in fields.iter().zip(off_names) {
        let fu = f.name.to_uppercase();
        let define = lit_str(&format!("#define {{}}{upper}_{fu}_OFFSET {{}}u"));
        out.push(quote! { ::core::writeln!(out, #define, up, #ty_ident::#off)?; });
        let func = format!("{c_name}_{}", f.name);
        let text = match &f.kind {
            Kind::Prim(p) => format!(
                "static inline {} {{}}{func}(const uint8_t *p) {{{{ return {}; }}}}",
                p.c_ty(),
                p.c_load("p + {}u")
            ),
            Kind::Array(_, Prim::U8) => format!(
                "static inline const uint8_t *{{}}{func}(const uint8_t *p) {{{{ return p + {{}}u; }}}}"
            ),
            Kind::Array(_, p) => format!(
                "static inline {} {{}}{func}(const uint8_t *p, size_t i) {{{{ return {}; }}}}",
                p.c_ty(),
                p.c_load(&format!("p + {{}}u + {}u * i", p.bytes()))
            ),
            Kind::Nested(_) => format!(
                "static inline const uint8_t *{{}}{func}(const uint8_t *p) {{{{ return p + {{}}u; }}}}"
            ),
        };
        let text = lit_str(&text);
        out.push(quote! { ::core::writeln!(out, #text, prefix, #ty_ident::#off)?; });
        if let Kind::Array(len, _) = &f.kind {
            let define = lit_str(&format!("#define {{}}{upper}_{fu}_COUNT {{}}u"));
            out.push(quote! { ::core::writeln!(out, #define, up, #len)?; });
        }
    }
    out
}

/// The fast path of a struct, `None` when it has no fixed layout and none is required.
pub(crate) fn fixed_struct(
    ident: &Ident,
    fields: &[Field],
    required: bool,
    cp: &CratePath,
) -> Result<Option<TokenStream>, String> {
    let Some(fields) = layout_fields(&struct_fields(fields), required)? else {
        return Ok(None);
    };
    let (defs, offs, end) = offset_consts(ident, "", &fields, quote! { 0 }, cp);
    let error = cp.error();
    let fixed = cp.item(quote! { fixed_layout::FixedLayout });
    let upper_ty = cp.item(quote! { fixed_layout::Upper });

    let mut reads = Vec::new();
    let mut writes = Vec::new();
    let mut nested_checks = Vec::new();
    for (f, off) in fields.iter().zip(&offs) {
        let field = format_ident!("{}", f.name);
        let off = quote! { #ident::#off };
        let read = read_expr(f, &off, cp);
        reads.push(quote! { #field: #read });
        writes.push(write_stmt(f, &quote! { (&self.#field) }, &off, cp));
        if let Kind::Nested(ty) = &f.kind {
            let msg = lit_str(&format!(
                "fixed_layout: field `{}` of `{ident}` has a variable length (an enum whose variants differ in size)",
                f.name
            ));
            nested_checks
                .push(quote! { const _: () = ::core::assert!(<#ty as #fixed>::SAME_LEN, #msg); });
        }
    }

    let c_name = snake(&ident.to_string());
    let c_upper = c_name.to_uppercase();
    let c_head = lit_str(&format!("\n/* {ident}: {{}} bytes */"));
    let c_len = lit_str(&format!("#define {{}}{c_upper}_LEN {{}}u"));
    let c_fields = c_fields(ident, &c_name, &fields, &offs);
    let doc_len = lit_str(&format!(" Length of `{ident}` on the wire, in bytes."));

    Ok(Some(quote! {
        #(#nested_checks)*

        impl #ident {
            #(#defs)*
            #[doc = #doc_len]
            pub const ENCODED_LEN: usize = #end;

            /// Reads the value at the start of `buf`, without a `BufReader`: the same bytes
            /// `des_shrink_wrap` reads.
            #[inline]
            pub fn from_fixed(buf: &[u8]) -> Result<Self, #error> {
                let Some(b) = buf.first_chunk::<{ #ident::ENCODED_LEN }>() else {
                    return Err(#error::OutOfBoundsReadRawSlice);
                };
                Ok(#ident { #(#reads),* })
            }

            /// Writes the value at the start of `buf`, without a `BufWriter`: the same bytes
            /// `ser_shrink_wrap` writes. Returns the length written.
            #[inline]
            pub fn to_fixed(&self, buf: &mut [u8]) -> Result<usize, #error> {
                let Some(b) = buf.first_chunk_mut::<{ #ident::ENCODED_LEN }>() else {
                    return Err(#error::OutOfBoundsWriteRawSlice);
                };
                #(#writes)*
                Ok(#ident::ENCODED_LEN)
            }
        }

        impl #fixed for #ident {
            const ENCODED_LEN: usize = #ident::ENCODED_LEN;
            const SAME_LEN: bool = true;

            #[inline]
            fn read_fixed(buf: &[u8]) -> Result<Self, #error> {
                #ident::from_fixed(buf)
            }

            #[inline]
            fn write_fixed(&self, buf: &mut [u8]) -> Result<usize, #error> {
                self.to_fixed(buf)
            }

            fn fixed_len(&self) -> usize {
                #ident::ENCODED_LEN
            }

            fn write_c(prefix: &str, out: &mut dyn ::core::fmt::Write) -> ::core::fmt::Result {
                let up = #upper_ty(prefix);
                ::core::writeln!(out, #c_head, #ident::ENCODED_LEN)?;
                ::core::writeln!(out, #c_len, up, #ident::ENCODED_LEN)?;
                #(#c_fields)*
                Ok(())
            }
        }
    }))
}

/// Enum size assumption with `fixed_layout`, and whether the enum can have a fixed layout at all.
pub(crate) fn enum_eligible(
    size_assumption: &Option<ObjectSize>,
    repr: Repr,
    variants: &[Variant],
    required: bool,
) -> Result<bool, String> {
    let size_ok = matches!(
        size_assumption,
        Some(ObjectSize::Sized { .. }) | Some(ObjectSize::SelfDescribing)
    );
    let fail = |why: &str| {
        if required {
            Err(format!("fixed_layout: {why}"))
        } else {
            Ok(false)
        }
    };
    if !size_ok {
        return fail("an enum must be sized or self_describing");
    }
    if repr_bytes(repr).is_none() {
        return fail("an enum needs ww_repr = u8, u16 or u32 (byte-aligned discriminant)");
    }
    if let Some(v) = variants.iter().find(|v| !v.cfg.is_empty()) {
        return fail(&format!("variant `{}` has a #[cfg]", v.ident));
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
        match layout_fields(&variant_fields(v), required)
            .map_err(|e| e.replacen("field", &format!("variant `{}`, field", v.ident), 1))?
        {
            Some(fields) => layouts.push((v, fields)),
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
    let mut len_names = Vec::new();
    let mut read_arms = Vec::new();
    let mut write_arms = Vec::new();
    let mut len_arms = Vec::new();
    let mut c_text = Vec::new();
    let mut nested_checks = Vec::new();
    let tag_tl = lit_str(&format!("#define {{}}{c_upper}_TAG_LEN {{}}u"));
    c_text.push(quote! { ::core::writeln!(out, #tag_tl, up, #tag_len)?; });
    let tag_fn = lit_str(&format!(
        "static inline {} {{}}{c_name}_tag(const uint8_t *p) {{{{ return {}; }}}}",
        tag_prim.c_ty(),
        tag_prim.c_load("p")
    ));
    c_text.push(quote! { ::core::writeln!(out, #tag_fn, prefix)?; });
    let mut c_len_cases = Vec::new();

    for (v, fields) in &layouts {
        let vu = snake(&v.ident.to_string()).to_uppercase();
        let vs = snake(&v.ident.to_string());
        let tag_name = format_ident!("{}_TAG", vu);
        let len_name = format_ident!("{}_LEN", vu);
        let disc = proc_macro2::Literal::u32_unsuffixed(v.discriminant);
        let (field_defs, offs, end) =
            offset_consts(ident, &format!("{vu}_"), fields, quote! { #tag_len }, cp);
        let doc_tag = lit_str(&format!(" Discriminant of `{}`.", v.ident));
        let doc_len = lit_str(&format!(" Length of `{}` on the wire, in bytes.", v.ident));
        defs.push(quote! {
            #[doc = #doc_tag]
            pub const #tag_name: #tag_ty = #disc;
            #[doc = #doc_len]
            pub const #len_name: usize = #end;
            #(#field_defs)*
        });
        len_names.push(len_name.clone());

        let v_ident = &v.ident;
        let bindings: Vec<Ident> = fields
            .iter()
            .map(|f| format_ident!("f{}", f.name.trim_start_matches('_')))
            .collect();
        let mut reads = Vec::new();
        let mut writes = Vec::new();
        for ((f, off), bind) in fields.iter().zip(&offs).zip(&bindings) {
            let off = quote! { #ident::#off };
            reads.push(read_expr(f, &off, cp));
            writes.push(write_stmt(f, &quote! { #bind }, &off, cp));
            if let Kind::Nested(ty) = &f.kind {
                let msg = lit_str(&format!(
                    "fixed_layout: field `{}` of `{ident}::{v_ident}` has a variable length",
                    f.name
                ));
                nested_checks.push(
                    quote! { const _: () = ::core::assert!(<#ty as #fixed>::SAME_LEN, #msg); },
                );
            }
        }
        let (construct, pattern) = match &v.fields {
            Fields::Named(_) => {
                let names: Vec<Ident> =
                    fields.iter().map(|f| format_ident!("{}", f.name)).collect();
                (
                    quote! { #ident::#v_ident { #(#names: #reads),* } },
                    quote! { #ident::#v_ident { #(#names: #bindings),* } },
                )
            }
            Fields::Unnamed(_) => (
                quote! { #ident::#v_ident ( #(#reads),* ) },
                quote! { #ident::#v_ident ( #(#bindings),* ) },
            ),
            Fields::Unit => (quote! { #ident::#v_ident }, quote! { #ident::#v_ident }),
        };
        let unused_b = if fields.is_empty() {
            quote! { let _ = b; }
        } else {
            quote! {}
        };
        read_arms.push(quote! {
            #disc => {
                let Some(b) = buf.first_chunk::<{ #ident::#len_name }>() else {
                    return Err(#error::OutOfBoundsReadRawSlice);
                };
                #unused_b
                Ok(#construct)
            }
        });
        write_arms.push(quote! {
            #pattern => {
                let Some(b) = buf.first_chunk_mut::<{ #ident::#len_name }>() else {
                    return Err(#error::OutOfBoundsWriteRawSlice);
                };
                b[..#tag_len].copy_from_slice(&#ident::#tag_name.to_le_bytes());
                #(#writes)*
                Ok(#ident::#len_name)
            }
        });
        len_arms.push(quote! { #pattern => #ident::#len_name, });

        let c_tag = lit_str(&format!("#define {{}}{c_upper}_{vu} {{}}u"));
        let c_len = lit_str(&format!("#define {{}}{c_upper}_{vu}_LEN {{}}u"));
        c_text.push(quote! {
            ::core::writeln!(out, #c_tag, up, #ident::#tag_name)?;
            ::core::writeln!(out, #c_len, up, #ident::#len_name)?;
        });
        c_text.extend(c_fields(ident, &format!("{c_name}_{vs}"), fields, &offs));
        let case = lit_str("    case {}u: return {}u;");
        c_len_cases
            .push(quote! { ::core::writeln!(out, #case, #ident::#tag_name, #ident::#len_name)?; });
    }

    let c_head = lit_str(&format!(
        "\n/* {ident}: a {tag_len}-byte tag, then the fields of its variant */"
    ));
    let c_len_fn = lit_str(&format!(
        "/* length of the value with this tag, 0 for an unknown tag */\nstatic inline size_t {{}}{c_name}_len(uint32_t tag) {{{{\n    switch (tag) {{{{"
    ));
    let c_len_end = lit_str("    default: return 0u;\n    }}\n}}");
    let first_len = &len_names[0];
    let rest_len = &len_names[1..];

    Ok(Some(quote! {
        #(#nested_checks)*

        impl #ident {
            /// Bytes of the discriminant.
            pub const TAG_LEN: usize = #tag_len;
            #(#defs)*
            /// Length of the longest variant on the wire, in bytes.
            pub const ENCODED_LEN: usize = {
                let mut max = #ident::#first_len;
                #(if #ident::#rest_len > max { max = #ident::#rest_len; })*
                max
            };
            /// Whether every variant has the same length.
            pub const SAME_LEN: bool = true #(&& #ident::#rest_len == #ident::#first_len)*;

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
            pub fn fixed_len(&self) -> usize {
                match self {
                    #(#len_arms)*
                }
            }
        }

        impl #fixed for #ident {
            const ENCODED_LEN: usize = #ident::ENCODED_LEN;
            const SAME_LEN: bool = #ident::SAME_LEN;

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
    }))
}
