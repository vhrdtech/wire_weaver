use proc_macro::TokenStream;

mod api;
mod ww_repr;

mod shrink_wrap;

/// Generates types definitions, serdes and API client or server side code from a .ww file.
///
/// Arguments:
/// * ww = "path to ww file" (optional)
/// * api_model = "client_server_v0_1" - whether to generate api model code as well (optional).
///   If not provided, use, for example, wire_weaver_client_server as a dependency.
/// * client = true/false - whether to generate client code or not.
/// * server = true/false - whether to generate server code or not.
/// * no_alloc = true/false - whether to use std types or RefVec for strings, vectors. Lifetime will be added automatically if no_alloc = true.
/// * use_async - whether to generate async-aware code.
/// * debug_to_file = "path to an output file" - save generated code to a file for debug purposes.
/// * derive = "A, B, C" - put additional derives on generated types definitions.
/// * method_model = "move_*=deferred, rotate_*=deferred, _=immediate" - list of comma separated regex expressions and deferred or immediate keywords.
///   Deferred methods can answer right away or later with a provided request id.
///   Immediate methods have to answer right away and ideally do not block.
///   Underscore captures all the unmatched methods.
/// * property_model = "_=get_set" - list of comma separated regex expressions and value_on_changed or get_set keywords.
///   Depending on the application, it might be more convenient to store property directly as a context struct member and
///   use value_on_changed, so that generated code directly reads and writes to it. Notification method is called when the value is changed.
///   In other cases, get_set is more useful, allowing to represent GPIO pin as a bool property, for example.
#[proc_macro_attribute]
pub fn wire_weaver_api(attr: TokenStream, item: TokenStream) -> TokenStream {
    api::api(attr.into(), item.into()).into()
}

/// Use Rust definition of an enum or struct to derive shrink wrap wire format.
/// Follow with ww_repr attribute to adjust discriminant size of enums.
#[proc_macro_attribute]
pub fn derive_shrink_wrap(attr: TokenStream, item: TokenStream) -> TokenStream {
    shrink_wrap::shrink_wrap(attr.into(), item.into()).into()
}

/// Allows to use u1, u2, ..., u32 or UNib32 (variable length, 1 or more nibbles) for enum discriminant,
/// when serializing and deserializing with ShrinkWrap.
#[proc_macro_attribute]
pub fn ww_repr(attr: TokenStream, item: TokenStream) -> TokenStream {
    ww_repr::ww_repr(attr.into(), item.into()).into()
}
