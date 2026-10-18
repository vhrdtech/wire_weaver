use darling::FromMeta;
use darling::ast::NestedMeta;
use proc_macro2::{Delimiter, TokenStream, TokenTree};
use syn::parse::{Parse, ParseStream};
use syn::{Error, ExprArray, Ident, Path, Result, Token};
use wire_weaver_core::codegen::omit::Omit;

pub(crate) struct ApiArgs {
    pub(crate) dep_name: Ident,
    _colon_colon: Token![::],
    pub(crate) trait_name: Ident,
    _for: Token![for],
    pub(crate) context_ident: Path,
    _comma: Token![,],
    pub(crate) ext: ImplExtArgs,
}

#[derive(Debug, FromMeta)]
pub(crate) struct ImplExtArgs {
    #[darling(default)]
    pub(crate) client: String,

    #[darling(default)]
    pub(crate) server: bool,

    #[darling(default)]
    pub(crate) no_alloc: bool,
    #[darling(default)]
    pub(crate) use_async: bool,

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

    #[darling(default)]
    pub(crate) multi_req: bool,

    #[darling(default)]
    pub(crate) medium: String,

    /// Samples per batch of a `+batched` property observed in `Samples` mode (API-39), 0: the default.
    #[darling(default)]
    pub(crate) observe_batch: usize,

    /// Bytes of the outbox of each trait level with `+batched` properties (API-39), 0: the default.
    #[darling(default)]
    pub(crate) observe_outbox: usize,

    /// Levels with `+batched` properties sampled through observe cells instead of `<Level>Sampler::sample`
    /// (API-50), by their `ObserveState` field: `observe_cells = "root_foc, root_control"`.
    #[darling(default)]
    pub(crate) observe_cells: String,

    /// `omit = ["a.b", ..]` and `omit_if(<cfg>) = [..]`, parsed before darling (`omit_if(..) = ..` is not a `Meta`).
    #[darling(skip)]
    pub(crate) omit: Vec<Omit>,
}

impl Parse for ApiArgs {
    fn parse(input: ParseStream) -> Result<Self> {
        Ok(ApiArgs {
            dep_name: input.parse()?,
            _colon_colon: input.parse()?,
            trait_name: input.parse()?,
            _for: input.parse()?,
            context_ident: input.parse()?,
            _comma: input.parse()?,
            ext: input.parse()?,
        })
    }
}

impl Parse for ImplExtArgs {
    fn parse(input: ParseStream) -> Result<Self> {
        let ts: TokenStream = input.parse()?;
        let mut rest = TokenStream::new();
        let mut omit = vec![];
        for arg in split_args(ts) {
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
        Ok(ext_args)
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
