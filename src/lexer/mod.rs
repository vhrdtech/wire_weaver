use nom_tracable::{histogram, cumulative_histogram};
use nom::multi::many1;
use nom_packrat::{init};

pub mod token;
pub mod tokenizer;

use token::{NLSpan, NLSpanInfo};
use crate::lexer::token::Token;
use crate::lexer::tokenizer::any_token;

pub fn lexer_play() {
    // println!("lexer_play():");
    // let info = TracableInfo::new().parser_width(64).fold("abc012abc");
    // let input = Span::new_extra("abc012abc", SpanInfo { tracable_info: info });
    // let output = parser(input);
    // println!("{:#?}", output);
    //
    // histogram();
    // cumulative_histogram();

    nom_packrat::init!();

    let test = "/ctrl(bitfield) {
  addr:
  type: u8
  description:
  default:

  /fdiv(7:5) {
    type: u2
    unit: 1
    default: 0b00
    access: rw
    description:
    examples:
    allowed: 'expression'
  }
}";

    let input = NLSpan::new_extra(test, NLSpanInfo::new() );
    let output = many1(any_token)(input);
    println!("{:?}", output);

    histogram();
    cumulative_histogram();
}

pub fn get_some_tokens() -> Vec<Token> {
    nom_packrat::init!();

    let input = NLSpan::new_extra("+-*+", NLSpanInfo::new() );
    let output = many1(tokenizer::expr_op)(input);
    if output.is_err() {
        println!("{:?}", output.err());
        return Vec::new()
    }
    let toks = output.unwrap();
    toks.1
}