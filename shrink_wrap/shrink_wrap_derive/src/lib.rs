use proc_macro::TokenStream;

mod shrink_wrap;
mod ww_repr;

/// Use Rust definition of an enum or struct to derive SerializeShrinkWrap and DeserializeShrinkWrap implementations.
/// This attribute macro re-writes type definition, which allows for some additional functionality:
/// * Automatic generation of `MyTypeOwned` from `MyType<'i>` struct/enum definition and respective serdes code.
/// * Support for `#[flag]` attributes to manually position where Option and Result flags are placed in the binary form
///   (for space savings and/or backwards compatibility).
///
/// See also [ShrinkWrap derive macro](ShrinkWrap).
///
/// Follow with ww_repr attribute to adjust discriminant size of enums.
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
