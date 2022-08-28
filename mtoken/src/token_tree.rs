use crate::token_stream::TokenStream;
use crate::token::{Ident, Punct, Literal, Comment, DelimiterRaw};
use std::fmt;
use std::fmt::{Display, Debug};
use crate::ToTokens;

/// A single token or a delimited sequence of token trees (e.g. `[1, (), ..]`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TokenTree {
    /// A token stream surrounded by bracket delimiters.
    Group(Group),
    /// An identifier.
    Ident(Ident),
    /// A single punctuation character (`+`, `,`, `$`, etc.).
    Punct(Punct),
    /// () {} or [] only in interpolate
    DelimiterRaw(DelimiterRaw),
    /// A literal character (`'a'`), string (`"hello"`), number (`2.3`), etc.
    Literal(Literal),
    /// A comment //, ///, #[doc = ""],
    Comment(Comment),
}

/// A delimited token stream.
///
/// A `Group` internally contains a `TokenStream` which is surrounded by
/// `Delimiter`s.
#[derive(Clone, Eq, PartialEq)]
pub struct Group {
    delimiter: Delimiter,
    stream: TokenStream,
}

impl Group {
    pub fn new(delimiter: Delimiter, stream: TokenStream) -> Self {
        Self {
            delimiter,
            stream
        }
    }
}

/// Describes how a sequence of token trees is delimited.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Delimiter {
    /// `( ... )`
    Parenthesis,
    /// `{ ... }`
    Brace,
    /// `[ ... ]`
    Bracket,
    // `Ø ... Ø`
    //
    // An implicit delimiter, that may, for example, appear around tokens
    // coming from a "macro variable" `$var`. It is important to preserve
    // operator priorities in cases like `$var * 3` where `$var` is `1 + 2`.
    // Implicit delimiters may not survive roundtrip of a token stream through
    // a string.
    // None,
}

impl From<Group> for TokenTree {
    fn from(group: Group) -> Self {
        TokenTree::Group(group)
    }
}

impl From<Ident> for TokenTree {
    fn from(ident: Ident) -> Self {
        TokenTree::Ident(ident)
    }
}

impl From<Punct> for TokenTree {
    fn from(punct: Punct) -> Self {
        TokenTree::Punct(punct)
    }
}

impl From<DelimiterRaw> for TokenTree {
    fn from(delim: DelimiterRaw) -> Self { TokenTree::DelimiterRaw(delim) }
}

impl From<Literal> for TokenTree {
    fn from(lit: Literal) -> Self {
        TokenTree::Literal(lit)
    }
}

impl From<Comment> for TokenTree {
    fn from(comment: Comment) -> Self {
        TokenTree::Comment(comment)
    }
}

impl Display for TokenTree {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            TokenTree::Group(t) => Display::fmt(t, f),
            TokenTree::Ident(t) => Display::fmt(t, f),
            TokenTree::Punct(t) => Display::fmt(t, f),
            TokenTree::DelimiterRaw(t) => Display::fmt(t, f),
            TokenTree::Literal(t) => Display::fmt(t, f),
            TokenTree::Comment(t) => Display::fmt(t, f),
        }
    }
}

impl Display for Group {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let (open, close) = match self.delimiter {
            Delimiter::Parenthesis => ("(", ")"),
            Delimiter::Brace => ("{ ", "}"),
            Delimiter::Bracket => ("[", "]"),
            // Delimiter::None => ("", ""),
        };

        f.write_str(open)?;
        Display::fmt(&self.stream, f)?;
        if self.delimiter == Delimiter::Brace && !self.stream.inner.is_empty() {
            f.write_str(" ")?;
        }
        f.write_str(close)?;

        Ok(())
    }
}

impl Debug for Group {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        let mut debug = fmt.debug_struct("Group");
        debug.field("delimiter", &self.delimiter);
        debug.field("stream", &self.stream);
        // debug_span_field_if_nontrivial(&mut debug, self.span);
        debug.finish()
    }
}

impl ToTokens for TokenTree {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.inner.push(self.clone())
    }
}