use super::crate_walker::CrateContext;
use anyhow::{Context, Result, anyhow};
use proc_macro2::Ident;
use semver::Version;
use shrink_wrap::ElementSize;
use syn::parse::{Parse, ParseStream};
use syn::{Attribute, Expr, Lit, Meta, Token, UseTree};
use ww_self::{Batching, Delivery, FlowControl, OptionalOwned, Repr};
use ww_version::VersionTriplet;

pub(crate) fn collect_docs(attrs: &[Attribute]) -> Vec<String> {
    let mut docs = vec![];
    for attr in attrs.iter() {
        if !attr.path().is_ident("doc") {
            continue;
        }
        if let Meta::NameValue(name_value) = &attr.meta
            && let Expr::Lit(expr_lit) = &name_value.value
            && let Lit::Str(lit_str) = &expr_lit.lit
        {
            docs.push(lit_str.value());
        }
    }
    let trim_space = docs
        .iter()
        .filter(|s| !s.is_empty())
        .map(|s| s.chars().take_while(|c| c.is_whitespace()).count())
        .min();
    if let Some(trim_space) = trim_space {
        for doc in docs.iter_mut() {
            for _ in 0..trim_space {
                if doc.is_empty() {
                    break;
                }
                doc.remove(0);
            }
        }
    }
    docs
}

pub(crate) fn get_since_attr(
    attrs: &[Attribute],
    current_crate: &CrateContext,
) -> Result<Option<VersionTriplet>> {
    let Some(attr) = attrs.iter().find(|a| a.path().is_ident("since")) else {
        return Ok(None);
    };
    if let Meta::NameValue(name_value) = &attr.meta
        && let Expr::Lit(expr_lit) = &name_value.value
        && let Lit::Str(lit_str) = &expr_lit.lit
    {
        let version = Version::parse(&lit_str.value()).context(current_crate.err_context())?;
        Ok(Some(VersionTriplet::new(
            version.major as u32,
            version.minor as u32,
            version.patch as u32,
        )))
    } else {
        Err(anyhow!("expected #[since = \"x.y.z\"]").context(current_crate.err_context()))
    }
}

/// Delivery kind of a `stream!` from `#[counted]` / `#[reliable]`, plain without either. Not allowed on `sink!`.
pub(crate) fn get_delivery_attr(
    attrs: &[Attribute],
    is_up: bool,
    current_crate: &CrateContext,
) -> Result<Delivery> {
    let mut delivery = Delivery::Plain;
    for attr in attrs {
        let d = if attr.path().is_ident("counted") {
            Delivery::Counted
        } else if attr.path().is_ident("reliable") {
            Delivery::Reliable
        } else {
            continue;
        };
        if !matches!(attr.meta, Meta::Path(_)) {
            return Err(
                anyhow!("expected #[counted] or #[reliable] without arguments")
                    .context(current_crate.err_context()),
            );
        }
        if !is_up {
            return Err(anyhow!(
                "#[counted] and #[reliable] are only supported on stream!, not on sink! (see #[credit])"
            )
            .context(current_crate.err_context()));
        }
        if delivery != Delivery::Plain {
            return Err(
                anyhow!("a stream is either #[counted] or #[reliable], not both")
                    .context(current_crate.err_context()),
            );
        }
        delivery = d;
    }
    Ok(delivery)
}

/// `#[stream(T)]`, `#[stream(T, counted)]` or `#[stream(T, reliable)]` on a trait fn: a stream function (API-31),
/// optionally with `batched` or `batched(untimed)` after the delivery (API-41).
pub(crate) struct StreamFnAttr {
    pub ty: syn::Type,
    pub delivery: Delivery,
    pub batched: Option<Batching>,
}

