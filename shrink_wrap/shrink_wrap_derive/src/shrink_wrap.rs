use proc_macro2::{Ident, Span, TokenStream};
use quote::{ToTokens, TokenStreamExt, format_ident, quote};
use syn::{File, Item, Path, parse2};

use crate::{
    args::Args,
    ast::{
        generics::Generics, item_enum::ItemEnum, item_struct::ItemStruct, object_size::ObjectSize,
        repr::Repr, ty::Type,
    },
    codegen::{
        fixed_layout, head_tail, item_enum::CGItemEnum, item_struct::CGItemStruct,
        series::SeriesSeed, util::CratePath,
    },
    transform::cfg_attr::{cfg_guards, expand_read_cfg_attrs},
    transform::docs_util::add_notes,
};

/// How generated code reaches the `shrink_wrap` crate: `crate_path(..)` if given, else the name it has in the
/// user's `Cargo.toml` (`::shrink_wrap`, also inside `shrink_wrap` itself thanks to `extern crate self`), else
/// through `wire_weaver`'s re-export when only that is a dependency. `None` when neither is found: names are left
/// unqualified, for `use shrink_wrap::prelude::*` in the user's module to resolve, as before.
fn crate_path(explicit: Option<TokenStream>) -> CratePath {
    use proc_macro_crate::{FoundCrate, crate_name};
    if let Some(path) = explicit {
        return CratePath(Some(path));
    }
    let found = match crate_name("shrink_wrap") {
        Ok(FoundCrate::Itself) => Some(quote::quote! { ::shrink_wrap }),
        Ok(FoundCrate::Name(name)) => {
            let name = Ident::new(&name, Span::call_site());
            Some(quote::quote! { ::#name })
        }
        Err(_) => match crate_name("wire_weaver") {
            Ok(FoundCrate::Itself) => Some(quote::quote! { crate::shrink_wrap }),
            Ok(FoundCrate::Name(name)) => {
                let name = Ident::new(&name, Span::call_site());
                Some(quote::quote! { ::#name::shrink_wrap })
            }
            Err(_) => None,
        },
    };
    CratePath(found)
}

pub fn shrink_wrap_attr(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut file = match syn::parse2::<File>(item) {
        Ok(f) => f,
        Err(e) => return e.to_compile_error(),
    };
    let Some(item) = file.items.pop() else {
        return syn::Error::new(Span::call_site(), "Expected one item (enum or struct)")
            .to_compile_error();
    };
    let args: Args = match parse2(attr) {
        Ok(args) => args,
        Err(e) => return e.to_compile_error(),
    };
    shrink_wrap_attr_inner(item, args)
        .unwrap_or_else(|e| syn::Error::new(Span::call_site(), e).to_compile_error())
}

pub fn shrink_wrap_derive(item: TokenStream) -> TokenStream {
    let file = match syn::parse2::<File>(item) {
        Ok(f) => f,
        Err(e) => return e.to_compile_error(),
    };
    shrink_wrap_derive_inner(file)
        .unwrap_or_else(|e| syn::Error::new(Span::call_site(), e).to_compile_error())
}

fn shrink_wrap_attr_inner(mut item: Item, args: Args) -> Result<TokenStream, String> {
    let (ty_kind, name) = TyKind::from_item(&item)?;
    let cfg_preds = expand_read_cfg_attrs(&mut item)?;
    let guards = cfg_guards(&name, &cfg_preds);
    // a head_tail type has one version, nothing to choose
    if matches!(ty_kind, TyKind::Ambiguous)
        && args.borrowed.is_none()
        && args.owned.is_none()
        && !args.head_tail
    {
        return Err(
            "Ambiguous declaration, add `Owned` to type name, add borrowed or owned directive, or use PhantomData to capture a lifetime".into(),
        );
    }
    let mut ts = generate_inner(item, ty_kind, name, args)?;
    ts.append_all(guards);
    Ok(ts)
}

fn generate_inner(
    item: Item,
    ty_kind: TyKind,
    name: String,
    mut args: Args,
) -> Result<TokenStream, String> {
    if args.sparse {
        check_sparse_args(&item, &args)?;
    }
    if args.transparent {
        check_transparent_args(&item, &args)?;
    }
    check_fieldless_struct_args(&item, &args)?;
    check_series_args(&item, &args)?;
    check_type_params_args(&item, &args)?;
    if args.validate.is_some()
        && (!matches!(item, Item::Struct(_)) || args.sparse || args.fixed_layout || args.head_tail)
    {
        return Err(
            "validate: only a plain struct (not sparse, fixed_layout or head_tail) can have a validate function"
                .into(),
        );
    }
    if args.head_tail {
        return head_tail_inner(item, ty_kind, name, args);
    }
    // `fixed_layout` implies `sized` on a struct, before the size assumption goes into the generated code
    let fixed_struct = match &item {
        Item::Struct(item_struct) => fixed_layout::struct_size_assumption(
            &mut args.size_assumption,
            args.fixed_layout,
            last_field_is_flagged(item_struct),
        )?,
        _ => false,
    };
    let fixed_required = args.fixed_layout;
    let do_generate_borrowed = matches!(ty_kind, TyKind::ImpliedRef) | args.borrowed.is_some();
    let ambiguous = matches!(ty_kind, TyKind::Ambiguous);
    let crate_path = crate_path(args.crate_path);
    // kept for `display`'s error messages, below `name` is moved into the borrowed/owned CGSeeds
    let type_name = name.clone();
    let generate_borrowed = if do_generate_borrowed {
        Some(
            CGSeed::new(
                item_name(name.clone(), ty_kind, true),
                args.borrowed.flatten(),
                args.cfg_attr_borrowed,
                &args.cfg_attr,
                args.derive_borrowed.clone(),
                &args.derive,
                args.size_assumption.clone(),
                ambiguous,
                &crate_path,
            )
            .base_if_named(&name),
        )
    } else {
        None
    };

    let generate_owned = matches!(ty_kind, TyKind::ExplicitOwned) | args.owned.is_some();
    let generate_owned = if generate_owned {
        Some(
            CGSeed::new(
                item_name(name, ty_kind, false),
                args.owned.flatten(),
                args.cfg_attr_owned,
                &args.cfg_attr,
                args.derive_owned,
                &args.derive,
                args.size_assumption.clone(),
                ambiguous,
                &crate_path,
            )
            .base_if_named(&type_name),
        )
    } else {
        None
    };

    let mut ts = TokenStream::new();
    match &item {
        Item::Enum(item_enum) => {
            let Some(repr) = args.ww_repr else {
                return Err("For enums, ww_repr must be specified".into());
            };
            let mut ww_item_enum = ItemEnum::from_syn(item_enum, repr)?;
            ww_item_enum.apply_future_version_arg(
                &args.future_version,
                &args.size_assumption,
                repr,
            )?;
            let has_tail_size = ww_item_enum.variants.iter().any(|v| match &v.fields {
                crate::ast::item_enum::Fields::Named(named) => {
                    named.iter().any(|f| f.ty.contains_tail_size())
                }
                crate::ast::item_enum::Fields::Unnamed(unnamed) => {
                    unnamed.iter().any(|ty| ty.contains_tail_size())
                }
                crate::ast::item_enum::Fields::Unit => false,
            });
            check_tail_size_unsized(has_tail_size, &args.size_assumption)?;
            ts.append_all(wire_cfg_guards(
                &type_name,
                ww_item_enum.variants.iter().flat_map(|v| match &v.fields {
                    crate::ast::item_enum::Fields::Named(named) => {
                        named.iter().map(|f| &f.ty).collect::<Vec<_>>()
                    }
                    crate::ast::item_enum::Fields::Unnamed(unnamed) => unnamed.iter().collect(),
                    crate::ast::item_enum::Fields::Unit => vec![],
                }),
            ));
            // otherwise the size would be the discriminant alone with every such variant compiled out
            if matches!(args.size_assumption, Some(ObjectSize::Sized { .. }))
                && ww_item_enum.variants.iter().all(|v| !v.cfg.is_empty())
            {
                return Err(
                    "sized enum needs at least one variant without #[cfg]: its size must not depend on features"
                        .into(),
                );
            }
            add_notes(&mut ww_item_enum.docs, args.size_assumption.clone(), true);
            if args.display {
                for v in &ww_item_enum.variants {
                    if v.display_message().is_none() {
                        return Err(format!(
                            "{type_name}::{}: display requires a doc comment (its first line is the message) \
                             or #[display(\"..\")]",
                            v.ident
                        ));
                    }
                }
            }
            // A single `MyTypeDiscriminants` enum is emitted (named off whichever of
            // borrowed/owned is generated, preferring borrowed), never mutating `ww_item_enum`
            // itself since it's still needed below to generate the real borrowed/owned types.
            if args.discriminants_enum {
                let seed = generate_borrowed.as_ref().or(generate_owned.as_ref());
                if let Some(seed) = seed {
                    let mut discriminants = ww_item_enum.clone();
                    discriminants.make_discriminants();
                    let discriminants_ident = format_ident!("{}Discriminants", seed.ident);
                    let mut cg_discriminants =
                        seed.cg_enum_with_ident(&discriminants_ident, &discriminants, repr);
                    // The discriminants enum is never (de)serialized on its own (only its
                    // `.discriminant()` inherent method is used), so the parent's size assumption
                    // (which asserts on Ser/DeserializeShrinkWrap impls that are never generated
                    // for it) doesn't apply here.
                    cg_discriminants.size_assumption = None;
                    ts.append_all(cg_discriminants.def_rust(false));
                    ts.append_all(cg_discriminants.impl_discriminant(false));
                }
            }

            // a generic enum's layout depends on its parameters (DER-14)
            if ww_item_enum.generics.is_empty()
                && fixed_layout::enum_eligible(
                    &args.size_assumption,
                    repr,
                    &ww_item_enum.variants,
                    ww_item_enum.future_version.is_some(),
                    fixed_required,
                )?
            {
                for ident in fixed_idents(&generate_borrowed, &generate_owned, ty_kind) {
                    if let Some(fast) = fixed_layout::fixed_enum(
                        ident,
                        repr,
                        &ww_item_enum.variants,
                        fixed_required,
                        &crate_path,
                    )? {
                        ts.append_all(fast);
                    }
                }
            }

            if let Some(borrowed) = generate_borrowed {
                let cg_item_enum = borrowed.cg_enum(&ww_item_enum, repr);
                ts.append_all(cg_item_enum.def_rust(true));
                ts.append_all(cg_item_enum.impl_discriminant(true));
                ts.append_all(cg_item_enum.serdes_rust(true));
                if args.display {
                    ts.append_all(cg_item_enum.display_and_error_rust(true));
                }
                ts.append_all(cg_item_enum.impl_common(true)?);
                if args.series {
                    ts.append_all(
                        borrowed
                            .series_seed(
                                true,
                                &ww_item_enum.generics,
                                args.series_store,
                                args.series_cells,
                            )
                            .series_enum(
                                &ww_item_enum.variants,
                                future_version_ident(&ww_item_enum),
                            )?,
                    );
                }
            }

            if let Some(owned) = generate_owned {
                ww_item_enum.make_owned();
                let cg_item_enum = owned.cg_enum(&ww_item_enum, repr);
                // skip when type consists of only plain types, as it is the same one as borrowed then
                if !(matches!(ty_kind, TyKind::Ambiguous) && do_generate_borrowed) {
                    ts.append_all(cg_item_enum.def_rust(false));
                    ts.append_all(cg_item_enum.impl_discriminant(false));
                }
                ts.append_all(cg_item_enum.serdes_rust(false));
                if args.display && !(matches!(ty_kind, TyKind::Ambiguous) && do_generate_borrowed) {
                    ts.append_all(cg_item_enum.display_and_error_rust(false));
                }
                // an ambiguous type is the same one as borrowed, which has the impls already
                if !(matches!(ty_kind, TyKind::Ambiguous) && do_generate_borrowed) {
                    ts.append_all(cg_item_enum.impl_common(false)?);
                    if args.series {
                        ts.append_all(
                            owned
                                .series_seed(
                                    false,
                                    &ww_item_enum.generics,
                                    args.series_store,
                                    args.series_cells,
                                )
                                .series_enum(
                                    &ww_item_enum.variants,
                                    future_version_ident(&ww_item_enum),
                                )?,
                        );
                    }
                }
            }
        }
        Item::Struct(item_struct) => {
            if args.ww_repr.is_some() {
                return Err("structs must not have a ww_repr".into());
            }
            let mut ww_item_struct = ItemStruct::from_syn(item_struct, args.sparse)?;
            let transparent = if args.transparent {
                Some(transparent_field(&ww_item_struct, &type_name)?)
            } else {
                None
            };
            let has_tail_size = ww_item_struct
                .fields
                .iter()
                .any(|f| f.ty.contains_tail_size());
            check_tail_size_unsized(has_tail_size, &args.size_assumption)?;
            ts.append_all(wire_cfg_guards(
                &type_name,
                ww_item_struct.fields.iter().map(|f| &f.ty),
            ));
            // computed before `add_notes` below adds its own doc lines, which would otherwise count as the
            // struct's "first doc line"
            let display_message = args.display.then(|| ww_item_struct.display_message());
            if let Some(None) = display_message {
                return Err(format!(
                    "{type_name}: display requires a doc comment (its first line is the message) \
                     or #[display(\"..\")]"
                ));
            }
            let display_message = display_message.flatten();
            add_notes(
                &mut ww_item_struct.docs,
                args.size_assumption.clone(),
                false,
            );

            // a generic struct's layout depends on its parameters (DER-14)
            if fixed_struct && ww_item_struct.generics.is_empty() {
                for ident in fixed_idents(&generate_borrowed, &generate_owned, ty_kind) {
                    if let Some(fast) = fixed_layout::fixed_struct(
                        ident,
                        &ww_item_struct.fields,
                        ww_item_struct.kind,
                        fixed_required,
                        &crate_path,
                    )? {
                        ts.append_all(fast);
                    }
                }
            }

            if let Some(borrowed) = generate_borrowed {
                let cg_item_struct =
                    borrowed.cg_struct(&ww_item_struct, args.validate.as_ref(), transparent);
                ts.append_all(cg_item_struct.def_rust(true));
                ts.append_all(cg_item_struct.serdes_rust(true));
                if let Some(message) = &display_message {
                    ts.append_all(cg_item_struct.display_and_error_rust(true, message));
                }
                if args.series {
                    let seed = borrowed.series_seed(
                        true,
                        &ww_item_struct.generics,
                        args.series_store,
                        args.series_cells,
                    );
                    ts.append_all(match transparent {
                        Some(idx) => seed.series_transparent(
                            &ww_item_struct.fields,
                            ww_item_struct.kind,
                            idx,
                        )?,
                        None => seed.series_struct(
                            &ww_item_struct.fields,
                            series_evolvable(&args.size_assumption),
                        )?,
                    });
                }
            }

            if let Some(owned) = generate_owned {
                ww_item_struct.make_owned();
                let cg_item_struct =
                    owned.cg_struct(&ww_item_struct, args.validate.as_ref(), transparent);
                // skip when type consists of only plain types, as it is the same one as borrowed then
                if !(matches!(ty_kind, TyKind::Ambiguous) && do_generate_borrowed) {
                    ts.append_all(cg_item_struct.def_rust(false));
                }
                ts.append_all(cg_item_struct.serdes_rust(false));
                if let Some(message) = &display_message
                    && !(matches!(ty_kind, TyKind::Ambiguous) && do_generate_borrowed)
                {
                    ts.append_all(cg_item_struct.display_and_error_rust(false, message));
                }
                if args.series && !(matches!(ty_kind, TyKind::Ambiguous) && do_generate_borrowed) {
                    let seed = owned.series_seed(
                        false,
                        &ww_item_struct.generics,
                        args.series_store,
                        args.series_cells,
                    );
                    ts.append_all(match transparent {
                        Some(idx) => seed.series_transparent(
                            &ww_item_struct.fields,
                            ww_item_struct.kind,
                            idx,
                        )?,
                        None => seed.series_struct(
                            &ww_item_struct.fields,
                            series_evolvable(&args.size_assumption),
                        )?,
                    });
                }
            }
        }
        _ => {}
    }
    Ok(ts)
}

/// `#[cfg_attr(pred, wire(W))]` fields (DER-19): the bytes must not depend on whether `pred` holds where the derive
/// runs, so `pred` must hold wherever the type is serialized. A compile error when it doesn't, once per predicate.
fn wire_cfg_guards<'a>(type_name: &str, types: impl Iterator<Item = &'a Type>) -> TokenStream {
    let mut preds: Vec<&syn::Meta> = vec![];
    for ty in types {
        if let Type::Wire {
            cfg: Some(pred), ..
        } = ty
            && !preds.contains(&pred)
        {
            preds.push(pred);
        }
    }
    let mut ts = TokenStream::new();
    for pred in preds {
        let message = format!(
            "{type_name}: #[cfg_attr({}, wire(..))] is off while the type is serialized, its bytes would change \
             with a cfg. Put the derive under the same cfg, or use #[wire(..)]",
            pred.to_token_stream()
        );
        ts.append_all(quote! {
            #[cfg(not(#pred))]
            compile_error!(#message);
        });
    }
    ts
}

/// A `head_tail` type (SW-46): the type itself, its discriminant fn, and the head+tail code instead of shrink_wrap's
/// serdes. One type, borrowed: it reads from the buffer without copying.
fn head_tail_inner(
    item: Item,
    ty_kind: TyKind,
    name: String,
    args: Args,
) -> Result<TokenStream, String> {
    if args.owned.is_some() || matches!(ty_kind, TyKind::ExplicitOwned) {
        return Err(
            "head_tail: a head_tail type borrows from the buffer; there is no owned version".into(),
        );
    }
    if args.fixed_layout {
        return Err("head_tail: a type is either fixed_layout or head_tail, not both".into());
    }
    if args.size_assumption.is_some() {
        return Err(
            "head_tail: sized, final_structure and self_describing are shrink_wrap's; a head_tail type has its \
             own encoding. Remove them"
                .into(),
        );
    }
    let has_lifetime = matches!(ty_kind, TyKind::ImpliedRef);
    let crate_path = crate_path(args.crate_path);
    let seed = CGSeed::new(
        Ident::new(&name, Span::call_site()),
        args.borrowed.flatten(),
        args.cfg_attr_borrowed,
        &args.cfg_attr,
        args.derive_borrowed,
        &args.derive,
        None,
        !has_lifetime,
        &crate_path,
    )
    .base_if_named(&name);
    let names = head_tail::Names {
        ident: seed.ident.clone(),
        has_lifetime,
    };
    let mut ts = TokenStream::new();
    match &item {
        Item::Enum(item_enum) => {
            let Some(repr) = args.ww_repr else {
                return Err("For enums, ww_repr must be specified".into());
            };
            let ww_item_enum = ItemEnum::from_syn(item_enum, repr)?;
            let cg = seed.cg_enum(&ww_item_enum, repr);
            ts.append_all(cg.def_rust(true));
            ts.append_all(cg.impl_discriminant(true));
            if ww_item_enum.future_version.is_some() {
                return Err("head_tail: a #[future_version] variant holds discriminants of a newer version, whose \
                            heads and tails this one cannot know. Remove it"
                    .into());
            }
            ts.append_all(head_tail::ht_enum(
                &names,
                repr,
                &ww_item_enum.variants,
                &crate_path,
            )?);
        }
        Item::Struct(item_struct) => {
            if args.ww_repr.is_some() {
                return Err("structs must not have a ww_repr".into());
            }
            let ww_item_struct = ItemStruct::from_syn(item_struct, false)?;
            let cg = seed.cg_struct(&ww_item_struct, args.validate.as_ref(), None);
            ts.append_all(cg.def_rust(true));
            ts.append_all(head_tail::ht_struct(
                &names,
                &ww_item_struct.fields,
                ww_item_struct.kind,
                &crate_path,
            )?);
        }
        _ => {}
    }
    Ok(ts)
}

/// A struct without fields (`struct Marker;`, `struct A {}`) is only worth having on the wire if it can grow: an
/// Unsized one records its length, so fields can be added later. With a fixed size it is zero bits forever, `()`.
fn check_fieldless_struct_args(item: &Item, args: &Args) -> Result<(), String> {
    let Item::Struct(item_struct) = item else {
        return Ok(());
    };
    if !item_struct.fields.is_empty() {
        return Ok(());
    }
    let directive = match &args.size_assumption {
        Some(ObjectSize::Sized { .. }) => "sized",
        Some(ObjectSize::UnsizedFinalStructure) => "final_structure",
        Some(ObjectSize::SelfDescribing) => "self_describing",
        _ if args.fixed_layout => "fixed_layout",
        _ => return Ok(()),
    };
    Err(format!(
        "`{}` has no fields: with `{directive}` it is zero bits that can never change. Remove `{directive}` to keep it \
         evolvable (it records its length, so fields can be added later), or give it reserved bits to grow into, \
         e.g. a `U4` field",
        item_struct.ident
    ))
}

/// `series` (SW-60): what v1 of `Series` takes (design `series.md`), and `#[series(opaque)]` only with it.
fn check_series_args(item: &Item, args: &Args) -> Result<(), String> {
    let (name, fields): (_, Vec<&syn::Field>) = match item {
        Item::Struct(s) => (&s.ident, s.fields.iter().collect()),
        Item::Enum(e) => (
            &e.ident,
            e.variants.iter().flat_map(|v| v.fields.iter()).collect(),
        ),
        _ => return Ok(()),
    };
    if !args.series {
        if fields
            .iter()
            .any(|f| f.attrs.iter().any(|a| a.path().is_ident("series")))
        {
            return Err(format!(
                "`{name}`: #[series(..)] on a field needs the `series` directive on the type"
            ));
        }
        return Ok(());
    }
    if args.sparse {
        return Err(format!(
            "series: `{name}` is sparse, sparse structs are not in Series v1 (their slots can be removed, which needs \
             a rule of its own)"
        ));
    }
    if args.head_tail {
        return Err(format!(
            "series: `{name}` is head_tail, a Series item is a shrink_wrap type"
        ));
    }
    if let Item::Struct(s) = item
        && let syn::Fields::Unnamed(_) = s.fields
        && !args.transparent
    {
        return Err(format!(
            "series: `{name}` is a tuple struct, not in Series v1: name its fields"
        ));
    }
    Ok(())
}

/// Type and const parameters and where clauses (DER-14) go with shrink_wrap's own encoding of a struct or enum,
/// `series`, `display` and `validate`; `fixed_layout`, `head_tail` and `sparse` lay out a concrete type.
fn check_type_params_args(item: &Item, args: &Args) -> Result<(), String> {
    let (name, generics) = match item {
        Item::Struct(s) => (&s.ident, &s.generics),
        Item::Enum(e) => (&e.ident, &e.generics),
        _ => return Ok(()),
    };
    if Generics::from_syn(generics).is_empty() {
        return Ok(());
    }
    let directive = if args.fixed_layout {
        "fixed_layout"
    } else if args.head_tail {
        "head_tail"
    } else if args.sparse {
        "sparse"
    } else {
        return Ok(());
    };
    Err(format!(
        "`{name}`: type and const parameters and where clauses don't go with `{directive}` (yet)"
    ))
}

/// A struct's group in a `Series` has a field count or length when the struct is `Unsized`.
fn series_evolvable(size_assumption: &Option<ObjectSize>) -> bool {
    matches!(size_assumption, None | Some(ObjectSize::Unsized))
}

fn future_version_ident(item: &ItemEnum) -> Option<&Ident> {
    item.future_version.as_ref().map(|fv| &fv.ident)
}

/// `sparse` (DER-13) is a struct encoding of its own: none of the other encodings or size assumptions go with it.
fn check_sparse_args(item: &Item, args: &Args) -> Result<(), String> {
    if !matches!(item, Item::Struct(_)) {
        return Err("sparse: only a struct can be sparse".into());
    }
    if args.size_assumption.is_some() {
        return Err(
            "sparse: a sparse struct is Unsized, so that fields can be appended; remove sized, final_structure or \
             self_describing"
                .into(),
        );
    }
    if args.fixed_layout || args.head_tail {
        return Err("sparse: a type is sparse, fixed_layout or head_tail, only one of them".into());
    }
    Ok(())
}

/// `transparent` (DER-18) is exactly its one field on the wire: its size class, no size assumption or encoding of
/// its own.
fn check_transparent_args(item: &Item, args: &Args) -> Result<(), String> {
    let Item::Struct(item_struct) = item else {
        return Err("transparent: only a struct can be transparent, wrapping its one field".into());
    };
    let name = &item_struct.ident;
    if args.size_assumption.is_some() {
        return Err(format!(
            "transparent: `{name}` is its field on the wire, its size class is the field's: remove sized, \
             final_structure or self_describing"
        ));
    }
    if args.sparse || args.fixed_layout || args.head_tail {
        return Err(format!(
            "transparent: `{name}` is its field on the wire, it can't be sparse, fixed_layout or head_tail too"
        ));
    }
    Ok(())
}

/// Index in `item.fields` of the one field of a `transparent` struct (DER-18) that isn't zero bits: every other one
/// is a `PhantomData<..>` or `()`, and the flag of an `Option` / `Result` field is part of it.
fn transparent_field(item: &ItemStruct, type_name: &str) -> Result<usize, String> {
    let real: Vec<usize> = item
        .fields
        .iter()
        .enumerate()
        .filter(|(_, f)| !matches!(f.ty, Type::IsSome(_) | Type::IsOk(_)) && !f.ty.is_zero_bits())
        .map(|(i, _)| i)
        .collect();
    let idx = match real.as_slice() {
        [idx] => *idx,
        [] => {
            return Err(format!(
                "transparent: `{type_name}` has no field that isn't zero bits (PhantomData, ()): a transparent struct \
                 is its one such field on the wire"
            ));
        }
        many => {
            let names: Vec<String> = many
                .iter()
                .map(|i| format!("`{}`", item.fields[*i].ident))
                .collect();
            return Err(format!(
                "transparent: `{type_name}` has {} fields that aren't zero bits ({}), a transparent struct wraps \
                 exactly one (the others can only be PhantomData or ())",
                many.len(),
                names.join(", ")
            ));
        }
    };
    let field = &item.fields[idx];
    let field_name = &field.ident;
    if field.default.is_some() {
        return Err(format!(
            "transparent: field `{field_name}` of `{type_name}` has #[default = ..], but a transparent struct is its \
             field and nothing more: it can't gain fields, remove it"
        ));
    }
    if field.series_opaque {
        return Err(format!(
            "transparent: field `{field_name}` of `{type_name}` is #[series(opaque)]: a transparent struct has its \
             field's series columns, mark the field's type instead"
        ));
    }
    if matches!(field.ty, Type::TailSize(_)) {
        return Err(format!(
            "transparent: field `{field_name}` of `{type_name}` is a TailSize slot, which is the size of a value it \
             is in, not a value of its own"
        ));
    }
    Ok(idx)
}

/// The last field is an `Option` or `Result`: a struct with `fixed_layout` is then `self_describing` by default.
fn last_field_is_flagged(item_struct: &syn::ItemStruct) -> bool {
    let Some(field) = item_struct.fields.iter().next_back() else {
        return false;
    };
    let syn::Type::Path(path) = &field.ty else {
        return false;
    };
    path.path
        .segments
        .last()
        .is_some_and(|s| s.ident == "Option" || s.ident == "Result")
}

/// The types to give the fixed-layout fast path: each generated type once (an ambiguous type with both
/// `borrowed` and `owned` is one type).
fn fixed_idents<'a>(
    borrowed: &'a Option<CGSeed>,
    owned: &'a Option<CGSeed>,
    ty_kind: TyKind,
) -> Vec<&'a Ident> {
    let mut out: Vec<&Ident> = borrowed.iter().map(|s| &s.ident).collect();
    if !(matches!(ty_kind, TyKind::Ambiguous) && borrowed.is_some()) {
        out.extend(owned.iter().map(|s| &s.ident));
    }
    out
}

/// A `TailSize` slot counts the bytes to the end of the value including the reverse lengths its fields keep at
/// the back, which only an `Unsized` type owns: a `sized`, `final_structure` or `self_describing` type has no end
/// of its own on the wire.
fn check_tail_size_unsized(
    has_tail_size: bool,
    size_assumption: &Option<ObjectSize>,
) -> Result<(), String> {
    if has_tail_size && size_assumption.is_some() {
        return Err(
            "TailSize is only allowed in Unsized types: remove sized, final_structure or self_describing"
                .into(),
        );
    }
    Ok(())
}

fn shrink_wrap_derive_inner(mut file: File) -> Result<TokenStream, String> {
    let Some(item) = file.items.pop() else {
        return Err("Expected one item (enum or struct)".into());
    };

    let (ty_kind, name) = TyKind::from_item(&item)?;
    if matches!(ty_kind, TyKind::Ambiguous) {
        return Err(
            "Ambiguous declaration, add `Owned` to type name, use #[derive_shrink_wrap] attribute macro, or use PhantomData to capture a lifetime".into(),
        );
    }

    generate_inner(item, ty_kind, name, Args::default())
}

fn item_name(name: String, ty_kind: TyKind, is_ref: bool) -> Ident {
    let ident = match (ty_kind, is_ref) {
        // E.g., `MyTypeOwned` and generating owned code -> leave name as is
        (TyKind::ExplicitOwned, false) => name,
        // E.g., `MyTypeOwned` and generating borrowed code -> strip the Owned suffix
        (TyKind::ExplicitOwned, true) => name.strip_suffix("Owned").unwrap_or(&name).to_string(),

        // E.g., MyType<'i> and generating borrowed code -> leave name as is
        (TyKind::ImpliedRef, true) => name,
        // E.g., MyType<'i> and generating owned code -> append Owned
        (TyKind::ImpliedRef, false) => format!("{name}Owned"),

        // E.g., MyType with only plain types -> leave name as is + only one type definition is emitted
        (TyKind::Ambiguous, _) => name,
    };
    Ident::new(&ident, Span::call_site())
}

struct CGSeed<'a> {
    ident: Ident,
    cfg: Option<TokenStream>,
    cfg_attr: Vec<TokenStream>,
    derive: Vec<Path>,
    size_assumption: Option<ObjectSize>,
    ambiguous: bool,
    crate_path: &'a CratePath,
    /// The type with the item's own name: it gets the item's outer attributes.
    base: bool,
}

impl<'a> CGSeed<'a> {
    #[allow(clippy::too_many_arguments)]
    fn new(
        ident: Ident,
        cfg: Option<TokenStream>,
        cfg_attr: Vec<TokenStream>,
        cfg_attr_add: &[TokenStream],
        derive: Vec<Path>,
        derive_add: &[Path],
        size_assumption: Option<ObjectSize>,
        ambiguous: bool,
        crate_path: &'a CratePath,
    ) -> Self {
        let mut cfg_attr = cfg_attr;
        cfg_attr.extend(cfg_attr_add.iter().cloned());
        let mut derive = derive;
        derive.extend(derive_add.iter().cloned());
        Self {
            ident,
            cfg,
            cfg_attr,
            derive,
            size_assumption,
            ambiguous,
            crate_path,
            base: false,
        }
    }

    /// Marks the seed whose type has the item's own name `name`.
    fn base_if_named(mut self, name: &str) -> Self {
        self.base = self.ident == name;
        self
    }

    /// The `series` impl's view of the type, `is_ref` on the borrowed side.
    fn series_seed<'p>(
        &'p self,
        is_ref: bool,
        generics: &'p Generics,
        store: bool,
        cells: bool,
    ) -> SeriesSeed<'p> {
        SeriesSeed {
            ident: &self.ident,
            generics,
            has_lifetime: is_ref && !self.ambiguous,
            is_ref,
            cfg: self.cfg.as_ref(),
            cp: self.crate_path,
            store,
            cells,
        }
    }

    fn cg_enum<'i>(&'i self, ww_item_enum: &'i ItemEnum, repr: Repr) -> CGItemEnum<'i> {
        self.cg_enum_with_ident(&self.ident, ww_item_enum, repr)
    }

    /// Same as [Self::cg_enum], but under a different name - used for the `MyTypeDiscriminants`
    /// enum generated by the `discriminants` directive, which must not collide with the main type.
    fn cg_enum_with_ident<'i>(
        &'i self,
        ident: &'i Ident,
        ww_item_enum: &'i ItemEnum,
        repr: Repr,
    ) -> CGItemEnum<'i> {
        CGItemEnum {
            docs: &ww_item_enum.docs,
            repr,
            ident,
            variants: &ww_item_enum.variants,
            future_version: ww_item_enum.future_version.as_ref(),
            cfg: self.cfg.as_ref(),
            cfg_attr: &self.cfg_attr,
            derive: &self.derive,
            size_assumption: self.size_assumption.clone(),
            ambiguous: self.ambiguous,
            crate_path: self.crate_path,
            generics: &ww_item_enum.generics,
            base: self.base && ident == &self.ident,
            attrs: if self.base && ident == &self.ident {
                &ww_item_enum.attrs
            } else {
                &[]
            },
        }
    }

    fn cg_struct<'i>(
        &'i self,
        ww_item_struct: &'i ItemStruct,
        validate: Option<&'i Path>,
        transparent: Option<usize>,
    ) -> CGItemStruct<'i> {
        CGItemStruct {
            transparent,
            docs: &ww_item_struct.docs,
            ident: &self.ident,
            fields: &ww_item_struct.fields,
            kind: ww_item_struct.kind,
            sparse_slots: ww_item_struct.sparse_slots.as_deref(),
            validate,
            cfg: self.cfg.as_ref(),
            cfg_attr: &self.cfg_attr,
            derive: &self.derive,
            size_assumption: self.size_assumption.clone(),
            ambiguous: self.ambiguous,
            crate_path: self.crate_path,
            generics: &ww_item_struct.generics,
            base: self.base,
            repr: &ww_item_struct.repr,
            attrs: if self.base {
                &ww_item_struct.attrs
            } else {
                &[]
            },
        }
    }
}

