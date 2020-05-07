use nom::bytes::complete::{tag, escaped, take_while1, take_while, take, take_until};
use nom::branch::{alt};
use nom::sequence::{tuple, terminated, preceded};
use nom::combinator::{peek, not, map, cut};
//use nom_packrat::{packrat_parser};
use nom_tracable::{tracable_parser};
use super::token::{NLSpan, IResult, Token, TokenKind};
use crate::lexer::token::{Lit, IdentKind, Span, Base, LiteralKind, DelimKind, TreeIndent};
use nom::error::{context, ParseError};
use nom::character::complete::{alphanumeric1, one_of, char as nomchar, alphanumeric0, alpha1};
use nom::character::is_alphanumeric;
use nom::multi::{many0, many1};
use nom_greedyerror::GreedyError;
use nom::error::ErrorKind::Space;

/// Eats "=="
#[tracable_parser]
//#[packrat_parser]
pub(crate) fn equal_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("==")(s)?;
    Ok( (s, Token { kind: TokenKind::EqEq, span: nls.into() } ) )
}

/// Eats "!="
#[tracable_parser]
//#[packrat_parser]
pub(crate) fn not_equal_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("!=")(s)?;
    Ok( (s, Token { kind: TokenKind::Ne, span: nls.into() } ) )
}

/// Eats "<="
#[tracable_parser]
//#[packrat_parser]
pub(crate) fn le_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("<=")(s)?;
    Ok( (s, Token { kind: TokenKind::Le, span: nls.into() } ) )
}

/// Eats "<" if not followed by `<` or `=`
#[tracable_parser]
//#[packrat_parser]
pub(crate) fn lt_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (_, _) = peek(not(tag("<=")))(s)?;
    let (_, _) = peek(not(tag("<<")))(s)?;
    let (s, nls) = tag("<")(s)?;
    Ok( (s, Token { kind: TokenKind::Lt, span: nls.into() } ) )
}

/// Eats "=" if not followed by `=`
#[tracable_parser]
//#[packrat_parser]
pub(crate) fn assign_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (_, _) = peek(not(tag("==")))(s)?;
    let (s, nls) = tag("=")(s)?;
    Ok( (s, Token { kind: TokenKind::Assign, span: nls.into() } ) )
}

/// Eats ">="
#[tracable_parser]
//#[packrat_parser]
pub(crate) fn ge_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag(">=")(s)?;
    Ok( (s, Token { kind: TokenKind::Ge, span: nls.into() } ) )
}

/// Eats ">" if not followed by `>` or `=`
#[tracable_parser]
//#[packrat_parser]
pub(crate) fn gt_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (_, _) = peek(not(tag(">>")))(s)?;
    let (_, _) = peek(not(tag(">=")))(s)?;
    let (s, nls) = tag(">")(s)?;
    Ok( (s, Token { kind: TokenKind::Gt, span: nls.into() } ) )
}

/// Eats "&&"
#[tracable_parser]
//#[packrat_parser]
pub(crate) fn andand_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("&&")(s)?;
    Ok( (s, Token { kind: TokenKind::AndAnd, span: nls.into() } ) )
}

/// Eats "||"
#[tracable_parser]
//#[packrat_parser]
pub(crate) fn oror_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("||")(s)?;
    Ok( (s, Token { kind: TokenKind::OrOr, span: nls.into() } ) )
}

/// Eats "~"
#[tracable_parser]
//#[packrat_parser]
pub(crate) fn tilde_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("~")(s)?;
    Ok( (s, Token { kind: TokenKind::Tilde, span: nls.into() } ) )
}

/// Eats "!"
#[tracable_parser]
//#[packrat_parser]
pub(crate) fn excl_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("!")(s)?;
    Ok( (s, Token { kind: TokenKind::Excl, span: nls.into() } ) )
}

/// Eats "+"
#[tracable_parser]
//#[packrat_parser]
pub(crate) fn plus_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("+")(s)?;
    Ok( (s, Token { kind: TokenKind::Plus, span: nls.into() } ) )
}

/// Eats "-"
#[tracable_parser]
//#[packrat_parser]
pub(crate) fn minus_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("-")(s)?;
    Ok( (s, Token { kind: TokenKind::Minus, span: nls.into() } ) )
}

