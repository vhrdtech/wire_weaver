use super::{
    crate_walker::{CrateContext, Scratch},
    ty::{convert_ty, convert_ty_path, convert_ty_path_segment},
    util::{collect_docs, get_since_attr},
};
use anyhow::{anyhow, Context, Result};
use proc_macro2::Ident;
use shrink_wrap::UNib32;
use std::ops::Deref;
use syn::parse::discouraged::AnyDelimiter;
use syn::parse::{Parse, ParseStream};
use syn::{
    parse2, FnArg, Item, Pat, PathSegment, ReturnType, Token, TraitItem, TraitItemFn,
    TraitItemMacro, TypePath,
};
use ww_self::{
    ApiItemKindOwned, ApiItemOwned, ApiLevelLocationOwned, ApiLevelOwned, ArgumentOwned,
    Multiplicity, PropertyAccess,
};

pub(crate) fn convert_api_items(
    item_trait: &syn::ItemTrait,
    current_crate: &CrateContext,
    scratch: &mut Scratch,
) -> Result<Vec<ApiItemOwned>> {
    let mut items = vec![];
    for (idx, item) in item_trait.items.iter().enumerate() {
        match item {
            TraitItem::Fn(item_fn) => {
                let api_item_fn = convert_api_item_fn(current_crate, scratch, item_fn, idx as u32)?;
                items.push(api_item_fn);
            }
            TraitItem::Macro(item_macro) => {
                let kind = item_macro
                    .mac
                    .path
                    .get_ident()
                    .ok_or(
                        anyhow!("ww_trait macro item without name")
                            .context(current_crate.err_context()),
                    )?
                    .to_string();
                match kind.as_str() {
                    "stream" | "sink" => {
                        let api_item_stream = convert_api_item_stream(
                            current_crate,
                            scratch,
                            item_macro,
                            idx as u32,
                            kind == "stream",
                        )?;
                        items.push(api_item_stream);
                    }
                    "property" => {
                        let api_item_property = convert_api_item_property(
                            current_crate,
                            scratch,
                            item_macro,
                            idx as u32,
                        )?;
                        items.push(api_item_property);
                    }
                    "ww_impl" => {
                        let api_item_impl =
                            convert_api_item_impl(current_crate, scratch, item_macro, idx as u32)?;
                        items.push(api_item_impl);
                    }
                    "reserved" => continue,
                    u => {
                        return Err(anyhow!("Unsupported resource kind: {u:?}")
                            .context(current_crate.err_context()));
                    }
                }
            }
            i => {
                return Err(anyhow!("Unsupported resource kind: {i:?}")
                    .context(current_crate.err_context()));
            }
        }
    }
    Ok(items)
}

fn convert_api_item_fn(
    current_crate: &CrateContext,
    scratch: &mut Scratch,
    item_fn: &TraitItemFn,
    idx: u32,
) -> Result<ApiItemOwned> {
    let mut args = vec![];
    for input in &item_fn.sig.inputs {
        let FnArg::Typed(pat_type) = input else {
            return Err(
                anyhow!("Unsupported fn input type {input:?}").context(current_crate.err_context())
            );
        };
        let Pat::Ident(arg_ident) = pat_type.pat.deref() else {
            return Err(anyhow!("Unsupported fn arg ident {:?}", pat_type.pat)
                .context(current_crate.err_context()));
        };
        let ty = convert_ty(&pat_type.ty, current_crate, scratch)?;
        args.push(ArgumentOwned {
            ident: arg_ident.ident.to_string(),
            ty,
        });
    }
    let return_ty = match &item_fn.sig.output {
        ReturnType::Default => None,
        ReturnType::Type(_, ty) => Some(convert_ty(ty, current_crate, scratch)?),
    };
    // let return_ty = convert_ty(&item_fn.sig.output, current_file, cache)?;
    let since = get_since_attr(&item_fn.attrs, current_crate)?;
    Ok(ApiItemOwned {
        id: UNib32(idx),
        kind: ApiItemKindOwned::Method { args, return_ty },
        multiplicity: Multiplicity::Flat,
        since,
        ident: item_fn.sig.ident.to_string(),
        docs: collect_docs(&item_fn.attrs),
    })
}

