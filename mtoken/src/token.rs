use crate::ext::TokenStreamExt;
use crate::{ToTokens, TokenStream, TokenTree};
use std::fmt;
use std::fmt::{Debug, Display, Formatter};
use std::rc::Rc;

/// A word of code, which may be a keyword or legal variable name
#[derive(Clone, Eq, PartialEq)]
pub struct Ident {
    sym: Rc<String>,
    // span: Span,
    flavor: IdentFlavor,
    spacing: Spacing,
}

impl Ident {
    pub fn new(sym: Rc<String>, flavor: IdentFlavor) -> Self {
        Ident { sym, flavor, spacing: Spacing::Alone }
    }

    pub fn set_spacing(&mut self, spacing: Spacing) {
        self.spacing = spacing;
    }

    pub fn spacing(&self) -> Spacing {
        self.spacing
    }
}

impl Display for Ident {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.flavor {
            IdentFlavor::Plain => {}
            IdentFlavor::RustAutoRaw => {
                if is_rust_keyword(self.sym.as_str()) {
                    f.write_str("r#")?;
                }
            }
            IdentFlavor::DartAutoRaw => {
                if is_dart_keyword(self.sym.as_str()) {
                    f.write_str("r_")?;
                }
            }
        }
        Display::fmt(&self.sym, f)
    }
}

impl Debug for Ident {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "Id(\x1b[35m{}\x1b[0m)", self.sym)
    }
}

fn is_rust_keyword(ident: &str) -> bool {
    // TODO: Add full list or Rust keywords
    match ident {
        "type" => true,

        _ => false,
    }
}

fn is_dart_keyword(ident: &str) -> bool {
    // TODO: Add full list of Dart keywords
    match ident {
        "part" => true,

        _ => false,
    }
}

impl ToTokens for Ident {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.inner.push_back(TokenTree::Ident(self.clone()))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IdentFlavor {
    Plain,
    RustAutoRaw,
    DartAutoRaw,
}

#[derive(Clone, Eq, PartialEq)]
pub struct Punct {
    ch: char,
    spacing: Spacing,
    // span: Span,
}

impl Punct {
    pub fn new(ch: char, spacing: Spacing) -> Self {
        Punct {
            ch,
            spacing,
            // span: Span::call_site()
        }
    }

    pub fn set_spacing(&mut self, spacing: Spacing) {
        self.spacing = spacing;
    }

    pub fn spacing(&self) -> Spacing {
        self.spacing
    }

    // Used to remove space in repetitions before these symbols only
    pub fn is_sequence_delimiter(&self) -> bool {
        (self.ch == ',') | (self.ch == ':')
    }
}

impl Display for Punct {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.ch == '⏎' {
            write!(f, "\n")
        } else {
            write!(f, "{}", self.ch)
        }
    }
}

impl Debug for Punct {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let joint = if self.spacing == Spacing::Joint {
            "J"
        } else {
            ""
        };
        write!(f, "P{}\x1b[33m{}\x1b[0m", joint, self.ch)
    }
}

impl ToTokens for Punct {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.inner.push_back(TokenTree::Punct(self.clone()))
    }
}

// /// () {} or [] only in interpolations, otherwise token_tree is yielded by pest parser
// #[derive(Copy, Clone, Eq, PartialEq)]
// pub enum DelimiterRaw {
//     ParenOpen,
//     ParenClose,
//     BraceOpen,
//     BraceClose,
//     BracketOpen,
//     BracketClose,
// }
//
// impl DelimiterRaw {
//     pub fn is_open(&self) -> bool {
//         use DelimiterRaw::*;
//         match self {
//             ParenOpen | BraceOpen | BracketOpen => true,
//             _ => false,
//         }
//     }
//
//     pub fn is_same_kind(&self, other: Self) -> bool {
//         use DelimiterRaw::*;
//         match self {
//             ParenOpen => other == ParenClose,
//             ParenClose => other == ParenOpen,
//             BraceOpen => other == BraceClose,
//             BraceClose => other == BraceOpen,
//             BracketOpen => other == BracketClose,
//             BracketClose => other == BracketOpen,
//         }
//     }
// }
//
// impl ToTokens for DelimiterRaw {
//     fn to_tokens(&self, tokens: &mut TokenStream) {
//         tokens
//             .inner
//             .push_back(TokenTree::DelimiterRaw(self.clone()))
//     }
// }

// impl Display for DelimiterRaw {
//     fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
//         let d = match self {
//             DelimiterRaw::ParenOpen => '(',
//             DelimiterRaw::ParenClose => ')',
//             DelimiterRaw::BraceOpen => '{',
//             DelimiterRaw::BraceClose => '}',
//             DelimiterRaw::BracketOpen => '[',
//             DelimiterRaw::BracketClose => ']',
//         };
//         write!(f, "{}", d)
//     }
// }
// impl Debug for DelimiterRaw {
//     fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
//         write!(f, "DR\x1b[31m{}\x1b[0m", self)
//     }
// }

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Spacing {
    Alone,
    Joint,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Literal {
    repr: String,
    // span: Span,
    spacing: Spacing,
}

impl Literal {
    pub fn new(repr: String) -> Self {
        Self {
            repr,
            spacing: Spacing::Alone
            // span: Span::call_site()
        }
    }

    pub fn set_spacing(&mut self, spacing: Spacing) {
        self.spacing = spacing;
    }

    pub fn spacing(&self) -> Spacing {
        self.spacing
    }
}

impl Display for Literal {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(&self.repr, f)
    }
}

impl ToTokens for Literal {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.inner.push_back(TokenTree::Literal(self.clone()))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Comment {
    line: String,
    flavor: CommentFlavor,
    // span: Span,
}

impl Comment {
    pub fn new(line: &str, flavor: CommentFlavor) -> Self {
        Self {
            line: line.to_owned(),
            flavor,
        }
    }
}

impl Display for Comment {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self.flavor {
            CommentFlavor::DoubleSlash => write!(f, "//{}", self.line),
            CommentFlavor::TripleSlash => write!(f, "///{}", self.line),
            CommentFlavor::SlashStarMultiline => write!(f, "/*{}*/", self.line),
            CommentFlavor::Pound => write!(f, "#{}", self.line),
            CommentFlavor::TripleQuote => write!(f, "\"\"\"{}\"\"\"", self.line),
            CommentFlavor::ExclamationMark => write!(f, "!{}", self.line),
        }
    }
}

impl ToTokens for Comment {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.inner.push_back(TokenTree::Comment(self.clone()))
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CommentFlavor {
    DoubleSlash,
    TripleSlash,
    TripleQuote,
    SlashStarMultiline,
    Pound,
    ExclamationMark,
}

impl ToTokens for String {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append(Ident::new(
            Rc::new(self.clone()),
            IdentFlavor::Plain,
            // Span::call_site()
        ));
    }
}

impl ToTokens for &str {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append(Ident::new(
            Rc::new(self.to_string()),
            IdentFlavor::Plain,
            // Span::call_site()
        ));
    }
}

impl ToTokens for usize {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append(Literal {
            repr: self.to_string(),
            spacing: Spacing::Alone
            // span: Span::call_site()
        })
    }
}

impl ToTokens for u32 {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append(Literal {
            repr: self.to_string(),
            spacing: Spacing::Alone,
            // span: Span::call_site()
        })
    }
}

impl ToTokens for i32 {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append(Literal {
            repr: self.to_string(),
            spacing: Spacing::Alone,
            // span: Span::call_site()
        })
    }
}
