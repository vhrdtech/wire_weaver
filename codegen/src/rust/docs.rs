use parser::ast::item::Docs;
use mtoken::{ToTokens, TokenStream, Span, Comment, CommentFlavor, ext::TokenStreamExt};
use mquote::mquote;

pub struct CGDocs<'i, 'c> {
    pub inner: &'c Docs<'i>
}

impl<'i, 'c> ToTokens for CGDocs<'i, 'c> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let lines = self.inner.lines.iter().map(|line|
            Comment::new(line, CommentFlavor::TripleSlash, Span::call_site())
        );
        tokens.append_all(mquote!(rust r#"
            #(#lines)*
        "#));
    }
}