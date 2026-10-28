use crate::args::FutureVersionArg;
use crate::ast::docs::Docs;
use crate::ast::generics::Generics;
use crate::ast::item_enum::ItemEnum;
use crate::ast::item_enum::{Fields, FutureVersion, Variant};
use crate::ast::object_size::ObjectSize;
use crate::ast::repr::Repr;
use crate::transform::syn_util::{
    collect_docs_attrs, collect_unknown_attributes, take_cfg_attrs, take_display_attr,
    take_flag_like_attr, take_flag_like_attr_or_cfg, take_since_attr,
};
use crate::transform::transform_struct::{change_is_ok_to_is_some, propagate_default_to_flags};
use crate::transform::util::{
    FieldPath, FieldPathRoot, check_backfill_position, check_flag_order, check_tail_bytes_position,
    check_tail_size_position, create_flags, create_tuple_flags, transform_field,
};
use proc_macro2::{Ident, Span};
use std::collections::HashSet;
use syn::{Expr, Lit, LitStr};

impl ItemEnum {
    pub(crate) fn from_syn(item_enum: &syn::ItemEnum, repr: Repr) -> Result<Self, String> {
        let mut variants = vec![];
        let mut future_version = None;
        let mut next_discriminant: u32 = 0;
        for (idx, variant) in item_enum.variants.iter().enumerate() {
            let mut attrs = variant.attrs.clone();
            if take_flag_like_attr_or_cfg(&mut attrs, "future_version") {
                future_version = Some(future_version_variant(
                    variant,
                    attrs,
                    idx + 1 == item_enum.variants.len(),
                )?);
                continue;
            }
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
            let common = take_flag_like_attr(&mut attrs, "common");
            if common {
                check_common_variant(variant, variants.iter().any(|v: &Variant| v.common))?;
            }
            let path = FieldPath::new(FieldPathRoot::EnumVariant(variant.ident.clone()));
            let (fields, unnamed_attrs) = convert_fields(&variant.fields, &path)?;
            let since = take_since_attr(&mut attrs)?;
            let cfg = take_cfg_attrs(&mut attrs)?;
            let display = take_display_attr(&mut attrs)?;
            let docs = collect_docs_attrs(&mut attrs);
            variants.push(Variant {
                docs,
                ident: variant.ident.clone(),
                fields,
                discriminant,
                _since: since,
                cfg,
                display,
                common,
                attrs,
                unnamed_attrs,
            });
        }
        if let Some(fv) = &future_version
            && let Some(v) = variants.iter().find(|v| v.discriminant > fv.max())
        {
            return Err(format!(
                "variant `{}` has discriminant {}, which the #[future_version] {}({}) can't hold: widen its field",
                v.ident, v.discriminant, fv.ident, fv.ty
            ));
        }
        let mut attrs = item_enum.attrs.clone();
        let docs = collect_docs_attrs(&mut attrs);
        // if add_evolve_docs {
        //     add_notes(&mut docs, size_assumption, true); TODO: add docs back
        // }

        attrs.retain(|a| !a.path().is_ident("repr"));
        Ok(ItemEnum {
            docs,
            attrs,
            // ident: item_enum.ident.clone(),
            variants,
            future_version,
            generics: Generics::from_syn(&item_enum.generics).with_field_types(
                item_enum
                    .variants
                    .iter()
                    .flat_map(|v| v.fields.iter().map(|f| &f.ty)),
            ),
            niche: false,
        })
    }
}

impl ItemEnum {
    /// SW-47: an `Unsized` enum gets `#[future_version] Unknown(u32)` unless it has one already, says
    /// `no_future_version`, or every discriminant of its repr is taken. `final_structure`, `self_describing` and `sized`
    /// enums only get it when written: they can't skip the fields of a newer variant.
    pub(crate) fn apply_future_version_arg(
        &mut self,
        arg: &FutureVersionArg,
        size_assumption: &Option<ObjectSize>,
        repr: Repr,
    ) -> Result<(), String> {
        if let Some(fv) = &self.future_version {
            return match arg {
                FutureVersionArg::Default => Ok(()),
                FutureVersionArg::Off => Err(format!(
                    "no_future_version, but the enum has #[future_version] {}({}): remove one of them",
                    fv.ident, fv.ty
                )),
                FutureVersionArg::Named(name) => Err(format!(
                    "future_version = {name}, but the enum has #[future_version] {}({}): remove one of them",
                    fv.ident, fv.ty
                )),
            };
        }
        let ident = match arg {
            FutureVersionArg::Off => return Ok(()),
            FutureVersionArg::Default => {
                if !matches!(size_assumption, None | Some(ObjectSize::Unsized)) {
                    return Ok(());
                }
                Ident::new("Unknown", Span::call_site())
            }
            FutureVersionArg::Named(name) => {
                if !matches!(size_assumption, None | Some(ObjectSize::Unsized)) {
                    return Err(format!(
                        "future_version = {name} works on Unsized enums only, write #[future_version] {name}(u32) \
                         as the last variant instead"
                    ));
                }
                name.clone()
            }
        };
        let used: HashSet<u32> = self.variants.iter().map(|v| v.discriminant).collect();
        if used.len() as u64 > repr.max_discriminant() as u64 {
            return match arg {
                FutureVersionArg::Named(name) => Err(format!(
                    "future_version = {name}, but every discriminant of ww_repr {repr:?} is taken"
                )),
                _ => Ok(()),
            };
        }
        if self.variants.iter().any(|v| v.ident == ident) {
            return Err(format!(
                "variant `{ident}` clashes with the variant for discriminants from a newer version (SW-47): \
                 rename it, or name the other one with future_version = <Name>, or opt out with no_future_version"
            ));
        }
        let mut docs = Docs::empty();
        docs.push(LitStr::new(
            "A variant from a newer version of this type, with its discriminant (added by `derive_shrink_wrap`).",
            Span::call_site(),
        ));
        self.future_version = Some(FutureVersion {
            docs,
            ident,
            attrs: vec![],
            ty: Ident::new("u32", Span::call_site()),
        });
        Ok(())
    }
}

