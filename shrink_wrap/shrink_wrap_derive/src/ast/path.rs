use proc_macro2::{Ident, Span, TokenStream};
use quote::{ToTokens, TokenStreamExt, quote};
use syn::GenericArgument;

/// A user type as named in a field: `Foo`, `a::b::Foo`, `Foo<u8>`, `Foo<'i, f32>`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Path {
    pub(crate) segments: Vec<Ident>,
    /// Generic arguments of the last segment as written, the leading lifetime included; emitted as written, except
    /// that [def](crate::ast::ty::Type::def) replaces the lifetime with the one of the generated type and
    /// [make_owned](Self::make_owned) drops it.
    pub(crate) args: Vec<GenericArgument>,
}

impl Path {
    pub(crate) fn new_ident(ident: Ident) -> Self {
        Path {
            segments: vec![ident],
            args: Vec::new(),
        }
    }

    pub(crate) fn new(segments: Vec<Ident>, args: Vec<GenericArgument>) -> Self {
        Path { segments, args }
    }

    /// The owned counterpart: `Foo<'i, T>` is `FooOwned<T>`.
    pub(crate) fn make_owned(&mut self) {
        if let Some(last_segment) = self.segments.last_mut() {
            *last_segment =
                Ident::new(format!("{}Owned", last_segment).as_str(), Span::call_site());
        }
        if matches!(self.args.first(), Some(GenericArgument::Lifetime(_))) {
            self.args.remove(0);
        }
    }

    /// The type with its generic arguments; `lifetime` (`'i`, `'static`) replaces the lifetime argument the type
    /// was written with, or is added in front of the other arguments when there was none.
    pub(crate) fn tokens(&self, lifetime: Option<TokenStream>) -> TokenStream {
        let segments = self.segments.iter();
        let mut args: Vec<TokenStream> = Vec::with_capacity(self.args.len() + 1);
        let mut rest = self.args.as_slice();
        if let Some(GenericArgument::Lifetime(_)) = rest.first() {
            rest = &rest[1..];
            if lifetime.is_none() {
                args.push(self.args[0].to_token_stream());
            }
        }
        if let Some(lifetime) = lifetime {
            args.push(lifetime);
        }
        args.extend(rest.iter().map(|a| a.to_token_stream()));
        if args.is_empty() {
            quote! { #(#segments)::* }
        } else {
            quote! { #(#segments)::* < #(#args),* > }
        }
    }
}

impl ToTokens for &Path {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append_all(self.tokens(None))
    }
}
