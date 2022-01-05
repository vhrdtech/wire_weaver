use parser::ast::item::Typename;

use proc_macro2::{Span, Ident, TokenStream};
use quote::{TokenStreamExt, ToTokens};

pub struct CGTypename<'i, 'c> {
    pub inner: &'c Typename<'i>
}

impl<'i, 'c> ToTokens for CGTypename<'i, 'c> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append(Ident::new(self.inner.typename, Span::call_site()));
    }
}

