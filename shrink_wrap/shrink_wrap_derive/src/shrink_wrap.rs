use proc_macro2::{Ident, Span, TokenStream};
use quote::TokenStreamExt;
use syn::{File, Item, Path, parse2};

use crate::{
    args::Args,
    ast::{item_enum::ItemEnum, item_struct::ItemStruct, object_size::ObjectSize, repr::Repr},
    codegen::{item_enum::CGItemEnum, item_struct::CGItemStruct},
    transform::docs_util::add_notes,
};

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

fn shrink_wrap_attr_inner(item: Item, args: Args) -> Result<TokenStream, String> {
    let (ty_kind, name) = TyKind::from_item(&item)?;
    if matches!(ty_kind, TyKind::Ambiguous) && args.borrowed.is_none() && args.owned.is_none() {
        return Err(
            "Ambiguous declaration, add `Ref` or `Owned` to type name, add borrowed or owned directive, or use PhantomData to capture a lifetime".into(),
        );
    }
    generate_inner(item, ty_kind, name, args)
}

fn generate_inner(
    item: Item,
    ty_kind: TyKind,
    name: String,
    args: Args,
) -> Result<TokenStream, String> {
    let do_generate_borrowed =
        matches!(ty_kind, TyKind::ExplicitRef | TyKind::ImpliedRef) | args.borrowed.is_some();
    let generate_borrowed = if do_generate_borrowed {
        Some(CGSeed::new(
            item_name(name.clone(), ty_kind, true),
            args.borrowed.flatten(),
            args.cfg_attr_borrowed,
            &args.cfg_attr,
            args.derive_borrowed.clone(),
            &args.derive,
            args.size_assumption,
        ))
    } else {
        None
    };

    let generate_owned =
        matches!(ty_kind, TyKind::ExplicitOwned | TyKind::ImpliedOwned) | args.owned.is_some();
    let generate_owned = if generate_owned {
        Some(CGSeed::new(
            item_name(name, ty_kind, false),
            args.owned.flatten(),
            args.cfg_attr_owned,
            &args.cfg_attr,
            args.derive_owned,
            &args.derive,
            args.size_assumption,
        ))
    } else {
        None
    };

    let mut ts = TokenStream::new();
    match &item {
        Item::Enum(item_enum) => {
            let Some(repr) = args.ww_repr else {
                return Err("For enums, ww_repr must be specified".into());
            };
            let mut ww_item_enum = ItemEnum::from_syn(item_enum)?;
            add_notes(&mut ww_item_enum.docs, args.size_assumption, true);
            if let Some(borrowed) = generate_borrowed {
                let cg_item_enum = borrowed.cg_enum(&ww_item_enum, repr);
                ts.append_all(cg_item_enum.def_rust(true));
                ts.append_all(cg_item_enum.impl_discriminant(true));
                ts.append_all(cg_item_enum.serdes_rust(true));

                if args.discriminants_enum {
                    ww_item_enum.to_discriminants();
                    let cg_discriminants = borrowed.cg_enum(&ww_item_enum, repr);
                    ts.append_all(cg_discriminants.def_rust(false));
                    ts.append_all(cg_discriminants.impl_discriminant(false));
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

                if args.discriminants_enum {
                    ww_item_enum.to_discriminants();
                    let cg_discriminants = owned.cg_enum(&ww_item_enum, repr);
                    ts.append_all(cg_discriminants.def_rust(false));
                    ts.append_all(cg_discriminants.impl_discriminant(false));
                }
            }
        }
        Item::Struct(item_struct) => {
            if args.ww_repr.is_some() {
                return Err("structs must not have a ww_repr".into());
            }
            let mut ww_item_struct = ItemStruct::from_syn(item_struct)?;
            add_notes(&mut ww_item_struct.docs, args.size_assumption, false);

            if let Some(borrowed) = generate_borrowed {
                let cg_item_struct = borrowed.cg_struct(&ww_item_struct);
                ts.append_all(cg_item_struct.def_rust(true));
                ts.append_all(cg_item_struct.serdes_rust(true));
            }

            if let Some(owned) = generate_owned {
                ww_item_struct.make_owned();
                let cg_item_struct = owned.cg_struct(&ww_item_struct);
                // skip when type consists of only plain types, as it is the same one as borrowed then
                if !(matches!(ty_kind, TyKind::Ambiguous) && do_generate_borrowed) {
                    ts.append_all(cg_item_struct.def_rust(false));
                }
                ts.append_all(cg_item_struct.serdes_rust(false));
            }
        }
        _ => {}
    }
    Ok(ts)
}

fn shrink_wrap_derive_inner(mut file: File) -> Result<TokenStream, String> {
    let Some(item) = file.items.pop() else {
        return Err("Expected one item (enum or struct)".into());
    };

    let (ty_kind, name) = TyKind::from_item(&item)?;
    if matches!(ty_kind, TyKind::Ambiguous) {
        return Err(
            "Ambiguous declaration, add `Ref` or `Owned` to type name, use #[derive_shrink_wrap] attribute macro, or use PhantomData to capture a lifetime".into(),
        );
    }

    generate_inner(item, ty_kind, name, Args::default())
}

fn item_name(name: String, ty_kind: TyKind, is_ref: bool) -> Ident {
    let ident = match (ty_kind, is_ref) {
        // E.g., `MyTypeRef<'i>` and generating borrowed code -> leave name as is
        (TyKind::ExplicitRef, true) => name,
        // E.g., `MyTypeOwned` and generating owned code -> leave name as is
        (TyKind::ExplicitOwned, false) => name,
        // E.g., `MyTypeOwned` and generating borrowed code -> replace Owned to Ref
        (TyKind::ExplicitOwned, true) => name.replace("Owned", "Ref").to_string(),
        // E.g., `MyTypeRef<'i>` and generating owned code -> replace Ref to Owned
        (TyKind::ExplicitRef, false) => name.replace("Ref", "Owned").to_string(),

        // E.g., MyType<'i> and generating borrowed code -> leave name as is
        (TyKind::ImpliedRef, true) => name,
        // E.g., MyType and generating owned code -> leave name as is
        (TyKind::ImpliedOwned, false) => name,
        // E.g., MyType<'i> and generating owned code -> append Owned
        (TyKind::ImpliedRef, false) => format!("{name}Owned"),
        // E.g., MyType and generating borrowed code -> append Ref
        (TyKind::ImpliedOwned, true) => format!("{name}Ref"),

        // E.g., MyType with only plain types -> leave name as is + only one type definition is emitted
        (TyKind::Ambiguous, _) => name,
    };
    Ident::new(&ident, Span::call_site())
}

struct CGSeed {
    ident: Ident,
    cfg: Option<TokenStream>,
    cfg_attr: Vec<TokenStream>,
    derive: Vec<Path>,
    size_assumption: Option<ObjectSize>,
}

impl CGSeed {
    fn new(
        ident: Ident,
        cfg: Option<TokenStream>,
        cfg_attr: Vec<TokenStream>,
        cfg_attr_add: &[TokenStream],
        derive: Vec<Path>,
        derive_add: &[Path],
        size_assumption: Option<ObjectSize>,
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
        }
    }

    fn cg_enum<'i>(&'i self, ww_item_enum: &'i ItemEnum, repr: Repr) -> CGItemEnum<'i> {
        CGItemEnum {
            docs: &ww_item_enum.docs,
            repr,
            ident: &self.ident,
            variants: &ww_item_enum.variants,
            cfg: self.cfg.as_ref(),
            cfg_attr: &self.cfg_attr,
            derive: &self.derive,
            size_assumption: self.size_assumption,
        }
    }

    fn cg_struct<'i>(&'i self, ww_item_struct: &'i ItemStruct) -> CGItemStruct<'i> {
        CGItemStruct {
            docs: &ww_item_struct.docs,
            ident: &self.ident,
            fields: &ww_item_struct.fields,
            cfg: self.cfg.as_ref(),
            cfg_attr: &self.cfg_attr,
            derive: &self.derive,
            size_assumption: self.size_assumption,
        }
    }
}

