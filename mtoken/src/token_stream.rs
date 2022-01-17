use crate::token_tree::TokenTree;
use std::fmt;
use std::fmt::Display;
use crate::Spacing;
use std::iter::FromIterator;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TokenStream {
    pub(crate) inner: Vec<TokenTree>
}

pub trait ToTokens {
    fn to_tokens(&self, tokens: &mut TokenStream);
}

impl TokenStream {
    pub fn new() -> Self {
        TokenStream {
            inner: Vec::new()
        }
    }

    // pub fn append<U>(&mut self, token: U)
    //     where U: Into<TokenTree>
    // {
    //     self.inner.push(token.into());
    // }
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

impl From<TokenTree> for TokenStream {
    fn from(tree: TokenTree) -> TokenStream {
        let mut stream = TokenStream::new();
        // stream.push_token(tree);
        stream.inner.push(tree);
        stream
    }
}

impl FromIterator<TokenTree> for TokenStream {
    fn from_iter<I: IntoIterator<Item = TokenTree>>(tokens: I) -> Self {
        let mut stream = TokenStream::new();
        stream.extend(tokens);
        stream
    }
}

impl FromIterator<TokenStream> for TokenStream {
    fn from_iter<I: IntoIterator<Item = TokenStream>>(streams: I) -> Self {
        let mut v = Vec::new();

        for mut stream in streams {
            v.extend(stream.inner);
        }

        TokenStream { inner: v }
    }
}

impl Extend<TokenTree> for TokenStream {
    fn extend<T: IntoIterator<Item=TokenTree>>(&mut self, tokens: T) {
        tokens.into_iter().for_each(|tt| self.inner.push(tt));
    }
}

impl Extend<TokenStream> for TokenStream {
    fn extend<T: IntoIterator<Item = TokenStream>>(&mut self, stream: T) {
        self.inner.extend(stream.into_iter().flatten());
    }
}

impl ToTokens for TokenStream {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.extend(self.clone().into_iter())
    }
}

pub(crate) type TokenTreeIter = std::vec::IntoIter<TokenTree>;

impl IntoIterator for TokenStream {
    type Item = TokenTree;
    type IntoIter = TokenTreeIter;

    fn into_iter(mut self) -> TokenTreeIter {
        self.inner.into_iter()
    }
}