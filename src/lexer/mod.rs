use nom_greedyerror::{GreedyError, Position};
use nom_tracable::{tracable_parser, TracableInfo, HasTracableInfo, histogram, cumulative_histogram};
use nom_locate::{LocatedSpan, position};
use nom::bytes::complete::{take_until, tag};
use nom::branch::alt;
use nom::sequence::tuple;
use nom::character::complete::{alpha1, digit1};
use std::fmt;
use nom::lib::std::fmt::Formatter;
use nom::multi::many1;
use crate::lexer::TokenKind::BinOp;
use nom::combinator::{peek, not};

/// Binary operation token
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BinOpToken {
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
    Shr
}

/// Boolean operation token
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BoolOpToken {
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
}

/// Unary operation token
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UnaryOpToken {
    /// "~"
    Tilde,
    /// "!"
    Excl,
}

/// `()` or `[]` or `{}`
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DelimToken {
    /// `(` or `)`
    Paren,
    /// `[` or `]`
    Bracket,
    /// `{` or `}`
    Brace,
    // An empty delimiter
    // NoDelim
}

/// Bool / Byte / Char / Integer / Float / Str / ByteStr / Err
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LitKind {
    Bool,
    Byte,
    Char,
    Integer,
    Float,
    Str,
    //StrRaw(u16),
    ByteStr,
    //ByteStrRaw(u16),
    Err
}

/// LitKind + Symbol + Optional suffix
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lit {
    pub kind: LitKind,
    //pub symbol: Symbol,
    //pub suffix: Option<Symbol>
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TokenKind {
    // Expression operators
    /// "="
    Eq,
    /// `!` `~`
    UnaryOp(UnaryOpToken),
    /// `<` `<=` `==` `!=` `>=` `&&` `||`
    BoolOp(BoolOpToken),
    /// `+` `-` `*` `/` `%` `^` `&` `|` `<<` `>>`
    BinOp(BinOpToken),
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
    /// An opening delimiter `{` or `(` or `[`
    OpenDelim(DelimToken),
    /// A closing delimiter `}` or `)` or `]`
    CloseDelim(DelimToken),

    // Literals
    Literal(Lit),

    Ident(/*name*/),

    /// Any whitespace
    Whitespace,
    Comment,

    Unkown(/*name*/),

    Eof,
}

// #[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Ord)]
// pub enum LiteralKind {
//     /// "127_u8", "0o100", "0b129i99"
//     Int { base: Base },
//     /// "12.34f32", "56f16"
//     Float { base: Base },
//     /// "'a'", "'\\'", "'''"
//     Char,
//     /// "b'a'", "b'\\'", "b'''"
//     //Byte,
//     /// ""abc""
//     Str,
//     /// "b"abc""
//     //ByteStr,
//     /// "r"abc""
//     //RawStr,
//     /// "br"abc""
//     //RawByteStr
// }
//
// /// Base of numeric literal
// #[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Ord)]
// pub enum Base {
//     /// 0b prefix
//     Binary,
//     /// 0o prefix
//     Octal,
//     /// 0x prefix
//     Hexadecimal,
//     /// Without prefix
//     Decimal
// }

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BytePos(pub u32);

/// Token span [lo, hi)
#[derive(Clone, Copy, PartialEq)]
pub struct Span {
    lo: BytePos,
    hi: BytePos
}

impl fmt::Debug for Span {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "bytes[{}..{})", self.lo.0, self.hi.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span
}

#[derive(Debug, Copy, Clone, Default, PartialEq)]
pub struct NLSpanInfo {
    #[cfg(feature = "trace")]
    pub traceable_info: TracableInfo,
    // pub recursive_info: RecursiveInfo
}

#[cfg(feature = "trace")]
impl NLSpanInfo {
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

pub type NLSpan<'a> = LocatedSpan<&'a str, NLSpanInfo>;
pub type IResult<T, U> = nom::IResult<T, U, GreedyError<T>>;

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

impl<'a> From<NLSpan<'a>> for Span {
    fn from(nlspan: NLSpan<'a>) -> Self {
        let pos = nlspan.position() as u32;
        let len = nlspan.fragment().len() as u32;
        Span {
            lo: BytePos(pos),
            hi: BytePos(pos + len)
        }
    }
}
//
// #[tracable_parser]
// fn parse1(s: Span) -> IResult<Span, ()> {
//     let (s, _) = take_until("foo")(s)?;
//     Ok((s, ()))
// }
//
// #[tracable_parser]
// fn parse_smth(s: Span) -> IResult<Span, Token> {
//     let (s, _) = parse1(s)?;
//     let (s, pos) = position(s)?;
//     let (s, foo) = tag("foo")(s)?;
//
//     Ok((
//         s,
//         Token {
//             position: pos,
//             foo: foo.fragment()
//         }
//         ))
// }

// #[tracable_parser]
// fn parser(s: Span) -> IResult<Span, (Span, Span, Span)> {
//     alt((
//         tuple((alpha1, digit1, alpha1)),
//         tuple((digit1, alpha1, digit1)),
//     ))(s)
// }

#[tracable_parser]
fn equal_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("==")(s)?;
    Ok( (s, Token { kind: TokenKind::BoolOp(BoolOpToken::EqEq), span: nls.into() } ) )
}

#[tracable_parser]
fn not_equal_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("!=")(s)?;
    Ok( (s, Token { kind: TokenKind::BoolOp(BoolOpToken::Ne), span: nls.into() } ) )
}

