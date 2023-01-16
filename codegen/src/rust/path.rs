use crate::rust::identifier::CGIdentifier;
use ast::Path;
use mquote::mquote;
use mtoken::ext::TokenStreamExt;
use mtoken::{ToTokens, TokenStream};

pub struct PathCG<'ast> {
    pub inner: &'ast Path,
}

impl<'ast> ToTokens for PathCG<'ast> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append_separated(
            self.inner
                .segments
                .iter()
                .map(|elem| CGIdentifier { inner: &elem.ident }),
            mquote!(rust r#" :: "#),
        );
    }
}
