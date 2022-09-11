use crate::{ToTokens, TokenStream, TokenTree};
use std::iter;

pub trait TokenStreamExt: private::Sealed {
    fn append<U>(&mut self, token: U)
    where
        U: Into<TokenTree>;

    fn append_all<I>(&mut self, iter: I)
    where
        I: IntoIterator,
        I::Item: ToTokens;

    fn append_separated<I, U>(&mut self, iter: I, op: U)
    where
        I: IntoIterator,
        I::Item: ToTokens,
        U: ToTokens;

    fn append_terminated<I, U>(&mut self, iter: I, term: U)
    where
        I: IntoIterator,
        I::Item: ToTokens,
        U: ToTokens;
}

impl TokenStreamExt for TokenStream {
    fn append<U>(&mut self, token: U)
    where
        U: Into<TokenTree>,
    {
        self.extend(iter::once(token.into()));
    }

    fn append_all<I>(&mut self, iter: I)
    where
        I: IntoIterator,
        I::Item: ToTokens,
    {
        for token in iter {
            token.to_tokens(self);
        }
    }

    fn append_separated<I, U>(&mut self, iter: I, op: U)
    where
        I: IntoIterator,
        I::Item: ToTokens,
        U: ToTokens,
    {
        for (i, token) in iter.into_iter().enumerate() {
            if i > 0 {
                op.to_tokens(self);
            }
            token.to_tokens(self);
        }
    }

    fn append_terminated<I, U>(&mut self, iter: I, term: U)
    where
        I: IntoIterator,
        I::Item: ToTokens,
        U: ToTokens,
    {
        for token in iter {
            token.to_tokens(self);
            term.to_tokens(self);
        }
    }
}

mod private {
    use crate::TokenStream;

    pub trait Sealed {}
    impl Sealed for TokenStream {}
}
