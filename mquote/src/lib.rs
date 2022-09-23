mod lexer;

use quote::{quote, TokenStreamExt, ToTokens};

use proc_macro2::{Ident, Literal, Span};

extern crate pest;
#[macro_use]
extern crate pest_derive;

extern crate proc_macro;

use pest::iterators::Pair;
use proc_macro::{TokenStream, TokenTree};
use std::collections::HashMap;
use std::hash::Hash;

use crate::pest::Parser;
use lexer::{MQuoteLexer, Rule};

#[derive(Copy, Clone, Eq, PartialEq)]
enum Language {
    Rust,
    Dart,
}

#[proc_macro]
pub fn mquote(ts: TokenStream) -> TokenStream {
    let mut ts = ts.into_iter();
    let language = match ts.next().unwrap() {
        TokenTree::Ident(ident) => match ident.to_string().as_str() {
            "rust" => Language::Rust,
            "dart" => Language::Dart,
            _ => panic!("Unsupported language: {}", ident),
        },
        _ => panic!("Expected language name"),
    };
    let mquote_ts = match ts.next().unwrap() {
        TokenTree::Literal(lit) => lit.to_string(),
        _ => panic!("Expected raw string literal with mtoken's"),
    };
    let debug = ts.next().is_some();

    let mquote_ts = if mquote_ts.starts_with("\"") {
        &mquote_ts[1..mquote_ts.len() - 1]
    } else {
        let mut pound_count = 1;
        let mquote_ts_ascii = mquote_ts.as_str().as_bytes();
        while mquote_ts_ascii[pound_count] == '#' as u8 {
            pound_count += 1;
        }
        &mquote_ts[pound_count + 1..mquote_ts.len() - pound_count]
    };
    if debug {
        eprintln!("\nParsing mquote str: '{}'", mquote_ts);
    }
    let mquote_ts = MQuoteLexer::parse(Rule::token_stream, mquote_ts).unwrap();
    if debug {
        // eprintln!("mquote! in {:?}", proc_macro::Span::call_site().source_file());
        eprintln!("Parsed: {:?}", mquote_ts);
    }

    let mut ts_builder = new_ts_builder();
    ts_builder.append_all(quote! {
        use std::rc::Rc;
        use mtoken::ext::TokenStreamExt;
        use mtoken::ToTokens;
    });
    let mut repetition_paths = HashMap::new();
    for token in mquote_ts {
        tt_append(token, &mut ts_builder, language, &mut repetition_paths);
    }
    let interpolate_repetitions = if repetition_paths.is_empty() {
        quote! {}
    } else {
        let mut streams_at_builder = quote! {
            use std::collections::{HashMap, VecDeque};
            let mut streams_at: HashMap<usize, VecDeque<mtoken::TokenStream>> = HashMap::new();
        };
        for (path, idx) in repetition_paths {
            let idx = Literal::usize_unsuffixed(idx);
            streams_at_builder.append_all(quote! {
                streams_at.insert(#idx, #path.into_iter().map(|t| {
                    let mut ts = mtoken::TokenStream::new();
                    t.to_tokens(&mut ts);
                    ts
                }).collect());
            });
        }
        quote! {
            #streams_at_builder
            ts.interpolate_repetitions(streams_at);
        }
    };
    let print_ts_if_debug = if debug {
        quote! {
            println!("{:?}", ts);
        }
    } else {
        quote!()
    };
    let ts_builder = quote! {
        {
            #ts_builder
            #interpolate_repetitions
            #print_ts_if_debug
            ts
        }
    };
    if debug {
        eprintln!("TS builder: {}", ts_builder);
    }
    ts_builder.into()
}

