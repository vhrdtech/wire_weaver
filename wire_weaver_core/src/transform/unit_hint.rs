//! TOOL-22: a plain integer field, argument or property whose name ends in a known unit suffix (`_us`, `_ms`,
//! `_ns`, `_s`, `_hz`, `_khz`, `_ppb`, `_ppm`, `_bytes`, `_bits`, `_kib`, `_mb`, ...), is named after a unit alone
//! (`bytes`, `bits`), reads as a per-second rate (`_kib_s`, `_b_s`, ...) or whose doc comment says what it counts
//! ("in bytes", "in tenths of a percent") notes the `exact_units::Fixed<U, P, T>` it could become: a note, never an
//! error, found by [suggest] while the crate walker (`load_crate`/`load`) runs, and silenced per item with
//! `#[no_unit_hint]` (bare on a struct/enum field, bare or naming arguments `#[no_unit_hint(a, b)]` on a trait
//! method, bare on a `property!`/`stream!`/`sink!`/`ww_impl!` item).

use syn::{Attribute, GenericArgument, Meta, PathArguments, Type};

/// A unit-suffix hint found while walking a crate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitHint {
    /// Where it was found, e.g. `MyStruct::shared_us` or `set_timeout::timeout_ms`.
    pub item: String,
    /// The plain Rust type found, e.g. `u64` or `Option<UVlq64>`.
    pub ty: String,
    /// What it could become, e.g. `exact_units::Fixed<Second, Micro, u64>` (`exact_units::Micros<u64>`).
    pub suggestion: String,
    /// The name without its unit suffix, e.g. `shared`; the same as the field's own name when nothing about it
    /// changes (a bare unit name, or a hint read from the doc comment rather than the name).
    pub renamed: String,
}

impl std::fmt::Display for UnitHint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {} could be {}", self.item, self.ty, self.suggestion)?;
        if self.renamed != name_of(&self.item) {
            write!(f, ", renamed to `{}`", self.renamed)?;
        }
        write!(f, "; silence with #[no_unit_hint]")
    }
}

/// The name part of an `item` path (`MyStruct::shared_us` -> `shared_us`).
fn name_of(item: &str) -> &str {
    item.rsplit("::").next().unwrap_or(item)
}

