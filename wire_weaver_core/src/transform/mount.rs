//! `mount!(hub)` and `mounts!(ext)` (SRV-76): mount slots, converted into [ApiItemKindOwned::Mount] items. A slot's
//! operations are not stored, `ww_self::mount::slot_level` builds them. `mounts!(ext)` is an array of slots keyed by
//! name: it takes two ids, the array `ext` and its key companion `ext_keys` (API-67), keyed by
//! `ww_api::mount::MountName`.

use super::{
    crate_walker::{CrateContext, Scratch},
    errors::reject_errors_attr,
    ty::convert_ty,
    util::{collect_docs, get_since_attr},
};
use anyhow::{Context, Result, anyhow};
use proc_macro2::{Ident, Span};
use shrink_wrap::UNib32;
use syn::parse::{Parse, ParseStream};
use syn::{Token, TraitItemMacro, Type, parse2};
use ww_self::{ApiItemKindOwned, ApiItemOwned, Multiplicity};

/// Arguments of `mount!(hub)`, `mount!(hub: pult_hub_api)`, and the same of `mounts!`.
pub struct MountMacroArgs {
    /// The slot's name: `hub`.
    pub name: Ident,
    /// Crate of the API a backend is expected to serve: `pult_hub_api`.
    pub expects: Option<Ident>,
}

impl Parse for MountMacroArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name = input.parse()?;
        let expects = if input.peek(Token![:]) {
            let _colon: Token![:] = input.parse()?;
            Some(input.parse()?)
        } else {
            None
        };
        if !input.is_empty() {
            return Err(input.error("expected mount!(name) or mount!(name: api_crate)"));
        }
        Ok(MountMacroArgs { name, expects })
    }
}

/// Names of the handlers a server has for the slot `name`: `get_<name>_presence`, `<name>_request`. An item named
/// like one of them, or like the companion `<name>_keys` of `mounts!`, is refused.
pub fn handler_idents(name: &str) -> [String; 2] {
    [format!("{name}_presence"), format!("{name}_request")]
}

