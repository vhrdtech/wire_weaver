use std::fmt;
use std::fmt::Display;

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Comment {
    line: String,
    span: Span,
}

impl Display for Comment {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("\n")?;
        Display::fmt(&self.line, f)?;
        f.write_str("\n")
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Span {

}

impl Span {
    pub fn call_site() -> Self {
        Span {}
    }
}