use super::{
    crate_walker::{CrateContext, Scratch},
    errors::{ListedOn, get_errors_attr, reject_errors_attr},
    ty::{convert_named_def, convert_ty, convert_ty_path, convert_ty_path_segment},
    util::{
        collect_docs, get_batched_attr, get_delivery_attr, get_flow_attr, get_optional_attr,
        get_since_attr, get_stream_fn_attr,
    },
};
use anyhow::{Context, Result, anyhow};
use proc_macro2::Ident;
use shrink_wrap::UNib32;
use std::ops::Deref;
use syn::parse::discouraged::AnyDelimiter;
use syn::parse::{Parse, ParseStream};
use syn::{
    FnArg, Item, LitInt, Pat, PathSegment, ReturnType, Token, TraitItem, TraitItemFn,
    TraitItemMacro, Type, TypePath, parse2,
};
use ww_self::{
    ApiItemKindOwned, ApiItemOwned, ApiLevelLocationOwned, ApiLevelOwned, ArgumentOwned, Batching,
    Delivery, ErrorVariantOwned, GenericArgOwned, Multiplicity, PropertyAccess, TypeOwned,
};

pub(crate) fn convert_api_items(
    item_trait: &syn::ItemTrait,
    current_crate: &CrateContext,
    scratch: &mut Scratch,
) -> Result<Vec<ApiItemOwned>> {
    let mut items = vec![];
    let mut idx = 0;
    for item in &item_trait.items {
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
                    "reserved" => {
                        let args: ReservedMacroArgs = parse2(item_macro.mac.tokens.clone())
                            .context("parsing reserved! arguments")
                            .context(current_crate.err_context())?;
                        let start: usize = args.start.base10_parse()?;
                        let end: usize = args.end.base10_parse()?;
                        if idx != start {
                            return Err(anyhow!("Reserved resource index is not monotonic, expected: {idx} got: {start}")
                            .context(current_crate.err_context()));
                        }
                        if end < start {
                            return Err(anyhow!(
                                "Reserved resource index is not monotonic, end must be >= start"
                            )
                            .context(current_crate.err_context()));
                        }
                        idx += end - start;
                    }
                    u => {
                        return Err(anyhow!("Unsupported resource kind: {u:?}")
                            .context(current_crate.err_context()));
                    }
                }
            }
            TraitItem::Const(trait_item_const) => {
                let ty = convert_ty(&trait_item_const.ty, current_crate, scratch)?;
                let since = get_since_attr(&trait_item_const.attrs, current_crate)?;
                let docs = collect_docs(&trait_item_const.attrs);
                reject_errors_attr(
                    &trait_item_const.attrs,
                    &trait_item_const.ident.to_string(),
                    "a const can't fail, only methods, stream functions and properties have error lists",
                )
                .context(current_crate.err_context())?;

                items.push(ApiItemOwned {
                    id: UNib32(idx as u32),
                    kind: ApiItemKindOwned::Property {
                        ty,
                        access: PropertyAccess::Const,
                        write_err_ty: None,
                        errors: None,
                        batched: None,
                    },
                    multiplicity: Multiplicity::Flat,
                    since,
                    ident: trait_item_const.ident.to_string(),
                    docs,
                    optional: get_optional_attr(&trait_item_const.attrs, current_crate)?,
                });
            }
            i => {
                return Err(anyhow!("Unsupported resource kind: {i:?}")
                    .context(current_crate.err_context()));
            }
        }
        idx += 1;
    }
    reject_group_item_collisions(&items).context(current_crate.err_context())?;
    Ok(items)
}

/// An `#[optional(group)]` named like an item of the same trait could not be omitted as a whole (`omit` resolves the
/// item first), so the name is refused.
fn reject_group_item_collisions(items: &[ApiItemOwned]) -> Result<()> {
    for item in items {
        let Some(group) = item.optional.as_ref().and_then(|o| o.group.as_deref()) else {
            continue;
        };
        if let Some(other) = items.iter().find(|i| i.ident == group) {
            return Err(anyhow!(
                "optional group `{group}` has the same name as the {} `{}` in the same trait, so `omit = [\"{group}\"]` \
                 could only leave out the item, never the whole group: give the group a different name, e.g. \
                 #[optional({group}_group)]",
                item_kind_name(&other.kind),
                other.ident
            ));
        }
    }
    Ok(())
}

