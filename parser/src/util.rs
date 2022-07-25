use pest::iterators::{Pair, Pairs};
use crate::lexer::Rule;

pub fn pest_print_tree(pairs: Pairs<Rule>) {
    let all_input = pairs.as_str().clone();
    let mut level = 0;
    for p in pairs {
        pest_print_pair(p, all_input, 0, vec![level]);
        level = level + 1;
    }
}

fn pest_print_pair(pair: Pair<Rule>, all_input: &str, indent: u32, level: Vec<u32>) {
    print_n_tabs(indent);
    print!("\x1b[35m{:?}\x1b[0m \x1b[1m{:?}\x1b[0m: ", level, pair.as_rule());
    highlight_span(all_input, pair.as_span().start(), pair.as_span().end());
    println!();
    let mut child = 0;
    for p in pair.into_inner() {
        let mut level = level.clone();
        level.push(child);
        pest_print_pair(p, all_input, indent + 1, level);
        child = child + 1;
    }
}

fn print_n_tabs(n: u32) {
    for _ in 0..n {
        print!("\t");
    }
}

fn highlight_span(text: &str, from: usize, to: usize) {
    print!("{}\x1b[42m{}\x1b[0m{}", &text[..from], &text[from..to], &text[to..]);
}