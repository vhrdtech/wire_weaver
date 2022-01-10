use crate::token_tree::TokenTree;
use std::fmt;
use std::fmt::Display;
use crate::Spacing;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TokenStream {
    pub(crate) inner: Vec<TokenTree>
}

impl TokenStream {
    pub fn new() -> Self {
        TokenStream {
            inner: Vec::new()
        }
    }

    pub fn append<U>(&mut self, token: U)
        where U: Into<TokenTree>
    {
        self.inner.push(token.into());
    }
}

impl Display for TokenStream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut joint = false;
        for (i, tt) in self.inner.iter().enumerate() {
            if i != 0 && !joint {
                write!(f, " ")?;
            }
            joint = false;
            match tt {
                TokenTree::Group(tt) => Display::fmt(tt, f),
                TokenTree::Ident(tt) => Display::fmt(tt, f),
                TokenTree::Punct(tt) => {
                    joint = tt.spacing() == Spacing::Joint;
                    Display::fmt(tt, f)
                }
                TokenTree::Literal(tt) => Display::fmt(tt, f),
                TokenTree::Comment(tt) => Display::fmt(tt, f),
            }?;
        }

        Ok(())
    }
}