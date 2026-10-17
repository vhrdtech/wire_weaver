use proc_macro2::Ident;
use quote::{format_ident, quote};
use syn::parse2;

use crate::ast::generics::Generics;
use crate::ast::item_struct::Field;
use crate::ast::item_struct::{ItemStruct, StructKind};
use crate::ast::ty::Type;
use crate::transform::syn_util::{
    collect_docs_attrs, collect_unknown_attributes, take_display_attr, take_slot_attr,
};
use crate::transform::util::{
    FieldPath, FieldPathRoot, check_backfill_position, check_flag_order, check_tail_bytes_position,
    check_tail_size_position, create_flags, transform_field,
};

impl ItemStruct {
    /// `sparse`: the struct has the `sparse` directive (DER-13), see [Self::from_syn_sparse].
    pub(crate) fn from_syn(item_struct: &syn::ItemStruct, sparse: bool) -> Result<Self, String> {
        if sparse {
            return Self::from_syn_sparse(item_struct);
        }
        if let Some(field) = item_struct
            .fields
            .iter()
            .enumerate()
            .find(|(_, f)| f.attrs.iter().any(|a| a.path().is_ident("slot")))
        {
            return Err(format!(
                "#[slot = N] on `{}`: only the fields of a `sparse` struct have slots",
                field_name(field.1, field.0)
            ));
        }
        let kind = StructKind::from_syn(&item_struct.fields);
        let mut fields = vec![];
        let mut explicit_flags = vec![];
        for (def_order_idx, field_syn) in item_struct.fields.iter().enumerate() {
            let path = FieldPath::new(FieldPathRoot::NamedField(field_ident(
                field_syn,
                def_order_idx,
            )));
            if kind == StructKind::Tuple
                && field_syn.attrs.iter().any(|a| a.path().is_ident("flag"))
            {
                return Err(format!(
                    "#[flag] on field {def_order_idx} of tuple struct `{}`: a relocated flag names the Option or \
                     Result field it belongs to, a tuple struct has no names. Name the fields",
                    item_struct.ident
                ));
            }
            let (field, is_explicit_flag) =
                transform_field(def_order_idx as u32, field_syn, &path)?;
            if is_explicit_flag {
                explicit_flags.push(field_ident(field_syn, def_order_idx));
            }
            fields.push(field);
        }
        let mut attrs = item_struct.attrs.clone();
        let docs = collect_docs_attrs(&mut attrs);
        // if add_evolve_docs {
        //     add_notes(&mut docs, size_assumption, false);
        // }
        let display = take_display_attr(&mut attrs)?;

        collect_unknown_attributes(&mut attrs);
        create_flags(&mut fields, &explicit_flags);
        check_flag_order(&fields)?;
        check_backfill_position(&fields, true)?;
        check_tail_bytes_position(&fields.iter().map(|f| &f.ty).collect::<Vec<_>>())?;
        check_tail_size_position(
            &fields.iter().map(|f| &f.ty).collect::<Vec<_>>(),
            &fields
                .iter()
                .map(|f| f.ident.to_string())
                .collect::<Vec<_>>(),
        )?;
        propagate_default_to_flags(&mut fields)?;
        change_is_ok_to_is_some(&mut fields);
        Ok(ItemStruct {
            docs,
            // ident: item_struct.ident.clone(),
            fields,
            display,
            sparse_slots: None,
            kind,
            generics: Generics::from_syn(&item_struct.generics),
        })
    }