/// Eats "*"
#[tracable_parser]
//#[packrat_parser]
pub(crate) fn star_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("*")(s)?;
    Ok( (s, Token { kind: TokenKind::Star, span: nls.into() } ) )
}

/// Eats "/"
#[tracable_parser]
//#[packrat_parser]
pub(crate) fn slash_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("/")(s)?;
    Ok( (s, Token { kind: TokenKind::Slash, span: nls.into() } ) )
}

/// Eats "%"
#[tracable_parser]
//#[packrat_parser]
pub(crate) fn percent_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("%")(s)?;
    Ok( (s, Token { kind: TokenKind::Percent, span: nls.into() } ) )
}

/// Eats "^"
#[tracable_parser]
//#[packrat_parser]
pub(crate) fn caret_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("^")(s)?;
    Ok( (s, Token { kind: TokenKind::Caret, span: nls.into() } ) )
}

/// Eats "&" if not followed by `&`
#[tracable_parser]
//#[packrat_parser]
pub(crate) fn and_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (_, _) = peek(not(tag("&&")))(s)?;
    let (s, nls) = tag("&")(s)?;
    Ok( (s, Token { kind: TokenKind::And, span: nls.into() } ) )
}

/// Eats "|" if not followed by `|`
#[tracable_parser]
//#[packrat_parser]
pub(crate) fn or_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (_, _) = peek(not(tag("||")))(s)?;
    let (s, nls) = tag("|")(s)?;
    Ok( (s, Token { kind: TokenKind::Or, span: nls.into() } ) )
}

/// Eats "<<"
#[tracable_parser]
//#[packrat_parser]
pub(crate) fn shl_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("<<")(s)?;
    Ok( (s, Token { kind: TokenKind::Shl, span: nls.into() } ) )
}

/// Eats ">>"
#[tracable_parser]
//#[packrat_parser]
pub(crate) fn shr_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag(">>")(s)?;
    Ok( (s, Token { kind: TokenKind::Shr, span: nls.into() } ) )
}

/// Eats binary operator
#[tracable_parser]
//#[packrat_parser]
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
//#[packrat_parser]
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
//#[packrat_parser]
pub(crate) fn unary_op(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, t) = alt((
        tilde_op,
        excl_op,
    ))(s)?;
    Ok( (s, t) )
}

/// Eats one expression operator
#[tracable_parser]
//#[packrat_parser]
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
//#[packrat_parser]
pub(crate) fn at_punct(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("@")(s)?;
    Ok( (s, Token { kind: TokenKind::At, span: nls.into() } ) )
}

/// Eats "." if not followed by another dot
#[tracable_parser]
//#[packrat_parser]
pub(crate) fn dot_punct(s: NLSpan) -> IResult<NLSpan, Token> {
    let (_, _) = peek(not(tag("..")))(s)?;
    let (s, nls) = tag(".")(s)?;
    Ok( (s, Token { kind: TokenKind::Dot, span: nls.into() } ) )
}

/// Eats ".."
#[tracable_parser]
//#[packrat_parser]
pub(crate) fn dotdot_punct(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("..")(s)?;
    Ok( (s, Token { kind: TokenKind::Dot, span: nls.into() } ) )
}

/// Eats ","
#[tracable_parser]
//#[packrat_parser]
pub(crate) fn comma_punct(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag(",")(s)?;
    Ok( (s, Token { kind: TokenKind::Comma, span: nls.into() } ) )
}

/// Eats ";"
#[tracable_parser]
//#[packrat_parser]
pub(crate) fn semicolon_punct(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag(";")(s)?;
    Ok( (s, Token { kind: TokenKind::Semicolon, span: nls.into() } ) )
}

/// Eats ":"
#[tracable_parser]
//#[packrat_parser]
pub(crate) fn colon_punct(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag(":")(s)?;
    Ok( (s, Token { kind: TokenKind::Colon, span: nls.into() } ) )
}

/// Eats and punct character('s)
#[tracable_parser]
//#[packrat_parser]
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
//#[packrat_parser]
pub(crate) fn open_paren_delim(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("(")(s)?;
    Ok( (s, Token { kind: TokenKind::OpenParen, span: nls.into() } ) )
}

