//! `#[moved_from(w2.1, Thermo::self_test)]` on a resource (EVO-40): where it was on an earlier wire line. Read from
//! the source for the evolution checker, not part of the API model.

use anyhow::{Result, anyhow};
use proc_macro2::{TokenStream, TokenTree};
use syn::{Attribute, Lit, Meta};
use ww_version::WireVersion;

/// A resource marked `#[moved_from(w2.1, Thermo::self_test)]`: the same resource as `self_test` of the trait
/// `Thermo` at wire version w2.1, moved to its place across a wire break. The evolution checker matches resources by
/// position within a line and only through this attribute across a break, verifies that the signature is the same,
/// and reports a move instead of a removal and an addition. It never takes a resource of the same name at another
/// place for the same one by itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MovedFrom {
    /// Crate of the trait the resource is in now.
    pub crate_id: String,
    /// The trait it is in now and its name, `Health` and `self_test`.
    pub trait_name: String,
    pub ident: String,
    /// The wire version it was at, `w2.1`.
    pub wire: WireVersion,
    /// The trait it was in and its name there, `Thermo` and `self_test`.
    pub from_trait: String,
    pub from_ident: String,
}

impl MovedFrom {
    /// Where it is now, `Health::self_test`.
    pub fn path(&self) -> String {
        format!("{}::{}", self.trait_name, self.ident)
    }

    /// Where it was, `Thermo::self_test`.
    pub fn from_path(&self) -> String {
        format!("{}::{}", self.from_trait, self.from_ident)
    }
}

impl std::fmt::Display for MovedFrom {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "#[moved_from({}, {})] on {}",
            self.wire,
            self.from_path(),
            self.path()
        )
    }
}

const EXPECTED: &str = "expected #[moved_from(w<line>.<revision>, Trait::resource)], e.g. #[moved_from(w2.1, \
                        Thermo::self_test)]";

/// Every `#[moved_from(..)]` among `attrs` of the resource `ident` of `trait_name`: one per earlier line it was at
/// another place on (a chain over several breaks keeps its older links), two naming one line refused.
pub(crate) fn get_moved_from_attrs(
    attrs: &[Attribute],
    crate_id: &str,
    trait_name: &str,
    ident: &str,
) -> Result<Vec<MovedFrom>> {
    let context = |e: anyhow::Error| e.context(format!("#[moved_from] on {trait_name}::{ident}"));
    let mut found: Vec<MovedFrom> = vec![];
    for attr in attrs.iter().filter(|a| a.path().is_ident("moved_from")) {
        let Meta::List(list) = &attr.meta else {
            return Err(context(anyhow!(EXPECTED)));
        };
        let (wire, path) = parse_args(list.tokens.clone()).map_err(context)?;
        if let Some(other) = found.iter().find(|m| m.wire.same_line(&wire)) {
            return Err(context(anyhow!(
                "two on line {}, {} and {}: a resource was at one place on each earlier line",
                wire.line.0,
                other.wire,
                wire
            )));
        }
        found.push(MovedFrom {
            crate_id: crate_id.to_string(),
            trait_name: trait_name.to_string(),
            ident: ident.to_string(),
            wire,
            from_trait: path.0,
            from_ident: path.1,
        });
    }
    Ok(found)
}

