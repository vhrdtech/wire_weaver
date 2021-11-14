#[derive(Clone, PartialEq, Debug)]
pub enum Token<'input> {
    Literal(Literal<'input>),
    Identifier(Identifier),
    // Identifier(&'input str),
    ShebangLine(&'input str),
    Comment(&'input str),
    DocComment(&'input str),

    // Expression operators
    /// "="
    Assign,

    // Unary op tokens
    /// "~"
    Tilde,
    /// "!"
    Excl,

    // Bool op tokens
    /// "<"
    Lt,
    /// "<="
    Le,
    /// "=="
    EqEq,
    /// "!="
    Ne,
    /// ">"
    Gt,
    /// ">="
    Ge,
    /// "&&"
    AndAnd,
    /// "||"
    OrOr,

    // Binary op tokens
    /// `+`
    Plus,
    /// `-`
    Minus,
    /// `*`
    Star,
    /// `/`
    Slash,
    /// `%`
    Percent,
    /// `^`
    Caret,
    /// `&`
    And,
    /// `|`
    Or,
    /// `<<`
    Shl,
    /// `>>`
    Shr,
    //?BinOpEq(BinOpToken),

    // Structural symbols
    /// "@"
    At,
    /// "."
    Dot,
    /// ".."
    DotDot,
    /// ","
    Comma,
    /// ";"
    Semicolon,
    /// ":"
    Colon,
    /// "<" as arrow
    RArrow,
    /// ">" as arrow
    LArrow,
    //FatArrow,
    /// "#"
    Pound,
    /// "$"
    Dollar,
    /// "?"
    Question,

    /// `(`
    OpenParen,
    /// `)`
    CloseParen,
    /// `[`
    OpenBracket,
    /// `]`
    CloseBracket,
    /// `{`
    OpenBrace,
    /// `}`
    CloseBrace,


    /// Any whitespace
    Whitespace,

    // Unknown,
    // TreeIndent(TreeIndent),
    Eof,
}

#[derive(Clone, PartialEq, Debug)]
pub enum Identifier {
    Let,
    Fn,
    If,
    Else,
    Return,
    Normal
}

#[derive(Clone, PartialEq, Debug)]
pub enum Literal<'input> {
    /// 127_u8, 0xff, 0o100, 0b129i99
    Discrete { base: Base, is_signed: bool, bits: u8, parsed: Option<u128> },
    /// 1.7q3.12, 0b1111uq1.3, 0xffuq1.7
    Fixed { base: Base, is_signed: bool, m: u8, n: u8, parsed: Option<u128> },
    /// 12.34f32, 56f16
    Float { base: Base, bits: u8, parsed: Option<u64> },
    /// 'a', '\\', ''', c'#'
    Char { r#char: char, kind: CharKind },
    // /// b'a', b'\\', b'''
    // Byte(u8),
    /// "abc", c"def"
    Str { r#str: &'input str, kind: CharKind },
    /// true or false
    Bool(bool),
    // /// "b"abc""
    // ByteStr(Vec<u8>),
    // /// "r"abc""
    //RawStr,
    // /// "br"abc""
    //RawByteStr
}

/// Base of numeric literal
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Base {
    /// 0b prefix
    Binary = 1,
    /// 0o prefix
    Octal = 2,
    /// 0x prefix
    Hexadecimal = 3,
    /// Without prefix
    Decimal = 4,
    // Used in parser to search literals in any base
    // Any = 5
}

/// Char kind in char or string
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum CharKind {
    Unicode,
    C,
}

pub enum Error {

}