#[tracable_parser]
fn le_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("<=")(s)?;
    Ok( (s, Token { kind: TokenKind::BoolOp(BoolOpToken::Le), span: nls.into() } ) )
}

#[tracable_parser]
fn lt_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, _) = peek(not(tag("<=")))(s)?;
    let (s, nls) = tag("<")(s)?;
    Ok( (s, Token { kind: TokenKind::BoolOp(BoolOpToken::Lt), span: nls.into() } ) )
}

#[tracable_parser]
fn assign_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("=")(s)?;
    Ok( (s, Token { kind: TokenKind::Eq, span: nls.into() } ) )
}

#[tracable_parser]
fn ge_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag(">=")(s)?;
    Ok( (s, Token { kind: TokenKind::BoolOp(BoolOpToken::Ge), span: nls.into() } ) )
}

#[tracable_parser]
fn gt_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag(">")(s)?;
    Ok( (s, Token { kind: TokenKind::BoolOp(BoolOpToken::Gt), span: nls.into() } ) )
}

#[tracable_parser]
fn andand_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("&&")(s)?;
    Ok( (s, Token { kind: TokenKind::BoolOp(BoolOpToken::AndAnd), span: nls.into() } ) )
}

#[tracable_parser]
fn oror_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("||")(s)?;
    Ok( (s, Token { kind: TokenKind::BoolOp(BoolOpToken::OrOr), span: nls.into() } ) )
}

#[tracable_parser]
fn tilde_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("~")(s)?;
    Ok( (s, Token { kind: TokenKind::UnaryOp(UnaryOpToken::Tilde), span: nls.into() } ) )
}

#[tracable_parser]
fn plus_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("+")(s)?;
    Ok( (s, Token { kind: TokenKind::BinOp(BinOpToken::Plus), span: nls.into() } ) )
}

#[tracable_parser]
fn minus_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("-")(s)?;
    Ok( (s, Token { kind: TokenKind::BinOp(BinOpToken::Minus), span: nls.into() } ) )
}

#[tracable_parser]
fn star_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("*")(s)?;
    Ok( (s, Token { kind: TokenKind::BinOp(BinOpToken::Star), span: nls.into() } ) )
}

#[tracable_parser]
fn slash_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("/")(s)?;
    Ok( (s, Token { kind: TokenKind::BinOp(BinOpToken::Slash), span: nls.into() } ) )
}

#[tracable_parser]
fn percent_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("%")(s)?;
    Ok( (s, Token { kind: TokenKind::BinOp(BinOpToken::Percent), span: nls.into() } ) )
}

#[tracable_parser]
fn caret_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("^")(s)?;
    Ok( (s, Token { kind: TokenKind::BinOp(BinOpToken::Caret), span: nls.into() } ) )
}

#[tracable_parser]
fn and_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("&")(s)?;
    Ok( (s, Token { kind: TokenKind::BinOp(BinOpToken::And), span: nls.into() } ) )
}

#[tracable_parser]
fn or_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("|")(s)?;
    Ok( (s, Token { kind: TokenKind::BinOp(BinOpToken::Or), span: nls.into() } ) )
}

#[tracable_parser]
fn shl_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("<<")(s)?;
    Ok( (s, Token { kind: TokenKind::BinOp(BinOpToken::Shl), span: nls.into() } ) )
}

#[tracable_parser]
fn shr_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag(">>")(s)?;
    Ok( (s, Token { kind: TokenKind::BinOp(BinOpToken::Shr), span: nls.into() } ) )
}

#[tracable_parser]
fn bin_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, t) = alt((
        plus_op,
        minus_op,
        star_op,
        slash_op,
        percent_op,
        caret_op,
        and_op,
        or_op,
        shl_op,
        shr_op
        ))(s)?;
    Ok( (s, t) )
}

#[tracable_parser]
fn bool_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, t) = alt((
        lt_op,
        le_op
    ))(s)?;
    Ok( (s, t) )
}

pub fn lexer_play() {
    // println!("lexer_play():");
    // let info = TracableInfo::new().parser_width(64).fold("abc012abc");
    // let input = Span::new_extra("abc012abc", SpanInfo { tracable_info: info });
    // let output = parser(input);
    // println!("{:#?}", output);
    //
    // histogram();
    // cumulative_histogram();

    let input = NLSpan::new_extra("<=<+++", NLSpanInfo::new() );
    let output = many1(bool_op)(input);
    println!("{:?}", output);

    histogram();
    cumulative_histogram();
}