use mtoken::ext::TokenStreamExt;
use mtoken::token::IdentFlavor;
use mtoken::{Ident, ToTokens, TokenStream};
use std::rc::Rc;

pub struct Identifier {
    pub inner: vhl::ast::identifier::Identifier,
}

impl ToTokens for Identifier {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append(Ident::new(
            Rc::clone(&self.inner.symbols),
            IdentFlavor::DartAutoRaw,
        ));
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use mquote::mquote;
    use vhl::span::Span;

    #[test]
    fn identifier_autoraw_mquote() {
        let ts = mquote!(dart r#"
            part
        "#);
        assert_eq!(format!("{}", ts), "r_part");
    }
}
