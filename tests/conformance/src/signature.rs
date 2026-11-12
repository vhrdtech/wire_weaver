//! Signature cases (EVO-69): the shape signature and the signature with docs of a definition of `conformance_api`,
//! as `ww_self::signature` calculates them for a `Skipped*` location. A device leaves a known definition out by
//! them and a host or snapshot finds it again by them, so they are as sealed as golden bytes.
//!
//! Besides the sealed values, each case checks what makes the two signatures work together: the definition with its
//! docs cleared has the same shape signature, and its signature with docs is then that shape signature; a reference
//! is the shape signature, then the signature with docs; an 8-byte reference matches by either.

use crate::corpus::SignatureCase;
use std::path::Path;
use std::sync::OnceLock;
use ww_self::signature::{self, Match, Signatures, no_resolve};
use ww_self::{ApiBundleOwned, ApiLevelLocationOwned, TypeLocationOwned, TypeOwned};

/// The API model of `conformance_api` as loaded from source, the bundle a device's introspection data is made of
/// (generic types not prepared, unlike [crate::dynamic::bundle]).
fn bundle() -> &'static ApiBundleOwned {
    static BUNDLE: OnceLock<ApiBundleOwned> = OnceLock::new();
    BUNDLE.get_or_init(|| {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../conformance_api");
        wire_weaver_core::load(&path, Some("Conformance".into()), false)
            .unwrap_or_else(|e| panic!("conformance_api: {e:#}"))
    })
}

/// Both signatures of `definition` (`type Name`, `trait Name`) in `bundle`.
fn signatures(bundle: &ApiBundleOwned, definition: &str) -> Result<Signatures, String> {
    let found = if let Some(name) = definition.strip_prefix("type ") {
        let idx = bundle
            .types
            .iter()
            .position(|location| match location {
                TypeLocationOwned::InLine {
                    ty: TypeOwned::Struct(s),
                    ..
                } => s.qualified_name() == name,
                TypeLocationOwned::InLine {
                    ty: TypeOwned::Enum(e),
                    ..
                } => e.qualified_name() == name,
                _ => false,
            })
            .ok_or_else(|| format!("no type {name} in-line in conformance_api's model"))?;
        signature::type_signatures(bundle, idx as u32, &no_resolve)
    } else if let Some(name) = definition.strip_prefix("trait ") {
        let idx = bundle
            .traits
            .iter()
            .position(|location| {
                matches!(location, ApiLevelLocationOwned::InLine { level, .. } if level.trait_name == name)
            })
            .ok_or_else(|| format!("no trait {name} in-line in conformance_api's model"))?;
        signature::trait_signatures(bundle, idx as u32, &no_resolve)
    } else {
        return Err(format!(
            "definition '{definition}': `type Name` or `trait Name`"
        ));
    };
    found.map_err(|e| format!("{e:#}"))
}

/// Checks one case: the sealed values and how the two signatures relate.
pub fn check(case: &SignatureCase) -> Result<(), String> {
    let s = signatures(bundle(), &case.definition)?;
    let (shape, with_docs) = (hex::encode(&s.shape), hex::encode(&s.with_docs));
    if case.shape == "?" || case.with_docs == "?" {
        return Err(format!(
            "a new case: fill in \"shape\": \"{shape}\", \"with_docs\": \"{with_docs}\""
        ));
    }
    if case.shape != shape || case.with_docs != with_docs {
        return Err(format!(
            "calculated \"shape\": \"{shape}\", \"with_docs\": \"{with_docs}\"; the corpus has \"{}\", \"{}\". \
             Signatures don't change once in: mark it \"wire_change\": \"EVO-n / reason\" if this is deliberate",
            case.shape, case.with_docs
        ));
    }
    let mut without_docs = bundle().clone();
    signature::drop_docs(&mut without_docs);
    let cleared = signatures(&without_docs, &case.definition)?;
    if cleared.shape != s.shape || cleared.with_docs != s.shape {
        return Err(format!(
            "with its docs cleared: shape {}, with docs {}; both must be the shape signature {shape}",
            hex::encode(&cleared.shape),
            hex::encode(&cleared.with_docs)
        ));
    }
    let reference = s.reference(true);
    if reference != [s.shape.as_slice(), &s.with_docs].concat() || s.reference(false) != s.shape {
        return Err(format!(
            "reference {} is not the shape signature, then the one with docs",
            hex::encode(&reference)
        ));
    }
    let expected = [
        (reference.clone(), Match::Exact),
        (s.with_docs.clone(), Match::Exact),
        (
            [s.shape.as_slice(), &[0; signature::SIGNATURE_LEN]].concat(),
            Match::DocsDiffer,
        ),
        ([0; signature::SIGNATURE_LEN].to_vec(), Match::No),
    ];
    for (r, want) in expected {
        let got = s.matches(&r);
        if got != want {
            return Err(format!(
                "reference {} matches as {got:?}, not {want:?}",
                hex::encode(&r)
            ));
        }
    }
    // the shape signature alone is Exact where there are no docs to differ, Shape otherwise
    let alone = if s.shape == s.with_docs {
        Match::Exact
    } else {
        Match::Shape
    };
    if s.matches(&s.shape) != alone {
        return Err(format!(
            "the shape signature alone matches as {:?}, not {alone:?}",
            s.matches(&s.shape)
        ));
    }
    Ok(())
}