fn convert_api_item_stream(
    current_crate: &CrateContext,
    scratch: &mut Scratch,
    item_macro: &TraitItemMacro,
    idx: u32,
    is_up: bool,
) -> Result<ApiItemOwned> {
    let macro_name = if is_up { "ww_stream" } else { "ww_sink" };
    let args: StreamAndImplMacroArgs = parse2(item_macro.mac.tokens.clone())
        .context(format!("parsing {macro_name}! arguments"))
        .context(current_crate.err_context())?;
    let ty = convert_ty_path(&args.type_or_trait, current_crate, scratch)?;
    let multiplicity = convert_multiplicity(&args.multiplicity, current_crate, scratch)?;
    let since = get_since_attr(&item_macro.attrs, current_crate)?;
    let docs = collect_docs(&item_macro.attrs);

    Ok(ApiItemOwned {
        id: UNib32(idx),
        kind: ApiItemKindOwned::Stream { ty, is_up },
        multiplicity,
        since,
        ident: args.resource_name.to_string(),
        docs,
    })
}

fn convert_api_item_property(
    current_crate: &CrateContext,
    scratch: &mut Scratch,
    item_macro: &TraitItemMacro,
    idx: u32,
) -> Result<ApiItemOwned> {
    let args: PropertyMacroArgs = parse2(item_macro.mac.tokens.clone())
        .context("parsing ww_property! arguments")
        .context(current_crate.err_context())?;
    let ty = convert_ty_path(&args.ty, current_crate, scratch)?;
    let write_err_ty = if let Some(write_err_ty) = &args.write_err_ty {
        Some(convert_ty_path(write_err_ty, current_crate, scratch)?)
    } else {
        None
    };
    let multiplicity = convert_multiplicity(&args.multiplicity, current_crate, scratch)?;
    let since = get_since_attr(&item_macro.attrs, current_crate)?;
    let docs = collect_docs(&item_macro.attrs);

    Ok(ApiItemOwned {
        id: UNib32(idx),
        kind: ApiItemKindOwned::Property {
            ty,
            access: args.access,
            write_err_ty,
        },
        multiplicity,
        since,
        ident: args.resource_name.to_string(),
        docs,
    })
}

fn convert_api_item_impl(
    current_crate: &CrateContext,
    scratch: &mut Scratch,
    item_macro: &TraitItemMacro,
    idx: u32,
) -> Result<ApiItemOwned> {
    let args: StreamAndImplMacroArgs = parse2(item_macro.mac.tokens.clone())
        .context("parsing ww_impl! arguments")
        .context(current_crate.err_context())?;

    let len = args.type_or_trait.path.segments.len();
    let kind = if len == 1 {
        let trait_name = &args.type_or_trait.path.segments[0];
        find_and_convert_trait(trait_name, current_crate, scratch)?
    } else if len == 2 {
        let dep_crate_name = args.type_or_trait.path.segments[0].ident.to_string();
        let dependent_crate = current_crate.load_dependent_crate(&dep_crate_name, scratch)?;
        let trait_name = &args.type_or_trait.path.segments[1];
        find_and_convert_trait(trait_name, &dependent_crate, scratch)?
    } else {
        return Err(
            anyhow!("Only support `MyTrait` and `ext_crate::MyTrait` for now")
                .context(current_crate.err_context()),
        );
    };
    let multiplicity = convert_multiplicity(&args.multiplicity, current_crate, scratch)?;
    let since = get_since_attr(&item_macro.attrs, current_crate)?;
    Ok(ApiItemOwned {
        id: UNib32(idx),
        kind,
        multiplicity,
        since,
        ident: args.resource_name.to_string(),
        docs: collect_docs(&item_macro.attrs),
    })
}

fn find_and_convert_trait(
    trait_name: &PathSegment,
    current_crate: &CrateContext,
    scratch: &mut Scratch,
) -> Result<ApiItemKindOwned> {
    for item in &current_crate.lib_rs_ast.items {
        let Item::Trait(item_trait) = item else {
            continue;
        };
        if item_trait.ident != trait_name.ident {
            continue;
        }
        let crate_idx = scratch.root_bundle.find_crate_or_create(current_crate);
        let items = convert_api_items(item_trait, current_crate, scratch)?;
        let trait_idx = scratch.root_bundle.traits.len() as u32;
        scratch
            .root_bundle
            .traits
            .push(ApiLevelLocationOwned::InLine {
                level: ApiLevelOwned {
                    docs: collect_docs(&item_trait.attrs),
                    crate_idx,
                    trait_name: trait_name.ident.to_string(),
                    items,
                },
                crate_idx,
            });
        return Ok(ApiItemKindOwned::Trait {
            trait_idx: trait_idx.into(),
        });
    }
    Err(anyhow!("Trait {trait_name:?} not found").context(current_crate.err_context()))
}