impl Parse for StreamFnAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let ty = input.parse()?;
        let mut delivery = None;
        let mut batched = None;
        while !input.is_empty() {
            let _comma: Token![,] = input.parse()?;
            if input.is_empty() {
                break;
            }
            let kind: Ident = input.parse()?;
            match kind.to_string().as_str() {
                "batched" => {
                    if batched.is_some() {
                        return Err(syn::Error::new(kind.span(), "batched given twice"));
                    }
                    batched = Some(parse_batching(input)?);
                }
                k @ ("plain" | "counted" | "reliable") => {
                    if delivery.is_some() || batched.is_some() {
                        return Err(syn::Error::new(
                            kind.span(),
                            "one delivery (counted or reliable), right after the item type, before batched",
                        ));
                    }
                    delivery = Some(match k {
                        "plain" => Delivery::Plain,
                        "counted" => Delivery::Counted,
                        _ => Delivery::Reliable,
                    });
                }
                u => {
                    return Err(syn::Error::new(
                        kind.span(),
                        format!("unknown stream kind {u}, expected counted, reliable or batched"),
                    ));
                }
            }
        }
        Ok(StreamFnAttr {
            ty,
            delivery: delivery.unwrap_or(Delivery::Plain),
            batched,
        })
    }
}

/// What follows `batched`: nothing, or `(untimed)`.
fn parse_batching(input: ParseStream) -> syn::Result<Batching> {
    if !input.peek(syn::token::Paren) {
        return Ok(Batching::Timed);
    }
    let content;
    syn::parenthesized!(content in input);
    let form: Ident = content.parse()?;
    if form != "untimed" || !content.is_empty() {
        return Err(syn::Error::new(
            form.span(),
            "expected batched or batched(untimed)",
        ));
    }
    Ok(Batching::Untimed)
}

/// `#[batched]` or `#[batched(untimed)]` next to `stream!` (API-41), None without it. Not allowed on `sink!`.
pub(crate) fn get_batched_attr(
    attrs: &[Attribute],
    is_up: bool,
    current_crate: &CrateContext,
) -> Result<Option<Batching>> {
    let mut batched = None;
    for attr in attrs.iter().filter(|a| a.path().is_ident("batched")) {
        if !is_up {
            return Err(
                anyhow!("#[batched] is only supported on stream!, not on sink!")
                    .context(current_crate.err_context()),
            );
        }
        if batched.is_some() {
            return Err(anyhow!("#[batched] given twice").context(current_crate.err_context()));
        }
        let batching = match &attr.meta {
            Meta::Path(_) => Batching::Timed,
            Meta::List(_) => attr
                .parse_args_with(|input: ParseStream| {
                    let form: Ident = input.parse()?;
                    if form != "untimed" || !input.is_empty() {
                        return Err(syn::Error::new(
                            form.span(),
                            "expected #[batched] or #[batched(untimed)]",
                        ));
                    }
                    Ok(Batching::Untimed)
                })
                .context(current_crate.err_context())?,
            Meta::NameValue(_) => {
                return Err(anyhow!("expected #[batched] or #[batched(untimed)]")
                    .context(current_crate.err_context()));
            }
        };
        batched = Some(batching);
    }
    Ok(batched)
}

/// The `#[stream(..)]` attribute of a trait fn, `None` for an ordinary method.
pub(crate) fn get_stream_fn_attr(
    attrs: &[Attribute],
    current_crate: &CrateContext,
) -> Result<Option<StreamFnAttr>> {
    let Some(attr) = attrs.iter().find(|a| a.path().is_ident("stream")) else {
        return Ok(None);
    };
    let parsed = attr
        .parse_args::<StreamFnAttr>()
        .context(
            "expected #[stream(ItemType)] or #[stream(ItemType, counted | reliable)], optionally with batched or \
             batched(untimed) last",
        )
        .context(current_crate.err_context())?;
    Ok(Some(parsed))
}

/// `#[optional]` or `#[optional(group)]` on a method, property or stream (API-16), `None` for a required item.
pub(crate) fn get_optional_attr(
    attrs: &[Attribute],
    current_crate: &CrateContext,
) -> Result<Option<OptionalOwned>> {
    let Some(attr) = attrs.iter().find(|a| a.path().is_ident("optional")) else {
        return Ok(None);
    };
    match &attr.meta {
        Meta::Path(_) => Ok(Some(OptionalOwned { group: None })),
        Meta::List(list) => {
            let group: Ident = list.parse_args().map_err(|e| {
                anyhow!("expected #[optional] or #[optional(group)]: {e}")
                    .context(current_crate.err_context())
            })?;
            Ok(Some(OptionalOwned {
                group: Some(group.to_string()),
            }))
        }
        Meta::NameValue(_) => Err(anyhow!("expected #[optional] or #[optional(group)]")
            .context(current_crate.err_context())),
    }
}

