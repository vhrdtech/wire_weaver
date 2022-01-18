use std::fmt;
use std::fmt::Display;
use crate::{ToTokens, TokenStream, TokenTree};

/// A word of code, which may be a keyword or legal variable name
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Ident {
    sym: String,
    span: Span,
    flavor: IdentFlavor
}

impl Ident {
    pub fn new(string: &str, span: Span) -> Self {
        Ident {
            sym: string.to_owned(),
            span,
            flavor: IdentFlavor::Plain
        }
    }
}

impl Display for Ident {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.flavor == IdentFlavor::RustRaw {
            f.write_str("r#")?;
        }
        Display::fmt(&self.sym, f)
    }
}

impl ToTokens for Ident {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.inner.push(TokenTree::Ident(self.clone()))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IdentFlavor {
    Plain,
    RustRaw
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Punct {
    ch: char,
    spacing: Spacing,
    span: Span,
}

impl Punct {
    pub fn new(ch: char, spacing: Spacing) -> Self {
        Punct {
            ch,
            spacing,
            span: Span::call_site()
        }
    }

    pub fn spacing(&self) -> Spacing {
        self.spacing
    }
}

impl Display for Punct {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(&self.ch, f)
    }
}

impl ToTokens for Punct {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.inner.push(TokenTree::Punct(self.clone()))
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Spacing {
    Alone,
    Joint
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Literal {
    repr: String,
    span: Span,
}

impl Display for Literal {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(&self.repr, f)
    }
}

impl ToTokens for Literal {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.inner.push(TokenTree::Literal(self.clone()))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Comment {
    line: String,
    flavor: CommentFlavor,
    span: Span,
}

impl Comment {
    pub fn new(line: &str, flavor: CommentFlavor, span: Span) -> Self {
        Self {
            line: line.to_owned(),
            flavor,
            span
        }
    }
}

impl Display for Comment {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.flavor {
            CommentFlavor::DoubleSlash => {
                f.write_str("// ")?;
                Display::fmt(&self.line, f)?;
                f.write_str("\n")
            }
            CommentFlavor::TripleSlash => {
                f.write_str("/// ")?;
                Display::fmt(&self.line, f)?;
                f.write_str("\n")
            }
            CommentFlavor::SlashStarMultiline => {
                f.write_str("/* ")?;
                Display::fmt(&self.line, f)?;
                f.write_str(" */")
            }
        }
    }
}

impl ToTokens for Comment {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.inner.push(TokenTree::Comment(self.clone()))
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CommentFlavor {
    DoubleSlash,
    TripleSlash,
    SlashStarMultiline
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Span {

}

impl Span {
    pub fn call_site() -> Self {
        Span {}
    }
}