/// ww_impl!(gpio: Gpio) or stream!(data: Packet)
struct StreamAndImplMacroArgs {
    resource_name: Ident,
    multiplicity: Option<Option<PathSegment>>,
    type_or_trait: TypePath,
}

impl Parse for StreamAndImplMacroArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let resource_name = input.parse()?;
        let multiplicity = parse_multiplicity(&input)?;
        let _colon: Token![:] = input.parse()?;
        let type_or_trait = input.parse()?;
        Ok(StreamAndImplMacroArgs {
            resource_name,
            multiplicity,
            type_or_trait,
        })
    }
}

fn parse_multiplicity(input: &ParseStream) -> syn::Result<Option<Option<PathSegment>>> {
    let lookahead = input.lookahead1();
    if lookahead.peek(syn::token::Bracket) {
        let (_, _, inside_brackets) = input.parse_any_delimiter()?;
        if inside_brackets.is_empty() {
            Ok(Some(None))
        } else {
            let index_type: PathSegment = inside_brackets.parse()?;
            Ok(Some(Some(index_type)))
        }
    } else {
        Ok(None)
    }
}

fn convert_multiplicity(
    multiplicity: &Option<Option<PathSegment>>,
    current_crate: &CrateContext,
    scratch: &mut Scratch,
) -> Result<Multiplicity> {
    match multiplicity {
        Some(Some(index_ty)) => {
            let ty = convert_ty_path_segment(&index_ty, current_crate, scratch)?;
            let index_type_idx = scratch.root_bundle.push_out_of_line_idx(ty, current_crate);
            Ok(Multiplicity::Array {
                index_type_idx: Some(index_type_idx),
            })
        }
        Some(None) => Ok(Multiplicity::Array {
            index_type_idx: None,
        }),
        None => Ok(Multiplicity::Flat),
    }
}

/// ww_property!(rw value: u8)
/// valid access: const, ro, rw, wo
/// ww_property!(rw+observe value: u8)
/// observe valid with: ro, rw
/// ww_property!(rw value: u8, MyError)
struct PropertyMacroArgs {
    access: PropertyAccess,
    resource_name: Ident,
    multiplicity: Option<Option<PathSegment>>,
    ty: TypePath,
    write_err_ty: Option<TypePath>,
}

impl Parse for PropertyMacroArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let access: Ident = input.parse()?;
        let access = match access.to_string().as_str() {
            "const" => PropertyAccess::Const,
            "ro" => PropertyAccess::ReadOnly { observe: false },
            "rw" => PropertyAccess::ReadWrite { observe: false },
            "wo" => PropertyAccess::WriteOnly,
            u => {
                return Err(syn::Error::new(
                    access.span(),
                    format!("invalid access: {}, expected: const/ro/rw/wo", u),
                ));
            }
        };
        let access = match access {
            PropertyAccess::ReadOnly { .. } | PropertyAccess::ReadWrite { .. } => {
                let lookahead = input.lookahead1();
                if lookahead.peek(Token![+]) {
                    let _: Token![+] = input.parse()?;
                    let observe: Ident = input.parse()?;
                    if observe.to_string().as_str() != "observe" {
                        return Err(syn::Error::new(
                            observe.span(),
                            "expected +observe or nothing",
                        ));
                    }
                    if matches!(access, PropertyAccess::ReadOnly { .. }) {
                        PropertyAccess::ReadOnly { observe: true }
                    } else {
                        PropertyAccess::ReadWrite { observe: true }
                    }
                } else {
                    access
                }
            }
            a => a,
        };
        let resource_name = input.parse()?;
        let multiplicity = parse_multiplicity(&input)?;
        let _colon: Token![:] = input.parse()?;
        let ty = input.parse()?;
        let write_err_ty = if input.peek(Token![,]) {
            let _: Token![,] = input.parse()?;
            Some(input.parse()?)
        } else {
            None
        };
        Ok(Self {
            access,
            resource_name,
            multiplicity,
            ty,
            write_err_ty,
        })
    }
}
