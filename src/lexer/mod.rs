use nom_tracable::{histogram, cumulative_histogram};
use nom::multi::many1;
use nom_packrat::{init};

pub mod token;
pub mod tokenizer;

use token::{NLSpan, NLSpanInfo};
use crate::lexer::token::Token;

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

    let input = NLSpan::new_extra("<=<+++", NLSpanInfo::new() );
    let output = many1(tokenizer::expr_op)(input);
    println!("{:?}", output);

    histogram();
    cumulative_histogram();
}

pub fn get_some_tokens() -> Vec<Token> {
    nom_packrat::init!();

    let input = NLSpan::new_extra("<=<+++", NLSpanInfo::new() );
    let output = many1(tokenizer::expr_op)(input);
    if output.is_err() {
        return Vec::new()
    }
    let toks = output.unwrap();
    toks.1
}

pub fn get_some_more() -> Vec<Token> {
    nom_packrat::init!();

    let input = NLSpan::new_extra("<=", NLSpanInfo::new() );
    let output = many1(tokenizer::expr_op)(input);
    //println!("{:?}", output);
    // if output.is_err() {
    //     //println!("{:?}", output.err());
    //     return Vec::new()
    // }
    let toks = output.unwrap();
    toks.1
}