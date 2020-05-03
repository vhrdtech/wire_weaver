use super::token::{Token, TokenKind, Lit, LiteralKind, Span};
use colored::*;

pub fn token(source: &str, tok: &Token) {
    println!("{}", tok.kind.as_ref());
}