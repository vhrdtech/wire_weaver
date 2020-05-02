use nom::bytes::complete::{tag, escaped, take_while1, take_while, take, take_until};
use nom::branch::{alt};
use nom::sequence::{tuple, terminated, preceded};
use nom::combinator::{peek, not, map, cut};
use nom_packrat::{packrat_parser};
use nom_tracable::{tracable_parser};
use super::token::{NLSpan, IResult, Token, TokenKind, BoolOpToken, BinOpToken, UnaryOpToken};
use crate::lexer::token::{DelimToken, LitKind, Lit, IdentToken, Span};
use nom::error::context;
use nom::character::complete::{alphanumeric1, one_of, char as nomchar, alphanumeric0, alpha1};
use nom::character::is_alphanumeric;
use nom::multi::many0;

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
    Ok( (s, Token { kind: TokenKind::Assign, span: nls.into() } ) )
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

/// Eats "@"
#[tracable_parser]
#[packrat_parser]
pub(crate) fn at_punct(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("@")(s)?;
    Ok( (s, Token { kind: TokenKind::At, span: nls.into() } ) )
}

/// Eats "." if not followed by another dot
#[tracable_parser]
#[packrat_parser]
pub(crate) fn dot_punct(s: NLSpan) -> IResult<NLSpan, Token> {
    let (_, _) = peek(not(tag("..")))(s)?;
    let (s, nls) = tag(".")(s)?;
    Ok( (s, Token { kind: TokenKind::Dot, span: nls.into() } ) )
}

/// Eats ".."
#[tracable_parser]
#[packrat_parser]
pub(crate) fn dotdot_punct(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("..")(s)?;
    Ok( (s, Token { kind: TokenKind::Dot, span: nls.into() } ) )
}

/// Eats ","
#[tracable_parser]
#[packrat_parser]
pub(crate) fn comma_punct(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag(",")(s)?;
    Ok( (s, Token { kind: TokenKind::Comma, span: nls.into() } ) )
}

/// Eats ";"
#[tracable_parser]
#[packrat_parser]
pub(crate) fn semicolon_punct(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag(";")(s)?;
    Ok( (s, Token { kind: TokenKind::Semicolon, span: nls.into() } ) )
}

/// Eats ":"
#[tracable_parser]
#[packrat_parser]
pub(crate) fn colon_punct(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag(":")(s)?;
    Ok( (s, Token { kind: TokenKind::Colon, span: nls.into() } ) )
}

/// Eats and punct character('s)
#[tracable_parser]
#[packrat_parser]
pub(crate) fn punct(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, t) = alt((
        at_punct,
        dotdot_punct,
        dot_punct,
        comma_punct,
        semicolon_punct,
        colon_punct
    ))(s)?;
    Ok( (s, t) )
}

/// Eats "("
#[tracable_parser]
#[packrat_parser]
pub(crate) fn open_paren_delim(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("(")(s)?;
    Ok( (s, Token { kind: TokenKind::OpenDelim(DelimToken::Paren), span: nls.into() } ) )
}

/// Eats ")"
#[tracable_parser]
#[packrat_parser]
pub(crate) fn close_paren_delim(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag(")")(s)?;
    Ok( (s, Token { kind: TokenKind::CloseDelim(DelimToken::Paren), span: nls.into() } ) )
}

/// Eats "["
#[tracable_parser]
#[packrat_parser]
pub(crate) fn open_bracket_delim(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("[")(s)?;
    Ok( (s, Token { kind: TokenKind::OpenDelim(DelimToken::Bracket), span: nls.into() } ) )
}

/// Eats "]"
#[tracable_parser]
#[packrat_parser]
pub(crate) fn close_bracket_delim(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("]")(s)?;
    Ok( (s, Token { kind: TokenKind::CloseDelim(DelimToken::Bracket), span: nls.into() } ) )
}

/// Eats "{"
#[tracable_parser]
#[packrat_parser]
pub(crate) fn open_brace_delim(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("{")(s)?;
    Ok( (s, Token { kind: TokenKind::OpenDelim(DelimToken::Brace), span: nls.into() } ) )
}

