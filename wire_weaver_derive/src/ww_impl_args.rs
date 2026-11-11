use darling::FromMeta;
use darling::ast::NestedMeta;
use proc_macro2::{Delimiter, TokenStream, TokenTree};
use syn::ext::IdentExt;
use syn::parse::{Parse, ParseStream, Parser};
use syn::punctuated::Punctuated;
use syn::{Error, ExprArray, Ident, Path, Result, Token};
use wire_weaver_core::codegen::api_client::SharedSubLevel;
use wire_weaver_core::codegen::omit::Omit;

pub(crate) struct ApiArgs {
    /// The API crate, or `crate` for the crate the macro is called in (`local_trait` only, CLT-57).
    pub(crate) dep_name: Ident,
    /// Modules from the API crate's root to the API root, empty at the crate root: `wire_v2` of
    /// `thermo_api :: wire_v2 :: Thermo` in a crate with several roots (EVO-37).
    pub(crate) modules: Vec<Ident>,
    pub(crate) trait_name: Ident,
    /// `for <server or client struct>`, optional when only `local_trait` is generated.
    pub(crate) context_ident: Option<Path>,
    pub(crate) ext: ImplExtArgs,
}

#[derive(Debug, FromMeta)]
pub(crate) struct ImplExtArgs {
    #[darling(default)]
    pub(crate) client: String,

    #[darling(default)]
    pub(crate) server: bool,

    /// Emit the API as an async Rust trait of this name, with its `<Name>Dyn` companion (CLT-57).
    #[darling(default)]
    pub(crate) local_trait: String,

    /// Path of the trait made by `local_trait`, to implement on the generated client (CLT-57).
    #[darling(default)]
    pub(crate) impl_local_trait: String,

    #[darling(default)]
    pub(crate) no_alloc: bool,
    #[darling(default)]
    pub(crate) use_async: bool,

    /// Which handlers are `async fn` and which plain `fn` (SRV-21): `"device.settings.*=async, _=sync"`.
    #[darling(default)]
    pub(crate) handler_model: String,

    #[darling(default)]
    pub(crate) method_model: String,

    #[darling(default)]
    pub(crate) property_model: String,

    #[darling(default)]
    pub(crate) const_model: String,

    #[darling(default)]
    pub(crate) debug_to_file: String,

    #[darling(default)]
    pub(crate) introspect: String,

    /// How the device keeps its introspection bytes (INT-16): `"none"` (or empty) or `"lz4"`.
    #[darling(default)]
    pub(crate) introspect_compression: String,

    #[darling(default)]
    pub(crate) multi_req: bool,

    /// Which global trait paths the server resolves itself (SRV-53): `"full"`, `"compact"`, `"none"`; empty: `"full"`
    /// without introspection, `"none"` with it.
    #[darling(default)]
    pub(crate) global_paths: String,

    #[darling(default)]
    pub(crate) medium: String,

    /// Samples per batch of a `+batched` property observed in `Samples` mode (API-39), 0: the default.
    #[darling(default)]
    pub(crate) observe_batch: usize,

    /// Bytes of the outbox of each trait level with `+batched` properties (API-39), 0: the default.
    #[darling(default)]
    pub(crate) observe_outbox: usize,

    /// Element tables of each trait level under resource arrays with `+observe` properties (API-63): how many
    /// elements of it hosts observe at the same time, 0: the default (4).
    #[darling(default)]
    pub(crate) observe_elements: usize,

    /// Levels with `+batched` properties sampled through observe cells instead of `<Level>Sampler::sample`
    /// (API-50), by their `ObserveState` field: `observe_cells = "root_foc, root_control"`.
    #[darling(default)]
    pub(crate) observe_cells: String,

    /// `false`: this firmware takes no samples of its `+batched` properties (SRV-56): no batcher, outbox or `Series`
    /// encoder, the API and the wire stay, `Samples` mode is refused with `ModeNotSupported`. Default `true`.
    #[darling(default)]
    pub(crate) observe_samples: Option<bool>,

    /// `omit = ["a.b", ..]` and `omit_if(<cfg>) = [..]`, parsed before darling (`omit_if(..) = ..` is not a `Meta`).
    #[darling(skip)]
    pub(crate) omit: Vec<Omit>,

    /// `sub_levels = [ww_indication::Indication => crate::IndicationClient, ..]` (CLT-68), parsed before darling.
    #[darling(skip)]
    pub(crate) sub_levels: Vec<SharedSubLevel>,
}

impl Parse for ApiArgs {
    fn parse(input: ParseStream) -> Result<Self> {
        let dep_name = Ident::parse_any(input)?;
        let _: Token![::] = input.parse()?;
        let mut modules = vec![];
        let mut trait_name: Ident = input.parse()?;
        while input.peek(Token![::]) {
            let _: Token![::] = input.parse()?;
            modules.push(std::mem::replace(&mut trait_name, input.parse()?));
        }
        let context_ident = if input.peek(Token![for]) {
            let _: Token![for] = input.parse()?;
            Some(input.parse()?)
        } else {
            None
        };
        let _: Token![,] = input.parse()?;
        Ok(ApiArgs {
            dep_name,
            modules,
            trait_name,
            context_ident,
            ext: input.parse()?,
        })
    }
}