/// `#[future_version] Unknown(u32)`: last, one `u32` field, no discriminant or `#[cfg]` of its own.
fn future_version_variant(
    variant: &syn::Variant,
    mut attrs: Vec<syn::Attribute>,
    is_last: bool,
) -> Result<FutureVersion, String> {
    let ident = &variant.ident;
    let err = |what: &str| {
        Err(format!(
            "#[future_version] variant `{ident}` {what}: expected `#[future_version] {ident}(u32)` (or `u16`, `u8`) as \
             the last variant"
        ))
    };
    if !is_last {
        return err("is not the last one");
    }
    if variant.discriminant.is_some() {
        return err("has an explicit discriminant, it takes none on the wire");
    }
    let syn::Fields::Unnamed(fields) = &variant.fields else {
        return err("must have one unnamed u32 field");
    };
    // DER-25: a narrower field keeps the enum small, discriminants beyond it fail to read as without the variant
    let ty = match &fields.unnamed.first().map(|f| &f.ty) {
        Some(syn::Type::Path(p)) if fields.unnamed.len() == 1 && p.qself.is_none() => p
            .path
            .get_ident()
            .filter(|i| ["u8", "u16", "u32"].contains(&i.to_string().as_str()))
            .cloned(),
        _ => None,
    };
    let Some(ty) = ty else {
        return err("must have one unnamed u32, u16 or u8 field");
    };
    if attrs.iter().any(|a| a.path().is_ident("cfg")) {
        return err("cannot have #[cfg]");
    }
    let docs = collect_docs_attrs(&mut attrs);
    Ok(FutureVersion {
        docs,
        ident: ident.clone(),
        attrs,
        ty,
    })
}

