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
            IdentFlavor::RustAutoRaw,
            self.inner.span.clone()
        ));
    }
}

#[cfg(test)]
mod test {
    use mquote::mquote;
    use vhl::ast::identifier::IdentifierContext;
    use vhl::span::Span;
    use super::*;

    #[test]
    fn identifier() {
        let ident = Identifier {
            inner: vhl::ast::identifier::Identifier {
                symbols: Rc::new("value".to_string()),
                context: IdentifierContext::UserTyName,
                span: Span::call_site()
            }
        };
        let mut ts = TokenStream::new();
        ident.to_tokens(&mut ts);
        assert_eq!(format!("{}", ts), "value");
    }

    #[test]
    fn identifier_via_mquote() {
        let ident = Identifier {
            inner: vhl::ast::identifier::Identifier {
                symbols: Rc::new("value".to_string()),
                context: IdentifierContext::UserTyName,
                span: Span::call_site()
            }
        };
        let ts = mquote!(rust r#"
            #ident
        "#);
        assert_eq!(format!("{}", ts), "value");
    }

    #[test]
    fn identifier_autoraw_mquote() {
        let ts = mquote!(rust r#"
            type
        "#);
        assert_eq!(format!("{}", ts), "r#type");
    }
}