/// `w2.1, Thermo::self_test` (the version also as a string, `"w2.1"`).
fn parse_args(tokens: TokenStream) -> Result<(WireVersion, (String, String))> {
    let mut parts: Vec<Vec<TokenTree>> = vec![vec![]];
    for token in tokens {
        match &token {
            TokenTree::Punct(p) if p.as_char() == ',' => parts.push(vec![]),
            _ => parts.last_mut().expect("one part").push(token),
        }
    }
    if parts.last().is_some_and(|p| p.is_empty()) {
        parts.pop();
    }
    let [version, path] = parts.as_slice() else {
        return Err(anyhow!(EXPECTED));
    };
    let version: TokenStream = version.iter().cloned().collect();
    let version = match syn::parse2::<Lit>(version.clone()) {
        Ok(Lit::Str(s)) => s.value(),
        _ => version.to_string().replace(' ', ""),
    };
    let wire = version
        .parse::<WireVersion>()
        .map_err(|e| anyhow!("`{version}`: {e}; {EXPECTED}"))?;
    let path: TokenStream = path.iter().cloned().collect();
    let path: syn::Path =
        syn::parse2(path.clone()).map_err(|_| anyhow!("`{path}` is not a path; {EXPECTED}"))?;
    let segments: Vec<String> = path
        .segments
        .iter()
        .map(|s| match s.arguments.is_none() {
            true => Ok(s.ident.to_string()),
            false => Err(anyhow!("`{}` has generic arguments; {EXPECTED}", s.ident)),
        })
        .collect::<Result<_>>()?;
    let [from_trait, from_ident] = <[String; 2]>::try_from(segments).map_err(|segments| {
        anyhow!(
            "`{}` names no resource of a trait: the path is the trait and the resource name, as the checker writes \
             them, `Thermo::self_test`",
            segments.join("::")
        )
    })?;
    Ok((wire, (from_trait, from_ident)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    fn parse(attr: TokenStream) -> Result<Option<MovedFrom>> {
        let attrs = syn::parse::Parser::parse2(Attribute::parse_outer, attr).unwrap();
        get_moved_from_attrs(&attrs, "thermo_api", "Health", "self_test")
            .map(|m| m.into_iter().next())
    }

    #[test]
    fn moved_from_parses() {
        let moved = parse(quote!(#[moved_from(w2.1, Thermo::self_test)]))
            .unwrap()
            .unwrap();
        assert_eq!(moved.wire, WireVersion::new(2, 1));
        assert_eq!(moved.from_path(), "Thermo::self_test");
        assert_eq!(moved.path(), "Health::self_test");
        assert_eq!(
            moved.to_string(),
            "#[moved_from(w2.1, Thermo::self_test)] on Health::self_test"
        );
        let quoted = parse(quote!(#[moved_from("w12.10", Thermo::self_test,)]))
            .unwrap()
            .unwrap();
        assert_eq!(quoted.wire, WireVersion::new(12, 10));
        assert_eq!(parse(quote!(#[since = "w1.0"])).unwrap(), None);
        // a chain over two breaks: one per earlier line
        let attrs = syn::parse::Parser::parse2(
            Attribute::parse_outer,
            quote!(#[moved_from(w2.1, Thermo::self_test)] #[moved_from(w1.3, Sensor::check)]),
        )
        .unwrap();
        let chain = get_moved_from_attrs(&attrs, "thermo_api", "Health", "self_test").unwrap();
        assert_eq!(chain.len(), 2);
        assert_eq!(chain[1].from_path(), "Sensor::check");
        assert_eq!(chain[1].wire, WireVersion::new(1, 3));
    }

    #[test]
    fn moved_from_refuses_what_it_cant_read() {
        for (attr, error) in [
            (quote!(#[moved_from]), "expected #[moved_from(w<line>"),
            (quote!(#[moved_from(Thermo::self_test)]), "expected"),
            (quote!(#[moved_from(2.1, Thermo::self_test)]), "`2.1`"),
            (
                quote!(#[moved_from(w2.1, self_test)]),
                "`self_test` names no resource",
            ),
            (
                quote!(#[moved_from(w2.1, wire_v2::Thermo::self_test)]),
                "names no resource",
            ),
            (
                quote!(#[moved_from(w2.1, Thermo::a)] #[moved_from(w2.0, Api::a)]),
                "two on line 2, w2.1 and w2.0",
            ),
        ] {
            let e = format!("{:#}", parse(attr.clone()).unwrap_err());
            assert!(e.contains(error), "{attr}: {e}");
            assert!(e.contains("on Health::self_test"), "{e}");
        }
    }
}
