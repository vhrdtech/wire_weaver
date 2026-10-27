//! Cargo features of the walked crates and the `#[cfg_attr(..)]` attributes they switch (API-60).
//!
//! A crate that isn't built on wire_weaver (`exact_units`) puts its ShrinkWrap serdes behind a feature:
//! `#[cfg_attr(feature = "shrink_wrap", derive_shrink_wrap(..))]`. The walker reads such an attribute as rustc does,
//! with the features the crate is built with: the ones its dependents ask for in their `[dependencies]` entries and
//! through `dep/feature` in their own `[features]`, its default features unless `default-features = false`, and
//! everything those enable in its `[features]` table. A crate reached from more than one dependent gets the union,
//! as Cargo unifies features.
//!
//! `#[cfg(..)]` on items is not evaluated: trait items and enum variants keep it as a condition the generated code
//! carries (DER-8), and every other item is found as if its `cfg` held.

use anyhow::{Result, anyhow};
use cargo_toml::{Dependency, Manifest};
use std::collections::BTreeSet;
use syn::punctuated::Punctuated;
use syn::{Attribute, Fields, File, Item, Meta, Token, TraitItem};

/// Enabled features of a crate, by name.
pub(crate) type Features = BTreeSet<String>;

/// What a dependent asks of a dependency in its `[dependencies]` entry.
#[derive(Debug)]
pub(crate) struct DepRequest {
    pub(crate) features: Vec<String>,
    pub(crate) default_features: bool,
}

impl DepRequest {
    /// The request of a dependency entry, without what `[workspace.dependencies]` adds for `{ workspace = true }`.
    pub(crate) fn of(dep: &Dependency) -> Self {
        match dep {
            Dependency::Simple(_) => DepRequest {
                features: vec![],
                default_features: true,
            },
            Dependency::Inherited(inherited) => DepRequest {
                features: inherited.features.clone(),
                // `default-features` is the workspace entry's
                default_features: true,
            },
            Dependency::Detailed(detail) => DepRequest {
                features: detail.features.clone(),
                default_features: detail.default_features,
            },
        }
    }
}

/// Features of the crate of `manifest` when `requested` are asked for: those, `default` with `default_features`,
/// and what they enable in its `[features]` table, transitively. Optional dependencies named as features (`dep:x`,
/// or the implicit feature `x`) don't enable more features of this crate.
pub(crate) fn enabled_features(
    manifest: &Manifest,
    requested: impl IntoIterator<Item = String>,
    default_features: bool,
) -> Features {
    let mut enabled = Features::new();
    let mut todo: Vec<String> = requested.into_iter().collect();
    if default_features {
        todo.push("default".into());
    }
    while let Some(feature) = todo.pop() {
        if feature.contains('/') || feature.starts_with("dep:") {
            continue;
        }
        if !enabled.insert(feature.clone()) {
            continue;
        }
        if let Some(enables) = manifest.features.get(&feature) {
            todo.extend(enables.iter().cloned());
        }
    }
    // `default` itself is no feature a `cfg` could test, but Cargo does set it
    if !manifest.features.contains_key("default") {
        enabled.remove("default");
    }
    enabled
}

/// Every feature of the crate of `manifest`, as `--all-features` does: each key of its `[features]` table, and the
/// implicit feature of each optional dependency.
pub(crate) fn all_features(manifest: &Manifest) -> Features {
    let optional_deps = manifest
        .dependencies
        .iter()
        .filter(|(_, dep)| dep.optional())
        .map(|(name, _)| name.clone());
    let requested = manifest.features.keys().cloned().chain(optional_deps);
    enabled_features(manifest, requested, true)
}

