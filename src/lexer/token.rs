use std::{fmt, ops};
use nom_locate::{LocatedSpan};
#[cfg(feature = "trace")]
use nom_tracable::{TracableInfo, HasTracableInfo};
use nom_greedyerror::{GreedyError, Position};
use std::fmt::Formatter;
//use nom_packrat::HasExtraState;
use nom::{InputLength, InputTake, Slice, InputIter, Compare, CompareResult};
use std::ops::{Range, RangeTo, RangeFrom, RangeFull, Index};
use std::iter::Enumerate;
use strum_macros::{AsRefStr};

// /// Bool / Byte / Char / Integer / Float / Str / ByteStr / Err
// #[derive(Clone, Copy, Debug, PartialEq)]
// pub enum LitKind {
//     Bool,
//     Byte,
//     Char,
//     Integer,
//     Float,
//     Str,
//     //StrRaw(u16),
//     ByteStr,
//     //ByteStrRaw(u16),
//     Err
// }

/// LitKind + Symbol + Optional suffix
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lit {
    pub kind: LiteralKind,
    //pub symbol: Symbol,
    //pub suffix: Option<Symbol>
}

/// Reserved or normal identifier
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum IdentKind {
    Let,
    Fn,
    If,
    Else,
    Return,
    Normal
}

#[derive(Clone, Copy, Debug, PartialEq, AsRefStr)]
pub enum TokenKind {
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

    // Literals
    Literal(Lit),

    Ident(IdentKind),

    /// Any whitespace
    Whitespace,
    Comment,

    Unknown,

    TreeIndent(TreeIndent),
    //Eof,
}

#[derive(Copy, Clone, Debug)]
pub struct TreeIndent(pub i32);

impl PartialEq for TreeIndent {
    fn eq(&self, other: &Self) -> bool {
        if self.0 == i32::max_value() || other.0 == i32::max_value() {
            true
        } else {
            self.0 == other.0
        }
    }
}

impl TreeIndent {
    pub fn any() -> Self {
        TreeIndent(i32::max_value())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, AsRefStr)]
pub enum LiteralKind {
    /// "127_u8", "0o100", "0b129i99"
    Int { base: Base },
    /// "12.34f32", "56f16"
    Float { base: Base },
    /// "'a'", "'\\'", "'''"
    Char,
    /// "b'a'", "b'\\'", "b'''"
    //Byte,
    /// ""abc""
    Str,
    Bool,
    // /// "b"abc""
    //ByteStr,
    // /// "r"abc""
    //RawStr,
    // /// "br"abc""
    //RawByteStr
}

/// Base of numeric literal
#[derive(Clone, Copy, Debug)]
pub enum Base {
    /// 0b prefix
    Binary = 1,
    /// 0o prefix
    Octal = 2,
    /// 0x prefix
    Hexadecimal = 3,
    /// Without prefix
    Decimal = 4,
    /// Used int parser to search literals in any base
    Any = 5
}

