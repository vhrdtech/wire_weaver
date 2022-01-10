/// proc-macro2 inspired crate but with support for other languages.
/// Used internally for code generation.

pub mod token;
pub mod token_tree;
pub mod token_stream;

pub use token::{Ident, Punct, Literal, Comment, Span, Spacing};
pub use token_tree::TokenTree;
pub use token_stream::TokenStream;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let mut ts = TokenStream::new();
        ts.append(Ident::new("x", Span::call_site()));
        ts.append(Punct::new('+', Spacing::Alone));
        ts.append(Ident::new("y", Span::call_site()));
        assert_eq!(format!("{}", ts), "x + y");
    }
}