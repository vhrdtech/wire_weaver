use std::rc::Rc;
use mtoken::{Ident, TokenStream, ToTokens};
use mtoken::ext::TokenStreamExt;
use mtoken::token::IdentFlavor;

pub struct Identifier {
    pub inner: vhl::ast::identifier::Identifier,
}

impl ToTokens for Identifier {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append(Ident::new(
            Rc::clone(&self.inner.symbols),
            IdentFlavor::DartAutoRaw,
            self.inner.span.clone()
        ));
    }
}

#[cfg(test)]
mod test {
    use mquote::mquote;
    use vhl::span::Span;
    use super::*;

    #[test]
    fn identifier_autoraw_mquote() {
        let ts = mquote!(dart r#"
            part
        "#);
        assert_eq!(format!("{}", ts), "r_part");
    }
}