//! `#[effect(..)]` (API-58): what calling a method, writing a property or sending to a sink does. Read by
//! [get_meta](super::meta::get_meta) into `Meta::effects`, then settled per trait by [settle]: an item without its own
//! takes the trait's, and every item that changes something must end up with one. See `docs/api/metadata.md`.

use anyhow::{Result, anyhow};
use proc_macro2::Ident;
use syn::punctuated::Punctuated;
use syn::{Attribute, Meta, Token};
use ww_self::{ApiItemKindOwned, ApiItemOwned, Effects, MetaOwned, PropertyAccess};

/// `#[effect(edit, persist)]`: known names, each once, `read` alone.
pub(crate) fn parse(attr: &Attribute) -> Result<Effects> {
    let names = Effects::NAMED.map(|(_, n)| n).join(", ");
    let expected = format!("expected #[effect(..)] with one or more of: {names}");
    let Meta::List(list) = &attr.meta else {
        return Err(anyhow!("{expected}"));
    };
    let idents = list
        .parse_args_with(Punctuated::<Ident, Token![,]>::parse_terminated)
        .map_err(|e| anyhow!("{expected}: {e}"))?;
    let mut effects = Effects::NONE;
    for ident in &idents {
        let name = ident.to_string();
        let effect = Effects::from_name(&name)
            .ok_or_else(|| anyhow!("unknown effect `{name}`, {expected}"))?;
        if effects.contains(effect) {
            return Err(anyhow!("effect `{name}` named twice"));
        }
        effects = effects | effect;
    }
    if effects.is_empty() {
        return Err(anyhow!("{expected}"));
    }
    if effects.contains(Effects::READ) && !effects.is_read() {
        return Err(anyhow!(
            "#[effect({effects})]: `read` means the item changes nothing, so it combines with no other effect"
        ));
    }
    Ok(effects)
}

/// Whether an item of `kind` needs an effect.
enum Need {
    /// It changes something: a method, a writable property, a sink.
    Required,
    /// A read by its kind unless it says otherwise: a stream open.
    ReadUnlessDeclared,
    /// Only ever read: a const or read-only property, a key companion, a `ww_impl!` (whose items carry their own).
    Never,
}

fn need(kind: &ApiItemKindOwned) -> Need {
    match kind {
        ApiItemKindOwned::Method { .. }
        | ApiItemKindOwned::Stream { is_up: false, .. }
        | ApiItemKindOwned::FlowSink { .. } => Need::Required,
        ApiItemKindOwned::Property { access, .. } => match access {
            PropertyAccess::ReadWrite { .. } | PropertyAccess::WriteOnly => Need::Required,
            PropertyAccess::Const | PropertyAccess::ReadOnly { .. } => Need::Never,
        },
        ApiItemKindOwned::Stream { is_up: true, .. } | ApiItemKindOwned::StreamFn { .. } => {
            Need::ReadUnlessDeclared
        }
        ApiItemKindOwned::Trait { .. } | ApiItemKindOwned::Keys { .. } => Need::Never,
    }
}

/// Gives each item of trait `trait_name` that needs an effect and has none of its own the trait's `default`
/// (`#[effect(..)]` on the trait definition), and refuses an item that still has none, or one where an effect
/// doesn't apply.
pub(crate) fn settle(trait_name: &str, default: Effects, items: &mut [ApiItemOwned]) -> Result<()> {
    for item in items {
        let own = item.meta.as_ref().map_or(Effects::NONE, |m| m.effects);
        match need(&item.kind) {
            Need::Never if !own.is_empty() => {
                return Err(anyhow!(
                    "#[effect({own})] on `{trait_name}::{}`: it is only ever read, effects describe what a method, \
                     a property write, a sink or a stream open does",
                    item.ident
                ));
            }
            Need::Required if own.is_empty() => {
                if default.is_empty() {
                    return Err(anyhow!(
                        "`{trait_name}::{}` has no effect: say what it does with #[effect(..)] on it, or on trait \
                         {trait_name} for all its items ({}), see docs/api/metadata.md",
                        item.ident,
                        Effects::NAMED.map(|(_, n)| n).join(", ")
                    ));
                }
                item.meta.get_or_insert_with(MetaOwned::default).effects = default;
            }
            Need::Never | Need::Required | Need::ReadUnlessDeclared => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_str(s: &str) -> Result<Effects> {
        let attr: Attribute = syn::parse_quote!(#[effect(placeholder)]);
        let mut attr = attr;
        attr.meta = syn::parse_str(s).unwrap();
        parse(&attr)
    }

    #[test]
    fn names_parse_and_read_stands_alone() {
        assert_eq!(
            parse_str("effect(edit, persist)").unwrap(),
            Effects::EDIT | Effects::PERSIST
        );
        assert_eq!(parse_str("effect(read)").unwrap(), Effects::READ);
        let e = parse_str("effect(read, edit)").unwrap_err().to_string();
        assert!(e.contains("combines with no other"), "{e}");
        let e = parse_str("effect(edit, edit)").unwrap_err().to_string();
        assert!(e.contains("twice"), "{e}");
        let e = parse_str("effect(wobble)").unwrap_err().to_string();
        assert!(e.contains("unknown effect `wobble`"), "{e}");
        assert!(parse_str("effect").is_err());
        assert!(parse_str("effect()").is_err());
    }
}
