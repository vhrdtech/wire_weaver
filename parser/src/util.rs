use std::fmt::Write;
use pest::iterators::{Pair, Pairs};
use crate::lexer::{Lexer, Rule};

pub fn pest_print_tree(pairs: Pairs<Rule>) {
    let mut s = String::new();
    pest_print_tree_inner(pairs, true,&mut s);
    println!("{}", s);
}

fn pest_print_tree_inner(pairs: Pairs<Rule>, print_input: bool, w: &mut dyn Write) {
    let all_input = pairs.as_str().clone();
    let mut level = 0;
    for p in pairs {
        pest_print_pair(p, all_input, print_input, 0, vec![level], w);
        level = level + 1;
    }
}

pub fn pest_file_parse_tree(input: &str) -> String {
    let parsed = <Lexer as pest::Parser<Rule>>::parse(Rule::file, input).unwrap();
    let mut s = String::new();
    pest_print_tree_inner(parsed, false, &mut s);
    s
}

fn pest_print_pair(pair: Pair<Rule>, all_input: &str, print_input: bool, indent: u32, level: Vec<u32>, w: &mut dyn Write) {
    print_n_tabs(indent, w);
    write!(w, "\x1b[35m{:?}\x1b[0m \x1b[1m{:?}\x1b[0m: ", level, pair.as_rule()).ok();
    if print_input {
        highlight_span(all_input, pair.as_span().start(), pair.as_span().end(), w);
        writeln!(w).ok();
    } else {
        let source_no_line_feeds = pair.as_str().chars().filter(|c| *c != '\n' && *c != '\r' ).collect::<String>();
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
        pest_print_pair(p, all_input, print_input, indent + 1, level, w);
        child = child + 1;
    }
}

fn print_n_tabs(n: u32, w: &mut dyn Write) {
    for _ in 0..n {
        write!(w, "\t").ok();
    }
}

fn highlight_span(text: &str, from: usize, to: usize, w: &mut dyn Write) {
    write!(w, "{}\x1b[42m{}\x1b[0m{}", &text[..from], &text[from..to], &text[to..]).ok();
}