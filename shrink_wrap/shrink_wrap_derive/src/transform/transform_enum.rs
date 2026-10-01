use crate::ast::item_enum::ItemEnum;
use crate::ast::item_enum::{Fields, Variant};
use crate::ast::repr::Repr;
use crate::transform::syn_util::{collect_docs_attrs, collect_unknown_attributes, take_since_attr};
use crate::transform::transform_struct::{change_is_ok_to_is_some, propagate_default_to_flags};
use crate::transform::util::{
    FieldPath, FieldPathRoot, check_backfill_position, check_flag_order, check_tail_bytes_position,
    create_flags, create_tuple_flags, transform_field,
};
use syn::{Expr, Lit};

impl ItemEnum {
    pub(crate) fn from_syn(item_enum: &syn::ItemEnum, repr: Repr) -> Result<Self, String> {
        let mut variants = vec![];
        let mut next_discriminant: u32 = 0;
        for variant in &item_enum.variants {
            // same numbering as Rust: implicit discriminant is the previous one + 1
            let discriminant = get_discriminant(variant)?.unwrap_or(next_discriminant);
            next_discriminant = discriminant.saturating_add(1);
            // Otherwise the discriminant is silently truncated on the wire and aliases another variant
            if discriminant > repr.max_discriminant() {
                return Err(format!(
                    "Discriminant {discriminant} of variant `{}` does not fit into ww_repr {repr:?}, max is {}",
                    variant.ident,
                    repr.max_discriminant()
                ));
            }
            let path = FieldPath::new(FieldPathRoot::EnumVariant(variant.ident.clone()));
            let fields = convert_fields(&variant.fields, &path)?;
            let mut attrs = variant.attrs.clone();
            let since = take_since_attr(&mut attrs)?;
            let docs = collect_docs_attrs(&mut attrs);
            collect_unknown_attributes(&mut attrs);
            variants.push(Variant {
                docs,
                ident: variant.ident.clone(),
                fields,
                discriminant,
                _since: since,
            });
        }
        let mut attrs = item_enum.attrs.clone();
        let docs = collect_docs_attrs(&mut attrs);
        // if add_evolve_docs {
        //     add_notes(&mut docs, size_assumption, true); TODO: add docs back
        // }

        collect_unknown_attributes(&mut attrs);
        Ok(ItemEnum {
            docs,
            // ident: item_enum.ident.clone(),
            variants,
        })
    }
}

fn get_discriminant(variant: &syn::Variant) -> Result<Option<u32>, String> {
    variant
        .discriminant
        .as_ref()
        .map(|(_, expr)| {
            if let Expr::Lit(lit) = expr {
                if let Lit::Int(lit_int) = &lit.lit {
                    let d = lit_int.base10_parse().unwrap();
                    Ok(Some(d))
                } else {
                    Err("Wrong discriminant".into())
                }
            } else {
                Err("Wrong discriminant".into())
            }
        })
        .unwrap_or(Ok(None))
}

fn convert_fields(fields: &syn::Fields, path: &FieldPath) -> Result<Fields, String> {
    match fields {
        syn::Fields::Named(fields_named) => {
            let mut named = vec![];
            let mut explicit_flags = vec![];
            for (def_order_idx, field_syn) in fields_named.named.iter().enumerate() {
                let (field, is_explicit_flag) =
                    transform_field(def_order_idx as u32, field_syn, path)?;
                if is_explicit_flag {
                    explicit_flags.push(field_syn.ident.clone().unwrap());
                }
                named.push(field)
            }
            create_flags(&mut named, &explicit_flags);
            check_flag_order(&named)?;
            check_backfill_position(&named, false)?;
            check_tail_bytes_position(&named.iter().map(|f| &f.ty).collect::<Vec<_>>())?;
            propagate_default_to_flags(&mut named)?;
            change_is_ok_to_is_some(&mut named);
            Ok(Fields::Named(named))
        }
        syn::Fields::Unnamed(fields_unnamed) => {
            let mut unnamed = vec![];
            for (def_order_idx, field) in fields_unnamed.unnamed.iter().enumerate() {
                let (field, _is_explicit_flag) =
                    transform_field(def_order_idx as u32, field, path)?;
                // TODO: Do unnamed fields have to have since, id, default, etc?
                // TODO: explicit flags in unnamed fields?
                if field.ty.contains_backfill() {
                    return Err(
                        "UVlq32Backfill must be the first field of a struct, enums are not supported"
                            .into(),
                    );
                }
                unnamed.push(field.ty);
            }
            let unnamed = create_tuple_flags(&unnamed);
            check_tail_bytes_position(&unnamed.iter().collect::<Vec<_>>())?;
            Ok(Fields::Unnamed(unnamed))
        }
        syn::Fields::Unit => Ok(Fields::Unit),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    #[test]
    fn discriminant_must_fit_into_repr() {
        let item: syn::ItemEnum = syn::parse2(quote! { enum E { A, B, C, D } }).unwrap();
        assert!(ItemEnum::from_syn(&item, Repr::U(2)).is_ok());

        let item: syn::ItemEnum = syn::parse2(quote! { enum E { A, B, C, D, F } }).unwrap();
        assert!(ItemEnum::from_syn(&item, Repr::U(2)).is_err());

        let item: syn::ItemEnum = syn::parse2(quote! { enum E { A = 15, B } }).unwrap();
        assert!(ItemEnum::from_syn(&item, Repr::Nibble).is_err());
        assert!(ItemEnum::from_syn(&item, Repr::U8).is_ok());
    }

    #[test]
    fn backfill_not_allowed_in_enums() {
        let item: syn::ItemEnum = syn::parse2(quote! { enum E { A(UVlq32Backfill) } }).unwrap();
        assert!(ItemEnum::from_syn(&item, Repr::U8).is_err());

        let item: syn::ItemEnum =
            syn::parse2(quote! { enum E { A { seq: UVlq32Backfill } } }).unwrap();
        assert!(ItemEnum::from_syn(&item, Repr::U8).is_err());
    }

    #[test]
    fn tail_bytes_must_be_last_in_variant() {
        let ok = [
            quote! { enum E<'i> { A(u8, TailBytes<'i>), B } },
            quote! { enum E { A(Option<u8>, TailBytesOwned) } },
            quote! { enum E<'i> { A { a: u8, data: TailBytes<'i> }, B { data: TailBytes<'i> } } },
        ];
        for ts in ok {
            let item: syn::ItemEnum = syn::parse2(ts).unwrap();
            assert!(ItemEnum::from_syn(&item, Repr::U8).is_ok());
        }
        let err = [
            quote! { enum E<'i> { A(TailBytes<'i>, u8) } },
            quote! { enum E { A(Option<TailBytesOwned>) } },
            quote! { enum E<'i> { A { data: TailBytes<'i>, a: u8 } } },
            quote! { enum E { A { data: Vec<TailBytesOwned> } } },
        ];
        for ts in err {
            let item: syn::ItemEnum = syn::parse2(ts).unwrap();
            assert!(ItemEnum::from_syn(&item, Repr::U8).is_err());
        }
    }
}
