use proc_macro::TokenStream;
use syn::parse_macro_input;

mod util;
mod version;
mod ww_api;
mod ww_impl_args;
mod ww_trait;

#[proc_macro]
#[deprecated = "use ww_codegen instead"]
pub fn ww_api(args: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as ww_impl_args::ApiArgs);
    ww_api::ww_api(args).into()
}

/// Generate types definitions, serdes and API client or server side code in place (proc-macro).
///
/// Optionally, consider using wire_weaver_core::gen_client and gen_server methods inside build.rs instead,
/// which might be more ergonomical and straight-forward.
///
/// Arguments:
/// * client = absent or "" - do not generate client code at all;
///     * "std_client" - std client starting from the API root, for a struct implementing
///       `wire_weaver_client::WwClient`; the transport (USB, RTT, WebSocket, UDP) is picked at connect time through
///       `ClientConfig`, see `examples/blinky`
///     * "trait_client" - std client for one trait only, independent of where a device implements it
///     * "raw" - no_std client, currently not working (FEATURES CLT-14)
/// * server = true/false - whether to generate server code or not.
/// * local_trait = "Name" - emit the API as an async Rust trait `Name` (with `NameDyn` for trait objects) that the
///   generated client and plain local types implement (CLT-57, `docs/api/local_trait.md`). Usually called in the
///   API crate itself as `ww_codegen!(crate :: Api, local_trait = "Name")`: `crate ::` and no `for <struct>`.
/// * impl_local_trait = "api_crate::Name" - with `client = "std_client"`, implement that trait on the client.
/// * no_alloc = true/false - whether to use std types or RefVec for strings, vectors. Lifetime will be added automatically if no_alloc = true.
/// * use_async - whether to generate async-aware code.
/// * handler_model = "device.settings.*|meter.reading=async, _=sync" - which user handlers are `async fn` and which
///   plain `fn` in an async server (SRV-21); without it all follow `use_async`. Each pattern is a regex matched
///   against an item's whole dotted path (`device.settings.vbus_mode`), first match wins, `_` is the default. Sync
///   handlers are called without `.await`; a handler written the other way is a compile error.
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
/// * const_model = "BOARD|IDS=fn" - list of comma separated regex expressions (matching the whole name of a `const`
///   item) and const or fn keywords, `_=const` if absent. The server struct implements the generated
///   `<Trait>Consts` trait with the value of every `const` item, no handler is called: `const NAME: T = ..;` for a
///   Rust const (the default), or `fn name(&self) -> T` with `fn` for a value fixed at boot or link time (OTP, chip
///   id, build id). Names carry the trait item chain prefix: `BOARD` of `board: BoardInfo` is `BOARD_BOARD` /
///   `board_board`.
/// * medium = "crate::Medium" - type (usually an enum) telling handlers which medium (USB, CAN, ...) a request came from,
///   `()` if absent. Every handler gets `cx: &mut wire_weaver::Context<'_, impl EventOut, Medium>` (`BlockingEventOut`
///   if `use_async = false`) as the first argument, with request seq, medium and a way to send stream updates.
#[proc_macro]
pub fn ww_codegen(args: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as ww_impl_args::ApiArgs);
    ww_api::ww_api(args).into()
}

/// Generate types definitions, serdes and trait client or server side code.
///
/// See [ww_codegen!] for supported arguments.
#[proc_macro]
pub fn ww_impl(args: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as ww_impl_args::ApiArgs);
    ww_api::ww_impl(args).into()
}

/// Define a ww_trait. All the work is done inside ww_codegen! / ww_impl! macros, which load the appropriate .rs file
/// again through a file system or from crates.io, find this marker and parse the trait definition.
///
/// In place, this macro leaves a Rust trait of the same name that only describes the API, so that rustdoc shows it with
/// all its resources (see `wire_weaver::api_doc`), and `*_FULL_GID` / `*_COMPACT_GID` consts with the API version.
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
/// Global IDs: a trait with an id in the [ww_global registry](https://github.com/vhrdtech/wire_weaver/tree/master/ww_global)
/// (keyed `crate::Trait` in `wire_weaver_gid.json`) gets it in its `*_COMPACT_GID` const, looked up by name; the
/// number is never written in code.
#[proc_macro_attribute]
pub fn ww_trait(attr: TokenStream, item: TokenStream) -> TokenStream {
    ww_trait::ww_trait(attr.into(), item.into()).into()
}

/// Define an API entry point (root level), otherwise the same as [ww_trait].
/// Provided for convenience, to distinguish between traits and API root's.
#[proc_macro_attribute]
pub fn ww_api_root(attr: TokenStream, item: TokenStream) -> TokenStream {
    ww_trait::ww_trait(attr.into(), item.into()).into()
}

/// Create FullVersion with the crate name and major.minor.patch numbers during compile time.
#[proc_macro]
pub fn full_version(item: TokenStream) -> TokenStream {
    version::full_version(item.into()).into()
}

/// Create CompactVersion with the global id and major.minor.patch numbers of this crate during compile time:
/// `compact_version!(Trait)` for `this_crate::Trait`, `compact_version!()` for the crate itself. `None` when the
/// [ww_global registry](https://github.com/vhrdtech/wire_weaver/tree/master/ww_global) has no id for it.
#[proc_macro]
pub fn compact_version(item: TokenStream) -> TokenStream {
    version::compact_version(item.into()).into()
}
