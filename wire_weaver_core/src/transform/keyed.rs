//! `keyed!(flow by FlowKey)` (API-67): the key companion of a resource array, converted into an
//! [ApiItemKindOwned::Keys] item. Its operations are not stored, `ww_self::keys::keys_level` builds them.

use super::{
    api::{PropertyMacroArgs, StreamAndImplMacroArgs},
    crate_walker::{CrateContext, Scratch},
    errors::reject_errors_attr,
    ty::convert_ty,
    util::{collect_docs, get_optional_attr, get_since_attr},
};
use anyhow::{Context, Result, anyhow};
use proc_macro2::Ident;
use shrink_wrap::UNib32;
use std::collections::HashSet;
use syn::parse::{Parse, ParseStream};
use syn::{PathSegment, TraitItem, TraitItemMacro, Type, parse2};
use ww_numeric::NumericAnyTypeOwned;
use ww_self::visit::{self, Visit};
use ww_self::{
    ApiBundleOwned, ApiItemKindOwned, ApiItemOwned, Multiplicity, NumericBaseType,
    TypeLocationOwned, TypeOwned,
};

/// Arguments of `keyed!(flow by FlowKey)`.
pub struct KeyedMacroArgs {
    /// The array it keys: `flow`.
    pub of: Ident,
    /// The key type: `FlowKey`.
    pub key: Type,
}

impl Parse for KeyedMacroArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let of = input.parse()?;
        let by: Ident = input.parse()?;
        if by != "by" {
            return Err(syn::Error::new(
                by.span(),
                "expected `by`: keyed!(flow by FlowKey)",
            ));
        }
        let key = input.parse()?;
        Ok(KeyedMacroArgs { of, key })
    }
}

/// Name of the key companion of the array `of`: `flow_keys`.
pub fn keys_ident(of: &str) -> String {
    format!("{of}_keys")
}