fn new_ts_builder() -> proc_macro2::TokenStream {
    quote! {
        let mut ts = mtoken::TokenStream::new();
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct Path {
    segments: Vec<String>,
    is_tail_call: bool,
}

impl ToTokens for Path {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let is_tail_call = if self.is_tail_call {
            quote! { () }
        } else {
            quote! {}
        };
        let segments = self.segments
            .iter()
            .map(|s| {
                Ident::new(s.as_str(), Span::call_site())
            });
        tokens.append_all(quote! {
             #(#segments).* #is_tail_call
        });
    }
}

fn interpolate_path(token: Pair<Rule>) -> Path {
    let mut is_tail_call = false;
    let segments = token
        .into_inner()
        .filter(|p| {
            if p.as_rule() == Rule::interpolate_call {
                is_tail_call = true;
                false
            } else {
                true
            }
        })
        .map(|path_segment| {
            match path_segment.as_rule() {
                Rule::interpolate_path_segment => {
                    path_segment.as_str().to_owned()
                }
                r => panic!("{:?} was unexpected in interpolate", r)
            }
        })
        .collect();
    Path {
        segments,
        is_tail_call,
    }
}

fn ident_flavor(language: Language) -> Ident {
    match language {
        Language::Rust => Ident::new("RustAutoRaw", Span::call_site()),
        Language::Dart => Ident::new("DartAutoRaw", Span::call_site()),
    }
}

fn tt_append(
    token: Pair<Rule>,
    ts_builder: &mut proc_macro2::TokenStream,
    language: Language,
    repetition_paths: &mut HashMap<Path, usize>,
) {
    match token.as_rule() {
        Rule::delim_token_tree => tt_append_delim_token_tree(token, ts_builder, language, repetition_paths),
        Rule::token => tt_append(token.into_inner().next().unwrap(), ts_builder, language, repetition_paths),
        Rule::interpolate => tt_append_interpolate(token, ts_builder, repetition_paths),
        Rule::repeat => tt_append_repetition(token, ts_builder, language, repetition_paths),
        Rule::ident => tt_append_ident(token, ts_builder, language),
        Rule::punctuation => tt_append_punctuation(token, ts_builder, language),
        // Rule::delimiter => tt_append_delimiter(token, ts_builder),
        Rule::literal => tt_append_literal(token, ts_builder),
        Rule::spacing_joint => ts_builder.append_all(quote! {
            ts.modify_last_spacing(mtoken::Spacing::Joint);
        }),
        Rule::spacing_enable => ts_builder.append_all(quote! {
            ts.append(mtoken::TokenTree::Spacing(true));
        }),
        Rule::spacing_disable => ts_builder.append_all(quote! {
            ts.append(mtoken::TokenTree::Spacing(false));
        }),
        Rule::COMMENT => tt_append_comment(token, ts_builder, language),
        Rule::EOI => {}
        _ => panic!("Internal error: expected a token tree, got: {:?}", token),
    }
}

fn tt_append_literal(token: Pair<Rule>, ts_builder: &mut proc_macro2::TokenStream) {
    let repr = Literal::string(token.as_str());
    ts_builder.append_all(quote! {
        ts.append(
            mtoken::Literal::new(#repr.to_string())
        );
    });
}

fn tt_append_interpolate(
    token: Pair<Rule>,
    ts_builder: &mut proc_macro2::TokenStream,
    repetition_paths: &mut HashMap<Path, usize>,
) {
    let interpolate = token.into_inner().next().expect("Wrong interpolate grammar");
    let kind = interpolate.as_rule();
    let path = interpolate_path(interpolate.into_inner().next().expect("Wrong interpolate grammar"));
    match kind {
        Rule::interpolate_one => {
            ts_builder.append_all(quote! {
                #path.to_tokens(&mut ts);
            });
        }
        Rule::interpolate_rep => {
            let paths_count = repetition_paths.len();
            let repetition_idx = *repetition_paths.entry(path.clone()).or_insert(paths_count);
            let repetition_idx = Literal::usize_unsuffixed(repetition_idx);
            ts_builder.append_all(quote! {
                ts.append(mtoken::TokenTree::Repetition(#repetition_idx));
            });
        }
        r => panic!("Unexpected {:?}", r)
    }
}

fn tt_append_delim_token_tree(
    token: Pair<Rule>,
    ts_builder: &mut proc_macro2::TokenStream,
    language: Language,
    repetition_paths: &mut HashMap<Path, usize>
) {
    let delimiter = match token.as_str().chars().next().unwrap() {
        '(' => proc_macro2::Ident::new("Parenthesis", Span::call_site()),
        '{' => proc_macro2::Ident::new("Brace", Span::call_site()),
        '[' => proc_macro2::Ident::new("Bracket", Span::call_site()),
        _ => panic!("Internal error, expected delimiter"),
    };
    let mut delim_stream = new_ts_builder();
    for token in token.into_inner() {
        tt_append(token, &mut delim_stream, language, repetition_paths);
    }
    ts_builder.append_all(quote! {
        let delim_stream = { #delim_stream ts };
        ts.append(mtoken::Group::new(mtoken::Delimiter::#delimiter, delim_stream));
    })
}

fn tt_append_repetition(
    token: Pair<Rule>,
    ts_builder: &mut proc_macro2::TokenStream,
    language: Language,
    repetition_paths: &mut HashMap<Path, usize>
) {
    let mut separator = quote! { None };
    let mut delim_stream = new_ts_builder();
    for token in token.into_inner() {
        match token.as_rule() {
            Rule::repetition_separator => {
                let ch = token.as_str().chars().next().expect("Wrong repetition_separator grammar");
                let punct_lit = Literal::character(ch);
                separator = quote! { Some(mtoken::Punct::new(#punct_lit, mtoken::Spacing::Alone)) };
            }
            _ => tt_append(token, &mut delim_stream, language, repetition_paths)
        }
    }
    ts_builder.append_all(quote! {
        let delim_stream = { #delim_stream ts };
        ts.append(mtoken::TokenTree::RepetitionGroup(delim_stream, #separator));
    })
}

fn tt_append_ident(
    token: Pair<Rule>,
    ts_builder: &mut proc_macro2::TokenStream,
    language: Language,
) {
    let cancel_auto_raw = token.as_str().chars().next().unwrap() == 'ȸ';
    let (ident_lit, flavor) = if cancel_auto_raw {
        let ident_skip: String = token.as_str().chars().skip(1).collect();
        (
            Literal::string(ident_skip.as_str()),
            Ident::new("Plain", Span::call_site()),
        )
    } else {
        (Literal::string(token.as_str()), ident_flavor(language))
    };
    ts_builder.append_all(quote! {
        ts.append(
            mtoken::Ident::new(
                Rc::new(#ident_lit.to_owned()),
                mtoken::token::IdentFlavor::#flavor,
            )
        );
    });
}

fn tt_append_punctuation(
    token: Pair<Rule>,
    ts_builder: &mut proc_macro2::TokenStream,
    language: Language,
) {
    let punct: Vec<char> = token.as_str().chars().collect();
    match punct.len() {
        1 => {
            let ch = punct[0];
            // handle 'lifetime for Rust
            let spacing = if language == Language::Rust && ch == '\'' {
                Ident::new("Joint", Span::call_site())
            } else {
                Ident::new("Alone", Span::call_site())
            };
            let punct_lit = Literal::character(ch);
            ts_builder.append_all(quote! {
                ts.append(mtoken::Punct::new(#punct_lit, mtoken::Spacing::#spacing));
            })
        }
        2 => {
            let punct_lit = Literal::character(punct[0]);
            let punct_lit2 = Literal::character(punct[1]);
            ts_builder.append_all(quote! {
                ts.append(mtoken::Punct::new(#punct_lit, mtoken::Spacing::Joint));
                ts.append(mtoken::Punct::new(#punct_lit2, mtoken::Spacing::Alone));
            })
        }
        3 => {
            let punct_lit = Literal::character(punct[0]);
            let punct_lit2 = Literal::character(punct[1]);
            let punct_lit3 = Literal::character(punct[2]);
            ts_builder.append_all(quote! {
                ts.append(mtoken::Punct::new(#punct_lit, mtoken::Spacing::Joint));
                ts.append(mtoken::Punct::new(#punct_lit2, mtoken::Spacing::Joint));
                ts.append(mtoken::Punct::new(#punct_lit3, mtoken::Spacing::Alone));
            })
        }
        _ => panic!("internal: Up to 3 chars in a punct is supported"),
    }
}

fn tt_append_comment(
    token: Pair<Rule>,
    ts_builder: &mut proc_macro2::TokenStream,
    language: Language,
) {
    let comment_style = token
        .into_inner()
        .next()
        .expect("internal: Wrong comment grammar");
    let flavor = match comment_style.as_rule() {
        Rule::doc_comment => match language {
            Language::Rust | Language::Dart => "TripleSlash",
        },
        Rule::single_line_comment => match language {
            Language::Rust | Language::Dart => "DoubleSlash",
        },
        Rule::multi_line_comment => match language {
            Language::Rust | Language::Dart => "SlashStarMultiline",
        },
        _ => panic!("internal: Wrong comment grammar kinds"),
    };
    let contents = comment_style
        .into_inner()
        .next()
        .expect("internal: Wrong comment grammar (no _inner)")
        .as_str();
    let contents = Literal::string(contents);
    let flavor = Ident::new(flavor, Span::call_site());
    ts_builder.append_all(quote! {
        ts.append(
            mtoken::Comment::new(#contents, mtoken::CommentFlavor::#flavor)
        );
    });
}
