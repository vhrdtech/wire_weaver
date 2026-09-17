use crate::ast::docs::Docs;
use crate::ast::util::Version;
use syn::{Expr, Lit, Meta};

/// Take `#[id = integer]` attribute and return the number
#[allow(clippy::ptr_arg)]
pub(crate) fn take_id_attr(_attrs: &mut Vec<syn::Attribute>) -> Option<u32> {
    // TODO: implement id's
    None
}

/// Take `#[since = "X.Y"]` attribute and return the Version
pub(crate) fn take_since_attr(attrs: &mut Vec<syn::Attribute>) -> Result<Option<Version>, String> {
    let attr_idx = attrs
        .iter()
        .enumerate()
        .find(|(_, a)| a.path().is_ident("since"))
        .map(|(idx, _)| idx);
    let Some(attr_idx) = attr_idx else {
        return Ok(None);
    };
    let attr = attrs.remove(attr_idx);
    let Meta::NameValue(name_value) = attr.meta else {
        return Err("Expected default = lit".into());
    };
    if let Expr::Lit(expr_lit) = name_value.value
        && let Lit::Str(lit_str) = expr_lit.lit
    {
        let value = lit_str.value();
        let mut components = value.split('.');
        let major = components.next().unwrap().parse::<u32>().unwrap();
        let minor = components.next().unwrap().parse::<u32>().unwrap();
        let patch = components.next().unwrap().parse::<u32>().unwrap();
        Ok(Some(Version {
            major,
            minor,
            patch,
        }))
    } else {
        Err("Expected #[since = \"x.y.z\"]".into())
    }
}

/// Take `#[default = lit]` attribute and return Value containing provided literal
pub(crate) fn take_default_attr(attrs: &mut Vec<syn::Attribute>) -> Result<Option<Expr>, String> {
    let attr_idx = attrs
        .iter()
        .enumerate()
        .find(|(_, a)| a.path().is_ident("default"))
        .map(|(idx, _)| idx);
    let Some(attr_idx) = attr_idx else {
        return Ok(None);
    };
    let attr = attrs.remove(attr_idx);
    let Meta::NameValue(name_value) = attr.meta else {
        return Err("Expected default = lit".into());
    };
    Ok(Some(name_value.value))
}

pub(crate) fn take_flag_attr(attrs: &mut Vec<syn::Attribute>) -> Option<()> {
    let (attr_idx, _) = attrs
        .iter()
        .enumerate()
        .find(|(_, a)| a.path().is_ident("flag"))?;
    let _attr = attrs.remove(attr_idx);
    Some(())
}

pub(crate) fn collect_docs_attrs(attrs: &mut Vec<syn::Attribute>) -> Docs {
    let mut docs = Docs::empty();
    for attr in attrs.iter() {
        if !attr.path().is_ident("doc") {
            continue;
        }
        let Meta::NameValue(name_value) = attr.meta.clone() else {
            continue;
        };
        let Expr::Lit(expr_lit) = name_value.value else {
            continue;
        };
        if let Lit::Str(lit_str) = expr_lit.lit {
            docs.push(lit_str);
        }
    }
    attrs.retain(|a| !a.path().is_ident("doc"));
    docs
}

pub(crate) fn collect_unknown_attributes(attrs: &mut Vec<syn::Attribute>) {
    for a in attrs {
        // ignore #[shrink_warp(...)] in after #[derive(ShrinkWrap)]
        // if a.path().is_ident("shrink_wrap") {
        //     continue;
        // }

        // // when used from wire_weaver introspect
        // if a.path().is_ident("derive_shrink_wrap") {
        //     continue;
        // }
        // if a.path().is_ident("owned") {
        //     continue;
        // }
        println!("Unknown attribute: {:?}", a.meta.path());
    }
}