/// Flow control of a sink: `#[credit]` (see `docs/api/streams.md`), `None` for a plain sink.
pub(crate) fn get_flow_attr(
    attrs: &[Attribute],
    is_up: bool,
    current_crate: &CrateContext,
) -> Result<Option<FlowControl>> {
    let Some(attr) = attrs.iter().find(|a| a.path().is_ident("credit")) else {
        return Ok(None);
    };
    if !matches!(attr.meta, Meta::Path(_)) {
        return Err(
            anyhow!("expected #[credit] without arguments").context(current_crate.err_context())
        );
    }
    if is_up {
        return Err(
            anyhow!("#[credit] is only supported on sink!, not on stream!")
                .context(current_crate.err_context()),
        );
    }
    Ok(Some(FlowControl::Credit))
}

/// Path segments leading to `type_name` imported by a `use` tree and the name it has there, e.g. `([a, b], Ty)` for
/// `use a::{b::Ty, Other}` and `([a], Ty)` for `use a::Ty as Renamed` looked up as `Renamed`.
pub(crate) fn use_tree_path_to<'t>(
    tree: &'t UseTree,
    type_name: &str,
) -> Option<(Vec<&'t Ident>, &'t Ident)> {
    use_tree_leaves(tree)
        .into_iter()
        .find_map(|(path, leaf)| match leaf {
            UseLeaf::Name(name) if name == type_name => Some((path, name)),
            UseLeaf::Rename { name, rename } if rename == type_name => Some((path, name)),
            _ => None,
        })
}

/// What a `use` tree brings in at the end of a path.
pub(crate) enum UseLeaf<'t> {
    /// `use a::Ty`
    Name(&'t Ident),
    /// `use a::Ty as Renamed`
    Rename { name: &'t Ident, rename: &'t Ident },
    /// `use a::*`
    Glob,
}

/// Everything a `use` tree imports, each with the path segments leading to it.
pub(crate) fn use_tree_leaves(tree: &UseTree) -> Vec<(Vec<&Ident>, UseLeaf<'_>)> {
    match tree {
        UseTree::Path(use_path) => use_tree_leaves(&use_path.tree)
            .into_iter()
            .map(|(mut path, leaf)| {
                path.insert(0, &use_path.ident);
                (path, leaf)
            })
            .collect(),
        UseTree::Name(use_name) => vec![(vec![], UseLeaf::Name(&use_name.ident))],
        UseTree::Rename(use_rename) => vec![(
            vec![],
            UseLeaf::Rename {
                name: &use_rename.ident,
                rename: &use_rename.rename,
            },
        )],
        UseTree::Glob(_) => vec![(vec![], UseLeaf::Glob)],
        UseTree::Group(use_group) => use_group.items.iter().flat_map(use_tree_leaves).collect(),
    }
}

/// Paths of the glob imports in a `use` tree: `use a::b::*` gives `[a, b]`.
pub(crate) fn use_tree_globs(tree: &UseTree) -> Vec<Vec<&Ident>> {
    match tree {
        UseTree::Path(use_path) => use_tree_globs(&use_path.tree)
            .into_iter()
            .map(|mut path| {
                path.insert(0, &use_path.ident);
                path
            })
            .collect(),
        UseTree::Glob(_) => vec![vec![]],
        UseTree::Name(_) | UseTree::Rename(_) => vec![],
        UseTree::Group(use_group) => use_group.items.iter().flat_map(use_tree_globs).collect(),
    }
}

/// All names imported by a `use` tree with the path segments leading to them, renames and globs are skipped.
pub(crate) fn use_tree_names(tree: &UseTree) -> Vec<(Vec<&Ident>, &Ident)> {
    match tree {
        UseTree::Path(use_path) => use_tree_names(&use_path.tree)
            .into_iter()
            .map(|(mut path, name)| {
                path.insert(0, &use_path.ident);
                (path, name)
            })
            .collect(),
        UseTree::Name(use_name) => vec![(vec![], &use_name.ident)],
        UseTree::Rename(_) | UseTree::Glob(_) => vec![],
        UseTree::Group(use_group) => use_group.items.iter().flat_map(use_tree_names).collect(),
    }
}

