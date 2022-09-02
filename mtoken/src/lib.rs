/// proc-macro2 inspired crate but with support for other languages.
/// Used internally for code generation.

pub mod token;
pub mod token_tree;
pub mod token_stream;
pub mod ext;

pub use token::{Ident, Punct, Literal, Comment, CommentFlavor, Spacing};
pub use token_tree::{Delimiter, Group, TokenTree};
pub use token_stream::{TokenStream, ToTokens};

#[cfg(test)]
mod tests {
    use std::rc::Rc;
    use vhl::span::Span;
    use crate::ext::TokenStreamExt;
    use crate::token::IdentFlavor;
    use super::*;

    #[test]
    fn it_works() {
        let mut ts = TokenStream::new();
        ts.append(Ident::new(Rc::new("x".to_string()), IdentFlavor::Plain, Span::call_site()));
        ts.append(Punct::new('+', Spacing::Alone));
        ts.append(Ident::new(Rc::new("y".to_string()), IdentFlavor::Plain, Span::call_site()));
        assert_eq!(format!("{}", ts), "x + y");
    }
}