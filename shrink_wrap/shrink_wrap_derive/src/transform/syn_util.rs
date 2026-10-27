use crate::ast::docs::Docs;
use crate::ast::util::Version;
use syn::{Expr, Lit, LitStr, Meta};

/// Take `#[id = integer]` attribute and return the number
#[allow(clippy::ptr_arg)]
/// Takes `#[id = N]`, which is accepted and not used yet: fields are written in order.
pub(crate) fn take_id_attr(attrs: &mut Vec<syn::Attribute>) -> Option<u32> {
    // TODO: implement id's
    attrs.retain(|a| !a.path().is_ident("id"));
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

/// Take `#[series(opaque)]` (SW-60): whether the field is an opaque run in a `Series`.
pub(crate) fn take_series_attr(attrs: &mut Vec<syn::Attribute>) -> Result<bool, String> {
    let Some(attr_idx) = attrs.iter().position(|a| a.path().is_ident("series")) else {
        return Ok(false);
    };
    let attr = attrs.remove(attr_idx);
    if attrs.iter().any(|a| a.path().is_ident("series")) {
        return Err("more than one #[series(..)]".into());
    }
    let ok = match &attr.meta {
        Meta::List(list) => list.tokens.to_string() == "opaque",
        _ => false,
    };
    if !ok {
        return Err("expected #[series(opaque)]".into());
    }
    Ok(true)
}

/// Take `#[wire(W)]` or `#[cfg_attr(pred, wire(W))]` (DER-19): the wire type `W` and the `cfg_attr` predicate.
/// rustc doesn't expand a field's `cfg_attr` before an attribute macro runs, so the derive sees it as written.
pub(crate) fn take_wire_attr(
    attrs: &mut Vec<syn::Attribute>,
) -> Result<Option<(syn::Type, Option<Meta>)>, String> {
    let mut found = None;
    let mut err = None;
    attrs.retain(|attr| {
        let wire = if attr.path().is_ident("wire") {
            match &attr.meta {
                Meta::List(list) => syn::parse2::<syn::Type>(list.tokens.clone())
                    .map(|ty| (ty, None))
                    .map_err(|_| "expected #[wire(W)] with an integer type W".to_string()),
                _ => Err("expected #[wire(W)] with an integer type W".to_string()),
            }
        } else if attr.path().is_ident("cfg_attr") {
            match cfg_attr_wire(attr) {
                Some(wire) => wire,
                None => return true,
            }
        } else {
            return true;
        };
        match wire {
            Ok(wire) if found.is_none() => found = Some(wire),
            Ok(_) => err = Some("more than one #[wire(..)]".to_string()),
            Err(e) => err = Some(e),
        }
        false
    });
    match err {
        Some(err) => Err(err),
        None => Ok(found),
    }
}

/// `#[cfg_attr(pred, wire(W))]`: `None` when the `cfg_attr` has no `wire(..)` in it.
fn cfg_attr_wire(attr: &syn::Attribute) -> Option<Result<(syn::Type, Option<Meta>), String>> {
    let Meta::List(list) = &attr.meta else {
        return None;
    };
    let metas = list
        .parse_args_with(syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated)
        .ok()?;
    let mut metas = metas.into_iter();
    let pred = metas.next()?;
    let rest: Vec<Meta> = metas.collect();
    if !rest.iter().any(|m| m.path().is_ident("wire")) {
        return None;
    }
    let [Meta::List(wire)] = rest.as_slice() else {
        return Some(Err(
            "expected #[cfg_attr(pred, wire(W))], with wire(W) alone: put other attributes in a cfg_attr of their own"
                .into(),
        ));
    };
    Some(
        syn::parse2::<syn::Type>(wire.tokens.clone())
            .map(|ty| (ty, Some(pred)))
            .map_err(|_| "expected #[cfg_attr(pred, wire(W))] with an integer type W".to_string()),
    )
}

/// Take `#[slot = N]` (DER-13, a field of a `sparse` struct) and return `N`.
pub(crate) fn take_slot_attr(attrs: &mut Vec<syn::Attribute>) -> Result<Option<u32>, String> {
    let Some(attr_idx) = attrs.iter().position(|a| a.path().is_ident("slot")) else {
        return Ok(None);
    };
    let attr = attrs.remove(attr_idx);
    if attrs.iter().any(|a| a.path().is_ident("slot")) {
        return Err("more than one #[slot = N]".into());
    }
    if let Meta::NameValue(name_value) = attr.meta
        && let Expr::Lit(expr_lit) = name_value.value
        && let Lit::Int(lit_int) = expr_lit.lit
    {
        let slot = lit_int
            .base10_parse::<u32>()
            .map_err(|e| format!("#[slot = N]: {e}"))?;
        Ok(Some(slot))
    } else {
        Err("expected #[slot = N] with an integer literal N".into())
    }
}

/// Take `#[display("..")]` attribute (DER-12) and return its format string literal, overriding the first doc
/// line as the `display` message.
pub(crate) fn take_display_attr(attrs: &mut Vec<syn::Attribute>) -> Result<Option<String>, String> {
    let attr_idx = attrs
        .iter()
        .enumerate()
        .find(|(_, a)| a.path().is_ident("display"))
        .map(|(idx, _)| idx);
    let Some(attr_idx) = attr_idx else {
        return Ok(None);
    };
    let attr = attrs.remove(attr_idx);
    let Meta::List(list) = &attr.meta else {
        return Err("expected #[display(\"..\")]".into());
    };
    let lit: LitStr =
        syn::parse2(list.tokens.clone()).map_err(|_| "expected #[display(\"..\")]".to_string())?;
    Ok(Some(lit.value()))
}

/// Take every `#[cfg(..)]` attribute and return the predicates.
pub(crate) fn take_cfg_attrs(
    attrs: &mut Vec<syn::Attribute>,
) -> Result<Vec<proc_macro2::TokenStream>, String> {
    let mut cfg = vec![];
    for attr in attrs.iter().filter(|a| a.path().is_ident("cfg")) {
        let Meta::List(list) = &attr.meta else {
            return Err("Expected #[cfg(predicate)]".into());
        };
        cfg.push(list.tokens.clone());
    }
    attrs.retain(|a| !a.path().is_ident("cfg"));
    Ok(cfg)
}

pub(crate) fn take_flag_attr(attrs: &mut Vec<syn::Attribute>) -> Option<()> {
    let (attr_idx, _) = attrs
        .iter()
        .enumerate()
        .find(|(_, a)| a.path().is_ident("flag"))?;
    let _attr = attrs.remove(attr_idx);
    Some(())
}

/// Take a `#[name]` attribute without arguments, true when it was there.
pub(crate) fn take_flag_like_attr(attrs: &mut Vec<syn::Attribute>, name: &str) -> bool {
    let len = attrs.len();
    attrs.retain(|a| !a.path().is_ident(name));
    attrs.len() != len
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