/// A type kind that determines what kind of code to generate
#[derive(Copy, Clone)]
enum TyKind {
    /// Type name ends in `Owned` and no ref types are used (&'i str, Type<'i>, etc.)
    /// A hint that if borrowed type is requested, it's name will be with the `Owned` suffix stripped
    ExplicitOwned,
    /// Type name does not end in `Owned`, but type contains at least one reference.
    /// A hint that if owned type is requested, it's name will be created by appending `Owned`
    ImpliedRef,
    /// Type name does not end in `Owned` and contains only plain types (u8, bool, etc.)
    /// Have to specify whether borrowed or owned variant is wanted.
    /// In this case no new type is generated and both owned and borrowed traits are derived on one type if requested.
    Ambiguous,
    // Mix of a plain (non-`Owned`) type name and alloc types, or `Owned` type name and reference types
    // Confusing,
}

impl TyKind {
    fn from_item(item: &syn::Item) -> Result<(Self, String), String> {
        let name = match item {
            Item::Enum(item_enum) => item_enum.ident.to_string(),
            Item::Struct(item_struct) => item_struct.ident.to_string(),
            _ => return Err("Expected enum or struct".into()),
        };
        let is_explicit_owned = name.ends_with("Owned");
        let types = collect_referenced_types(item);
        // const AMBIGUOUS_ERR: &str = "";
        if types.is_empty() {
            return Ok((Self::Ambiguous, name));
            // return Err(AMBIGUOUS_ERR.into());
        }
        let any_lifetimes = types.iter().any(|t| t.has_lifetime);
        let any_alloc_types = types.iter().any(|t| ALLOC_TYPES.contains(&t.name.as_str()));
        match (is_explicit_owned, any_lifetimes, any_alloc_types) {
            (true, false, _) => Ok((Self::ExplicitOwned, name)),
            (false, true, false) => Ok((Self::ImpliedRef, name)),
            (_, true, true) => Err(
                "Both reference and owned types are mixed, use either one to not cause confusion"
                    .into(),
            ),
            (true, true, false) => Err("Type name ends in Owned, but reference types are used, either rename or use owned types".into()),
            (false, false, true) => Err("Type contains owned types (String, Vec, Box, etc.), add `Owned` to the type name".into()),
            (false, false, false) => Ok((Self::Ambiguous, name)),
        }
    }
}

