use proc_macro::TokenStream;

mod args;
mod ast;
mod codegen;
mod shrink_wrap;
mod transform;
mod ww_repr;

/// Use Rust definition of an enum or struct to derive `SerializeShrinkWrap` and `DeserializeShrinkWrap`
/// implementations. This attribute macro re-writes the type definition, which allows for some additional
/// functionality:
/// * Automatic generation of an owned `MyTypeOwned` type from a borrowed `MyType<'i>` struct/enum
///   definition (`&str` becomes `String`, `&[T]` becomes `Vec<T>`, etc), along with
///   `SerializeShrinkWrapOwned` and `DeserializeShrinkWrapOwned` implementations for it, when owned
///   generation is enabled (see the `owned` directive/attribute below).
/// * Support for `#[flag]` field attributes to manually position where `Option` and `Result` flags are
///   placed in the binary form (for space savings and/or backwards compatibility).
/// * TODO: Support for `#[since = "x.y.z"]` field attributes to generate correct evolution code.
///
/// # Attribute arguments
///
/// `#[derive_shrink_wrap(directive, directive, ..)]` accepts a comma separated list of directives:
/// * `borrowed(<cfg predicate>)` - gate generation of the borrowed/base type behind a `cfg`
///   predicate, e.g. `borrowed(feature = "ref")`. The borrowed type is generated unconditionally
///   when this is omitted.
/// * `owned` / `owned(<cfg predicate>)` - enable owned type generation, as described above. Bare
///   `owned` generates it unconditionally, `owned(<cfg predicate>)` gates it behind an arbitrary
///   `cfg` predicate, e.g. `owned(any(feature = "std", feature = "alloc"))`.
/// * `cfg_attr(condition, attr, ..)` / `cfg_attr_owned(..)` / `cfg_attr_borrowed(..)` - splice a
///   `#[cfg_attr(condition, attr, ..)]` verbatim onto the base / owned / borrowed generated type.
///   Can be repeated. Useful for optional trait impls, e.g.
///   `cfg_attr_borrowed(feature = "defmt", derive(defmt::Format))`.
/// * `derive(Path, ..)` / `derive_owned(..)` / `derive_borrowed(..)` - extra derives (paths, so
///   `core::fmt::Debug` works too) added to the base / owned / borrowed generated type, like Rust's
///   own `#[derive(..)]`.
/// * `final_structure` / `self_describing` / `sized` - lower the assumed element size of the type to
///   save space at the cost of restricting further changes (mutually exclusive with each other).
/// * `fixed_layout` - require a fixed little-endian byte layout and its fast path (`ENCODED_LEN`, offset consts,
///   `from_fixed` / `to_fixed`, `FixedLayout`); a field that breaks it is a compile error. Types that have such a
///   layout get the fast path without it, see `shrink_wrap::fixed_layout`.
/// * `head_tail` - the head+tail encoding instead of shrink_wrap's: every fixed-width field at a constant offset in a
///   head, lists (`head_tail::List`), strings and option payloads one after another in a tail (`from_head_tail` /
///   `to_head_tail`, `HeadTail`, a C reader); no `SerializeShrinkWrap`, see `shrink_wrap::head_tail`.
/// * `sparse` - a struct whose fields are all `Option<T>` as an `OptionAnyVec`: one slot per field (or at
///   `#[slot = N]` on the field, increasing, gaps allowed), only the present ones written. Fields are only appended.
/// * `transparent` - a struct of one field that isn't zero bits (the others `PhantomData<..>` or `()`) is exactly
///   that field on the wire: its `ELEMENT_SIZE`, bytes, series columns, `SeriesStore` and `SeriesCells`. For
///   wrappers that only add meaning in the type, like a unit (`In<Micro, Second, u64>` is a `u64`).
/// * `ww_repr = <u1,u2.. | nib | unib32 | u8 | u16 ..>` - set the enum discriminant representation.
/// * `discriminants` - additionally generate a `MyTypeDiscriminants` enum (enum items only).
/// * `display` - generate `core::fmt::Display` and `core::error::Error` from the first doc line of each
///   variant/struct, `{field}` filled in; `#[display("..")]` overrides it.
///
/// Other outer attributes of the item (`#[derive(..)]`, `#[cfg_attr(..)]`, `#[allow(..)]`) stay on the generated type
/// with the item's own name, so the derive can be behind a `cfg_attr` of an optional feature with the plain derives
/// written once.
///
/// See also [ShrinkWrap derive macro](ShrinkWrap).
#[proc_macro_attribute]
pub fn derive_shrink_wrap(attr: TokenStream, item: TokenStream) -> TokenStream {
    shrink_wrap::shrink_wrap_attr(attr.into(), item.into()).into()
}

/// Derive SerializeShrinkWrap and DeserializeShrinkWrap implementations.
/// Also see [derive_shrink_wrap attribute macro](derive_shrink_wrap) which can additionally generate
/// Owned structs and enums from reference types and use `#[flag]` attributes.
#[proc_macro_derive(ShrinkWrap)]
pub fn derive_shrink_wrap_derive(item: TokenStream) -> TokenStream {
    shrink_wrap::shrink_wrap_derive(item.into()).into()
}

/// Allows to use u1, u2, ..., u32 or UNib32 (variable length, 1 or more nibbles) for enum discriminant,
/// when serializing and deserializing with ShrinkWrap.
#[proc_macro_attribute]
pub fn ww_repr(attr: TokenStream, item: TokenStream) -> TokenStream {
    ww_repr::ww_repr(attr.into(), item.into()).into()
}