/// Eats "}"
#[tracable_parser]
#[packrat_parser]
pub(crate) fn close_brace_delim(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("}")(s)?;
    Ok( (s, Token { kind: TokenKind::CloseDelim(DelimToken::Brace), span: nls.into() } ) )
}

/// Eats delimiter
#[tracable_parser]
#[packrat_parser]
pub(crate) fn delim(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, t) = alt((
        open_paren_delim,
        close_paren_delim,
        open_bracket_delim,
        close_bracket_delim,
        open_brace_delim,
        close_brace_delim
    ))(s)?;
    Ok ( (s, t) )
}

fn str_lit_inside(s: NLSpan) -> IResult<NLSpan, NLSpan> {
    let (s, nls) = escaped(
        alphanumeric1,
        '\\',
        one_of(r#""n\"#))(s)?;
    Ok( (s, nls) )
}

/// Eats a string in `""`
#[tracable_parser]
#[packrat_parser]
pub(crate) fn str_lit(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = context("str_lit",
        preceded(nomchar('\"'), cut(terminated(str_lit_inside, nomchar('\"'))))
    )(s)?;
    Ok( (s, Token { kind: TokenKind::Literal(Lit{kind: LitKind::Str}), span: nls.into() } ) )
}

fn eat_ident(s: NLSpan) -> IResult<NLSpan, NLSpan> {
    let (s, ident) = alt((
        preceded(peek(tag("_")), take_while1(|c| c == '_' || is_alphanumeric(c as u8))),
        preceded(peek(alpha1), alphanumeric0)
    ))(s)?;
    Ok( (s, ident) )
}

/// Eats an ident
#[tracable_parser]
#[packrat_parser]
pub(crate) fn ident(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, ident) = eat_ident(s)?;
    match *ident.fragment() {
        "if" => Ok( (s, Token { kind: TokenKind::Ident(IdentToken::If), span: ident.into() } ) ),
        _ => Ok( (s, Token { kind: TokenKind::Ident(IdentToken::Normal), span: ident.into() } ) )
    }
}

/// Eats single line comment.
#[tracable_parser]
#[packrat_parser]
pub(crate) fn comment(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, cs) = tag("//")(s)?;
    let (s, cb) = take_while(|c| c != '\n')(s)?;
    Ok( (s, Token { kind: TokenKind::Comment, span: Span::from(cs) + Span::from(cb) }))
}

/// Eats nested block comments. `/*` must have corresponding `*/`.
#[tracable_parser]
#[packrat_parser]
pub(crate) fn blockcomment(s: NLSpan) -> IResult<NLSpan, Token> {
    let (mut rs, bc) = tag("/*")(s)?;
    let mut depth = 1_usize;
    let mut bc_span = Span::from(bc);

    let mut prev_c: Option<char> = None;
    while depth >= 1 {
        // Eat one symbol at a time, error out on EOF if inner block comment is not closed.
        let (s, bc) = context("block comment",
                              cut(take(1_usize)))(rs)?;
        bc_span = bc_span + Span::from(bc);
        rs = s;
        let c = bc.fragment().chars().nth(0).unwrap(); // safe, cut() will return on EOF
        if let Some(p) = prev_c {
            if p == '*' && c == '/' {
                depth = depth - 1;
            } else if p == '/' && c == '*' {
                depth = depth + 1;
            }
            prev_c = Some(c);
        } else {
            prev_c = Some(c);
            continue;
        }
    }

    Ok( (rs, Token { kind: TokenKind::Comment, span: bc_span }))
}

/// Eats whitespace characters.
/// See [librustc_lexer](https://github.com/rust-lang/rust/blob/master/src/librustc_lexer/src/lib.rs)
#[tracable_parser]
#[packrat_parser]
pub(crate) fn whitespace(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = take_while1(
        |c| " \n\t\r\u{000B}\u{000C}\u{0085}\u{200E}\u{200F}\u{2028}\u{2029}".contains(c) )(s)?;
    Ok( (s, Token { kind: TokenKind::Whitespace, span: nls.into() } ) )
}

#[tracable_parser]
#[packrat_parser]
pub(crate) fn any_token(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, t) = alt((
        whitespace,
        comment,
        blockcomment,
        expr_op,
        punct,
        delim,
        ident,
        str_lit,
    ))(s)?;
    Ok ( (s, t) )
}