const ALLOC_TYPES: &[&str] = &["Vec", "String", "Box"];

/// A type referenced from a struct or enum's fields.
struct ReferencedType {
    name: String,
    has_lifetime: bool,
}

/// Collects every type referenced from `item`'s fields (a struct's fields, or an enum's
/// variants' fields), recursing into generic arguments, tuples, arrays/slices and references.
fn collect_referenced_types(item: &Item) -> Vec<ReferencedType> {
    let mut out = Vec::new();
    match item {
        Item::Struct(item_struct) => {
            for field in &item_struct.fields {
                collect_referenced_types_in_ty(&field.ty, false, &mut out);
            }
        }
        Item::Enum(item_enum) => {
            for variant in &item_enum.variants {
                for field in &variant.fields {
                    collect_referenced_types_in_ty(&field.ty, false, &mut out);
                }
            }
        }
        _ => {}
    }
    out
}

/// `forced_lifetime` is set when the immediately enclosing type is a reference with an explicit
/// lifetime (e.g. `&'i str`), so that lifetime is attributed to the type it refers to.
fn collect_referenced_types_in_ty(
    ty: &syn::Type,
    forced_lifetime: bool,
    out: &mut Vec<ReferencedType>,
) {
    match ty {
        syn::Type::Path(type_path) => {
            let Some(segment) = type_path.path.segments.last() else {
                return;
            };
            let has_lifetime = forced_lifetime
                || matches!(&segment.arguments, syn::PathArguments::AngleBracketed(args)
                    if args.args.iter().any(|a| matches!(a, syn::GenericArgument::Lifetime(_))));
            out.push(ReferencedType {
                name: segment.ident.to_string(),
                has_lifetime,
            });
            if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                for arg in &args.args {
                    if let syn::GenericArgument::Type(inner) = arg {
                        collect_referenced_types_in_ty(inner, false, out);
                    }
                }
            }
        }
        syn::Type::Reference(type_reference) => {
            collect_referenced_types_in_ty(
                &type_reference.elem,
                type_reference.lifetime.is_some(),
                out,
            );
        }
        syn::Type::Tuple(type_tuple) => {
            for elem in &type_tuple.elems {
                collect_referenced_types_in_ty(elem, false, out);
            }
        }
        syn::Type::Array(type_array) => {
            collect_referenced_types_in_ty(&type_array.elem, forced_lifetime, out);
        }
        syn::Type::Slice(type_slice) => {
            collect_referenced_types_in_ty(&type_slice.elem, forced_lifetime, out);
        }
        syn::Type::Group(type_group) => {
            collect_referenced_types_in_ty(&type_group.elem, forced_lifetime, out);
        }
        syn::Type::Paren(type_paren) => {
            collect_referenced_types_in_ty(&type_paren.elem, forced_lifetime, out);
        }
        _ => {}
    }
}