    /// A `sparse` struct (DER-13): every field is an `Option<T>`, written as one slot of an `OptionAnyVec`, so there
    /// are no flag fields. A field's slot is its `#[slot = N]`, or the one after the previous field's (0 for the
    /// first); slots increase, gaps are allowed.
    fn from_syn_sparse(item_struct: &syn::ItemStruct) -> Result<Self, String> {
        let mut fields = vec![];
        let mut slots: Vec<u32> = vec![];
        for (def_order_idx, field_syn) in item_struct.fields.iter().enumerate() {
            let name = field_name(field_syn, def_order_idx);
            let mut field_syn = field_syn.clone();
            let slot = take_slot_attr(&mut field_syn.attrs)
                .map_err(|e| format!("sparse field `{name}`: {e}"))?;
            for attr in ["flag", "default"] {
                if field_syn.attrs.iter().any(|a| a.path().is_ident(attr)) {
                    return Err(format!(
                        "sparse field `{name}`: #[{attr}] is not used in a sparse struct, every field is a slot that \
                         can be absent (a field added later is None in older data)"
                    ));
                }
            }
            let next = match slots.last() {
                Some(last) => last.checked_add(1),
                None => Some(0),
            };
            let slot = match (slot, next) {
                (Some(slot), Some(next)) if slot < next => {
                    return Err(format!(
                        "sparse field `{name}`: #[slot = {slot}] must be after the previous field's slot {}",
                        next - 1
                    ));
                }
                (Some(slot), _) => slot,
                (None, Some(next)) => next,
                (None, None) => return Err(format!("sparse field `{name}`: slot past u32::MAX")),
            };
            if slot == u32::MAX {
                return Err(format!("sparse field `{name}`: slots are below u32::MAX"));
            }
            let path = FieldPath::new(FieldPathRoot::NamedField(field_ident(
                &field_syn,
                def_order_idx,
            )));
            let (field, _) = transform_field(def_order_idx as u32, &field_syn, &path)?;
            if !matches!(field.ty, Type::Option(_, _)) {
                return Err(format!(
                    "sparse field `{name}` must be an Option<T>: every field of a sparse struct is a slot that can \
                     be absent"
                ));
            }
            if field.ty.contains_tail_size()
                || field.ty.contains_tail_bytes()
                || field.ty.contains_backfill()
            {
                return Err(format!(
                    "sparse field `{name}`: TailSize, TailBytes and UVlq32Backfill can't be a slot"
                ));
            }
            slots.push(slot);
            fields.push(field);
        }
        let mut attrs = item_struct.attrs.clone();
        let docs = collect_docs_attrs(&mut attrs);
        let display = take_display_attr(&mut attrs)?;
        collect_unknown_attributes(&mut attrs);
        Ok(ItemStruct {
            docs,
            fields,
            display,
            sparse_slots: Some(slots),
            kind: StructKind::from_syn(&item_struct.fields),
            generics: Generics::from_syn(&item_struct.generics),
        })
    }
}

/// The field's name, `_0`, `_1`, ... for the fields of a tuple struct (SW-59).
fn field_ident(field: &syn::Field, def_order_idx: usize) -> Ident {
    field
        .ident
        .clone()
        .unwrap_or_else(|| format_ident!("_{def_order_idx}"))
}

fn field_name(field: &syn::Field, def_order_idx: usize) -> String {
    field_ident(field, def_order_idx).to_string()
}

/// Fields with `#[default = ..]` were added in a later version: they and the fields after them can be missing
/// from older data. The flag of an evolved `Option` or `Result` gets a default too, so its absence is noticed.
pub(crate) fn propagate_default_to_flags(fields: &mut [Field]) -> Result<(), String> {
    let mut evolved = vec![];
    let mut default_found = false;
    let mut default_is_not_last = false;
    for f in fields.iter() {
        if f.default.is_none() {
            if default_found && !matches!(f.ty, Type::IsSome(_) | Type::IsOk(_)) {
                default_is_not_last = true;
            }
            continue;
        }
        default_found = true;
        if matches!(f.ty, Type::TailSize(_) | Type::IsSome(_) | Type::IsOk(_)) {
            return Err(format!(
                "#[default = ...] on `{}`: a TailSize or flag field cannot be added later",
                f.ident
            ));
        }
        if f.ty.contains_tail_bytes() || f.ty.contains_backfill() {
            return Err(format!(
                "#[default = ...] on `{}`: TailBytes and UVlq32Backfill fields cannot be added later",
                f.ident
            ));
        }
        evolved.push(f.ident.clone());
    }
    for f in fields.iter_mut() {
        if let Type::IsSome(value_ident) | Type::IsOk(value_ident) = &f.ty
            && evolved.contains(value_ident)
        {
            // read as false when missing, the value field then becomes its default
            f.default = Some(parse2(quote! { false }).unwrap());
        }
    }
    if default_is_not_last {
        return Err("Wrong evolved type order (default is not last)".into());
    }
    Ok(())
}

