use mquote::mquote;
use mtoken::{TokenStream, ToTokens};
use mtoken::ext::TokenStreamExt;
use vhl_stdlib::serdes::SerDesSize;

pub struct SerDesSizeCG {
    pub inner: SerDesSize,
}

impl ToTokens for SerDesSizeCG {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let variant = match self.inner {
            SerDesSize::Sized(size) => mquote!(rust r#" Sized(#size) "#),
            SerDesSize::SizedAligned(size_min, size_max) => mquote!(rust r#" SizedAligned(#size_min, #size_max) "#),
            SerDesSize::Unsized => mquote!(rust r#" Unsized "#),
            SerDesSize::UnsizedBound(max_size) => mquote!(rust r#" UnsizedBound(#max_size) "#),
        };
        tokens.append_all(mquote!(rust r#" SerDesSize::#variant "#));
    }
}