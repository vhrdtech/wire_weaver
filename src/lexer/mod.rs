use nom_tracable::{histogram, cumulative_histogram};
use nom::multi::many1;
//use nom_packrat::{init};

pub mod token;
pub mod tokenizer;
pub mod prettyprinter;

use token::{NLSpan, NLSpanInfo};
use crate::lexer::token::{Token, TokenStream};
use crate::lexer::tokenizer::{any_token, tokenize};
use prettyprinter::token as prettyprint_token;
use prettyprinter::token_stream as prettyprint_ts;

#[macro_export]
macro_rules! ts {
    ($($kind:ident),+) => {
        TokenStream::new_with_slice(&[ $(Token{kind: TokenKind::$kind, span: Span::any()}),* ])
    };
}

macro_rules! tok {
    ($kind:ident/$lo:literal-$hi:literal) => {
        Token{kind: TokenKind::$kind, span: Span::new($lo, $hi)}
    };
    ($kind:ident) => {
        Token{kind: TokenKind::$kind, span: Span::any()}
    };
    (l/$kind:ident) => {
        Token{kind: TokenKind::Literal(Lit{ kind: LiteralKind::$kind }), span: Span::any()}
    };
    (l/$kind:ident/$lo:literal-$hi:literal) => {
        Token{kind: TokenKind::Literal(Lit{ kind: LiteralKind::$kind }), span: Span::new($lo, $hi)}
    };
    (l/$int:ident/$base:ident/$lo:literal-$hi:literal) => {
        Token{kind: TokenKind::Literal(Lit{ kind: LiteralKind::Int{base: Base::$base} }), span: Span::new($lo, $hi)}
    };
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

    //nom_packrat::init!();

    let test = "/ctrl(bitfield) {
  addr: // comment
  type: u8 //
  description:
  default:

  /fdiv(7:5) {
    type: u2
    unit: 1
    default: 1 /*
        abcd
    */
    access: rw
    description:
    examples:
    allowed: \"expression\"
  }
}";

    let input = NLSpan::new_extra(test, NLSpanInfo::new() );
    let output = tokenize(input);
    if output.is_ok() {
        let toks = output.unwrap().1;
        let ts = TokenStream::new(&toks);
        prettyprint_ts(test, ts);
    }
    //println!("{:+#?}", output);

    histogram();
    cumulative_histogram();
}

pub fn get_some_tokens() -> Vec<Token> {
    //nom_packrat::init!();

    let input = NLSpan::new_extra("+-*+", NLSpanInfo::new() );
    let output = many1(tokenizer::expr_op)(input);
    if output.is_err() {
        println!("{:?}", output.err());
        return Vec::new()
    }
    let toks = output.unwrap();
    toks.1
}

#[cfg(test)]
mod tests {
    use crate::lexer::token::nlspan_from;
    use crate::lexer::tokenizer::{tokenize, number_and_suffix_lit};
    use crate::lexer::token::{Token, TokenKind, Lit, LiteralKind, Base, Span};

    #[test]
    fn test_bool_lit() {
        let input = "true";
        let span = nlspan_from(input);
        let output = tokenize(span);
        assert!(output.is_ok());
        let output = output.unwrap().1;
        let expected = vec![tok!(l/Bool/0-4)];
        assert_eq!(output, expected);
    }

    fn tokenize_or_fail(input: &str) -> Vec<Token> {
        let span = nlspan_from(input);
        let output = tokenize(span);
        output.unwrap().1
    }

    #[test]
    fn test_numbers() {
        // let span = nlspan_from("0x123");
        // let output = number_and_suffix_lit(span);
        // println!("{:?}", output);
        assert_eq!(tokenize_or_fail("0"), vec![tok!(l/Int/Decimal/0-1)]);
        assert_eq!(tokenize_or_fail("1\n"), vec![tok!(l/Int/Decimal/0-1), tok!(Whitespace/1-2)]);
        assert_eq!(tokenize_or_fail("123"), vec![tok!(l/Int/Decimal/0-3)]);
        assert_eq!(tokenize_or_fail("0x123"), vec![tok!(l/Int/Hexadecimal/0-5)]);
    }
}