/// `(suffix, unit, prefix, alias)`, `exact_units::units::{unit}` and `exact_units::prefix::{prefix}`; `alias` is
/// the matching `exact_units` alias, if there is one. Checked after [RATE_SUFFIXES], so a rate's own `_s` is read
/// as a per-second rate, not these seconds.
const SUFFIXES: &[(&str, &str, &str, Option<&str>)] = &[
    ("_khz", "Hertz", "Kilo", None),
    ("_kib", "Byte", "Kibi", None),
    ("_mb_default", "Byte", "Mebi", None),
    ("_mb_min", "Byte", "Mebi", None),
    ("_mb_max", "Byte", "Mebi", None),
    ("_mb", "Byte", "Mebi", Some("Mebibytes")),
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

/// `(suffix, unit, prefix)`: a per-something-per-second rate, read as `exact_units::Fixed<Per<{unit}, Second>, ..>`
/// rather than as plain seconds (TOOL-22 gap: `write_kib_s` was hinted as `Second`, the `_kib` lost).
const RATE_SUFFIXES: &[(&str, &str, &str)] = &[
    ("_kib_s", "Byte", "Kibi"),
    ("_mib_s", "Byte", "Mebi"),
    ("_gib_s", "Byte", "Gibi"),
    ("_bytes_s", "Byte", "One"),
    ("_b_s", "Byte", "One"),
    ("_bits_s", "Bit", "One"),
];

/// A suffix read as a rate whose counted unit isn't known from the name alone: recognized so it isn't misread as
/// plain seconds, but no suggestion is made up for it.
const AMBIGUOUS_RATE_SUFFIXES: &[&str] = &["_per_s"];

/// `(name, unit, prefix, alias)`: a field named after the unit alone, matched exactly or as a `{name}_...` prefix
/// (`bytes_written`), where a suffix match finds nothing (the unit word isn't at the end).
const BARE_UNIT_NAMES: &[(&str, &str, &str, Option<&str>)] = &[
    ("bytes", "Byte", "One", Some("Bytes")),
    ("bits", "Bit", "One", None),
];

/// `(phrase, unit, prefix, alias)`: a doc-comment phrasing read as a unit when the name gives no suffix or bare-name
/// hint. Checked in order, lowercase, substring match.
const DOC_PHRASES: &[(&str, &str, &str, Option<&str>)] = &[
    ("in tenths of a percent", "Ratio", "Milli", Some("Permille")),
    ("in bytes", "Byte", "One", Some("Bytes")),
    ("in bits", "Bit", "One", None),
];

const PLAIN_INTEGERS: &[&str] = &[
    "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64", "i128", "isize",
];

/// `shrink_wrap`'s variable-length wire integers: the same "a number with the unit in its name, not its type" gap
/// as a plain integer (TOOL-22 gap: `RecorderCounters::bytes_written: UVlq64` printed no hint).
const WIRE_INT_TYPES: &[&str] = &["UVlq32", "UVlq32Backfill", "UVlq64", "UNib32", "INib32"];

/// A bare integer type recognized by [suggest], and whether it was itself wrapped in `Option<..>`.
struct Shape {
    /// The type as written, e.g. `u32` or `Option<UVlq64>`.
    display: String,
    /// The plain integer or wire integer name, e.g. `u32`, `UVlq64`.
    int_name: &'static str,
    optional: bool,
}

/// `item` (a path for messages, e.g. `MyStruct::shared_us`) suggested as an `exact_units::Fixed`, if `name` or
/// `docs` (the item's own doc comment, outer to inner) says what it is and `ty` is a bare integer, a `shrink_wrap`
/// variable-length integer, or an `Option` of either (not already a `Fixed`, `Quantity` or other wrapper).
pub(crate) fn suggest(item: &str, name: &str, ty: &Type, docs: &[String]) -> Option<UnitHint> {
    let shape = integer_shape(ty)?;
    suggest_from_name(item, name, &shape).or_else(|| suggest_from_docs(item, name, &shape, docs))
}

fn suggest_from_name(item: &str, name: &str, shape: &Shape) -> Option<UnitHint> {
    if AMBIGUOUS_RATE_SUFFIXES
        .iter()
        .any(|suffix| name.ends_with(suffix))
    {
        return None;
    }
    if let Some((suffix, unit, prefix)) = RATE_SUFFIXES
        .iter()
        .find(|(suffix, ..)| name.ends_with(suffix))
    {
        let renamed = name.strip_suffix(suffix).filter(|s| !s.is_empty())?;
        let per_unit = format!("Per<{unit}, Second>");
        return Some(build_hint(item, renamed, shape, &per_unit, prefix, None));
    }
    if let Some((suffix, unit, prefix, alias)) =
        SUFFIXES.iter().find(|(suffix, ..)| name.ends_with(suffix))
    {
        let renamed = name.strip_suffix(suffix).filter(|s| !s.is_empty())?;
        return Some(build_hint(item, renamed, shape, unit, prefix, *alias));
    }
    if let Some((_, unit, prefix, alias)) = BARE_UNIT_NAMES
        .iter()
        .find(|(bare, ..)| name == *bare || name.strip_prefix(&format!("{bare}_")).is_some())
    {
        return Some(build_hint(item, name, shape, unit, prefix, *alias));
    }
    None
}

fn suggest_from_docs(item: &str, name: &str, shape: &Shape, docs: &[String]) -> Option<UnitHint> {
    let joined = docs.join(" ").to_lowercase();
    let (_, unit, prefix, alias) = DOC_PHRASES
        .iter()
        .find(|(phrase, ..)| joined.contains(phrase))?;
    Some(build_hint(item, name, shape, unit, prefix, *alias))
}

fn build_hint(
    item: &str,
    renamed: &str,
    shape: &Shape,
    unit: &str,
    prefix: &str,
    alias: Option<&str>,
) -> UnitHint {
    let int_name = shape.int_name;
    let main = format!("exact_units::Fixed<{unit}, {prefix}, {int_name}>");
    let suggestion = match alias {
        Some(alias) => {
            let aliased = format!("exact_units::{alias}<{int_name}>");
            format!(
                "{} ({})",
                wrap_optional(shape.optional, &main),
                wrap_optional(shape.optional, &aliased)
            )
        }
        None => wrap_optional(shape.optional, &main),
    };
    UnitHint {
        item: item.to_string(),
        ty: shape.display.clone(),
        suggestion,
        renamed: renamed.to_string(),
    }
}

fn wrap_optional(optional: bool, s: &str) -> String {
    if optional {
        format!("Option<{s}>")
    } else {
        s.to_string()
    }
}

/// `ty` as a recognized integer shape, unwrapping one level of `Option<..>`.
fn integer_shape(ty: &Type) -> Option<Shape> {
    if let Some(inner) = option_inner(ty) {
        let int_name = bare_integer(inner)?;
        return Some(Shape {
            display: format!("Option<{int_name}>"),
            int_name,
            optional: true,
        });
    }
    let int_name = bare_integer(ty)?;
    Some(Shape {
        display: int_name.to_string(),
        int_name,
        optional: false,
    })
}

/// `ty`'s single type argument if it is a bare, single-segment `Option<..>`.
fn option_inner(ty: &Type) -> Option<&Type> {
    let Type::Path(p) = ty else { return None };
    if p.qself.is_some() {
        return None;
    }
    let seg = p.path.segments.last()?;
    if seg.ident != "Option" {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &seg.arguments else {
        return None;
    };
    if args.args.len() != 1 {
        return None;
    }
    match args.args.first()? {
        GenericArgument::Type(t) => Some(t),
        _ => None,
    }
}

/// The name of `ty` if it is a bare, single-segment, non-generic plain or `shrink_wrap` wire integer type.
fn bare_integer(ty: &Type) -> Option<&'static str> {
    let Type::Path(p) = ty else { return None };
    if p.qself.is_some() {
        return None;
    }
    let seg = p.path.segments.last()?;
    if !matches!(seg.arguments, PathArguments::None) {
        return None;
    }
    PLAIN_INTEGERS
        .iter()
        .chain(WIRE_INT_TYPES.iter())
        .copied()
        .find(|i| seg.ident == i)
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
        let hint = suggest("S::shared_us", "shared_us", &ty, &[]).unwrap();
        assert_eq!(hint.renamed, "shared");
        assert!(hint.suggestion.contains("Fixed<Second, Micro, u64>"));
        assert!(hint.suggestion.contains("Micros<u64>"));

        let ty: Type = parse_quote!(u32);
        let hint = suggest("S::rate_khz", "rate_khz", &ty, &[]).unwrap();
        assert!(hint.suggestion.contains("Fixed<Hertz, Kilo, u32>"));

        let ty: Type = parse_quote!(u16);
        let hint = suggest("S::ring_kib", "ring_kib", &ty, &[]).unwrap();
        assert!(hint.suggestion.contains("Fixed<Byte, Kibi, u16>"));
    }

    #[test]
    fn no_hint_without_a_suffix_or_a_plain_integer() {
        let ty: Type = parse_quote!(u64);
        assert!(suggest("S::count", "count", &ty, &[]).is_none());
        let ty: Type = parse_quote!(exact_units::Fixed<Second, Micro, u64>);
        assert!(suggest("S::shared_us", "shared_us", &ty, &[]).is_none());
        let ty: Type = parse_quote!(u64);
        assert!(
            suggest("S::_us", "_us", &ty, &[]).is_none(),
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

    #[test]
    fn rate_suffix_is_not_read_as_seconds() {
        let ty: Type = parse_quote!(u32);
        let hint = suggest("CardCheck::write_kib_s", "write_kib_s", &ty, &[]).unwrap();
        assert_eq!(hint.renamed, "write");
        assert!(
            hint.suggestion
                .contains("Fixed<Per<Byte, Second>, Kibi, u32>"),
            "{}",
            hint.suggestion
        );
        assert!(!hint.suggestion.contains("Fixed<Second"));
    }

    #[test]
    fn ambiguous_rate_suffix_is_suppressed_not_guessed() {
        let ty: Type = parse_quote!(u32);
        assert!(suggest("S::events_per_s", "events_per_s", &ty, &[]).is_none());
    }

    #[test]
    fn wire_integer_types_are_recognized() {
        let ty: Type = parse_quote!(UVlq64);
        let hint = suggest("RecorderCounters::ring_bytes", "ring_bytes", &ty, &[]).unwrap();
        assert!(hint.suggestion.contains("Fixed<Byte, One, UVlq64>"));
        assert!(hint.suggestion.contains("Bytes<UVlq64>"));
    }

    #[test]
    fn option_of_integer_is_recognized() {
        let ty: Type = parse_quote!(Option<u32>);
        let hint = suggest("S::timeout_ms", "timeout_ms", &ty, &[]).unwrap();
        assert_eq!(hint.ty, "Option<u32>");
        assert!(hint.suggestion.starts_with("Option<exact_units::Fixed"));
        assert!(
            hint.suggestion
                .contains("(Option<exact_units::Millis<u32>>)")
        );
    }

    #[test]
    fn bare_and_prefixed_unit_names_are_recognized() {
        let ty: Type = parse_quote!(u32);
        let hint = suggest("S::bytes", "bytes", &ty, &[]).unwrap();
        assert_eq!(hint.renamed, "bytes");
        assert!(hint.suggestion.contains("Fixed<Byte, One, u32>"));

        let ty: Type = parse_quote!(UVlq64);
        let hint = suggest("RecorderCounters::bytes_written", "bytes_written", &ty, &[]).unwrap();
        assert_eq!(
            hint.renamed, "bytes_written",
            "kept, stripping the prefix would lose meaning"
        );
        assert!(hint.suggestion.contains("Fixed<Byte, One, UVlq64>"));
    }

    #[test]
    fn compound_mb_suffixes_are_recognized() {
        let ty: Type = parse_quote!(u32);
        let hint = suggest("FormatOffer::rw_mb_max", "rw_mb_max", &ty, &[]).unwrap();
        assert_eq!(hint.renamed, "rw");
        assert!(hint.suggestion.contains("Fixed<Byte, Mebi, u32>"));
    }

    #[test]
    fn doc_phrase_is_used_when_the_name_gives_no_hint() {
        let ty: Type = parse_quote!(u32);
        let docs = vec!["Size of the register map in bytes, 0 if there is none.".to_string()];
        let hint = suggest("I2cSlaveCapabilities::map_len", "map_len", &ty, &docs).unwrap();
        assert_eq!(hint.renamed, "map_len");
        assert!(hint.suggestion.contains("Fixed<Byte, One, u32>"));

        let ty: Type = parse_quote!(u16);
        let docs = vec!["Busy time of the bus in the window, in tenths of a percent.".to_string()];
        let hint = suggest("BusLoad::load", "load", &ty, &docs).unwrap();
        assert!(hint.suggestion.contains("Fixed<Ratio, Milli, u16>"));
        assert!(hint.suggestion.contains("Permille<u16>"));
    }

    #[test]
    fn display_skips_the_rename_clause_when_the_name_is_unchanged() {
        let ty: Type = parse_quote!(u32);
        let hint = suggest("S::bytes", "bytes", &ty, &[]).unwrap();
        assert!(!hint.to_string().contains("renamed to"));

        let ty: Type = parse_quote!(u64);
        let hint = suggest("S::shared_us", "shared_us", &ty, &[]).unwrap();
        assert!(hint.to_string().contains("renamed to `shared`"));
    }
}
