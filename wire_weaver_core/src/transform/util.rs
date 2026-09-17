use super::crate_walker::CrateContext;
use anyhow::{Context, Result, anyhow};
use proc_macro2::Ident;
use semver::Version;
use shrink_wrap::ElementSize;
use syn::parse::{Parse, ParseStream};
use syn::{Attribute, Expr, Lit, Meta, Token, UseTree};
use ww_self::Repr;
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

pub(crate) fn use_tree_has_type(tree: &UseTree, type_name: &str) -> bool {
    match tree {
        UseTree::Path(use_path) => use_tree_has_type(&use_path.tree, type_name),
        UseTree::Name(use_name) => use_name.ident == type_name,
        UseTree::Rename(_) => false,
        UseTree::Glob(_) => false,
        UseTree::Group(use_group) => {
            for item in &use_group.items {
                if use_tree_has_type(item, type_name) {
                    return true;
                }
            }
            false
        }
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
/// wire_weaver_core: `size_assumption` (`final_structure`/`self_describing`/`sized`) and `ww_repr`.
#[derive(Clone, Debug, Default)]
pub(crate) struct Args {
    pub(crate) size_assumption: Option<ElementSize>,
    pub(crate) ww_repr: Option<Repr>,
}

impl Parse for Args {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut size_assumption = None;
        let mut ww_repr = None;

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
                "borrowed" | "owned" | "derive" | "derive_owned" | "derive_borrowed"
                | "cfg_attr" | "cfg_attr_owned" | "cfg_attr_borrowed" | "discriminants" => {
                    // Directives irrelevant to wire_weaver_core's introspection (it only cares
                    // about `ww_repr` and the size assumption) - skip their arguments, if any.
                    if input.peek(syn::token::Paren) {
                        let _group: proc_macro2::Group = input.parse()?;
                    }
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
        })
    }
}

const SUPPORTED_DIRECTIVES: &str =
    "final_structure, self_describing, sized, ww_repr=<u1,u2,..,nib,unib32,u8,..>";

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
            cfg_attr_borrowed(feature = "c"), discriminants, sized
        };
        let args: Args = parse2(ts).unwrap();
        assert!(args.ww_repr.is_some());
        assert!(args.size_assumption.is_some());
    }
}