/// The `mount!` (`keyed` false) or `mounts!` (`keyed` true) item at `idx`: one item, or the array and its key
/// companion at `idx` and `idx + 1`.
pub(crate) fn convert_api_item_mount(
    current_crate: &CrateContext,
    scratch: &mut Scratch,
    item_macro: &TraitItemMacro,
    idx: u32,
    keyed: bool,
) -> Result<Vec<ApiItemOwned>> {
    let macro_name = if keyed { "mounts" } else { "mount" };
    let args: MountMacroArgs = parse2(item_macro.mac.tokens.clone())
        .context(format!(
            "parsing {macro_name}! arguments: '{}', expected {macro_name}!(name) or {macro_name}!(name: api_crate)",
            item_macro.mac.tokens
        ))
        .context(current_crate.err_context())?;
    let name = args.name.to_string();
    reject_errors_attr(
        &item_macro.attrs,
        &name,
        "a mount slot has no error list, the mounted API's items have their own",
    )
    .context(current_crate.err_context())?;
    if item_macro
        .attrs
        .iter()
        .any(|a| a.path().is_ident("optional"))
    {
        return Err(anyhow!(
            "#[optional] on {macro_name}!({name}): a slot is always there, it may only be empty"
        )
        .context(current_crate.err_context()));
    }
    let ww_api = Ident::new("ww_api", Span::call_site());
    current_crate
        .resolve_path(std::iter::once(&ww_api), scratch)
        .context(format!(
            "{macro_name}! needs ww_api as a dependency of the API crate (MountPresence comes from there)"
        ))?;
    let presence: Type = syn::parse_quote!(ww_api::mount::MountPresence<'static>);
    let presence_ty = convert_ty(&presence, current_crate, scratch)?;
    let since = get_since_attr(&item_macro.attrs, current_crate, scratch)?;
    let docs = collect_docs(&item_macro.attrs);
    let mount = ApiItemOwned {
        id: UNib32(idx),
        kind: ApiItemKindOwned::Mount {
            expects: args.expects.as_ref().map(Ident::to_string),
            presence_ty,
        },
        multiplicity: if keyed {
            Multiplicity::Array {
                index_type_idx: None,
            }
        } else {
            Multiplicity::Flat
        },
        since,
        ident: name.clone(),
        docs: docs.clone(),
        optional: None,
        meta: None,
        identity: None,
    };
    if !keyed {
        return Ok(vec![mount]);
    }
    let ww_keyed = Ident::new("ww_keyed", Span::call_site());
    current_crate
        .resolve_path(std::iter::once(&ww_keyed), scratch)
        .context(
            "mounts! needs ww_keyed as a dependency of the API crate (KeyEvent, the item of its `keys` stream, comes \
             from there)",
        )?;
    let key: Type = syn::parse_quote!(ww_api::mount::MountName<'static>);
    let event: Type =
        syn::parse_quote!(ww_keyed::KeyEvent<UNib32, ww_api::mount::MountName<'static>>);
    let keys = ApiItemOwned {
        id: UNib32(idx + 1),
        kind: ApiItemKindOwned::Keys {
            of: UNib32(idx),
            key: convert_ty(&key, current_crate, scratch)?,
            event_ty: convert_ty(&event, current_crate, scratch)?,
        },
        multiplicity: Multiplicity::Flat,
        since,
        ident: ww_self::mount::keys_ident(&name),
        docs,
        optional: None,
        meta: None,
        identity: None,
    };
    Ok(vec![mount, keys])
}

/// An item named like a handler of a slot of its trait (`hub_presence`, `hub_request`) is refused, and so is a slot
/// named like another item.
pub(crate) fn reject_mount_ident_collisions(items: &[ApiItemOwned]) -> Result<()> {
    for mount in items {
        if !matches!(mount.kind, ApiItemKindOwned::Mount { .. }) {
            continue;
        }
        if let Some(other) = items
            .iter()
            .find(|item| item.ident == mount.ident && item.id != mount.id)
        {
            return Err(anyhow!(
                "mount slot `{}` has the same name as item {} of the same trait: rename one of them",
                mount.ident,
                other.id.0
            ));
        }
        let handlers = handler_idents(&mount.ident);
        if let Some(other) = items.iter().find(|item| handlers.contains(&item.ident)) {
            return Err(anyhow!(
                "item {} `{}` is named like a handler of mount slot `{}`: rename that item",
                other.id.0,
                other.ident,
                mount.ident
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::api::tests::load_src_with;
    use super::super::keyed::tests::deps;
    use std::path::Path;
    use ww_self::ApiBundleOwned;
    use ww_self::{ApiItemKindOwned, Multiplicity};

    fn load_api(name: &str, body: &str, keyed: bool) -> anyhow::Result<ApiBundleOwned> {
        let lib_rs = format!(
            "use ww_api::prelude::*;\n#[ww_api_root(wire = \"w1.0\")]\n#[effect(edit)]\npub trait Api {{\n{body}\n}}\n"
        );
        load_src_with(name, &deps(keyed), &lib_rs)
    }

    /// `tests/mounts_api`: `hub` one slot expecting `hub_api`, `ext` an array of slots with its key companion at the
    /// next id, `mute` after them.
    #[test]
    fn tv_bundle_holds_the_slots() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/mounts_api");
        let bundle = crate::load(&dir, Some("Tv".into()), false).unwrap();
        let items: Vec<_> = bundle
            .root
            .items
            .iter()
            .map(|i| (i.id.0, i.ident.as_str()))
            .collect();
        assert_eq!(
            items,
            [(0, "hub"), (1, "ext"), (2, "ext_keys"), (3, "mute")]
        );
        let [hub, ext, keys, _] = &bundle.root.items[..] else {
            panic!("four items");
        };
        assert!(
            matches!(&hub.kind, ApiItemKindOwned::Mount { expects: Some(c), .. } if c == "hub_api"),
            "{hub:?}"
        );
        assert_eq!(hub.multiplicity, Multiplicity::Flat);
        assert!(matches!(
            &ext.kind,
            ApiItemKindOwned::Mount { expects: None, .. }
        ));
        assert!(matches!(ext.multiplicity, Multiplicity::Array { .. }));
        assert!(matches!(&keys.kind, ApiItemKindOwned::Keys { of, .. } if of.0 == 1));
        assert_eq!(
            hub.docs.first().map(|d| d.trim()),
            Some("The home hub, when its process runs.")
        );
    }

    /// The presence type of every slot is `ww_api::mount::MountPresence`, the same type for `mount!` and `mounts!`.
    #[test]
    fn slots_share_the_presence_type() {
        let bundle = load_api("mount_presence", "mount!(a);\nmounts!(b);", true).unwrap();
        let presence = |i: usize| match &bundle.root.items[i].kind {
            ApiItemKindOwned::Mount { presence_ty, .. } => presence_ty.clone(),
            other => panic!("not a slot: {other:?}"),
        };
        assert_eq!(presence(0), presence(1));
    }

    /// `mounts!` needs `ww_keyed` in the API crate, `mount!` doesn't.
    #[test]
    fn mounts_needs_ww_keyed() {
        load_api("mount_no_keyed", "mount!(hub);", false).unwrap();
        let err = format!(
            "{:#}",
            load_api("mounts_no_keyed", "mounts!(ext);", false).unwrap_err()
        );
        assert!(err.contains("needs ww_keyed as a dependency"), "{err}");
    }

    #[test]
    fn refused_forms() {
        let cases = [
            (
                "mount_optional",
                "#[optional]\nmount!(hub);",
                "#[optional] on mount!(hub)",
            ),
            (
                "mount_errors",
                "#[errors(Busy)]\nmount!(hub);",
                "a mount slot has no error list",
            ),
            (
                "mount_clash",
                "mount!(hub);\nfn hub();",
                "same name as item 1",
            ),
            (
                "mount_handler_clash",
                "mount!(hub);\nfn hub_request();",
                "named like a handler of mount slot `hub`",
            ),
            (
                "mounts_keys_clash",
                "mounts!(ext);\nfn ext_keys();",
                "same name",
            ),
            (
                "mount_syntax",
                "mount!(hub, extra);",
                "expected mount!(name)",
            ),
            (
                "mount_path",
                "mount!(hub: a::b);",
                "mount!(name: api_crate)",
            ),
        ];
        for (name, body, expected) in cases {
            let err = format!("{:#}", load_api(name, body, true).unwrap_err());
            assert!(
                err.contains(expected),
                "{name}: expected `{expected}` in: {err}"
            );
        }
    }
}