/// Finds the `#[derive_shrink_wrap(..)]` attribute (the new, unified syntax) among `attrs` and
/// parses its directives. Returns `Args::default()` when the attribute is absent, e.g. for a
/// plain type that isn't (de)serialized with ShrinkWrap.
pub(crate) fn derive_shrink_wrap_args(
    attrs: &[Attribute],
    current_crate: &CrateContext,
) -> Result<Args> {
    let Some(attr) = attrs
        .iter()
        .find(|a| a.path().is_ident("derive_shrink_wrap"))
    else {
        return Ok(Args::default());
    };
    match &attr.meta {
        Meta::Path(_) => Ok(Args::default()),
        Meta::List(meta_list) => syn::parse2(meta_list.tokens.clone())
            .map_err(|e| anyhow!("{e}").context(current_crate.err_context())),
        Meta::NameValue(_) => Err(anyhow!(
            "expected #[derive_shrink_wrap] or #[derive_shrink_wrap(..)]"
        )
        .context(current_crate.err_context())),
    }
}

/// Trimmed down version of shrink_wrap_derive's `Args`, keeping only the directives relevant to
/// wire_weaver_core: `size_assumption` (`final_structure`/`self_describing`/`sized`), `ww_repr`, `sparse`
/// (TOOL-11: the API model needs to know a struct is sparse to decode it dynamically) and `fixed_layout` (same
/// bytes as without it, but it makes a struct without a size assumption `sized`, as the derive does).
#[derive(Clone, Debug, Default)]
pub(crate) struct Args {
    pub(crate) size_assumption: Option<ElementSize>,
    pub(crate) ww_repr: Option<Repr>,
    pub(crate) sparse: bool,
    pub(crate) fixed_layout: bool,
}

impl Parse for Args {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut size_assumption = None;
        let mut ww_repr = None;
        let mut sparse = false;
        let mut fixed_layout = false;

        while !input.is_empty() {
            let ident: Ident = input.parse()?;
            match ident.to_string().as_str() {
                "final_structure" => {
                    set_size_assumption(
                        &mut size_assumption,
                        ElementSize::UnsizedFinalStructure,
                        &ident,
                    )?;
                }
                "self_describing" => {
                    set_size_assumption(&mut size_assumption, ElementSize::SelfDescribing, &ident)?;
                }
                "sized" => {
                    set_size_assumption(
                        &mut size_assumption,
                        ElementSize::Sized { size_bits: 0 },
                        &ident,
                    )?;
                }
                "ww_repr" => {
                    input.parse::<Token![=]>()?;
                    let repr_ident: Ident = input.parse()?;
                    ww_repr =
                        Some(parse_repr(repr_ident.to_string().as_str()).ok_or_else(|| {
                            syn::Error::new(
                                repr_ident.span(),
                                "expected one of: unib32, nib, u1..u32, ub<N>",
                            )
                        })?);
                }
                "sparse" => sparse = true,
                "fixed_layout" => fixed_layout = true,
                "head_tail" => {
                    return Err(syn::Error::new(
                        ident.span(),
                        "a head_tail type has a wire format of its own and no ShrinkWrap serdes, it can't be \
                         used in an API",
                    ));
                }
                "borrowed" | "owned" | "derive" | "derive_owned" | "derive_borrowed"
                | "cfg_attr" | "cfg_attr_owned" | "cfg_attr_borrowed" | "discriminants"
                | "crate_path" | "non_exhaustive" | "no_future_version" | "display" | "series" => {
                    // Directives irrelevant to wire_weaver_core's introspection (it only cares
                    // about `ww_repr`, the size assumption and `sparse`) - skip their arguments, if any.
                    if input.peek(syn::token::Paren) {
                        let _group: proc_macro2::Group = input.parse()?;
                    }
                }
                "validate" => {
                    input.parse::<Token![=]>()?;
                    let _path: syn::Path = input.parse()?;
                }
                "future_version" => {
                    // the variant is left out of the API model (SW-16, SW-47)
                    input.parse::<Token![=]>()?;
                    let _name: Ident = input.parse()?;
                }
                u => {
                    return Err(syn::Error::new(
                        ident.span(),
                        format!(
                            "unsupported directive '{u}', expected one of: {SUPPORTED_DIRECTIVES}"
                        ),
                    ));
                }
            }

            if input.is_empty() {
                break;
            }
            input.parse::<Token![,]>()?;
        }

