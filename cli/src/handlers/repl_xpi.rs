use super::prelude::*;
use crate::commands::ReplArgs;
use rustyline::completion::FilenameCompleter;
use rustyline::error::ReadlineError;
use rustyline::highlight::{Highlighter, MatchingBracketHighlighter};
use rustyline::hint::HistoryHinter;
use rustyline::validate::MatchingBracketValidator;
use rustyline::{Cmd, CompletionType, Config, EditMode, Editor, KeyEvent};
use rustyline_derive::{Completer, Helper, Hinter, Validator};
use std::borrow::Cow::{self, Borrowed, Owned};

#[derive(Helper, Completer, Hinter, Validator)]
struct MyHelper {
    #[rustyline(Completer)]
    completer: FilenameCompleter,
    highlighter: MatchingBracketHighlighter,
    #[rustyline(Validator)]
    validator: MatchingBracketValidator,
    #[rustyline(Hinter)]
    hinter: HistoryHinter,
    colored_prompt: String,
}

impl Highlighter for MyHelper {
    fn highlight<'l>(&self, line: &'l str, pos: usize) -> Cow<'l, str> {
        self.highlighter.highlight(line, pos)
    }

    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        prompt: &'p str,
        default: bool,
    ) -> Cow<'b, str> {
        if default {
            Borrowed(&self.colored_prompt)
        } else {
            Borrowed(prompt)
        }
    }

    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        Owned("\x1b[2m".to_owned() + hint + "\x1b[m")
    }

    fn highlight_char(&self, line: &str, pos: usize) -> bool {
        self.highlighter.highlight_char(line, pos)
    }
}

pub fn repl_xpi_cmd(repl_xpi: ReplArgs) -> Result<()> {
    println!("Loading: {}", repl_xpi.vhl_source);
    let origin = SpanOrigin::Parser(SourceOrigin::File(repl_xpi.vhl_source.into()));

    let rl_config = Config::builder()
        .history_ignore_space(true)
        .completion_type(CompletionType::List)
        .edit_mode(EditMode::Emacs)
        .build();
    let h = MyHelper {
        completer: FilenameCompleter::new(),
        highlighter: MatchingBracketHighlighter::new(),
        hinter: HistoryHinter {},
        colored_prompt: "".to_owned(),
        validator: MatchingBracketValidator::new(),
    };
    let mut rl = Editor::with_config(rl_config)?;
    rl.set_helper(Some(h));
    rl.bind_sequence(KeyEvent::alt('n'), Cmd::HistorySearchForward);
    rl.bind_sequence(KeyEvent::alt('p'), Cmd::HistorySearchBackward);
    if rl.load_history("history.txt").is_err() {
        println!("No previous history.");
    }
    let mut count = 1;
    loop {
        let p = format!("{}> ", count);
        rl.helper_mut().expect("No helper").colored_prompt = format!("\x1b[1;32m{}\x1b[0m", p);
        let readline = rl.readline(&p);
        let stmt = match readline {
            Ok(line) => {
                rl.add_history_entry(line.as_str());
                line
            }
            Err(ReadlineError::Interrupted) => {
                println!("Ctrl-C");
                rl.append_history("history.txt")?;
                return Ok(());
            }
            Err(ReadlineError::Eof) => {
                rl.append_history("history.txt")?;
                println!("Ctrl-D");
                return Ok(());
            }
            Err(e) => {
                return Err(anyhow!("rustyline error: {:?}", e));
            }
        };

        // match parser::ast::stmt::Stmt::parse(stmt.as_str(), origin.clone()) {
        //     Ok(stmt) => {
        //         println!("{:?}", stmt);
        //     }
        //     Err(e) => {
        //         println!("Error parsing: {:?}", e);
        //     }
        // }

        count += 1;
    }
}
