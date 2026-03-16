use super::crate_walker::CrateContext;
use anyhow::{anyhow, Context, Result};
use semver::Version;
use syn::{Attribute, Expr, Lit, Meta, UseTree};
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