        Ok(Args {
            size_assumption,
            ww_repr,
            sparse,
            fixed_layout,
        })
    }
}

const SUPPORTED_DIRECTIVES: &str = "final_structure, self_describing, sized, sparse, fixed_layout, validate = <path>, ww_repr=<u1,u2,..,nib,unib32,u8,..>";

/// Sets `size_assumption`, erroring if it was already set by an earlier directive since
/// `final_structure`, `self_describing` and `sized` are mutually exclusive.
fn set_size_assumption(
    size_assumption: &mut Option<ElementSize>,
    value: ElementSize,
    ident: &Ident,
) -> syn::Result<()> {
    if size_assumption.is_some() {
        return Err(syn::Error::new(
            ident.span(),
            "'final_structure', 'self_describing' and 'sized' are mutually exclusive, only one can be specified",
        ));
    }
    *size_assumption = Some(value);
    Ok(())
}

fn parse_repr(s: &str) -> Option<Repr> {
    if s == "unib32" || s == "UNib32" {
        return Some(Repr::UNib32);
    }
    if s == "nib" || s == "Nibble" {
        return Some(Repr::Nibble);
    }
    if let Some(s) = s.strip_prefix("ub") {
        let bits: u8 = s.parse().ok()?;
        return Some(Repr::BitAligned(bits));
    }
    let s = s.strip_prefix("u")?;
    let bits: u8 = s.parse().ok()?;
    match bits {
        8 => Some(Repr::ByteAlignedU8),
        16 => Some(Repr::ByteAlignedU16),
        32 => Some(Repr::ByteAlignedU32),
        other => Some(Repr::BitAligned(other)),
    }
}

#[cfg(test)]
mod derive_shrink_wrap_args_tests {
    use super::Args;
    use quote::quote;
    use syn::parse2;

    #[test]
    fn ignores_directives_it_does_not_care_about() {
        let ts = quote! {
            borrowed, owned(feature = "std"), ww_repr = nib,
            derive(Clone, Debug), derive_owned(Clone), derive_borrowed(Copy),
            cfg_attr(feature = "a"), cfg_attr_owned(feature = "b", derive(X)),
            cfg_attr_borrowed(feature = "c"), discriminants, display, sized
        };
        let args: Args = parse2(ts).unwrap();
        assert!(args.ww_repr.is_some());
        assert!(args.size_assumption.is_some());
    }

    #[test]
    fn fixed_layout_and_head_tail() {
        let args: Args = parse2(quote! { borrowed, fixed_layout, ww_repr = u8 }).unwrap();
        assert!(args.fixed_layout);
        assert!(args.size_assumption.is_none());
        let err = parse2::<Args>(quote! { head_tail })
            .unwrap_err()
            .to_string();
        assert!(err.contains("can't be used in an API"), "{err}");
    }
}

#[cfg(test)]
mod stream_fn_attr_tests {
    use super::StreamFnAttr;
    use ww_self::{Batching, Delivery};

    fn parse(s: &str) -> syn::Result<(Delivery, Option<Batching>)> {
        syn::parse_str::<StreamFnAttr>(s).map(|a| (a.delivery, a.batched))
    }

    #[test]
    fn delivery_and_batching() {
        assert_eq!(parse("u8").unwrap(), (Delivery::Plain, None));
        assert_eq!(parse("u8, counted,").unwrap(), (Delivery::Counted, None));
        assert_eq!(
            parse("Sample, reliable, batched").unwrap(),
            (Delivery::Reliable, Some(Batching::Timed))
        );
        assert_eq!(
            parse("Sample, batched").unwrap(),
            (Delivery::Plain, Some(Batching::Timed))
        );
        assert_eq!(
            parse("Frame, counted, batched(untimed)").unwrap(),
            (Delivery::Counted, Some(Batching::Untimed))
        );
        assert_eq!(
            parse("Pair<u8, u16>, counted, batched").unwrap(),
            (Delivery::Counted, Some(Batching::Timed))
        );
    }

    #[test]
    fn wrong_forms() {
        for s in [
            "u8, batched, counted",
            "u8, counted, reliable",
            "u8, batched, batched",
            "u8, batched(timed)",
            "u8, batched(untimed, x)",
            "u8, lossy",
        ] {
            assert!(parse(s).is_err(), "{s}");
        }
    }
}