/// Features the crate of `manifest`, built with `enabled`, asks of its dependency `dep_key` through its own
/// `[features]` table: `dep_key/feature` and `dep_key?/feature`.
pub(crate) fn dep_requests(manifest: &Manifest, enabled: &Features, dep_key: &str) -> Vec<String> {
    let mut requests = vec![];
    for feature in enabled {
        let Some(enables) = manifest.features.get(feature) else {
            continue;
        };
        for item in enables {
            let Some((dep, dep_feature)) = item.split_once('/') else {
                continue;
            };
            if dep.strip_suffix('?').unwrap_or(dep) == dep_key {
                requests.push(dep_feature.to_string());
            }
        }
    }
    requests
}

/// Replaces each `#[cfg_attr(predicate, attrs..)]` in `file` (on the file, items, fields, enum variants and trait
/// items, in in-line modules too) with `attrs` when `predicate` holds for `features`, and drops it otherwise.
pub(crate) fn expand_cfg_attrs(file: &mut File, features: &Features) -> Result<()> {
    let expander = Expander { features };
    expander.attrs(&mut file.attrs)?;
    for item in &mut file.items {
        expander.item(item)?;
    }
    Ok(())
}

struct Expander<'f> {
    features: &'f Features,
}

impl Expander<'_> {
    fn item(&self, item: &mut Item) -> Result<()> {
        match item {
            Item::Struct(item_struct) => {
                self.attrs(&mut item_struct.attrs)?;
                self.fields(&mut item_struct.fields)?;
            }
            Item::Enum(item_enum) => {
                self.attrs(&mut item_enum.attrs)?;
                for variant in &mut item_enum.variants {
                    self.attrs(&mut variant.attrs)?;
                    self.fields(&mut variant.fields)?;
                }
            }
            Item::Mod(item_mod) => {
                self.attrs(&mut item_mod.attrs)?;
                if let Some((_, items)) = &mut item_mod.content {
                    for item in items {
                        self.item(item)?;
                    }
                }
            }
            Item::Trait(item_trait) => {
                self.attrs(&mut item_trait.attrs)?;
                for trait_item in &mut item_trait.items {
                    match trait_item {
                        TraitItem::Const(c) => self.attrs(&mut c.attrs)?,
                        TraitItem::Fn(f) => self.attrs(&mut f.attrs)?,
                        TraitItem::Type(t) => self.attrs(&mut t.attrs)?,
                        TraitItem::Macro(m) => self.attrs(&mut m.attrs)?,
                        _ => {}
                    }
                }
            }
            Item::Type(item_type) => self.attrs(&mut item_type.attrs)?,
            Item::Use(item_use) => self.attrs(&mut item_use.attrs)?,
            Item::Const(item_const) => self.attrs(&mut item_const.attrs)?,
            _ => {}
        }
        Ok(())
    }

    fn fields(&self, fields: &mut Fields) -> Result<()> {
        for field in fields.iter_mut() {
            self.attrs(&mut field.attrs)?;
        }
        Ok(())
    }

    fn attrs(&self, attrs: &mut Vec<Attribute>) -> Result<()> {
        if !attrs.iter().any(is_cfg_attr) {
            return Ok(());
        }
        let mut expanded = Vec::with_capacity(attrs.len());
        for attr in attrs.drain(..) {
            self.expand(attr, &mut expanded)?;
        }
        *attrs = expanded;
        Ok(())
    }

    /// `attr` into `out`, expanded if it is a `cfg_attr` (nested ones too, as rustc does).
    fn expand(&self, attr: Attribute, out: &mut Vec<Attribute>) -> Result<()> {
        if !is_cfg_attr(&attr) {
            out.push(attr);
            return Ok(());
        }
        let metas = attr
            .parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
            .map_err(|e| anyhow!("#[cfg_attr(..)]: {e}"))?;
        let mut metas = metas.into_iter();
        let predicate = metas
            .next()
            .ok_or_else(|| anyhow!("#[cfg_attr(..)] without a predicate"))?;
        let inner: Vec<Attribute> = metas
            .map(|meta| Attribute {
                pound_token: attr.pound_token,
                style: attr.style,
                bracket_token: attr.bracket_token,
                meta,
            })
            .collect();
        let holds = match eval(&predicate, self.features)? {
            Some(holds) => holds,
            // e.g. `#[cfg_attr(target_os = "none", derive(defmt::Format))]`: nothing the walker reads
            None if inner.iter().all(not_read_by_walker) => false,
            None => {
                return Err(anyhow!(
                    "#[cfg_attr({}, ..)]: the walker evaluates `feature = \"..\"`, `all`, `any` and `not` only, \
                     an API can't depend on the target",
                    quote::ToTokens::to_token_stream(&predicate)
                ));
            }
        };
        if holds {
            for attr in inner {
                self.expand(attr, out)?;
            }
        }
        Ok(())
    }
}