/// Eats ")"
#[tracable_parser]
//#[packrat_parser]
pub(crate) fn close_paren_delim(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag(")")(s)?;
    Ok( (s, Token { kind: TokenKind::CloseParen, span: nls.into() } ) )
}

/// Eats "["
#[tracable_parser]
//#[packrat_parser]
pub(crate) fn open_bracket_delim(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("[")(s)?;
    Ok( (s, Token { kind: TokenKind::OpenBracket, span: nls.into() } ) )
}

/// Eats "]"
#[tracable_parser]
//#[packrat_parser]
pub(crate) fn close_bracket_delim(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("]")(s)?;
    Ok( (s, Token { kind: TokenKind::CloseBracket, span: nls.into() } ) )
}

/// Eats "{"
#[tracable_parser]
//#[packrat_parser]
pub(crate) fn open_brace_delim(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("{")(s)?;
    Ok( (s, Token { kind: TokenKind::OpenBrace, span: nls.into() } ) )
}

/// Eats "}"
#[tracable_parser]
//#[packrat_parser]
pub(crate) fn close_brace_delim(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = tag("}")(s)?;
    Ok( (s, Token { kind: TokenKind::CloseBrace, span: nls.into() } ) )
}

/// Eats delimiter
#[tracable_parser]
//#[packrat_parser]
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
//#[packrat_parser]
pub(crate) fn str_lit(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = context("str_lit",
        preceded(nomchar('\"'), cut(terminated(str_lit_inside, nomchar('\"'))))
    )(s)?;
    Ok( (s, Token { kind: TokenKind::Literal(Lit{kind: LiteralKind::Str}), span: nls.into() } ) )
}


/// Eats bool literal
#[tracable_parser]
//#[packrat_parser]
pub(crate) fn bool_lit(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s2, l) = alt((
        tag("true"),
        tag("false")
    ))(s)?;
    let next: IResult<NLSpan, NLSpan> = peek(take(1_usize))(s2);
    if next.is_ok() {
        let ns = next.unwrap().1;
        let c = ns.fragment().chars().nth(0).unwrap();
        if c.is_ascii_alphanumeric() || c == '_' {
            return Err(nom::Err::Error(GreedyError::from_error_kind(ns, nom::error::ErrorKind::Char)));
        }
    }
    Ok( (s2, Token { kind: TokenKind::Literal(Lit{kind: LiteralKind::Bool}), span: l.into() }) )
}

/// Eats a number
pub(crate) fn eat_number(s: NLSpan) -> IResult<NLSpan, (Base, Span)> {
    let (s1, fd) = context("eat_number/1", take(1_usize))(s)?;
    let fdc = fd.fragment().chars().nth(0).unwrap();
    if !fdc.is_numeric() {
        return Err(nom::Err::Error(GreedyError::from_error_kind(s, nom::error::ErrorKind::Digit)));
    }
    let sdr: IResult<NLSpan, NLSpan> = take(1_usize)(s1);
    if sdr.is_err() { // Only 1 char left and it is a digit.
        return Ok( (s1, (Base::Decimal, Span::from(fd) ) ) )
    }
    let (s2, sd) = sdr.unwrap();
    let sdc = sd.fragment().chars().nth(0).unwrap();
    let mut base = Base::Decimal;
    if fdc == '0' {
        match sdc {
            'b' => { base = Base::Binary; }
            'o' => { base = Base::Octal; }
            'x' => { base = Base::Hexadecimal; }
            '0'..='9' | '_' | '.' | 'e' | 'E' => {}
            _ => { // Eat just zero, but this is probably will cause an error later.
                println!("here i am");
                return Ok( (s1, (Base::Decimal, Span::from(fd) ) ) )
            }
        }
    } else {
        if !(sdc.is_numeric() || sdc == '_') {
            // First char was 1-9 and second char is incorrect, eat first.
            return Ok( (s1, (Base::Decimal, Span::from(fd) ) ) )
        }
    }
    // Eat the rest of the digits, including possible `_` at the end.
    let (s3, rd) = take_while(|c: char| c.is_numeric() || c == '_')(s2)?;
    let number_span = Span::from(fd) + Span::from(sd) + Span::from(rd);
    Ok( (s3, (base, number_span) ) )
}