fn item_kind_name(kind: &ApiItemKindOwned) -> &'static str {
    match kind {
        ApiItemKindOwned::Method { .. } => "method",
        ApiItemKindOwned::Property { .. } => "property",
        ApiItemKindOwned::Trait { .. } => "ww_impl!",
        _ => "item",
    }
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
    let stream_fn = get_stream_fn_attr(&item_fn.attrs, current_crate)?;
    let errors = get_errors_attr(
        &item_fn.attrs,
        &item_fn.sig.ident.to_string(),
        ListedOn::Fn {
            stream: stream_fn.is_some(),
            return_ty: return_ty.as_ref(),
        },
        scratch,
    )
    .context(current_crate.err_context())?;
    let since = get_since_attr(&item_fn.attrs, current_crate)?;
    let kind = match stream_fn {
        Some(_)
            if return_ty
                .as_ref()
                .is_some_and(|ty| !matches!(ty, TypeOwned::Result { .. })) =>
        {
            return Err(anyhow!(
                "stream function {} returns neither nothing nor a Result<T, E>: a stream function \
                 answers its open with its result, wrap it into Result<T, E>",
                item_fn.sig.ident
            )
            .context(current_crate.err_context()));
        }
        Some(stream) => {
            let ty = convert_ty(&stream.ty, current_crate, scratch)?;
            let batch_item_ty = batch_item_ty(&ty, stream.batched, current_crate, scratch)?;
            stream_kind(
                ty,
                stream.delivery,
                stream.batched,
                args,
                return_ty,
                errors,
                batch_item_ty,
            )
        }
        None => ApiItemKindOwned::Method {
            args,
            return_ty,
            errors,
        },
    };
    Ok(ApiItemOwned {
        id: UNib32(idx),
        kind,
        multiplicity: Multiplicity::Flat,
        since,
        ident: item_fn.sig.ident.to_string(),
        docs: collect_docs(&item_fn.attrs),
        optional: get_optional_attr(&item_fn.attrs, current_crate)?,
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
        .context(format!(
            "parsing {macro_name}! arguments: '{}'",
            item_macro.mac.tokens
        ))
        .context(current_crate.err_context())?;
    let ty = convert_ty(&args.type_or_trait, current_crate, scratch)?;
    let multiplicity = convert_multiplicity(&args.multiplicity, current_crate, scratch)?;
    let since = get_since_attr(&item_macro.attrs, current_crate)?;
    let docs = collect_docs(&item_macro.attrs);
    let flow = get_flow_attr(&item_macro.attrs, is_up, current_crate)?;
    let delivery = get_delivery_attr(&item_macro.attrs, is_up, current_crate)?;
    let batched = get_batched_attr(&item_macro.attrs, is_up, current_crate)?;
    reject_errors_attr(
        &item_macro.attrs,
        &args.resource_name.to_string(),
        &format!(
            "{macro_name}! has no result to list errors of, a stream function can have one: \
             #[errors(..)] #[stream(T)] fn name() -> Result<(), E>"
        ),
    )
    .context(current_crate.err_context())?;
    let kind = match flow {
        Some(flow) => ApiItemKindOwned::FlowSink { ty, flow },
        None if is_up => {
            let batch_item_ty = batch_item_ty(&ty, batched, current_crate, scratch)?;
            stream_kind(ty, delivery, batched, vec![], None, None, batch_item_ty)
        }
        None => ApiItemKindOwned::Stream { ty, is_up },
    };

    Ok(ApiItemOwned {
        id: UNib32(idx),
        kind,
        multiplicity,
        since,
        ident: args.resource_name.to_string(),
        docs,
        optional: get_optional_attr(&item_macro.attrs, current_crate)?,
    })
}

/// A stream function, or `Stream` for a plain one without arguments, result and batching (`stream!`, same bytes as
/// before stream functions existed).
fn stream_kind(
    ty: TypeOwned,
    delivery: Delivery,
    batched: Option<Batching>,
    args: Vec<ArgumentOwned>,
    return_ty: Option<TypeOwned>,
    errors: Option<Vec<ErrorVariantOwned>>,
    batch_item_ty: Option<TypeOwned>,
) -> ApiItemKindOwned {
    if delivery == Delivery::Plain && batched.is_none() && args.is_empty() && return_ty.is_none() {
        return ApiItemKindOwned::Stream { ty, is_up: true };
    }
    ApiItemKindOwned::StreamFn {
        ty,
        delivery,
        args,
        return_ty,
        errors,
        batched,
        batch_item_ty,
    }
}

/// [stream_kind]'s `batch_item_ty`: `Timed<ty>` for a batched timed stream, `None` otherwise (API-41).
fn batch_item_ty(
    ty: &TypeOwned,
    batched: Option<Batching>,
    current_crate: &CrateContext,
    scratch: &mut Scratch,
) -> Result<Option<TypeOwned>> {
    match batched {
        Some(Batching::Timed) => Ok(Some(timed_instance(ty, current_crate, scratch)?)),
        // parsed from this same `ww_self` version's attributes, never the `Unknown` catch-all of an older one
        Some(Batching::Untimed)
        | Some(Batching::SharedClock)
        | Some(Batching::Unknown(_))
        | None => Ok(None),
    }
}

/// The `ww_si::Timed` generic definition instantiated with `item_ty` (API-41): the wire item of a batched timed
/// stream is `Series` of this. `Timed` isn't named anywhere in the trait's own source (only `item_ty` is, as the
/// stream's plain item type), so it's resolved from `ww_si` directly rather than walked from a type a user wrote.
pub(crate) fn timed_instance(
    item_ty: &TypeOwned,
    current_crate: &CrateContext,
    scratch: &mut Scratch,
) -> Result<TypeOwned> {
    let ww_si = Ident::new("ww_si", proc_macro2::Span::call_site());
    let ww_si_cx = current_crate
        .resolve_path(std::iter::once(&ww_si), scratch)
        .context(
            "a batched timed stream or +batched property needs ww_si as a dependency (Timed<T>'s wire form \
             comes from there); depend on it, or for a stream use batched(untimed) if T already carries its own time",
        )?;
    let timed = Ident::new("Timed", proc_macro2::Span::call_site());
    let type_idx = convert_named_def(&timed, &ww_si_cx, scratch)?;
    Ok(TypeOwned::Instance {
        type_idx,
        args: vec![GenericArgOwned::Type(item_ty.clone())],
    })
}

fn convert_api_item_property(
    current_crate: &CrateContext,
    scratch: &mut Scratch,
    item_macro: &TraitItemMacro,
    idx: u32,
) -> Result<ApiItemOwned> {
    let args: PropertyMacroArgs = parse2(item_macro.mac.tokens.clone())
        .context(format!("wrong code snippet: '{}'", item_macro.mac.tokens))
        .context("parsing ww_property! arguments")
        .context(current_crate.err_context())?;
    let ty = convert_ty(&args.ty, current_crate, scratch)?;
    let write_err_ty = if let Some(write_err_ty) = &args.write_err_ty {
        Some(convert_ty_path(write_err_ty, current_crate, scratch)?)
    } else {
        None
    };
    let multiplicity = convert_multiplicity(&args.multiplicity, current_crate, scratch)?;
    let since = get_since_attr(&item_macro.attrs, current_crate)?;
    let docs = collect_docs(&item_macro.attrs);
    let errors = get_errors_attr(
        &item_macro.attrs,
        &args.resource_name.to_string(),
        ListedOn::Property {
            write_err_ty: write_err_ty.as_ref(),
        },
        scratch,
    )
    .context(current_crate.err_context())?;
    // the samples of a level share one clock (API-39): the `sample_times` of its updates
    let batched = args.batched.then_some(Batching::SharedClock);

    Ok(ApiItemOwned {
        id: UNib32(idx),
        kind: ApiItemKindOwned::Property {
            ty,
            access: args.access,
            write_err_ty,
            errors,
            batched,
        },
        multiplicity,
        since,
        ident: args.resource_name.to_string(),
        docs,
        optional: get_optional_attr(&item_macro.attrs, current_crate)?,
    })
}

fn convert_api_item_impl(
    current_crate: &CrateContext,
    scratch: &mut Scratch,
    item_macro: &TraitItemMacro,
    idx: u32,
) -> Result<ApiItemOwned> {
    let args: StreamAndImplMacroArgs = parse2(item_macro.mac.tokens.clone())
        .context(format!(
            "parsing ww_impl! arguments: '{}'",
            item_macro.mac.tokens
        ))
        .context(current_crate.err_context())?;

    let Type::Path(type_path) = &args.type_or_trait else {
        return Err(anyhow!(
            "Expected trait path, got: {:?}",
            args.type_or_trait
        ));
    };
    let segments: Vec<_> = type_path.path.segments.iter().collect();
    let Some((trait_name, leading)) = segments.split_last() else {
        return Err(anyhow!("Empty trait path").context(current_crate.err_context()));
    };
    let kind = if leading.is_empty() {
        find_and_convert_trait(trait_name, current_crate, scratch)?
    } else {
        let cx = current_crate.resolve_path(leading.iter().map(|s| &s.ident), scratch)?;
        find_and_convert_trait(trait_name, &cx, scratch)?
    };
    let multiplicity = convert_multiplicity(&args.multiplicity, current_crate, scratch)?;
    let since = get_since_attr(&item_macro.attrs, current_crate)?;
    reject_errors_attr(
        &item_macro.attrs,
        &args.resource_name.to_string(),
        "ww_impl! is a trait, list the errors on its items",
    )
    .context(current_crate.err_context())?;
    let optional = get_optional_attr(&item_macro.attrs, current_crate)?;
    Ok(ApiItemOwned {
        id: UNib32(idx),
        kind,
        multiplicity,
        since,
        ident: args.resource_name.to_string(),
        docs: collect_docs(&item_macro.attrs),
        optional,
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
        let trait_idx = convert_trait(item_trait, current_crate, scratch)?;
        return Ok(ApiItemKindOwned::Trait {
            trait_idx,
            omitted: None,
        });
    }
    Err(anyhow!("Trait {trait_name:?} not found").context(current_crate.err_context()))
}

/// Convert a trait and put it into [Scratch] traits array, returns its index.
/// If [Scratch::dedup_traits] is set, an already converted trait with the same name and from the same crate is reused.
pub(crate) fn convert_trait(
    item_trait: &syn::ItemTrait,
    current_crate: &CrateContext,
    scratch: &mut Scratch,
) -> Result<UNib32> {
    let crate_idx = scratch.root_bundle.find_crate_or_create(current_crate);
    let trait_name = item_trait.ident.to_string();
    if scratch.dedup_traits
        && let Some(idx) = scratch.root_bundle.traits.iter().position(|l| {
            matches!(l, ApiLevelLocationOwned::InLine { level, .. } if level.crate_idx == crate_idx && level.trait_name == trait_name)
        })
    {
        return Ok(UNib32(idx as u32));
    }
    let items = convert_api_items(item_trait, current_crate, scratch)?;
    let trait_idx = scratch.root_bundle.traits.len() as u32;
    scratch
        .root_bundle
        .traits
        .push(ApiLevelLocationOwned::InLine {
            level: ApiLevelOwned {
                docs: collect_docs(&item_trait.attrs),
                crate_idx,
                trait_name,
                items,
            },
            crate_idx,
        });
    Ok(UNib32(trait_idx))
}

/// ww_impl!(gpio: Gpio) or stream!(data: Packet)
pub struct StreamAndImplMacroArgs {
    pub resource_name: Ident,
    pub multiplicity: Option<Option<PathSegment>>,
    pub type_or_trait: Type,
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
            let ty = convert_ty_path_segment(index_ty, current_crate, scratch)?;
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
/// ww_property!(ro+observe+batched value: u8): every sample observable, in batches (API-39)
/// ww_property!(rw value: u8, MyError)
pub struct PropertyMacroArgs {
    pub access: PropertyAccess,
    /// `+batched` (API-39), after `+observe`.
    pub batched: bool,
    pub resource_name: Ident,
    pub multiplicity: Option<Option<PathSegment>>,
    pub ty: Type,
    pub write_err_ty: Option<TypePath>,
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
        let mut batched = false;
        let access = match access {
            PropertyAccess::ReadOnly { .. } | PropertyAccess::ReadWrite { .. } => {
                // `+observe`, then `+batched` (API-39), which needs it
                let mut observe = false;
                while input.peek(Token![+]) {
                    let _: Token![+] = input.parse()?;
                    let flag: Ident = input.parse()?;
                    match flag.to_string().as_str() {
                        "observe" if !observe => observe = true,
                        "batched" if observe && !batched => batched = true,
                        "batched" if !observe => {
                            return Err(syn::Error::new(
                                flag.span(),
                                "+batched needs +observe first: `ro+observe+batched`",
                            ));
                        }
                        _ => {
                            return Err(syn::Error::new(
                                flag.span(),
                                "expected +observe, +observe+batched or nothing",
                            ));
                        }
                    }
                }
                if matches!(access, PropertyAccess::ReadOnly { .. }) {
                    PropertyAccess::ReadOnly { observe }
                } else {
                    PropertyAccess::ReadWrite { observe }
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
            batched,
            resource_name,
            multiplicity,
            ty,
            write_err_ty,
        })
    }
}

struct ReservedMacroArgs {
    start: LitInt,
    end: LitInt,
}

impl Parse for ReservedMacroArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let start = input.parse()?;
        let _: Token![..=] = input.parse()?;
        let end = input.parse()?;
        Ok(ReservedMacroArgs { start, end })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use ww_self::OptionalOwned;

    fn method(ident: &str, group: Option<&str>) -> ApiItemOwned {
        ApiItemOwned {
            id: UNib32(0),
            kind: ApiItemKindOwned::Method {
                args: vec![],
                return_ty: None,
                errors: None,
            },
            multiplicity: Multiplicity::Flat,
            since: None,
            ident: ident.into(),
            docs: vec![],
            optional: group.map(|g| OptionalOwned {
                group: Some(g.into()),
            }),
        }
    }

    #[test]
    fn group_named_like_item_is_refused() {
        let items = [method("format", None), method("fmt_x", Some("format"))];
        let err = reject_group_item_collisions(&items)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("group `format`") && err.contains("method `format`"),
            "{err}"
        );
    }

    #[test]
    fn distinct_names_pass() {
        let items = [method("format", None), method("fmt_x", Some("formatting"))];
        reject_group_item_collisions(&items).unwrap();
    }

    /// A batched timed stream's wire item (API-41): `Timed<T>` isn't named in the trait's own source, only resolved
    /// from `ww_si` when building the model, found again on the host after a device left it out as a known type
    /// and `ww_self::generics::prepare` instantiated it with this API's own item type.
    #[test]
    fn batched_timed_stream_wire_item_round_trips() {
        wire_item_round_trips(
            "streams_batched_api",
            None,
            "reliable",
            "frames",
            "Timed<Sample>",
        );
    }

    /// A `+batched` property (API-39) is `Batching::SharedClock`: its samples are a `Series<T>` of its own type,
    /// their times the `sample_times` of the level's update; it needs nothing else in the model.
    #[test]
    fn batched_property_shared_clock() {
        use ww_self::Batching;

        let crate_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/observe_batched_api");
        let full = crate::load(&crate_path, Some("ObserveBatched".into()), false).unwrap();
        let batched = |name: &str| match &full
            .root
            .items
            .iter()
            .find(|i| i.ident == name)
            .unwrap()
            .kind
        {
            ApiItemKindOwned::Property { batched, .. } => *batched,
            other => panic!("{other:?}"),
        };
        assert_eq!(batched("mode"), None);
        assert_eq!(batched("reading"), Some(Batching::SharedClock));
    }

    /// `timed` has a `Timed<T>` batch item that survives a device leaving `Timed` out, `plain` has none.
    fn wire_item_round_trips(
        api_crate: &str,
        trait_name: Option<&str>,
        timed: &str,
        plain: &str,
        instance_name: &str,
    ) {
        use shrink_wrap::SerializeShrinkWrapOwned;
        use ww_self::{FieldsOwned, TypeLocationOwned};

        let crate_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tests")
            .join(api_crate);
        let full = crate::load(&crate_path, trait_name.map(String::from), false).unwrap();

        // untimed stream or a property that isn't batched: no extra wire data
        assert_eq!(batch_item_ty_of(&full, plain), None);

        // a Timed<T> instance, pointing at the Timed generic definition in ww_si
        let ty = batch_item_ty_of(&full, timed).expect("a batch_item_ty");
        let TypeOwned::Instance { type_idx, args } = &ty else {
            panic!("expected an Instance, got {ty:?}");
        };
        assert_eq!(args.len(), 1);
        let TypeLocationOwned::InLine {
            ty: TypeOwned::Struct(timed_def),
            crate_idx,
        } = &full.types[type_idx.0 as usize]
        else {
            panic!("expected Timed's generic definition in-line");
        };
        assert_eq!(timed_def.ident, "Timed");
        assert_eq!(full.ext_crates[crate_idx.0 as usize].crate_id, "ww_si");

        // sent by a device: Timed is known from ww_si's snapshot, left out and replaced by a signature
        let sent = crate::transform::snapshot::skip_known(&full).unwrap();
        assert!(
            sent.types
                .iter()
                .any(|l| matches!(l, TypeLocationOwned::SkippedFullVersion { type_name, .. } if type_name == "Timed"))
        );
        assert!(sent.to_ww_bytes_owned().unwrap().len() < full.to_ww_bytes_owned().unwrap().len());

        // on the host: put back from the embedded snapshot and instantiate with this API's own item type
        let mut received = sent;
        let not_inlined = wire_weaver_snapshots::inline_skipped(&mut received);
        assert!(not_inlined.is_empty(), "{not_inlined:?}");
        ww_self::generics::prepare(&mut received).unwrap();

        let ty = batch_item_ty_of(&received, timed).expect("a batch_item_ty");
        let TypeOwned::OutOfLine { type_idx } = ty else {
            panic!("expected the instance resolved to a concrete definition, got {ty:?}");
        };
        let TypeLocationOwned::InLine {
            ty: TypeOwned::Struct(timed_def),
            ..
        } = &received.types[type_idx.0 as usize]
        else {
            panic!("expected a concrete {instance_name} definition");
        };
        assert_eq!(timed_def.ident, instance_name);
        let FieldsOwned::Named(fields) = &timed_def.fields else {
            panic!("expected named fields")
        };
        let names: Vec<_> = fields.iter().map(|f| f.ident.as_deref()).collect();
        assert_eq!(names, [Some("t"), Some("value")]);
    }

    fn batch_item_ty_of(bundle: &ww_self::ApiBundleOwned, name: &str) -> Option<TypeOwned> {
        match &bundle
            .root
            .items
            .iter()
            .find(|i| i.ident == name)
            .unwrap_or_else(|| panic!("no item named {name}"))
            .kind
        {
            ApiItemKindOwned::StreamFn { batch_item_ty, .. } => batch_item_ty.clone(),
            other => panic!("'{name}' is not a stream function: {other:?}"),
        }
    }

    fn access_of(tokens: &str) -> Result<(PropertyAccess, bool), String> {
        syn::parse_str::<PropertyMacroArgs>(tokens)
            .map(|args| (args.access, args.batched))
            .map_err(|e| e.to_string())
    }

    /// `+batched` after `+observe` (API-39), never alone or twice
    #[test]
    fn batched_property_access() {
        assert!(matches!(
            access_of("ro+observe+batched i_a: Ampere"),
            Ok((PropertyAccess::ReadOnly { observe: true }, true))
        ));
        assert!(matches!(
            access_of("rw+observe value: u8"),
            Ok((PropertyAccess::ReadWrite { observe: true }, false))
        ));
        let err = access_of("ro+batched i_a: Ampere").unwrap_err();
        assert!(err.contains("needs +observe first"), "{err}");
        for twice in [
            "ro+observe+batched+batched x: u8",
            "ro+observe+observe x: u8",
        ] {
            let err = access_of(twice).unwrap_err();
            assert!(err.contains("expected +observe"), "{err}");
        }
    }
}
