use mquote::mquote;
use mtoken::ext::TokenStreamExt;
use mtoken::{ToTokens, TokenStream};
use vhl_stdlib::serdes::SerDesSize;

pub struct SerDesSizeCG {
    pub inner: SerDesSize,
}

impl ToTokens for SerDesSizeCG {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let variant = match self.inner {
            SerDesSize::Sized(size) => mquote!(rust r#" Sized(Λsize) "#),
            SerDesSize::SizedAligned(size_min, size_max) => {
                mquote!(rust r#" SizedAligned(Λsize_min, Λsize_max) "#)
            }
            SerDesSize::Unsized => mquote!(rust r#" Unsized "#),
            SerDesSize::UnsizedBound(max_size) => mquote!(rust r#" UnsizedBound(Λmax_size) "#),
        };
        tokens.append_all(mquote!(rust r#" SerDesSize::Λvariant "#));
    }
}
