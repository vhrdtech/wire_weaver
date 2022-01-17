use parser::ast::item::Docs;
use mtoken::{ToTokens, TokenStream, Punct, Spacing, Ident, Span, ext::TokenStreamExt};

pub struct CGDocs<'i, 'c> {
    pub inner: &'c Docs<'i>
}

impl<'i, 'c> ToTokens for CGDocs<'i, 'c> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        for l in &self.inner.lines {
            tokens.append(Punct::new('/', Spacing::Joint));
            tokens.append(Punct::new('/', Spacing::Joint));
            tokens.append(Punct::new('/', Spacing::Joint));
            tokens.append(Ident::new(l, Span::call_site()));
        }
    }
}