#[cfg(test)]
mod display_tests {
    use super::shrink_wrap_attr;
    use quote::quote;

    #[test]
    fn missing_doc_or_display_is_a_compile_error() {
        let ts = shrink_wrap_attr(
            quote! { display, borrowed, derive(Debug), ww_repr = u8 },
            quote! {
                enum E {
                    /// documented
                    A,
                    B,
                }
            },
        );
        let s = ts.to_string();
        assert!(s.contains("compile_error"));
        assert!(s.contains("E::B"));
        assert!(s.contains("display requires a doc comment"));
    }

    #[test]
    fn missing_doc_or_display_on_struct_is_a_compile_error() {
        let ts = shrink_wrap_attr(
            quote! { display, borrowed, derive(Debug) },
            quote! {
                struct S {
                    a: u8,
                }
            },
        );
        let s = ts.to_string();
        assert!(s.contains("compile_error"));
        assert!(s.contains("display requires a doc comment"));
    }

    #[test]
    fn doc_or_display_present_does_not_error() {
        let ts = shrink_wrap_attr(
            quote! { display, borrowed, derive(Debug), ww_repr = u8 },
            quote! {
                enum E {
                    /// documented
                    A,
                    #[display("overridden")]
                    B,
                }
            },
        );
        assert!(!ts.to_string().contains("compile_error"));
    }

