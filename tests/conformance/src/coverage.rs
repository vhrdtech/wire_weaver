//! TEST-25: every wire-relevant `ww_self::Type` variant, and every wire-relevant `shrink_wrap_derive` directive, is
//! exercised by at least one corpus case. A kind unlisted and unused fails the test; a kind listed as a gap that is
//! now used fails too, same rules as a case's own [gap marks](crate::corpus::Case::gap).
//!
//! `Type` is walked with an exhaustive match (`type_kind`): a new variant there is a compile error, not a
//! silently unchecked one. `shrink_wrap_derive`'s directives have no such enum to match on (they are parsed from
//! free-form tokens, see `shrink_wrap_derive::args::Args`), so `DIRECTIVE_KINDS` is a hand-kept list, checked by
//! whether its directive text appears in `tests/conformance_api/src/lib.rs` (the only crate whose types back
//! corpus cases): keep it in sync with `SUPPORTED_DIRECTIVES` there when the derive gains or drops one.
//!
//! Directives that don't change serialized bytes (which Rust type is generated: `borrowed`/`owned`/`derive(..)`/
//! `cfg_attr*`/`crate_path`/`non_exhaustive`/`discriminants`/`display`; `series(no_store)`/`series(cells)`, which
//! toggle extra trait impls, not the bytes of `Series<T>` itself) aren't wire-relevant and are left out of
//! `DIRECTIVE_KINDS` rather than listed as gaps: there is nothing a case could ever exercise about them.

use crate::corpus::Corpus;
use crate::dynamic::{bundle, property_ty};
use std::collections::BTreeSet;
use ww_self::{ApiBundleOwned, FieldsOwned, GenericArgOwned, TypeLocationOwned, TypeOwned};

/// A gap mark for a kind no case exercises yet: same shape and rules as `Case::gaps` (`docs/serdes/conformance.md`).
pub struct Gap {
    pub kind: &'static str,
    pub reason: &'static str,
}

/// `ww_self::Type` variants with no case yet.
pub const TYPE_GAPS: &[Gap] = &[
    Gap {
        kind: "Instance",
        reason: "by design: `ww_self::generics::prepare` (API-43) turns every `Instance` into an `OutOfLine` \
            reference to a monomorphized struct/enum before any client sees the bundle, as a device's own bundle \
            is also prepared; no case can reach one through a property's resolved type (INT-11)",
    },
    Gap {
        kind: "Param",
        reason: "by design: a type parameter only appears inside an un-instantiated generic definition; \
            `generics::prepare` (API-43) leaves it unreferenced by any property once the monomorphized struct/enum \
            takes its place (INT-11)",
    },
];

/// `shrink_wrap_derive` directives with no case yet.
pub const DIRECTIVE_GAPS: &[Gap] = &[
    Gap {
        kind: "no_future_version",
        reason: "no case turns off the `Unknown(u32)` catch-all (SW-47) and then feeds an out-of-range \
            discriminant: the corpus has no hand-written reject bytes for it yet (SW-67)",
    },
    Gap {
        kind: "series_opaque",
        reason: "no `Series<T>` (SW-60) case has a `#[series(opaque)]` field, so its own column form is \
            untested (SW-67)",
    },
];

/// Directives that change serialized bytes, checked by whether their text appears in `conformance_api`'s source
/// (see the module doc for why `borrowed`/`owned`/`derive`/`cfg_attr*`/.. are left out entirely).
const DIRECTIVE_KINDS: &[(&str, &str)] = &[
    ("final_structure", "final_structure"),
    ("self_describing", "self_describing"),
    ("sized", "sized"),
    ("fixed_layout", "fixed_layout"),
    ("head_tail", "head_tail"),
    ("sparse", "sparse"),
    ("validate", "validate ="),
    ("ww_repr", "ww_repr ="),
    ("series", "series,"),
    ("transparent", "transparent,"),
    ("series_opaque", "series(opaque)"),
    ("no_future_version", "no_future_version"),
    ("flag", "#[flag]"),
    ("default", "#[default = "),
    ("wire", "#[wire("),
];

/// Name of a `ww_self::Type` variant: exhaustive, so a new variant fails to compile here instead of being missed.
fn type_kind(ty: &TypeOwned) -> &'static str {
    match ty {
        TypeOwned::Bool => "Bool",
        TypeOwned::NumericAny(_) => "NumericAny",
        TypeOwned::OutOfLine { .. } => "OutOfLine",
        TypeOwned::Flag => "Flag",
        TypeOwned::String => "String",
        TypeOwned::Vec(_) => "Vec",
        TypeOwned::Array { .. } => "Array",
        TypeOwned::Tuple(_) => "Tuple",
        TypeOwned::Struct(_) => "Struct",
        TypeOwned::Enum(_) => "Enum",
        TypeOwned::Option { .. } => "Option",
        TypeOwned::Result { .. } => "Result",
        TypeOwned::Box(_) => "Box",
        TypeOwned::Range(_) => "Range",
        TypeOwned::RangeInclusive(_) => "RangeInclusive",
        TypeOwned::MaxLen { .. } => "MaxLen",
        TypeOwned::Instance { .. } => "Instance",
        TypeOwned::Param(_) => "Param",
        TypeOwned::Series(_) => "Series",
    }
}

/// All 19 names `type_kind` can give, for the "unlisted and unused fails" check.
pub const ALL_TYPE_KINDS: &[&str] = &[
    "Bool",
    "NumericAny",
    "OutOfLine",
    "Flag",
    "String",
    "Vec",
    "Array",
    "Tuple",
    "Struct",
    "Enum",
    "Option",
    "Result",
    "Box",
    "Range",
    "RangeInclusive",
    "MaxLen",
    "Instance",
    "Param",
    "Series",
];

