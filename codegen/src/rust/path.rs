use ast::Path;
use mquote::mquote;
use mtoken::{TokenStream, ToTokens};
use mtoken::ext::TokenStreamExt;
use crate::rust::identifier::CGIdentifier;

pub struct PathCG<'ast> {
    pub inner: &'ast Path,
}

impl<'ast> ToTokens for PathCG<'ast> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append_separated(
            self.inner.segments.iter().map(|elem| CGIdentifier { inner: elem }),
            mquote!(rust r#" :: "#),
        );
    }
}