    #[test]
    fn display_not_requested_skips_the_check() {
        let ts = shrink_wrap_attr(
            quote! { borrowed, derive(Debug), ww_repr = u8 },
            quote! {
                enum E {
                    A,
                    B,
                }
            },
        );
        assert!(!ts.to_string().contains("compile_error"));
    }
}

#[cfg(test)]
mod sparse_tests {
    use super::shrink_wrap_attr;
    use proc_macro2::TokenStream;
    use quote::quote;

    fn error_of(args: TokenStream, item: TokenStream) -> Option<String> {
        let s = shrink_wrap_attr(args, item).to_string();
        s.contains("compile_error").then_some(s)
    }

    #[test]
    fn option_fields_with_slots_compile() {
        let item = quote! {
            struct S<'i> {
                a: Option<u8>,
                #[slot = 4]
                b: Option<&'i str>,
                c: Option<RefVec<'i, u8>>,
            }
        };
        assert_eq!(error_of(quote! { sparse, owned }, item), None);
    }

    #[test]
    fn non_option_field_is_an_error() {
        let e = error_of(
            quote! { sparse, borrowed },
            quote! { struct S { a: Option<u8>, b: u8 } },
        );
        assert!(e.unwrap().contains("sparse field `b` must be an Option<T>"));
        let e = error_of(
            quote! { sparse, borrowed },
            quote! { struct S { a: Result<u8, u8> } },
        );
        assert!(e.unwrap().contains("must be an Option<T>"));
    }

    #[test]
    fn other_encodings_are_an_error() {
        let item = quote! { struct S { a: Option<u8> } };
        for args in [
            quote! { sparse, borrowed, final_structure },
            quote! { sparse, borrowed, sized },
            quote! { sparse, borrowed, self_describing },
        ] {
            assert!(error_of(args, item.clone()).unwrap().contains("Unsized"));
        }
        for args in [
            quote! { sparse, borrowed, head_tail },
            quote! { sparse, borrowed, fixed_layout },
        ] {
            assert!(
                error_of(args, item.clone())
                    .unwrap()
                    .contains("only one of them")
            );
        }
        let e = error_of(
            quote! { sparse, borrowed, ww_repr = u8 },
            quote! { enum E { A } },
        );
        assert!(e.unwrap().contains("only a struct"));
    }

    #[test]
    fn slots_must_increase() {
        let e = error_of(
            quote! { sparse, borrowed },
            quote! { struct S { #[slot = 3] a: Option<u8>, #[slot = 3] b: Option<u8> } },
        );
        assert!(
            e.unwrap()
                .contains("must be after the previous field's slot 3")
        );
        let e = error_of(
            quote! { sparse, borrowed },
            quote! { struct S { #[slot = 3] a: Option<u8>, #[slot = 1] b: Option<u8> } },
        );
        assert!(e.is_some());
        let e = error_of(
            quote! { sparse, borrowed },
            quote! { struct S { #[slot = 4294967295] a: Option<u8> } },
        );
        assert!(e.unwrap().contains("below u32::MAX"));
        let e = error_of(
            quote! { sparse, borrowed },
            quote! { struct S { #[slot = "1"] a: Option<u8> } },
        );
        assert!(e.unwrap().contains("integer literal"));
    }

    #[test]
    fn slot_without_sparse_is_an_error() {
        let e = error_of(
            quote! { borrowed },
            quote! { struct S { #[slot = 1] a: Option<u8> } },
        );
        assert!(e.unwrap().contains("only the fields of a `sparse` struct"));
    }

    #[test]
    fn default_and_flag_are_an_error() {
        let e = error_of(
            quote! { sparse, borrowed },
            quote! { struct S { #[default = None] a: Option<u8> } },
        );
        assert!(e.unwrap().contains("#[default]"));
        let e = error_of(
            quote! { sparse, borrowed },
            quote! { struct S { #[flag] a_flag: bool, a: Option<u8> } },
        );
        assert!(e.unwrap().contains("#[flag]"));
    }
}

#[cfg(test)]
mod tuple_struct_tests {
    use super::shrink_wrap_attr;
    use quote::quote;

    #[test]
    fn tuple_and_unit_structs_compile() {
        let plain = [
            quote! { borrowed, owned },
            quote! { borrowed, owned, sized },
            quote! { borrowed, owned, final_structure },
            quote! { borrowed, fixed_layout },
            quote! { head_tail },
        ];
        let cases = [
            (quote! { struct Rgb(u8, u8, u8); }, &plain[..]),
            (
                quote! { struct Marker; },
                &[quote! { borrowed, owned }, quote! { head_tail }][..],
            ),
            (
                quote! { struct Pair<'i>(u8, &'i str); },
                &[quote! { owned }, quote! { head_tail }][..],
            ),
        ];
        for (item, args) in cases {
            for args in args {
                let s = shrink_wrap_attr(args.clone(), item.clone()).to_string();
                assert!(!s.contains("compile_error"), "{args} {item}: {s}");
            }
        }
    }

    #[test]
    fn fieldless_struct_with_a_fixed_size_is_an_error() {
        for item in [
            quote! { struct Marker; },
            quote! { struct Marker {} },
            quote! { struct Marker(); },
        ] {
            for (args, directive) in [
                (quote! { borrowed, sized }, "sized"),
                (quote! { borrowed, final_structure }, "final_structure"),
                (quote! { borrowed, self_describing }, "self_describing"),
                (quote! { borrowed, fixed_layout }, "fixed_layout"),
            ] {
                let s = shrink_wrap_attr(args.clone(), item.clone()).to_string();
                assert!(s.contains("compile_error"), "{args} {item}: {s}");
                assert!(
                    s.contains(&format!("with `{directive}` it is zero bits")),
                    "{args} {item}: {s}"
                );
            }
        }
    }

    #[test]
    fn relocated_flag_is_a_compile_error() {
        let s = shrink_wrap_attr(
            quote! { borrowed },
            quote! { struct Pair(#[flag] bool, Option<u8>); },
        )
        .to_string();
        assert!(s.contains("compile_error"), "{s}");
        assert!(s.contains("tuple struct `Pair`"), "{s}");
    }
}

#[cfg(test)]
mod wire_tests {
    use super::shrink_wrap_attr;
    use proc_macro2::TokenStream;
    use quote::quote;

    fn output(item: TokenStream) -> String {
        shrink_wrap_attr(quote! { borrowed }, item).to_string()
    }

    fn error_of(item: TokenStream) -> Option<String> {
        let s = output(item);
        s.contains("compile_error").then_some(s)
    }

    #[test]
    fn integer_pairs_compile() {
        assert_eq!(
            error_of(
                quote! { struct S { #[wire(I4)] a: i8, #[wire(UNib32)] b: u64, #[wire(shrink_wrap::U3)] c: usize } }
            ),
            None
        );
    }

    #[test]
    fn signedness_must_match() {
        let e = error_of(quote! { struct S { #[wire(U4)] a: i8 } }).unwrap();
        assert!(e.contains("both must be signed or both unsigned"), "{e}");
    }

    #[test]
    fn field_must_be_a_plain_integer() {
        for item in [
            quote! { struct S { #[wire(U4)] a: f32 } },
            quote! { struct S { #[wire(U4)] a: Option<u8> } },
            quote! { struct S { #[wire(U4)] a: [u8; 2] } },
        ] {
            let e = error_of(item).unwrap();
            assert!(e.contains("goes on a plain integer field"), "{e}");
        }
    }

    #[test]
    fn wire_type_must_be_an_integer_of_64_bits_or_less() {
        for item in [
            quote! { struct S { #[wire(u128)] a: u128 } },
            quote! { struct S { #[wire(U65)] a: u128 } },
            quote! { struct S { #[wire(I1)] a: i8 } },
            quote! { struct S { #[wire(Foo)] a: u8 } },
        ] {
            let e = error_of(item).unwrap();
            assert!(e.contains("W is an integer type"), "{e}");
        }
    }

    #[test]
    fn one_wire_per_field() {
        let e = error_of(quote! { struct S { #[wire(U4)] #[wire(U5)] a: u8 } }).unwrap();
        assert!(e.contains("more than one"), "{e}");
    }

    #[test]
    fn cfg_attr_with_other_attributes_is_an_error() {
        let e = error_of(
            quote! { struct S { #[cfg_attr(feature = "sw", wire(U4), doc = "x")] a: u8 } },
        )
        .unwrap();
        assert!(e.contains("wire(W) alone"), "{e}");
    }

    #[test]
    fn cfg_attr_guard_is_emitted_once_per_predicate() {
        let s = output(quote! {
            struct S {
                #[cfg_attr(feature = "sw", wire(U4))] a: u8,
                #[cfg_attr(feature = "sw", wire(I4))] b: i8,
            }
        });
        assert_eq!(s.matches("compile_error").count(), 1, "{s}");
        assert!(s.contains("# [cfg (not (feature = \"sw\"))]"), "{s}");
    }

    #[test]
    fn fixed_layout_names_the_feature_item() {
        let s = shrink_wrap_attr(
            quote! { borrowed, fixed_layout },
            quote! { struct S { #[wire(U4)] a: u8, b: u8 } },
        )
        .to_string();
        assert!(s.contains("DER-21"), "{s}");
    }
}

#[cfg(test)]
mod transparent_tests {
    use super::shrink_wrap_attr;
    use proc_macro2::TokenStream;
    use quote::quote;

    fn error_of(args: TokenStream, item: TokenStream) -> String {
        let s = shrink_wrap_attr(args, item).to_string();
        assert!(s.contains("compile_error"), "{s}");
        s
    }

    #[test]
    fn zero_or_two_real_fields_name_the_type() {
        let s = error_of(
            quote! { borrowed, owned, transparent },
            quote! { struct Marker<U> { _u: PhantomData<U>, _x: () } },
        );
        assert!(
            s.contains("`Marker` has no field that isn't zero bits"),
            "{s}"
        );
        let s = error_of(
            quote! { borrowed, owned, transparent },
            quote! { struct Two<U> { a: u8, _u: PhantomData<U>, b: u16 } },
        );
        assert!(
            s.contains("`Two` has 2 fields that aren't zero bits (`a`, `b`)"),
            "{s}"
        );
    }

    #[test]
    fn other_encodings_and_size_classes_are_errors() {
        for args in [
            quote! { borrowed, owned, transparent, sized },
            quote! { borrowed, owned, transparent, final_structure },
            quote! { borrowed, owned, transparent, self_describing },
        ] {
            let s = error_of(args, quote! { struct W { v: u8 } });
            assert!(s.contains("its size class is the field's"), "{s}");
        }
        for args in [
            quote! { borrowed, owned, transparent, fixed_layout },
            quote! { borrowed, owned, transparent, sparse },
            quote! { transparent, head_tail },
        ] {
            let s = error_of(args, quote! { struct W { v: Option<u8> } });
            assert!(
                s.contains("can't be sparse, fixed_layout or head_tail"),
                "{s}"
            );
        }
        let s = error_of(
            quote! { borrowed, owned, transparent, ww_repr = u4 },
            quote! { enum E { A(u8) } },
        );
        assert!(s.contains("only a struct can be transparent"), "{s}");
    }

    #[test]
    fn default_opaque_and_tail_size_fields_are_errors() {
        let s = error_of(
            quote! { borrowed, owned, transparent },
            quote! { struct W { #[default = 0] v: u8 } },
        );
        assert!(s.contains("field `v` of `W` has #[default = ..]"), "{s}");
        let s = error_of(
            quote! { borrowed, owned, transparent, series },
            quote! { struct W { #[series(opaque)] v: u8 } },
        );
        assert!(s.contains("field `v` of `W` is #[series(opaque)]"), "{s}");
    }

    #[test]
    fn repr_is_kept_on_both_types() {
        let s = shrink_wrap_attr(
            quote! { borrowed, owned, transparent },
            quote! { #[repr(transparent)] struct W<'i> { v: &'i str } },
        )
        .to_string();
        assert_eq!(s.matches("# [repr (transparent)]").count(), 2, "{s}");
    }

    #[test]
    fn tuple_struct_with_series_is_fine() {
        let s = shrink_wrap_attr(
            quote! { borrowed, owned, transparent, series(cells) },
            quote! { struct W<U>(u32, PhantomData<U>); },
        )
        .to_string();
        assert!(!s.contains("compile_error"), "{s}");
    }
}