impl PartialEq for Base {
    fn eq(&self, other: &Self) -> bool {
        let b1 = *self as u8;
        let b2 = *other as u8;
        println!("base cmp here {} {}", b1, b2);
        if b1 == 5 || b2 == 5 {
            true
        } else {
            b1 == b2
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BytePos(pub u32);

/// Token span [lo, hi)
#[derive(Clone, Copy)]
pub struct Span {
    pub lo: BytePos,
    pub hi: BytePos,
    pub line: u32
}

impl PartialEq for Span {
    fn eq(&self, other: &Span) -> bool {
        if self.lo.0 > self.hi.0 || // `any` span is equal to every real span, used in parser
            other.lo.0 > other.hi.0 {
            true
        } else {
            self.lo.0 == other.lo.0 && self.hi.0 == other.hi.0
        }
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if self.hi.0 > self.lo.0 {
            write!(f, "bytes[{}..{})", self.lo.0, self.hi.0)
        } else if self.hi.0 == 0u32 && self.lo.0 == 0u32 {
            write!(f, "∅")
        } else {
            write!(f, "any")
        }
    }
}

impl fmt::Debug for Span {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl Span {
    pub fn new(lo: u32, hi: u32) -> Self {
        Span {
            lo: BytePos(lo),
            hi: BytePos(hi),
            line: 0
        }
    }
    /// Constructs invalid Span that is equal to any other Span (when comparing TokenStream's).
    pub fn any() -> Self {
        Span {
            lo: BytePos(1u32),
            hi: BytePos(0u32),
            line: 0
        }
    }
    /// Constructs invalid Span with zero length, used as a span for TreeIndent tokens.
    pub fn zero() -> Self {
        Span {
            lo: BytePos(0u32),
            hi: BytePos(0u32),
            line: 0
        }
    }
}

impl ops::Add<Span> for Span {
    type Output = Span;

    fn add(self, rhs: Span) -> Span {
        Span {
            lo: BytePos(self.lo.0),
            hi: BytePos(rhs.hi.0),
            line: self.line
        }
    }
}

impl From<Span> for Range<usize> {
    fn from(s: Span) -> Range<usize> {
        Range { start: s.lo.0 as usize, end: s.hi.0 as usize }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DelimKind {
    Open,
    Close,
    Wrong
}

impl Token {
    pub fn is_punct(&self) -> bool {
        match self.kind {
            TokenKind::Comma |
            TokenKind::Dot |
            TokenKind::Colon |
            TokenKind::Semicolon => true,
            _ => false
        }
    }

    pub fn is_delim(&self) -> bool {
        match self.kind {
            TokenKind::OpenBrace |
            TokenKind::CloseBrace |
            TokenKind::OpenBracket |
            TokenKind::CloseBracket |
            TokenKind::OpenParen |
            TokenKind::CloseParen => true,
            _ => false
        }
    }

    pub fn delim_kind(&self) -> DelimKind {
        match self.kind {
            TokenKind::OpenBrace |
            TokenKind::OpenBracket |
            TokenKind::OpenParen => DelimKind::Open,
            TokenKind::CloseBrace |
            TokenKind::CloseBracket |
            TokenKind::CloseParen => DelimKind::Close,
            _ => DelimKind::Wrong
        }
    }

    pub fn is_bool_op(&self) -> bool {
        match self.kind {
            TokenKind::Lt |
            TokenKind::Le |
            TokenKind::EqEq |
            TokenKind::Ne |
            TokenKind::Gt |
            TokenKind::Ge |
            TokenKind::AndAnd |
            TokenKind::OrOr => true,
            _ => false
        }
    }

    pub fn is_binary_op(&self) -> bool {
        match self.kind {
            TokenKind::Plus |
            TokenKind::Minus |
            TokenKind::Star |
            TokenKind::Slash |
            TokenKind::Percent |
            TokenKind::Caret |
            TokenKind::And |
            TokenKind::Or |
            TokenKind::Shl |
            TokenKind::Shr => true,
            _ => false
        }
    }
}

impl PartialEq for Token {
    fn eq(&self, other: &Token) -> bool {
        if self.span != other.span {
            return false;
        }

        return self.kind == other.kind
    }
}

#[derive(Debug, Copy, Clone, Default, PartialEq)]
pub(crate) struct NLSpanInfo {
    #[cfg(feature = "trace")]
    pub traceable_info: TracableInfo,
    //pub recursive_info: RecursiveInfo
}

impl NLSpanInfo {
    #[cfg(feature = "trace")]
    pub fn new() -> Self {
        NLSpanInfo {
            traceable_info: TracableInfo::new()
        }
    }
    #[cfg(not(feature = "trace"))]
    pub fn new() -> Self {
        NLSpanInfo { }
    }
}

pub(crate) type NLSpan<'a> = LocatedSpan<&'a str, NLSpanInfo>;
pub(crate) type IResult<T, U> = nom::IResult<T, U, GreedyError<T>>;

pub(crate) fn nlspan_from(program: &str) -> NLSpan {
    NLSpan::new_extra(program, NLSpanInfo::new() )
}

// impl HasRecursiveInfo for NLSpanInfo {
//     fn get_recursive_info(&self) -> RecursiveInfo {
//         self.recursive_info
//     }
//
//     fn set_recursive_info(mut self, info: RecursiveInfo) -> Self {
//         self.recursive_info = info;
//         self
//     }
// }

#[cfg(feature = "trace")]
impl HasTracableInfo for NLSpanInfo {
    fn get_tracable_info(&self) -> TracableInfo {
        self.traceable_info
    }

    fn set_tracable_info(mut self, info: TracableInfo) -> Self {
        self.traceable_info = info;
        self
    }
}

// impl HasExtraState<()> for NLSpanInfo {
//     fn get_extra_state(&self) -> () {
//         ()
//     }
// }

impl<'a> From<NLSpan<'a>> for Span {
    fn from(nlspan: NLSpan<'a>) -> Self {
        let pos = nlspan.location_offset() as u32;
        let len = nlspan.fragment().len() as u32;
        Span {
            lo: BytePos(pos),
            hi: BytePos(pos + len),
            line: nlspan.location_line()
        }
    }
}

//nom_packrat::storage!(Token);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TokenStream<'a> {
    pub toks: &'a [Token],
    pub start: usize,
    pub end: usize
}

impl<'a> TokenStream<'a> {
    pub fn new(vec: &'a Vec<Token>) -> Self {
        TokenStream {
            toks: vec.as_slice(),
            start: 0,
            end: vec.len()
        }
    }

    pub fn new_with_slice(slice: &'a [Token]) -> Self {
        TokenStream {
            toks: slice,
            start: 0,
            end: slice.len()
        }
    }
}

impl<'a, 'b> Compare<TokenStream<'b>> for TokenStream<'a> {
    fn compare(&self, t: TokenStream<'b>) -> CompareResult {
        let pos = self.iter_elements().zip(
            t.iter_elements()).position(
            |(a, b)| a.kind != b.kind);

        match pos {
            Some(_) => CompareResult::Error,
            None => {
                if self.input_len() >= t.input_len() {
                    CompareResult::Ok
                } else {
                    CompareResult::Incomplete
                }
            }
        }
    }

    fn compare_no_case(&self, t: TokenStream<'b>) -> CompareResult {
        CompareResult::Ok
    }
}

impl<'a> InputLength for TokenStream<'a> {
    fn input_len(&self) -> usize {
        self.toks.len()
    }
}

impl<'a> InputTake for TokenStream<'a> {
    fn take(&self, count: usize) -> Self {
        TokenStream {
            toks: &self.toks[0..count],
            start: 0,
            end: count
        }
    }

    fn take_split(&self, count: usize) -> (Self, Self) {
        let (prefix, suffix) = self.toks.split_at(count);
        let first = TokenStream {
            toks: prefix,
            start: 0,
            end: prefix.len()
        };
        let second = TokenStream {
            toks: suffix,
            start: 0,
            end: suffix.len()
        };
        (second, first)
    }
}

impl InputLength for Token {
    fn input_len(&self) -> usize {
        1
    }
}

impl<'a> Slice<Range<usize>> for TokenStream<'a> {
    fn slice(&self, range: Range<usize>) -> Self {
        TokenStream {
            toks: &self.toks[range.clone()],
            start: self.start + range.start,
            end: self.start + range.end
        }
    }
}

impl<'a> Slice<RangeTo<usize>> for TokenStream<'a> {
    fn slice(&self, range: RangeTo<usize>) -> Self {
        self.slice(0..range.end)
    }
}

impl<'a> Slice<RangeFrom<usize>> for TokenStream<'a> {
    fn slice(&self, range: RangeFrom<usize>) -> Self {
        self.slice(range.start..self.end - self.start)
    }
}

impl<'a> Slice<RangeFull> for TokenStream<'a> {
    fn slice(&self, _: RangeFull) -> Self {
        TokenStream {
            toks: self.toks,
            start: self.start,
            end: self.end
        }
    }
}

impl<'a> InputIter for TokenStream<'a> {
    type Item = &'a Token;
    type Iter = Enumerate<::std::slice::Iter<'a, Token>>;
    type IterElem = ::std::slice::Iter<'a, Token>;

    fn iter_indices(&self) -> Self::Iter {
        self.toks.iter().enumerate()
    }

    fn iter_elements(&self) -> Self::IterElem {
        self.toks.iter()
    }

    fn position<P>(&self, predicate: P) -> Option<usize>
    where
        P: Fn(Self::Item) -> bool,
    {
        self.toks.iter().position(|b| predicate(b))
    }

    fn slice_index(&self, count: usize) -> Option<usize> {
        if self.toks.len() >= count {
            Some(count)
        } else {
            None
        }
    }
}

impl<'a> Index<usize> for TokenStream<'a> {
    type Output = Token;

    fn index(&self, index: usize) -> &Self::Output {
        &self.toks[index]
    }
}