/// The `keyed!` item at `idx` of `item_trait`; `items` are the items before it, the array it keys among them.
pub(crate) fn convert_api_item_keyed(
    current_crate: &CrateContext,
    scratch: &mut Scratch,
    item_trait: &syn::ItemTrait,
    item_macro: &TraitItemMacro,
    items: &[ApiItemOwned],
    idx: u32,
) -> Result<ApiItemOwned> {
    let trait_name = item_trait.ident.to_string();
    let args: KeyedMacroArgs = parse2(item_macro.mac.tokens.clone())
        .context(format!(
            "parsing keyed! arguments: '{}', expected keyed!(array by KeyType)",
            item_macro.mac.tokens
        ))
        .context(current_crate.err_context())?;
    let of_name = args.of.to_string();
    let err = |msg: String| {
        Err(anyhow!("keyed!({of_name} by ..) in {trait_name}: {msg}")
            .context(current_crate.err_context()))
    };
    let Some(array) = items.iter().find(|item| item.ident == of_name) else {
        return err(format!(
            "no item `{of_name}` before it; keyed! comes after the array it keys"
        ));
    };
    match (&array.kind, &array.multiplicity) {
        (ApiItemKindOwned::Keys { .. }, _) => {
            return err("names another keyed! companion, only an array can be keyed".to_owned());
        }
        (ApiItemKindOwned::Method { .. }, _) => {
            return err("names a method, only an array of traits, properties, streams or sinks can be keyed".to_owned());
        }
        (_, Multiplicity::Flat) => {
            return err(format!(
                "`{of_name}` is not an array, declare it as `{of_name}[]` to key its elements"
            ));
        }
        _ => {}
    }
    if items
        .iter()
        .any(|item| matches!(item.kind, ApiItemKindOwned::Keys { of, .. } if of == array.id))
    {
        return err(format!(
            "`{of_name}` is keyed already, one keyed! per array"
        ));
    }
    reject_errors_attr(
        &item_macro.attrs,
        &of_name,
        "keyed! has no error list of its own",
    )
    .context(current_crate.err_context())?;

    let key = convert_ty(&args.key, current_crate, scratch)?;
    if let Some(float) = find_float(&key, &scratch.root_bundle.to_bundle()) {
        return err(format!(
            "the key type has a {float:?} in it; a key must compare equal to itself (no NaN), use an integer or a \
             fixed-point type instead"
        ));
    }
    let index: Type = match array_index_ty(item_trait, &of_name)? {
        Some(segment) => syn::parse_quote!(#segment),
        None => syn::parse_quote!(UNib32),
    };
    let ww_keyed = Ident::new("ww_keyed", proc_macro2::Span::call_site());
    current_crate
        .resolve_path(std::iter::once(&ww_keyed), scratch)
        .context(
            "keyed! needs ww_keyed as a dependency of the API crate (KeyEvent, the item of its `keys` stream, comes \
             from there)",
        )?;
    let key_ty = &args.key;
    let event: Type = syn::parse_quote!(ww_keyed::KeyEvent<#index, #key_ty>);
    let event_ty = convert_ty(&event, current_crate, scratch)?;

    Ok(ApiItemOwned {
        id: UNib32(idx),
        kind: ApiItemKindOwned::Keys {
            of: array.id,
            key,
            event_ty,
        },
        multiplicity: Multiplicity::Flat,
        since: get_since_attr(&item_macro.attrs, current_crate, scratch)?,
        ident: keys_ident(&of_name),
        docs: collect_docs(&item_macro.attrs),
        optional: get_optional_attr(&item_macro.attrs, current_crate)?,
        meta: None,
        identity: None,
    })
}

/// A key companion named like another item of its trait (`flow_keys` next to `keyed!(flow by ..)`) is refused, and
/// so is an item named like a handler of the companion (`flow_index_of`, `flow_key`, SRV-62).
pub(crate) fn reject_keys_ident_collisions(items: &[ApiItemOwned]) -> Result<()> {
    for keys in items {
        let ApiItemKindOwned::Keys { of, .. } = &keys.kind else {
            continue;
        };
        if let Some(other) = items
            .iter()
            .find(|item| item.ident == keys.ident && item.id != keys.id)
        {
            return Err(anyhow!(
                "keyed! companion `{}` has the same name as item {} of the same trait: rename that item",
                keys.ident,
                other.id.0
            ));
        }
        let Some(array) = items.iter().find(|item| item.id == *of) else {
            continue;
        };
        let handlers = [
            format!("{}_index_of", array.ident),
            format!("{}_key", array.ident),
        ];
        if let Some(other) = items.iter().find(|item| handlers.contains(&item.ident)) {
            return Err(anyhow!(
                "item {} `{}` is named like a handler of keyed! companion `{}`: rename that item",
                other.id.0,
                other.ident,
                keys.ident
            ));
        }
    }
    Ok(())
}

/// The index type written in the array's brackets (`flow[FlowIdx]`), `None` for `flow[]`.
///
/// # Errors
///
/// No stream, sink, property or trait array named `of` in `item_trait`, or its macro arguments don't parse.
pub fn array_index_ty(item_trait: &syn::ItemTrait, of: &str) -> Result<Option<PathSegment>> {
    for item in &item_trait.items {
        let TraitItem::Macro(item_macro) = item else {
            continue;
        };
        let tokens = item_macro.mac.tokens.clone();
        let multiplicity = match item_macro
            .mac
            .path
            .get_ident()
            .map(Ident::to_string)
            .as_deref()
        {
            Some("stream" | "sink" | "ww_impl") => {
                let args: StreamAndImplMacroArgs = parse2(tokens)?;
                (args.resource_name == of).then_some(args.multiplicity)
            }
            Some("property") => {
                let args: PropertyMacroArgs = parse2(tokens)?;
                (args.resource_name == of).then_some(args.multiplicity)
            }
            _ => None,
        };
        if let Some(multiplicity) = multiplicity {
            return Ok(multiplicity.flatten());
        }
    }
    Err(anyhow!(
        "array `{of}` not found among the macro items of its trait"
    ))
}

/// A float anywhere in `ty`, through the definitions of `bundle`.
fn find_float(ty: &TypeOwned, bundle: &ApiBundleOwned) -> Option<NumericBaseType> {
    let mut finder = FloatFinder::default();
    finder.visit_type(ty);
    let mut seen = HashSet::new();
    while finder.float.is_none() {
        let Some(type_idx) = finder.pending.pop() else {
            break;
        };
        if !seen.insert(type_idx) {
            continue;
        }
        if let Some(TypeLocationOwned::InLine { ty, .. }) = bundle.types.get(type_idx as usize) {
            finder.visit_type(ty);
        }
    }
    finder.float
}

#[derive(Default)]
struct FloatFinder {
    float: Option<NumericBaseType>,
    pending: Vec<u32>,
}

impl<'ast> Visit<'ast> for FloatFinder {
    fn visit_type(&mut self, node: &'ast TypeOwned) {
        match node {
            TypeOwned::NumericAny(
                NumericAnyTypeOwned::Base(base)
                | NumericAnyTypeOwned::SubType { base, .. }
                | NumericAnyTypeOwned::ShiftScale { base, .. },
            ) if matches!(
                base,
                NumericBaseType::F16 | NumericBaseType::F32 | NumericBaseType::F64
            ) =>
            {
                self.float.get_or_insert(base.clone());
            }
            TypeOwned::OutOfLine { type_idx } | TypeOwned::Instance { type_idx, .. } => {
                self.pending.push(type_idx.0);
            }
            _ => {}
        }
        visit::visit_type(self, node);
    }
}

#[cfg(test)]
mod tests {
    use super::super::api::tests::load_src_with;
    use super::*;
    use std::path::Path;
    use ww_self::keys::{self, keys_level};
    use ww_self::{ApiLevelOwned, PropertyAccess};

    /// `[package]` lines of a throwaway API crate that depends on this checkout's `wire_weaver`, and on its
    /// `ww_keyed` with `keyed`.
    fn deps(keyed: bool) -> String {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let dep = |name: &str, path: &str| {
            format!(
                "{name} = {{ path = {:?} }}\n",
                root.join(path).display().to_string()
            )
        };
        let mut deps = format!(
            "publish = false\n[dependencies]\n{}",
            dep("ww_api", "ww_api")
        );
        if keyed {
            let mut sources = ww_snapshots::cargo::CargoSources::default();
            sources.entry_dir = Some(Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf());
            let ww_keyed = sources.locked_package_dir("ww_keyed").unwrap();
            deps += &format!(
                "ww_keyed = {{ path = {:?} }}\n",
                ww_keyed.display().to_string()
            );
        }
        deps
    }

    fn load_api(name: &str, body: &str) -> Result<ApiBundleOwned> {
        let lib_rs = format!(
            "use ww_api::prelude::*;\n#[ww_api_root(wire = \"w1.0\")]\npub trait Api {{\n{body}\n}}\n\
             #[ww_trait]\npub trait Flow {{ fn close(); }}\n"
        );
        load_src_with(name, &deps(true), &lib_rs)
    }

    fn refused(name: &str, body: &str) -> String {
        format!("{:#}", load_api(name, body).unwrap_err())
    }

    fn item<'a>(level: &'a ApiLevelOwned, ident: &str) -> &'a ApiItemOwned {
        level
            .items
            .iter()
            .find(|i| i.ident == ident)
            .unwrap_or_else(|| panic!("no item {ident}"))
    }

    /// `tests/keyed_api`: the companion is one item at its position, its level has `index_of`, `key[]` and `keys`
    /// with the array's index type.
    #[test]
    fn dissector_bundle_holds_the_companion() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/keyed_api");
        let bundle = crate::load(&dir, Some("Dissector".into()), false).unwrap();
        let root = &bundle.root;
        let keys = item(root, "flow_keys");
        assert_eq!(keys.id, UNib32(1));
        assert_eq!(item(root, "clear").id, UNib32(2));
        let ApiItemKindOwned::Keys { of, event_ty, .. } = &keys.kind else {
            panic!("not a Keys item: {keys:?}");
        };
        assert_eq!(*of, item(root, "flow").id);
        assert!(
            matches!(
                event_ty,
                TypeOwned::Instance { .. } | TypeOwned::OutOfLine { .. }
            ),
            "{event_ty:?}"
        );

        let level = keys_level(root, keys).unwrap();
        let ids: Vec<_> = level
            .items
            .iter()
            .map(|i| (i.id.0, i.ident.as_str()))
            .collect();
        assert_eq!(
            ids,
            [
                (keys::INDEX_OF, "index_of"),
                (keys::KEY, "key"),
                (keys::KEYS, "keys")
            ]
        );
        let ApiItemKindOwned::Method {
            args, return_ty, ..
        } = &level.items[0].kind
        else {
            panic!("index_of is not a method");
        };
        assert_eq!(args.len(), 1);
        assert_eq!(
            return_ty,
            &Some(TypeOwned::Option {
                some_ty: Box::new(TypeOwned::NumericAny(NumericAnyTypeOwned::Base(
                    NumericBaseType::UNib32
                )))
            })
        );
        assert!(matches!(
            level.items[1].kind,
            ApiItemKindOwned::Property {
                access: PropertyAccess::ReadOnly { observe: false },
                ..
            }
        ));
        assert_eq!(
            level.items[1].multiplicity,
            Multiplicity::Array {
                index_type_idx: None
            }
        );
        assert!(matches!(
            &level.items[2].kind,
            ApiItemKindOwned::Stream { ty, is_up: true } if ty == event_ty
        ));
    }

    /// A typed index (`flow[FlowIdx]`) is the index of `index_of`, `key[]` and the events.
    #[test]
    fn typed_index() {
        let bundle = load_api(
            "keyed_typed_index",
            "ww_impl!(flow[FlowIdx]: Flow);\nkeyed!(flow by u32);\n}\n\
             #[derive_shrink_wrap]\npub struct FlowIdx(u16);\npub trait Unused {",
        )
        .unwrap();
        let keys = item(&bundle.root, "flow_keys");
        let level = keys_level(&bundle.root, keys).unwrap();
        let Multiplicity::Array {
            index_type_idx: Some(type_idx),
        } = item(&bundle.root, "flow").multiplicity
        else {
            panic!("flow is not a typed array");
        };
        assert_eq!(
            level.items[1].multiplicity,
            Multiplicity::Array {
                index_type_idx: Some(type_idx)
            }
        );
        let ApiItemKindOwned::Method { return_ty, .. } = &level.items[0].kind else {
            panic!("index_of is not a method");
        };
        assert_eq!(
            return_ty,
            &Some(TypeOwned::Option {
                some_ty: Box::new(TypeOwned::OutOfLine { type_idx })
            })
        );
    }

    /// The API crate depends on `ww_keyed` itself, so `wire_weaver` (and every firmware) doesn't.
    #[test]
    fn needs_ww_keyed() {
        let lib_rs = "use ww_api::prelude::*;\n#[ww_api_root(wire = \"w1.0\")]\npub trait Api {\n\
                      stream!(flow[]: u8);\nkeyed!(flow by u32);\n}\n";
        let err = format!(
            "{:#}",
            load_src_with("keyed_no_dep", &deps(false), lib_rs).unwrap_err()
        );
        assert!(err.contains("needs ww_keyed as a dependency"), "{err}");
    }

    #[test]
    fn refused_forms() {
        let cases = [
            (
                "keyed_missing",
                "keyed!(flow by u32);",
                "no item `flow` before it",
            ),
            (
                "keyed_before",
                "keyed!(flow by u32);\nww_impl!(flow[]: Flow);",
                "no item `flow` before it",
            ),
            (
                "keyed_method",
                "fn flow();\nkeyed!(flow by u32);",
                "names a method",
            ),
            (
                "keyed_flat",
                "ww_impl!(flow: Flow);\nkeyed!(flow by u32);",
                "`flow` is not an array",
            ),
            (
                "keyed_twice",
                "ww_impl!(flow[]: Flow);\nkeyed!(flow by u32);\nkeyed!(flow by u16);",
                "keyed already",
            ),
            (
                "keyed_keys",
                "ww_impl!(flow[]: Flow);\nkeyed!(flow by u32);\nkeyed!(flow_keys by u16);",
                "another keyed! companion",
            ),
            (
                "keyed_float",
                "stream!(flow[]: u8);\nkeyed!(flow by f32);",
                "F32",
            ),
            (
                "keyed_nested_float",
                "property!(ro flow[]: u8);\nkeyed!(flow by Key);\n}\n\
                 #[derive_shrink_wrap]\npub struct Key { pub id: u8, pub gain: Option<f64> }\npub trait Unused {",
                "F64",
            ),
            (
                "keyed_collision",
                "ww_impl!(flow[]: Flow);\nkeyed!(flow by u32);\nfn flow_keys();",
                "same name as item 2",
            ),
            (
                "keyed_handler_collision",
                "ww_impl!(flow[]: Flow);\nkeyed!(flow by u32);\nfn flow_index_of();",
                "named like a handler of keyed! companion `flow_keys`",
            ),
            (
                "keyed_syntax",
                "ww_impl!(flow[]: Flow);\nkeyed!(flow: u32);",
                "keyed!(array by KeyType)",
            ),
        ];
        for (name, body, expected) in cases {
            let err = refused(name, body);
            assert!(
                err.contains(expected),
                "{name}: expected `{expected}` in: {err}"
            );
        }
    }
}
