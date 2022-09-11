pub mod ext;
/// proc-macro2 inspired crate but with support for other languages.
/// Used internally for code generation.
pub mod token;
pub mod token_stream;
pub mod token_tree;

pub use token::{Comment, CommentFlavor, Ident, Literal, Punct, Spacing};
pub use token_stream::{ToTokens, TokenStream};
pub use token_tree::{Delimiter, Group, TokenTree};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ext::TokenStreamExt;
    use crate::token::IdentFlavor;
    use std::rc::Rc;

    #[test]
    fn it_works() {
        let mut ts = TokenStream::new();
        ts.append(Ident::new(
            Rc::new("x".to_string()),
            IdentFlavor::Plain,
        ));
        ts.append(Punct::new('+', Spacing::Alone));
        ts.append(Ident::new(
            Rc::new("y".to_string()),
            IdentFlavor::Plain,
        ));
        assert_eq!(format!("{}", ts), "x + y");
    }
}
