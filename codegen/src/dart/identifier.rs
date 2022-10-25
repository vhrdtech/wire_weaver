use mtoken::ext::TokenStreamExt;
use mtoken::token::IdentFlavor;
use mtoken::{Ident, ToTokens, TokenStream};
use std::rc::Rc;
use ast::Identifier;

pub struct IdentifierCG {
    pub inner: Identifier,
}

impl ToTokens for IdentifierCG {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append(Ident::new(
            Rc::clone(&self.inner.symbols),
            IdentFlavor::DartAutoRaw,
        ));
    }
}

#[cfg(test)]
mod test {
    use mquote::mquote;

    #[test]
    fn identifier_autoraw_mquote() {
        let ts = mquote!(dart r#"
            part
        "#);
        assert_eq!(format!("{}", ts), "r_part");
    }
}
