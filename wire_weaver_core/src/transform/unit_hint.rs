//! TOOL-22: a plain integer field, argument or property whose name ends in a known unit suffix (`_us`, `_ms`,
//! `_ns`, `_s`, `_hz`, `_khz`, `_ppb`, `_ppm`, `_bytes`, `_bits`, `_kib`) notes the `exact_units::Fixed<U, P, T>` it
//! could become: a note, never an error, found by [suggest] while the crate walker (`load_crate`/`load`) runs, and
//! silenced per item with `#[no_unit_hint]` (bare on a struct/enum field, bare or naming arguments
//! `#[no_unit_hint(a, b)]` on a trait method, bare on a `property!`/`stream!`/`sink!`/`ww_impl!` item).

use syn::{Attribute, Meta, Type};

/// A unit-suffix hint found while walking a crate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitHint {
    /// Where it was found, e.g. `MyStruct::shared_us` or `set_timeout::timeout_ms`.
    pub item: String,
    /// The plain Rust type found, e.g. `u64`.
    pub ty: String,
    /// What it could become, e.g. `exact_units::Fixed<Second, Micro, u64>` (`exact_units::Micros<u64>`).
    pub suggestion: String,
    /// The name without its unit suffix, e.g. `shared`.
    pub renamed: String,
}

impl std::fmt::Display for UnitHint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: {} could be {}, renamed to `{}`; silence with #[no_unit_hint]",
            self.item, self.ty, self.suggestion, self.renamed
        )
    }
}

/// `(suffix, unit, prefix, alias)`, `exact_units::units::{unit}` and `exact_units::prefix::{prefix}`; `alias` is
/// the matching `exact_units` alias, if there is one.
const SUFFIXES: &[(&str, &str, &str, Option<&str>)] = &[
    ("_khz", "Hertz", "Kilo", None),
    ("_kib", "Byte", "Kibi", None),
    ("_bytes", "Byte", "One", Some("Bytes")),
    ("_bits", "Bit", "One", None),
    ("_ppb", "Ratio", "Nano", Some("Ppb")),
    ("_ppm", "Ratio", "Micro", None),
    ("_hz", "Hertz", "One", None),
    ("_us", "Second", "Micro", Some("Micros")),
    ("_ms", "Second", "Milli", Some("Millis")),
    ("_ns", "Second", "Nano", Some("Nanos")),
    ("_s", "Second", "One", None),
];

const PLAIN_INTEGERS: &[&str] = &[
    "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64", "i128", "isize",
];

/// `item` (a path for messages, e.g. `MyStruct::shared_us`) suggested as an `exact_units::Fixed`, if `name` ends
/// with a known unit suffix and `ty` is a bare integer (not already a `Fixed`, `Quantity` or other wrapper).
pub(crate) fn suggest(item: &str, name: &str, ty: &Type) -> Option<UnitHint> {
    let int_name = plain_integer(ty)?;
    let (suffix, unit, prefix, alias) = SUFFIXES
        .iter()
        .find(|(suffix, ..)| name.ends_with(suffix))?;
    let renamed = name.strip_suffix(suffix).filter(|s| !s.is_empty())?;
    let suggestion = match alias {
        Some(alias) => format!(
            "exact_units::Fixed<{unit}, {prefix}, {int_name}> (exact_units::{alias}<{int_name}>)"
        ),
        None => format!("exact_units::Fixed<{unit}, {prefix}, {int_name}>"),
    };
    Some(UnitHint {
        item: item.to_string(),
        ty: int_name.to_string(),
        suggestion,
        renamed: renamed.to_string(),
    })
}

/// The name of `ty` if it is a bare, single-segment, non-generic integer type.
fn plain_integer(ty: &Type) -> Option<&'static str> {
    let Type::Path(p) = ty else { return None };
    if p.qself.is_some() {
        return None;
    }
    let seg = p.path.segments.last()?;
    if !matches!(seg.arguments, syn::PathArguments::None) {
        return None;
    }
    PLAIN_INTEGERS.iter().copied().find(|i| seg.ident == i)
}

/// Whether `#[no_unit_hint]` on `attrs` silences the hint for `name`: bare silences everything on the item it is
/// on (a field, or every argument of a method), `#[no_unit_hint(a, b)]` only the named arguments.
pub(crate) fn is_silenced(attrs: &[Attribute], name: &str) -> bool {
    attrs.iter().any(|a| {
        if !a.path().is_ident("no_unit_hint") {
            return false;
        }
        match &a.meta {
            Meta::Path(_) => true,
            Meta::List(list) => list
                .parse_args_with(
                    syn::punctuated::Punctuated::<syn::Ident, syn::Token![,]>::parse_terminated,
                )
                .is_ok_and(|idents| idents.iter().any(|i| i == name)),
            Meta::NameValue(_) => false,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn suggests_known_suffixes() {
        let ty: Type = parse_quote!(u64);
        let hint = suggest("S::shared_us", "shared_us", &ty).unwrap();
        assert_eq!(hint.renamed, "shared");
        assert!(hint.suggestion.contains("Fixed<Second, Micro, u64>"));
        assert!(hint.suggestion.contains("Micros<u64>"));

        let ty: Type = parse_quote!(u32);
        let hint = suggest("S::rate_khz", "rate_khz", &ty).unwrap();
        assert!(hint.suggestion.contains("Fixed<Hertz, Kilo, u32>"));

        let ty: Type = parse_quote!(u16);
        let hint = suggest("S::ring_kib", "ring_kib", &ty).unwrap();
        assert!(hint.suggestion.contains("Fixed<Byte, Kibi, u16>"));
    }

    #[test]
    fn no_hint_without_a_suffix_or_a_plain_integer() {
        let ty: Type = parse_quote!(u64);
        assert!(suggest("S::count", "count", &ty).is_none());
        let ty: Type = parse_quote!(exact_units::Fixed<Second, Micro, u64>);
        assert!(suggest("S::shared_us", "shared_us", &ty).is_none());
        let ty: Type = parse_quote!(u64);
        assert!(
            suggest("S::_us", "_us", &ty).is_none(),
            "bare suffix leaves no name"
        );
    }

    #[test]
    fn silenced_by_attribute() {
        let bare: Attribute = parse_quote!(#[no_unit_hint]);
        assert!(is_silenced(&[bare], "shared_us"));
        let named: Attribute = parse_quote!(#[no_unit_hint(timeout_ms, other_hz)]);
        assert!(is_silenced(std::slice::from_ref(&named), "timeout_ms"));
        assert!(!is_silenced(&[named], "unrelated_ms"));
        assert!(!is_silenced(&[], "shared_us"));
    }
}