fn is_cfg_attr(attr: &Attribute) -> bool {
    attr.path().is_ident("cfg_attr")
}

/// Attributes the walker never reads: their predicate doesn't need to be known.
fn not_read_by_walker(attr: &Attribute) -> bool {
    const IGNORED: &[&str] = &[
        "derive",
        "allow",
        "warn",
        "deny",
        "forbid",
        "expect",
        "repr",
        "no_std",
        "must_use",
        "inline",
        "deprecated",
    ];
    IGNORED.iter().any(|name| attr.path().is_ident(name))
}

/// Value of a `cfg` predicate for a crate built with `features`, `None` for one that depends on the target
/// (`target_os = "none"`, `unix`, `debug_assertions`). The walker models a plain build: `test`, `doc`, `docsrs`,
/// `miri` and `fuzzing` are false.
pub(crate) fn eval(predicate: &Meta, features: &Features) -> Result<Option<bool>> {
    let name = predicate
        .path()
        .get_ident()
        .map(|ident| ident.to_string())
        .unwrap_or_default();
    match (name.as_str(), predicate) {
        ("feature", Meta::NameValue(nv)) => match &nv.value {
            syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) => Ok(Some(features.contains(&s.value()))),
            _ => Err(anyhow!("cfg: expected feature = \"name\"")),
        },
        ("all" | "any" | "not", Meta::List(list)) => {
            let args = list
                .parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
                .map_err(|e| anyhow!("cfg({name}(..)): {e}"))?;
            let mut values = vec![];
            for arg in &args {
                values.push(eval(arg, features)?);
            }
            Ok(match name.as_str() {
                "not" => match values.as_slice() {
                    [value] => value.map(|v| !v),
                    _ => return Err(anyhow!("cfg(not(..)) takes one predicate")),
                },
                // a known false decides `all` and a known true decides `any`, whatever the unknown ones are
                "all" if values.contains(&Some(false)) => Some(false),
                "any" if values.contains(&Some(true)) => Some(true),
                _ if values.contains(&None) => None,
                "all" => Some(true),
                _ => Some(false),
            })
        }
        ("test" | "doc" | "docsrs" | "miri" | "fuzzing", Meta::Path(_)) => Ok(Some(false)),
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(toml: &str) -> Manifest {
        Manifest::from_str(&format!(
            "[package]\nname = \"m\"\nversion = \"0.1.0\"\n\n{toml}"
        ))
        .unwrap()
    }

    fn set(features: &[&str]) -> Features {
        features.iter().map(|f| f.to_string()).collect()
    }

    #[test]
    fn features_enable_features() {
        let m = manifest(
            "[features]\ndefault = [\"a\"]\na = [\"b\", \"dep:x\", \"y/std\"]\nb = []\nc = [\"b\"]\n\n\
             [dependencies]\nx = { version = \"1\", optional = true }\ny = \"1\"\n",
        );
        assert_eq!(enabled_features(&m, [], true), set(&["a", "b", "default"]));
        assert_eq!(enabled_features(&m, [], false), set(&[]));
        assert_eq!(enabled_features(&m, ["c".into()], false), set(&["b", "c"]));
        assert_eq!(all_features(&m), set(&["a", "b", "c", "default", "x"]));
        let enabled = enabled_features(&m, [], true);
        assert_eq!(dep_requests(&m, &enabled, "y"), ["std"]);
        assert!(dep_requests(&m, &enabled, "x").is_empty());
    }

    #[test]
    fn optional_dependency_features() {
        let m = manifest(
            "[features]\nshrink_wrap = [\"dep:shrink_wrap\"]\nstd = [\"shrink_wrap?/std\"]\n\n\
             [dependencies]\nshrink_wrap = { version = \"1\", optional = true }\n",
        );
        let enabled = enabled_features(&m, ["std".into()], true);
        assert_eq!(enabled, set(&["std"]));
        assert_eq!(dep_requests(&m, &enabled, "shrink_wrap"), ["std"]);
    }

    fn eval_str(predicate: &str, features: &[&str]) -> Option<bool> {
        eval(&syn::parse_str(predicate).unwrap(), &set(features)).unwrap()
    }

    #[test]
    fn predicates() {
        assert_eq!(eval_str("feature = \"a\"", &["a"]), Some(true));
        assert_eq!(eval_str("feature = \"a\"", &[]), Some(false));
        assert_eq!(eval_str("not(feature = \"a\")", &[]), Some(true));
        assert_eq!(
            eval_str("all(feature = \"a\", feature = \"b\")", &["a"]),
            Some(false)
        );
        assert_eq!(
            eval_str("any(feature = \"a\", feature = \"b\")", &["b"]),
            Some(true)
        );
        assert_eq!(eval_str("all()", &[]), Some(true));
        assert_eq!(eval_str("any()", &[]), Some(false));
        assert_eq!(eval_str("test", &[]), Some(false));
        assert_eq!(eval_str("target_os = \"none\"", &[]), None);
        assert_eq!(
            eval_str("all(target_os = \"none\", feature = \"a\")", &[]),
            Some(false)
        );
        assert_eq!(
            eval_str("any(target_os = \"none\", feature = \"a\")", &["a"]),
            Some(true)
        );
        assert_eq!(
            eval_str("any(target_os = \"none\", feature = \"a\")", &[]),
            None
        );
    }

    fn expand(src: &str, features: &[&str]) -> Result<String> {
        let mut file: File = syn::parse_str(src).unwrap();
        expand_cfg_attrs(&mut file, &set(features))?;
        Ok(quote::ToTokens::to_token_stream(&file).to_string())
    }

    #[test]
    fn cfg_attrs_expand_where_the_walker_reads_them() {
        let src = r#"
            #[cfg_attr(feature = "sw", derive_shrink_wrap(sized), cfg_attr(feature = "x", doc = "x"))]
            #[cfg_attr(not(feature = "sw"), derive(Debug))]
            #[cfg_attr(target_os = "none", derive(defmt::Format))]
            pub struct S {
                #[cfg_attr(feature = "sw", default = 7)]
                a: u8,
            }
            mod m {
                #[cfg_attr(feature = "sw", derive_shrink_wrap)]
                pub enum E { #[cfg_attr(feature = "sw", since = "0.2")] A(#[cfg_attr(feature = "sw", flag)] u8) }
            }
        "#;
        let on = expand(src, &["sw", "x"]).unwrap();
        assert!(
            on.starts_with("# [derive_shrink_wrap (sized)] # [doc = \"x\"] pub struct S { # [default = 7] a : u8"),
            "{on}"
        );
        assert!(
            on.contains(
                "# [derive_shrink_wrap] pub enum E { # [since = \"0.2\"] A (# [flag] u8) }"
            ),
            "{on}"
        );
        let off = expand(src, &[]).unwrap();
        assert!(
            off.starts_with("# [derive (Debug)] pub struct S { a : u8"),
            "{off}"
        );
        assert!(off.contains("pub enum E { A (u8) }"), "{off}");

        let err = expand(
            "#[cfg_attr(target_os = \"none\", derive_shrink_wrap(sized))] struct T;",
            &[],
        )
        .unwrap_err();
        assert!(err.to_string().contains("target_os = \"none\""), "{err}");
    }
}