/// `#[common] Common(CommonError<'i>)`: one per enum, one unnamed field (STD-16).
fn check_common_variant(variant: &syn::Variant, has_one_already: bool) -> Result<(), String> {
    let ident = &variant.ident;
    if has_one_already {
        return Err(format!(
            "#[common] variant `{ident}`: only one variant can be #[common]"
        ));
    }
    match &variant.fields {
        syn::Fields::Unnamed(fields) if fields.unnamed.len() == 1 => Ok(()),
        _ => Err(format!(
            "#[common] variant `{ident}` must have one unnamed field: `#[common] {ident}(CommonError<'i>)`"
        )),
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

/// The variant's fields, and the attributes the derive doesn't read of each field of a tuple variant.
fn convert_fields(
    fields: &syn::Fields,
    path: &FieldPath,
) -> Result<(Fields, Vec<Vec<syn::Attribute>>), String> {
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
            check_tail_size_position(
                &named.iter().map(|f| &f.ty).collect::<Vec<_>>(),
                &named
                    .iter()
                    .map(|f| f.ident.to_string())
                    .collect::<Vec<_>>(),
            )?;
            propagate_default_to_flags(&mut named)?;
            change_is_ok_to_is_some(&mut named);
            Ok((Fields::Named(named), vec![]))
        }
        syn::Fields::Unnamed(fields_unnamed) => {
            let mut unnamed = vec![];
            let mut unnamed_attrs = vec![];
            for (def_order_idx, field) in fields_unnamed.unnamed.iter().enumerate() {
                let (field, _is_explicit_flag) =
                    transform_field(def_order_idx as u32, field, path)?;
                if field.series_opaque {
                    return Err(format!(
                        "field {def_order_idx} of a tuple variant: #[series(opaque)] is only on named fields, \
                         name the variant's fields"
                    ));
                }
                // TODO: Do unnamed fields have to have since, id, default, etc?
                // TODO: explicit flags in unnamed fields?
                if field.ty.contains_backfill() {
                    return Err(
                        "UVlq32Backfill must be the first field of a struct, enums are not supported"
                            .into(),
                    );
                }
                unnamed.push(field.ty);
                unnamed_attrs.push(field.attrs);
            }
            let unnamed = create_tuple_flags(&unnamed);
            check_tail_bytes_position(&unnamed.iter().collect::<Vec<_>>())?;
            check_tail_size_position(
                &unnamed.iter().collect::<Vec<_>>(),
                &(0..unnamed.len())
                    .map(|i| i.to_string())
                    .collect::<Vec<_>>(),
            )?;
            Ok((Fields::Unnamed(unnamed), unnamed_attrs))
        }
        syn::Fields::Unit => Ok((Fields::Unit, vec![])),
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
    fn tail_size_in_variants() {
        let ok = [
            quote! { enum E { A(u8, TailSize, Vec<u8>), B } },
            quote! { enum E { A { size: TailSize<2>, s: String }, B { size: TailSize } } },
        ];
        for ts in ok {
            let item: syn::ItemEnum = syn::parse2(ts).unwrap();
            assert!(ItemEnum::from_syn(&item, Repr::U8).is_ok());
        }
        let err = [
            quote! { enum E { A(Vec<u8>, TailSize) } },
            quote! { enum E { A { s: String, size: TailSize } } },
            quote! { enum E { A(TailSize, TailSize) } },
            quote! { enum E { A(Option<TailSize>) } },
        ];
        for ts in err {
            let item: syn::ItemEnum = syn::parse2(ts).unwrap();
            assert!(ItemEnum::from_syn(&item, Repr::U8).is_err());
        }
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

    #[test]
    fn future_version_by_default() {
        use crate::args::FutureVersionArg as A;
        use syn::parse_quote;
        let apply = |item: syn::ItemEnum, arg: A, size: Option<ObjectSize>, repr: Repr| {
            let mut e = ItemEnum::from_syn(&item, repr).unwrap();
            e.apply_future_version_arg(&arg, &size, repr)
                .map(|()| e.future_version.map(|fv| fv.ident.to_string()))
        };
        let named = |n: &str| A::Named(Ident::new(n, Span::call_site()));
        let ab: syn::ItemEnum = parse_quote! { enum E { A, B } };
        let u8r = Repr::U8;
        assert_eq!(
            apply(ab.clone(), A::Default, None, u8r),
            Ok(Some("Unknown".into()))
        );
        assert_eq!(apply(ab.clone(), A::Off, None, u8r), Ok(None));
        assert_eq!(
            apply(ab.clone(), named("Newer"), None, u8r),
            Ok(Some("Newer".into()))
        );
        // other size assumptions only when written
        for size in [
            ObjectSize::SelfDescribing,
            ObjectSize::UnsizedFinalStructure,
            ObjectSize::sized(0),
        ] {
            assert_eq!(
                apply(ab.clone(), A::Default, Some(size.clone()), u8r),
                Ok(None)
            );
            assert!(apply(ab.clone(), named("Newer"), Some(size), u8r).is_err());
        }
        // every discriminant taken
        assert_eq!(apply(ab.clone(), A::Default, None, Repr::U(1)), Ok(None));
        assert!(apply(ab, named("Newer"), None, Repr::U(1)).is_err());
        // a written one stays, an argument next to it is a contradiction
        let written: syn::ItemEnum = parse_quote! { enum E { A, #[future_version] Later(u32) } };
        assert_eq!(
            apply(written.clone(), A::Default, None, u8r),
            Ok(Some("Later".into()))
        );
        assert!(apply(written.clone(), A::Off, None, u8r).is_err());
        assert!(apply(written, named("Newer"), None, u8r).is_err());
        // a clash with a user variant names the fix
        let clash: syn::ItemEnum = parse_quote! { enum E { A, Unknown } };
        assert!(
            apply(clash.clone(), A::Default, None, u8r)
                .unwrap_err()
                .contains("future_version = <Name>")
        );
        assert_eq!(
            apply(clash, named("Newer"), None, u8r),
            Ok(Some("Newer".into()))
        );
    }

    #[test]
    fn future_version_rules() {
        let parse = |ts| ItemEnum::from_syn(&syn::parse2::<syn::ItemEnum>(ts).unwrap(), Repr::U8);
        let item = parse(quote! { enum E { A, B, #[future_version] Unknown(u32) } }).unwrap();
        assert_eq!(item.variants.len(), 2);
        assert_eq!(item.future_version.unwrap().ident, "Unknown");
        // DER-25: a narrower field, as long as it holds every known discriminant
        let item = parse(quote! { enum E { A, B, #[future_version] Unknown(u16) } }).unwrap();
        assert_eq!(item.future_version.unwrap().max(), u16::MAX.into());
        let err = [
            quote! { enum E { #[future_version] Unknown(u32), A } },
            quote! { enum E { A, #[future_version] Unknown(i8) } },
            quote! { enum E { A, B = 300, #[future_version] Unknown(u8) } },
            quote! { enum E { A, #[future_version] Unknown } },
            quote! { enum E { A, #[future_version] Unknown { d: u32 } } },
            quote! { enum E { A, #[future_version] Unknown(u32, u8) } },
            quote! { enum E { A, #[future_version] Unknown(u32) = 7 } },
            quote! { enum E { A, #[future_version] #[cfg(feature = "x")] Unknown(u32) } },
        ];
        for ts in err {
            assert!(parse(ts).is_err());
        }
    }
}