/// Change IsOk to IsSome for explicit flags, as a full field list is needed to determine which one to use.
pub(crate) fn change_is_ok_to_is_some(fields: &mut [Field]) {
    let mut flip = vec![];
    for (idx, f) in fields.iter().enumerate() {
        let Type::IsOk(ident) = &f.ty else { continue };
        if fields
            .iter()
            .any(|f| (f.ident == *ident) && matches!(f.ty, Type::Option(_, _)))
        {
            flip.push(idx);
        }
    }
    for (idx, f) in fields.iter_mut().enumerate() {
        if flip.contains(&idx) {
            let Type::IsOk(ident) = &f.ty else { continue };
            f.ty = Type::IsSome(ident.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn from_tokens(ts: proc_macro2::TokenStream) -> Result<ItemStruct, String> {
        let item: syn::ItemStruct = syn::parse2(ts).map_err(|e| e.to_string())?;
        ItemStruct::from_syn(&item, false)
    }

    fn err_of(ts: proc_macro2::TokenStream, sparse: bool) -> String {
        let item: syn::ItemStruct = syn::parse2(ts).unwrap();
        ItemStruct::from_syn(&item, sparse).unwrap_err()
    }

    #[test]
    fn tuple_and_unit_structs() {
        let s = from_tokens(quote! { struct S(u8, Option<u16>, #[default = 3] u8); }).unwrap();
        assert_eq!(s.kind, StructKind::Tuple);
        let names: Vec<_> = s.fields.iter().map(|f| f.ident.to_string()).collect();
        // the implicit flag of `_1` is in front of it
        assert_eq!(names, ["_0", "__1_flag", "_1", "_2"]);
        assert_eq!(
            StructKind::Tuple.member(&s.fields[2].ident).to_string(),
            "1"
        );
        assert_eq!(
            StructKind::Named.member(&s.fields[2].ident).to_string(),
            "_1"
        );
        let s = from_tokens(quote! { struct S; }).unwrap();
        assert_eq!(s.kind, StructKind::Unit);
        assert!(s.fields.is_empty());
        let s = from_tokens(quote! { struct S(); }).unwrap();
        assert_eq!(s.kind, StructKind::Tuple);
    }

    #[test]
    fn tuple_struct_refusals() {
        let e = err_of(quote! { struct Pair(#[flag] bool, Option<u8>); }, false);
        assert!(
            e.contains("#[flag] on field 0 of tuple struct `Pair`"),
            "{e}"
        );
        let e = err_of(quote! { struct S(#[slot = 1] Option<u8>); }, false);
        assert!(e.contains("#[slot = N] on `_0`"), "{e}");
        let e = err_of(quote! { struct S(Option<u8>, u8); }, true);
        assert!(e.contains("sparse field `_1` must be an Option<T>"), "{e}");
        let e = err_of(quote! { struct S(#[default = 1] u8, u8); }, false);
        assert!(e.contains("default is not last"), "{e}");
        let e = err_of(quote! { struct S(Vec<u8>, TailSize); }, false);
        assert!(e.contains("field `_0` before the TailSize slot"), "{e}");
    }

    #[test]
    fn backfill_must_be_first() {
        assert!(from_tokens(quote! { struct S { seq: UVlq32Backfill, a: u8 } }).is_ok());
        assert!(
            from_tokens(quote! { struct S { seq: shrink_wrap::UVlq32Backfill, a: u8 } }).is_ok()
        );
        assert!(from_tokens(quote! { struct S { a: u8, seq: UVlq32Backfill } }).is_err());
        // is_some flag of an Option is written in front of it, bool is written first as well
        assert!(from_tokens(quote! { struct S { a: bool, seq: UVlq32Backfill } }).is_err());
        assert!(from_tokens(quote! { struct S { seq: Option<UVlq32Backfill> } }).is_err());
        assert!(from_tokens(quote! { struct S { seq: Vec<UVlq32Backfill> } }).is_err());
        assert!(from_tokens(quote! { struct S { seq: (UVlq32Backfill, u8) } }).is_err());
        assert!(from_tokens(quote! { struct S { seq: [UVlq32Backfill; 1] } }).is_err());
        assert!(from_tokens(quote! { struct S { a: UVlq32Backfill, b: UVlq32Backfill } }).is_err());
    }

    #[test]
    fn generic_external_types() {
        use crate::ast::ty::Type;
        let s = from_tokens(
            quote! { struct S<'i> { a: Foo<u8>, b: Bar<'i, f32>, c: a::b::Baz<u8, 4>, d: Plain } },
        )
        .unwrap();
        let ext = |i: usize| match &s.fields[i].ty {
            Type::External(path, is_lifetime) => (path.clone(), *is_lifetime),
            other => panic!("{other:?}"),
        };
        let (a, a_lt) = ext(0);
        assert_eq!(a.args.len(), 1);
        assert!(!a_lt);
        assert_eq!(a.tokens(None).to_string(), "Foo < u8 >");
        let (mut b, b_lt) = ext(1);
        assert!(b_lt);
        assert_eq!(b.tokens(None).to_string(), "Bar < 'i , f32 >");
        assert_eq!(
            b.tokens(Some(quote! { 'static })).to_string(),
            "Bar < 'static , f32 >"
        );
        b.make_owned();
        assert_eq!(b.tokens(None).to_string(), "BarOwned < f32 >");
        let (c, _) = ext(2);
        assert_eq!(c.tokens(None).to_string(), "a :: b :: Baz < u8 , 4 >");
        let (mut d, _) = ext(3);
        assert_eq!(d.tokens(None).to_string(), "Plain");
        assert_eq!(d.tokens(Some(quote! { 'i })).to_string(), "Plain < 'i >");
        d.make_owned();
        assert_eq!(d.tokens(None).to_string(), "PlainOwned");
    }

    #[test]
    fn tail_size_rules() {
        assert!(from_tokens(quote! { struct S { size: TailSize, a: u8 } }).is_ok());
        assert!(from_tokens(quote! { struct S { a: u8, size: TailSize<2>, b: u8 } }).is_ok());
        assert!(
            from_tokens(quote! { struct S { a: u8, size: shrink_wrap::TailSize<2>, b: u8 } })
                .is_ok()
        );
        assert!(from_tokens(quote! { struct S { a: u8, size: TailSize<N>, b: u8 } }).is_ok());
        assert!(from_tokens(quote! { struct S { a: u8, size: TailSize<{ 2 }>, b: u8 } }).is_ok());
        // anything Sized or SelfDescribing before it: bool, Option flag, UNib32, Option<u8>, a plain external
        assert!(
            from_tokens(quote! { struct S { a: bool, b: Option<u8>, c: UNib32, d: Ext, size: TailSize, e: Vec<u8> } })
                .is_ok()
        );
        // the fields before it must not keep their length at the back of the buffer
        assert!(from_tokens(quote! { struct S { a: Vec<u8>, size: TailSize } }).is_err());
        assert!(from_tokens(quote! { struct S { a: String, size: TailSize } }).is_err());
        assert!(from_tokens(quote! { struct S { a: Option<Vec<u8>>, size: TailSize } }).is_err());
        assert!(from_tokens(quote! { struct S { a: (u8, String), size: TailSize } }).is_err());
        assert!(from_tokens(quote! { struct S { a: RefBox<'i, u8>, size: TailSize } }).is_err());
        // plain field only, one per struct
        assert!(from_tokens(quote! { struct S { size: Option<TailSize> } }).is_err());
        assert!(from_tokens(quote! { struct S { size: Vec<TailSize> } }).is_err());
        assert!(from_tokens(quote! { struct S { size: (TailSize, u8) } }).is_err());
        assert!(from_tokens(quote! { struct S { size: [TailSize; 1] } }).is_err());
        assert!(from_tokens(quote! { struct S { a: TailSize, b: TailSize } }).is_err());
        assert!(from_tokens(quote! { struct S { size: TailSize(u8) } }).is_err());
        assert!(from_tokens(quote! { struct S { size: TailSize<2, 3> } }).is_err());
    }

    #[test]
    fn array_len_const_or_literal() {
        assert!(from_tokens(quote! { struct S { a: [u8; 4] } }).is_ok());
        assert!(from_tokens(quote! { struct S { a: [u8; N] } }).is_ok());
        assert!(from_tokens(quote! { struct S { a: [u8; crate::N * 2] } }).is_ok());
        assert!(from_tokens(quote! { struct S { a: [u8; { 1 + 2 }] } }).is_ok());
        assert!(from_tokens(quote! { struct S { a: [u8; "x"] } }).is_err());
    }

    #[test]
    fn tail_bytes_must_be_last() {
        assert!(from_tokens(quote! { struct S<'i> { a: u8, data: TailBytes<'i> } }).is_ok());
        assert!(from_tokens(quote! { struct S { a: u8, data: TailBytesOwned } }).is_ok());
        assert!(
            from_tokens(quote! { struct S<'i> { a: Option<u8>, data: shrink_wrap::tail_bytes::TailBytes<'i> } })
                .is_ok()
        );
        assert!(from_tokens(quote! { struct S<'i> { data: TailBytes<'i> } }).is_ok());
        assert!(from_tokens(quote! { struct S<'i> { data: TailBytes<'i>, a: u8 } }).is_err());
        assert!(
            from_tokens(quote! { struct S<'i> { data: TailBytes<'i>, a: Option<u8> } }).is_err()
        );
        assert!(from_tokens(quote! { struct S { data: Option<TailBytesOwned> } }).is_err());
        assert!(from_tokens(quote! { struct S { data: Vec<TailBytesOwned> } }).is_err());
        assert!(from_tokens(quote! { struct S<'i> { data: (u8, TailBytes<'i>) } }).is_err());
        assert!(from_tokens(quote! { struct S<'i> { data: [TailBytes<'i>; 1] } }).is_err());
        assert!(
            from_tokens(quote! { struct S<'i> { a: TailBytes<'i>, b: TailBytes<'i> } }).is_err()
        );
    }
}