fn walk_fields(
    fields: &FieldsOwned,
    bundle: &ApiBundleOwned,
    visited: &mut BTreeSet<u32>,
    out: &mut BTreeSet<&'static str>,
) {
    match fields {
        FieldsOwned::Named(fs) => fs.iter().for_each(|f| walk(&f.ty, bundle, visited, out)),
        FieldsOwned::Unnamed(fs) => fs.iter().for_each(|f| walk(&f.ty, bundle, visited, out)),
        FieldsOwned::Unit => {}
    }
}

/// Resolves an `OutOfLine`/`Instance` index into `bundle.types` at most once (self-referential types recurse).
fn resolve(
    idx: u32,
    bundle: &ApiBundleOwned,
    visited: &mut BTreeSet<u32>,
    out: &mut BTreeSet<&'static str>,
) {
    if !visited.insert(idx) {
        return;
    }
    if let Some(TypeLocationOwned::InLine { ty, .. }) = bundle.types.get(idx as usize) {
        walk(ty, bundle, visited, out);
    }
}

/// Every `ww_self::Type` variant reachable from `ty`, following `OutOfLine`/`Instance` into `bundle.types`.
fn walk(
    ty: &TypeOwned,
    bundle: &ApiBundleOwned,
    visited: &mut BTreeSet<u32>,
    out: &mut BTreeSet<&'static str>,
) {
    out.insert(type_kind(ty));
    match ty {
        TypeOwned::Bool
        | TypeOwned::Flag
        | TypeOwned::String
        | TypeOwned::NumericAny(_)
        | TypeOwned::Range(_)
        | TypeOwned::RangeInclusive(_)
        | TypeOwned::Param(_) => {}
        TypeOwned::Vec(t) | TypeOwned::Box(t) | TypeOwned::Series(t) => {
            walk(t, bundle, visited, out)
        }
        TypeOwned::Array { ty: t, .. }
        | TypeOwned::MaxLen { ty: t, .. }
        | TypeOwned::Option { some_ty: t } => walk(t, bundle, visited, out),
        TypeOwned::Tuple(ts) => ts.iter().for_each(|t| walk(t, bundle, visited, out)),
        TypeOwned::Struct(s) => walk_fields(&s.fields, bundle, visited, out),
        TypeOwned::Enum(e) => e
            .variants
            .iter()
            .for_each(|v| walk_fields(&v.fields, bundle, visited, out)),
        TypeOwned::Result { ok_ty, err_ty } => {
            walk(ok_ty, bundle, visited, out);
            walk(err_ty, bundle, visited, out);
        }
        TypeOwned::OutOfLine { type_idx } => resolve(type_idx.0, bundle, visited, out),
        TypeOwned::Instance { type_idx, args } => {
            for a in args.iter() {
                if let GenericArgOwned::Type(t) = a {
                    walk(t, bundle, visited, out);
                }
            }
            resolve(type_idx.0, bundle, visited, out);
        }
    }
}

/// `ww_self::Type` variants reachable from the type of some case in `corpus` (cases whose `type` isn't in the API
/// model, e.g. `head_tail`'s own types, can't be walked and are skipped: they have no `TypeOwned` to start from).
pub fn used_type_kinds(corpus: &Corpus) -> BTreeSet<&'static str> {
    let bundle = bundle();
    let mut visited = BTreeSet::new();
    let mut out = BTreeSet::new();
    let mut seen_tys = BTreeSet::new();
    for case in &corpus.cases {
        if !seen_tys.insert(case.ty.as_str()) {
            continue;
        }
        if let Some(ty) = property_ty(&case.ty) {
            walk(ty, bundle, &mut visited, &mut out);
        }
    }
    out
}

/// `shrink_wrap_derive` directives of `DIRECTIVE_KINDS` whose text appears in `conformance_api`'s source: a
/// stand-in for "exercised by some case", true because every type there backs at least one corpus case (checked by
/// a Python script cross-referencing `property!` idents against `corpus.json` while writing this guard; a type
/// added without a case would be unused dead code and `cargo check` would warn).
pub fn used_directive_kinds() -> BTreeSet<&'static str> {
    const SRC: &str = include_str!("../../conformance_api/src/lib.rs");
    DIRECTIVE_KINDS
        .iter()
        .filter(|(_, needle)| SRC.contains(needle))
        .map(|(name, _)| *name)
        .collect()
}

/// All directive names [`used_directive_kinds`] can give, for the "unlisted and unused fails" check.
pub fn all_directive_kinds() -> BTreeSet<&'static str> {
    DIRECTIVE_KINDS.iter().map(|(name, _)| *name).collect()
}

/// One failure line per kind that is unlisted-and-unused or listed-but-now-used, empty when the guard is satisfied.
pub fn check(
    all: &BTreeSet<&'static str>,
    used: &BTreeSet<&'static str>,
    gaps: &[Gap],
) -> Vec<String> {
    let mut failures = vec![];
    let gapped: BTreeSet<&str> = gaps.iter().map(|g| g.kind).collect();
    for gap in gaps {
        if !all.contains(&gap.kind) {
            failures.push(format!(
                "kind '{}' has a gap mark but isn't a kind this guard knows: stale entry, remove it",
                gap.kind
            ));
        } else if used.contains(&gap.kind) {
            failures.push(format!(
                "kind '{}' passes now: remove its gap mark and close what it names ({})",
                gap.kind, gap.reason
            ));
        }
    }
    for kind in all {
        if !used.contains(kind) && !gapped.contains(kind) {
            failures.push(format!(
                "kind '{kind}' is used by no case: give it a case, or a gap mark naming why not"
            ));
        }
    }
    failures
}
