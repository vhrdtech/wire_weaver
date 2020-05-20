use super::token::{Token, TokenKind, Lit, LiteralKind, Span};
use colored::*;
use nom::InputIter;
use crate::lexer::token::TokenStream;
use std::ops::Range;

pub fn line_numbers(source: &str) -> Vec<usize> {
    let mut ln: Vec<usize> = Vec::new();
    for c in source.iter_elements().enumerate() {
        if c.1 == '\n' {
            ln.push(c.0)
        }
    }
    ln
}

fn print_lit(l: &Lit) {
    match &l.kind {
        LiteralKind::Int { base, parsed } => { print!("Int={}{:?}", base, parsed) },
        LiteralKind::Float { base, parsed } => { print!("Float={:?}", parsed) },
        LiteralKind::Char(c) => { print!("Char=`{}`", c) },
        LiteralKind::Byte(b) => { print!("Byte={:#04x}", b) },
        LiteralKind::Str(s) => { print!("Str=\"{}\"", s) },
        LiteralKind::Bool(b) => { print!("Bool={}", b) },
        LiteralKind::ByteStr(bs) => { print!("ByteStr=`unimplemented!`") },
    }
}

pub fn token(source: &str, tok: &Token) {
    if tok.is_delim() {
        println!("{}({}=`{}`, {})",
                 "Delim".bright_yellow(),
                 tok.kind.as_ref(), &source[Range::from(tok.span)], tok.span);
    } else if tok.is_punct() {
        println!("{}({}=`{}`, {})",
                 "Punct".green(),
                 tok.kind.as_ref(), &source[Range::from(tok.span)], tok.span);
    } else if let TokenKind::Literal(l) = &tok.kind {
        print!("{}(", "Literal".purple());
        print_lit(l);
        println!(", {})", tok.span);
    } else if let TokenKind::Ident(id) = tok.kind {
        println!("{}({}=`{}`, {})",
                 "Ident".cyan(),
                 tok.kind.as_ref(), &source[Range::from(tok.span)], tok.span);
    } else if tok.is_binary_op() {
        println!("{}({}=`{}`, {})",
                 "BinOp".blue(),
                 tok.kind.as_ref(), &source[Range::from(tok.span)], tok.span);
    } else if tok.is_bool_op() {
        println!("{}({}=`{}`, {})",
                 "BoolOp".blue(),
                 tok.kind.as_ref(), &source[Range::from(tok.span)], tok.span);
    } else if tok.kind == TokenKind::Unknown {
        println!("{}({})", "Unknown".red(), tok.span);
    } else if let TokenKind::TreeIndent(i) = tok.kind {
        println!("{}({})", "TreeIndent".black().on_white(), i.0);
    } else {
        println!("{}({})", tok.kind.as_ref(), tok.span);
    }
}

fn print_one_line(line: &str, number: u32) {
    let n = format!("{} | ", number);
    println!("{}{}", n.bold().bright_blue(), line);
}

fn print_n_tabs(n: u32) {
    for _ in 0..n {
        print!("    ");
    }
}

pub fn token_stream(source: &str, ts: TokenStream) {
    let line_numbers = line_numbers(source);
    if line_numbers.len() < 1 {
        println!("TokenStream(empty)");
        return;
    }
    print_one_line(&source[0..line_numbers[0]], 1);

    let mut print_line = false;
    let mut tree_indent = 0;
    for tok in ts.iter_elements() {
        if let TokenKind::TreeIndent(i) = tok.kind {
            tree_indent = i.0 as u32;

            print_n_tabs(tree_indent);
            token(source, tok);
            continue;
        }

        if print_line {
            let line = tok.span.line as usize;
            if line >= 2 {
                let line_start_pos = tok.span.lo.0 as usize - 1; //line_numbers[line - 2];
                let line_end_pos = if line <= line_numbers.len() {
                    line_numbers[line - 1]
                } else {
                    source.len()
                };
                print_n_tabs(tree_indent);
                print_one_line(&source[line_start_pos + 1..line_end_pos], tok.span.line);
            }
            print_line = false;
        }
        if tok.kind == TokenKind::Whitespace {
            let fragment = &source[Range::from(tok.span)];
            if fragment.contains('\n') {
                print_line = true;
                print!("\n");
            }
        } else {
            print_n_tabs(tree_indent);
            token(source, tok);
        }
    }
}