/// Eats a number and then a suffix, if there is one
#[tracable_parser]
//#[packrat_parser]
pub(crate) fn number_and_suffix_lit(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, (base, number_sp) ) = context("eat_number", eat_number)(s)?;
    let possible_suffix = eat_ident(s);
    if possible_suffix.is_ok() {
        let (s, suffix) = possible_suffix.unwrap();
        let span = Span::from(suffix) + number_sp;
        return Ok( (s, Token{ kind: TokenKind::Literal(Lit{kind: LiteralKind::Int{base}}), span }) );
    }
    Ok( (s, Token{ kind: TokenKind::Literal(Lit{kind: LiteralKind::Int{base}}), span: number_sp }) )
}

/// Returns `true` if `c` is valid as a non-first character of an identifier.
/// Taken from `librustc_lexer`
fn is_id_continue(c: char) -> bool {
    ('a' <= c && c <= 'z')
        || ('A' <= c && c <= 'Z')
        || ('0' <= c && c <= '9')
        || c == '_'
}

fn eat_ident(s: NLSpan) -> IResult<NLSpan, NLSpan> {
    let (s, ident) = alt((
        preceded(peek(tag("_")), take_while1(|c| is_id_continue(c))),
        preceded(peek(alpha1), take_while1(|c| is_id_continue(c)))
    ))(s)?;
    Ok( (s, ident) )
}

/// Eats an ident
#[tracable_parser]
//#[packrat_parser]
pub(crate) fn ident(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, ident) = eat_ident(s)?;
    match *ident.fragment() {
        "if" => Ok( (s, Token { kind: TokenKind::Ident(IdentKind::If), span: ident.into() } ) ),
        _ => Ok( (s, Token { kind: TokenKind::Ident(IdentKind::Normal), span: ident.into() } ) )
    }
}

/// Eats single line comment.
#[tracable_parser]
//#[packrat_parser]
pub(crate) fn comment(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, cs) = tag("//")(s)?;
    let (s, cb) = take_while(|c| c != '\n')(s)?;
    Ok( (s, Token { kind: TokenKind::Comment, span: Span::from(cs) + Span::from(cb) }))
}

/// Eats nested block comments. `/*` must have corresponding `*/`.
#[tracable_parser]
//#[packrat_parser]
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

    Ok( (rs, Token { kind: TokenKind::Comment, span: bc_span }) )
}

/// Eats whitespace characters.
/// See [librustc_lexer](https://github.com/rust-lang/rust/blob/master/src/librustc_lexer/src/lib.rs)
#[tracable_parser]
//#[packrat_parser]
pub(crate) fn whitespace(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, nls) = take_while1(
        |c| " \n\t\r\u{000B}\u{000C}\u{0085}\u{200E}\u{200F}\u{2028}\u{2029}".contains(c) )(s)?;
    Ok( (s, Token { kind: TokenKind::Whitespace, span: nls.into() } ) )
}

#[tracable_parser]
//#[packrat_parser]
pub(crate) fn any_token(s: NLSpan) -> IResult<NLSpan, Token> {
    let (s, t) = alt((
        whitespace,
        comment,
        blockcomment,
        number_and_suffix_lit,
        expr_op,
        punct,
        delim,
        bool_lit, // keep before ident
        ident,
        str_lit,
    ))(s)?;
    Ok ( (s, t) )
}

pub(crate) fn tokenize(s: NLSpan) -> IResult<NLSpan, Vec<Token>> {
    let (s, mut v) = many1(any_token)(s)?;
    // let mut indent = 0;
    // let mut patch: Vec<(usize, Token)> = Vec::new();
    // let mut acc = 0;
    // for (i, tok) in v.iter().enumerate() {
    //     if tok.is_delim() {
    //         if tok.delim_kind() == DelimKind::Open {
    //             indent = indent + 1;
    //             patch.push((i + 1 + acc, Token { kind: TokenKind::TreeIndent(TreeIndent(indent)), span: Span::zero() } ) );
    //         } else {
    //             indent = indent - 1;
    //             patch.push((i + acc, Token { kind: TokenKind::TreeIndent(TreeIndent(indent)), span: Span::zero() } ) );
    //         }
    //         acc = acc + 1;
    //     }
    // }
    // for (i, t) in patch {
    //     v.insert(i, t);
    // }
    Ok( (s, v) ) // return own Result with LexerError
}