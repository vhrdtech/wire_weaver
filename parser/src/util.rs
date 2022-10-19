use crate::lexer::{Lexer, Rule};
use pest::iterators::{Pair, Pairs};
use std::fmt::Write;
use ast::{SourceOrigin, SpanOrigin};

#[allow(unused_macros)]
macro_rules! ppt {
    ($p:expr) => {
        crate::util::pest_print_tree($p.clone());
    };
}
#[allow(unused_imports)]
pub(crate) use ppt;
use crate::error::{Error, ErrorKind};

pub fn pest_print_tree(pairs: Pairs<Rule>) {
    let mut s = String::new();
    pest_print_tree_inner(pairs, true, &mut s);
    println!("{}", s);
}

pub fn pest_tree(pairs: Pairs<Rule>) -> String {
    let mut s = String::new();
    pest_print_tree_inner(pairs, true, &mut s);
    s
}

fn pest_print_tree_inner(pairs: Pairs<Rule>, print_input: bool, w: &mut dyn Write) {
    let all_input = pairs.as_str().clone();
    let mut level = 0;
    let all_input_index_shift = pairs.peek().map(|p| p.as_span().start()).unwrap_or(0);
    for p in pairs {
        pest_print_pair(
            p,
            all_input,
            all_input_index_shift,
            print_input,
            0,
            vec![level],
            w,
        );
        level = level + 1;
    }
}

pub fn pest_file_parse_tree(input: &str) -> Result<String, Error> {
    let parsed = match <Lexer as pest::Parser<Rule>>::parse(Rule::file, input) {
        Ok(parsed) => parsed,
        Err(e) => {
            return Err(Error {
                kind: ErrorKind::Grammar(e),
                origin: SpanOrigin::Parser(SourceOrigin::Str),
                input: input.to_owned(),
            })
        }
    };
    let mut s = String::new();
    pest_print_tree_inner(parsed, false, &mut s);
    Ok(s)
}

pub fn pest_stmt_parse_tree(input: &str) -> Result<String, Error> {
    let parsed = match <Lexer as pest::Parser<Rule>>::parse(Rule::statement, input) {
        Ok(parsed) => parsed,
        Err(e) => {
            return Err(Error {
                kind: ErrorKind::Grammar(e),
                origin: SpanOrigin::Parser(SourceOrigin::Str),
                input: input.to_owned(),
            })
        }
    };
    let mut s = String::new();
    pest_print_tree_inner(parsed, false, &mut s);
    Ok(s)
}

fn pest_print_pair(
    pair: Pair<Rule>,
    all_input: &str,
    all_input_index_shift: usize,
    print_input: bool,
    indent: u32,
    level: Vec<u32>,
    w: &mut dyn Write,
) {
    print_n_tabs(indent, w);
    write!(
        w,
        "\x1b[35m{:?}\x1b[0m \x1b[1m{:?} ({}, {})\x1b[0m: ",
        level,
        pair.as_rule(),
        pair.as_span().start(),
        pair.as_span().end()
    )
    .ok();

    if print_input {
        highlight_span(
            all_input,
            pair.as_span().start() - all_input_index_shift,
            pair.as_span().end() - all_input_index_shift,
            w,
        );
        writeln!(w).ok();
    } else {
        let source_no_line_feeds = pair
            .as_str()
            .chars()
            .filter(|c| *c != '\n' && *c != '\r')
            .collect::<String>();
        if source_no_line_feeds.len() < 100 {
            writeln!(w, "{}", source_no_line_feeds).ok();
        } else {
            writeln!(w, "{}\x1b[36m...\x1b[0m", &source_no_line_feeds[..100]).ok();
        }
    }

    let mut child = 0;
    for p in pair.into_inner() {
        let mut level = level.clone();
        level.push(child);
        pest_print_pair(
            p,
            all_input,
            all_input_index_shift,
            print_input,
            indent + 1,
            level,
            w,
        );
        child = child + 1;
    }
}

fn print_n_tabs(n: u32, w: &mut dyn Write) {
    for _ in 0..n {
        write!(w, "\t").ok();
    }
}

fn highlight_span(text: &str, from: usize, to: usize, w: &mut dyn Write) {
    if from == 0 && to == text.len() {
        write!(w, "\x1b[43m{}\x1b[0m", text).ok();
        return;
    }
    for (i, c) in text.chars().enumerate() {
        if i == from {
            write!(w, "\x1b[42m").ok();
        } else if i == to {
            write!(w, "\x1b[0m").ok();
        }
        if c == '\n' || c == '\r' || c == '\t' {
            write!(w, " ").ok();
        } else {
            write!(w, "{}", c).ok();
        }
    }
    write!(w, "\x1b[0m").ok();
}