impl Parse for ImplExtArgs {
    fn parse(input: ParseStream) -> Result<Self> {
        let ts: TokenStream = input.parse()?;
        let mut rest = TokenStream::new();
        let mut omit = vec![];
        let mut sub_levels = vec![];
        for arg in split_args(ts) {
            if let Some(s) = parse_sub_levels(&arg)? {
                sub_levels.extend(s);
                continue;
            }
            match parse_omit(&arg)? {
                Some(o) => omit.extend(o),
                None => {
                    rest.extend(arg);
                    rest.extend(quote::quote! { , });
                }
            }
        }
        let attr_args = NestedMeta::parse_meta_list(rest)?;
        let mut ext_args = ImplExtArgs::from_list(&attr_args)?;
        ext_args.omit = omit;
        ext_args.sub_levels = sub_levels;
        Ok(ext_args)
    }
}

/// `sub_levels = [dep::Trait => path::Client, ..]` (CLT-68), `None` for any other argument.
fn parse_sub_levels(arg: &[TokenTree]) -> Result<Option<Vec<SharedSubLevel>>> {
    let value = match arg {
        [TokenTree::Ident(ident), TokenTree::Punct(eq), value @ ..]
            if ident == "sub_levels" && eq.as_char() == '=' =>
        {
            value
        }
        [TokenTree::Ident(ident), ..] if ident == "sub_levels" => {
            return Err(Error::new(
                ident.span(),
                "expected sub_levels = [dep::Trait => path::SharedClient, ..]",
            ));
        }
        _ => return Ok(None),
    };
    let group = match value {
        [TokenTree::Group(g)] if g.delimiter() == Delimiter::Bracket => g,
        _ => {
            return Err(Error::new(
                arg[0].span(),
                "expected sub_levels = [dep::Trait => path::SharedClient, ..]",
            ));
        }
    };
    let entries: Punctuated<SubLevelEntry, Token![,]> =
        Punctuated::parse_terminated.parse2(group.stream())?;
    Ok(Some(
        entries
            .into_iter()
            .map(|e| {
                let mut segments: Vec<String> = e
                    .trait_path
                    .segments
                    .iter()
                    .map(|s| s.ident.to_string())
                    .collect();
                let trait_name = segments.pop().unwrap_or_default();
                let crate_name = segments.remove(0);
                SharedSubLevel {
                    crate_name,
                    modules: segments,
                    trait_name,
                    client: e.client,
                }
            })
            .collect(),
    ))
}

/// `dep::Trait => path::SharedClient`
struct SubLevelEntry {
    trait_path: Path,
    client: Path,
}

impl Parse for SubLevelEntry {
    fn parse(input: ParseStream) -> Result<Self> {
        let trait_path: Path = input.parse()?;
        if trait_path.segments.len() < 2 {
            return Err(Error::new_spanned(
                &trait_path,
                "name the trait with its crate: `ww_indication::Indication`",
            ));
        }
        let _: Token![=>] = input.parse()?;
        Ok(SubLevelEntry {
            trait_path,
            client: input.parse()?,
        })
    }
}

/// Splits macro arguments at top-level commas.
fn split_args(ts: TokenStream) -> Vec<Vec<TokenTree>> {
    let mut args = vec![vec![]];
    for tt in ts {
        match &tt {
            TokenTree::Punct(p) if p.as_char() == ',' => args.push(vec![]),
            _ => args.last_mut().unwrap().push(tt),
        }
    }
    args.retain(|arg| !arg.is_empty());
    args
}

/// `omit = ["a.b", ..]` or `omit_if(<cfg>) = [..]`, `None` for any other argument.
fn parse_omit(arg: &[TokenTree]) -> Result<Option<Vec<Omit>>> {
    let (cfg, value) = match arg {
        [TokenTree::Ident(ident), TokenTree::Punct(eq), value @ ..]
            if ident == "omit" && eq.as_char() == '=' =>
        {
            (None, value)
        }
        [
            TokenTree::Ident(ident),
            TokenTree::Group(cfg),
            TokenTree::Punct(eq),
            value @ ..,
        ] if ident == "omit_if"
            && cfg.delimiter() == Delimiter::Parenthesis
            && eq.as_char() == '=' =>
        {
            (Some(cfg.stream().to_string()), value)
        }
        [TokenTree::Ident(ident), ..] if ident == "omit" || ident == "omit_if" => {
            return Err(Error::new(
                ident.span(),
                "expected omit = [\"trait_item.item\", ..] or omit_if(<cfg>) = [..]",
            ));
        }
        _ => return Ok(None),
    };
    let value: TokenStream = value.iter().cloned().collect();
    let array: ExprArray = syn::parse2(value)?;
    let mut omit = vec![];
    for elem in array.elems {
        let syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(path),
            ..
        }) = elem
        else {
            return Err(Error::new_spanned(
                elem,
                "expected a string, \"trait_item.item\" or \"trait_item.group\"",
            ));
        };
        omit.push(Omit {
            path: path.value(),
            cfg: cfg.clone(),
        });
    }
    Ok(Some(omit))
}
