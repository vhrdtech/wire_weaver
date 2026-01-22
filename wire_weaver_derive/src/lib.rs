use proc_macro::TokenStream;
use syn::parse_macro_input;

mod util;
mod version;
mod ww_api;
mod ww_impl_args;
mod ww_trait;

/// Generate types definitions, serdes and API client or server side code.
///
/// Arguments:
/// * client = absent or "" - do not generate client code at all;
///     * "full_client" - generate client code starting from API root
///     * "full_client+usb" - additionally, generate init function code that starts USB event loop
///     * "trait_client" - generate client code only for one trait
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
#[proc_macro]
pub fn ww_api(args: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as ww_impl_args::ApiArgs);
    ww_api::ww_api(args).into()
}

/// Generate types definitions, serdes and trait client or server side code.
///
/// See [ww_api](ww_api) for supported arguments.
#[proc_macro]
pub fn ww_impl(args: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as ww_impl_args::ApiArgs);
    ww_api::ww_impl(args).into()
}

/// Define a ww_trait, this macro is only a marker and produces no Rust code. All the work is done inside ww_impl! macro, which
/// loads the appropriate .rs file again through a file system or from crates.io, finds this marker and parses the trait definition.
/// TODO: transform ww_trait into valid Rust trait?
/// TODO: emit unit constant to check for name collisions
///
/// Example:
/// ```ignore
/// use wire_weaver_derive::ww_trait;
///
/// #[ww_trait]
/// pub trait BuildInfo {
///     fn date() -> u32;
///     fn compiler_version() -> String;
/// }
/// ```
///
/// Global IDs:
/// If trait have a global unique ID in the registry, provide it in parenthesis:
/// ```ignore
/// #[ww_trait(123)]
/// pub trait Abc { }
/// ```
#[proc_macro_attribute]
pub fn ww_trait(attr: TokenStream, item: TokenStream) -> TokenStream {
    ww_trait::ww_trait(attr.into(), item.into()).into()
}

/// Create FullVersion with the crate name and major.minor.patch numbers during compile time.
#[proc_macro]
pub fn full_version(item: TokenStream) -> TokenStream {
    version::full_version(item.into()).into()
}

/// Create CompactVersion with the crate name and major.minor.patch numbers during compile time.
#[proc_macro]
pub fn compact_version(item: TokenStream) -> TokenStream {
    version::compact_version(item.into()).into()
}