/// A type kind that determines what kind of code to generate
#[derive(Copy, Clone)]
enum TyKind {
    /// Type name ends in `Ref` and no alloc types are used (String, Vec, etc.)
    /// A hint that if owned type is requested, it's name will be with `Ref` part replaced to `Owned`
    ExplicitRef,
    /// Type name ends in `Owned` and no ref types are used (&'i str, Type<'i>, etc.)
    /// A hint that if borrowed type is requested, it's name will be with `Owned` part replaced to `Ref`
    ExplicitOwned,
    /// Type name does not end in either `Ref` or `Owned`, but type contains at least one reference
    /// A hint that if owned type is requested, it's name will created by appending `Owned`
    ImpliedRef,
    /// Type name does not end in either `Ref` or `Owned`, but type contains only alloc types
    /// A hint that if borrowed type is requested, it's name will created by appending `Ref`
    ImpliedOwned,
    /// Type name does not end in `Ref` or `Owned` and contains only plain types (u8, bool, etc.)
    /// Have to specify whether borrowed or owned variant is wanted.
    /// In this case no new type is generated and both owned and borrowed traits are derived on one type if requested.
    Ambiguous,
    // Mix of either `Ref` in type name and alloc types or the other way around
    // Confusing,
}

impl TyKind {
    fn from_item(item: &syn::Item) -> Result<(Self, String), String> {
        let name = match item {
            Item::Enum(item_enum) => item_enum.ident.to_string(),
            Item::Struct(item_struct) => item_struct.ident.to_string(),
            _ => return Err("Expected enum or struct".into()),
        };
        let is_explicit_ref = name.ends_with("Ref");
        let is_explicit_owned = name.ends_with("Owned");
        let name_hint = match (is_explicit_ref, is_explicit_owned) {
            (true, false) => NameHint::Ref,
            (false, true) => NameHint::Owned,
            _ => NameHint::Absent,
        };
        let types = collect_referenced_types(item);
        // const AMBIGUOUS_ERR: &str = "";
        if types.is_empty() {
            return Ok((Self::Ambiguous, name));
            // return Err(AMBIGUOUS_ERR.into());
        }
        let any_lifetimes = types.iter().any(|t| t.has_lifetime);
        let any_alloc_types = types.iter().any(|t| ALLOC_TYPES.contains(&t.name.as_str()));
        match (name_hint, any_lifetimes, any_alloc_types) {
            (NameHint::Ref, _, false) => Ok((Self::ExplicitRef, name)),
            (NameHint::Owned, false, _) => Ok((Self::ExplicitOwned, name)),
            (NameHint::Absent, true, false) => Ok((Self::ImpliedRef, name)),
            (NameHint::Absent, false, true) => Ok((Self::ImpliedOwned, name)),
            (_, true, true) => Err(
                "Both reference and owned types are mixed, use either one to not cause confusion"
                    .into(),
            ),
            (NameHint::Ref, false, true) => Err("Type name ends in Ref, but owned types are used, either rename or use ref types".into()),
            (NameHint::Owned, true, false) => Err("Type name ends in Owned, but reference types are used, either rename or use owned types".into()),
            (NameHint::Absent, false, false) => Ok((Self::Ambiguous, name)),
        }
    }
}

#[derive(Copy, Clone)]
enum NameHint {
    Ref,
    Owned,
    Absent,
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
            collect_referenced_types_in_ty(&type_array.elem, false, out);
        }
        syn::Type::Slice(type_slice) => {
            collect_referenced_types_in_ty(&type_slice.elem, false, out);
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
