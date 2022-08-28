use parser::ast::naming::Identifier;
use mtoken::{ToTokens, TokenStream, Ident, Span, ext::TokenStreamExt};

pub struct CGTypename<'i, 'c> {
    pub inner: &'c Identifier<'i>
}

impl<'i, 'c> ToTokens for CGTypename<'i, 'c> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append(Ident::new(self.inner.name, Span::call_site()));
    }
}

