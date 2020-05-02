use nom::bytes::complete::{tag};
use nom::branch::{alt};
use nom::sequence::{tuple};
use nom::combinator::{peek, not};
use nom_packrat::{packrat_parser};
use nom_tracable::{tracable_parser};
use super::token::{NLSpan, IResult, Token, TokenKind, BoolOpToken, BinOpToken, UnaryOpToken};

/// Eats "=="
#[tracable_parser]
#[packrat_parser]
pub(crate) fn equal_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("==")(s)?;
    Ok( (s, Token { kind: TokenKind::BoolOp(BoolOpToken::EqEq), span: nls.into() } ) )
}

/// Eats "!="
#[tracable_parser]
#[packrat_parser]
pub(crate) fn not_equal_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("!=")(s)?;
    Ok( (s, Token { kind: TokenKind::BoolOp(BoolOpToken::Ne), span: nls.into() } ) )
}

/// Eats "<="
#[tracable_parser]
#[packrat_parser]
pub(crate) fn le_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("<=")(s)?;
    Ok( (s, Token { kind: TokenKind::BoolOp(BoolOpToken::Le), span: nls.into() } ) )
}

/// Eats "<" if not followed by `<` or `=`
#[tracable_parser]
#[packrat_parser]
pub(crate) fn lt_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (_, _) = peek(not(tag("<=")))(s)?;
    let (_, _) = peek(not(tag("<<")))(s)?;
    let (s, nls) = tag("<")(s)?;
    Ok( (s, Token { kind: TokenKind::BoolOp(BoolOpToken::Lt), span: nls.into() } ) )
}

/// Eats "=" if not followed by `=`
#[tracable_parser]
#[packrat_parser]
pub(crate) fn assign_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (_, _) = peek(not(tag("==")))(s)?;
    let (s, nls) = tag("=")(s)?;
    Ok( (s, Token { kind: TokenKind::Eq, span: nls.into() } ) )
}

/// Eats ">="
#[tracable_parser]
#[packrat_parser]
pub(crate) fn ge_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag(">=")(s)?;
    Ok( (s, Token { kind: TokenKind::BoolOp(BoolOpToken::Ge), span: nls.into() } ) )
}

/// Eats ">" if not followed by `>` or `=`
#[tracable_parser]
#[packrat_parser]
pub(crate) fn gt_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (_, _) = peek(not(tag(">>")))(s)?;
    let (_, _) = peek(not(tag(">=")))(s)?;
    let (s, nls) = tag(">")(s)?;
    Ok( (s, Token { kind: TokenKind::BoolOp(BoolOpToken::Gt), span: nls.into() } ) )
}

/// Eats "&&"
#[tracable_parser]
#[packrat_parser]
pub(crate) fn andand_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("&&")(s)?;
    Ok( (s, Token { kind: TokenKind::BoolOp(BoolOpToken::AndAnd), span: nls.into() } ) )
}

/// Eats "||"
#[tracable_parser]
#[packrat_parser]
pub(crate) fn oror_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("||")(s)?;
    Ok( (s, Token { kind: TokenKind::BoolOp(BoolOpToken::OrOr), span: nls.into() } ) )
}

/// Eats "~"
#[tracable_parser]
#[packrat_parser]
pub(crate) fn tilde_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("~")(s)?;
    Ok( (s, Token { kind: TokenKind::UnaryOp(UnaryOpToken::Tilde), span: nls.into() } ) )
}

/// Eats "!"
#[tracable_parser]
#[packrat_parser]
pub(crate) fn excl_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("!")(s)?;
    Ok( (s, Token { kind: TokenKind::UnaryOp(UnaryOpToken::Excl), span: nls.into() } ) )
}

/// Eats "+"
#[tracable_parser]
#[packrat_parser]
pub(crate) fn plus_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("+")(s)?;
    Ok( (s, Token { kind: TokenKind::BinOp(BinOpToken::Plus), span: nls.into() } ) )
}

/// Eats "-"
#[tracable_parser]
#[packrat_parser]
pub(crate) fn minus_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("-")(s)?;
    Ok( (s, Token { kind: TokenKind::BinOp(BinOpToken::Minus), span: nls.into() } ) )
}

/// Eats "*"
#[tracable_parser]
#[packrat_parser]
pub(crate) fn star_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("*")(s)?;
    Ok( (s, Token { kind: TokenKind::BinOp(BinOpToken::Star), span: nls.into() } ) )
}

/// Eats "/"
#[tracable_parser]
#[packrat_parser]
pub(crate) fn slash_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("/")(s)?;
    Ok( (s, Token { kind: TokenKind::BinOp(BinOpToken::Slash), span: nls.into() } ) )
}

/// Eats "%"
#[tracable_parser]
#[packrat_parser]
pub(crate) fn percent_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("%")(s)?;
    Ok( (s, Token { kind: TokenKind::BinOp(BinOpToken::Percent), span: nls.into() } ) )
}

/// Eats "^"
#[tracable_parser]
#[packrat_parser]
pub(crate) fn caret_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("^")(s)?;
    Ok( (s, Token { kind: TokenKind::BinOp(BinOpToken::Caret), span: nls.into() } ) )
}

/// Eats "&" if not followed by `&`
#[tracable_parser]
#[packrat_parser]
pub(crate) fn and_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (_, _) = peek(not(tag("&&")))(s)?;
    let (s, nls) = tag("&")(s)?;
    Ok( (s, Token { kind: TokenKind::BinOp(BinOpToken::And), span: nls.into() } ) )
}

/// Eats "|" if not followed by `|`
#[tracable_parser]
#[packrat_parser]
pub(crate) fn or_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (_, _) = peek(not(tag("||")))(s)?;
    let (s, nls) = tag("|")(s)?;
    Ok( (s, Token { kind: TokenKind::BinOp(BinOpToken::Or), span: nls.into() } ) )
}

/// Eats "<<"
#[tracable_parser]
#[packrat_parser]
pub(crate) fn shl_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("<<")(s)?;
    Ok( (s, Token { kind: TokenKind::BinOp(BinOpToken::Shl), span: nls.into() } ) )
}

/// Eats ">>"
#[tracable_parser]
#[packrat_parser]
pub(crate) fn shr_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag(">>")(s)?;
    Ok( (s, Token { kind: TokenKind::BinOp(BinOpToken::Shr), span: nls.into() } ) )
}

/// Eats binary operator
#[tracable_parser]
#[packrat_parser]
pub(crate) fn bin_op(s: NLSpan) -> IResult<NLSpan, Token> {
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

/// Eats boolean operator
#[tracable_parser]
#[packrat_parser]
pub(crate) fn bool_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, t) = alt((
        equal_op,
        not_equal_op,
        ge_op,
        gt_op,
        lt_op,
        le_op,
        andand_op,
        oror_op
    ))(s)?;
    Ok( (s, t) )
}

/// Eats unary operator
#[tracable_parser]
#[packrat_parser]
pub(crate) fn unary_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, t) = alt((
        tilde_op,
        excl_op,
    ))(s)?;
    Ok( (s, t) )
}

/// Eats one expression operator
#[tracable_parser]
#[packrat_parser]
pub(crate) fn expr_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, t) = alt((
        bin_op,
        bool_op,
        unary_op
    ))(s)?;
    Ok( (s, t) )
}