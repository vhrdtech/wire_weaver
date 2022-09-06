use std::io::BufRead;
use crate::commands::ReplXpi;

pub fn repl_xpi_cmd(repl_xpi: ReplXpi) {
    println!("Loading: {}", repl_xpi.vhl_source);
    let stdin = std::io::stdin();
    let mut iterator = stdin.lock().lines();
    loop {
        print!("\x1b[32m>\x1b[0m ");
        let input = iterator.next().unwrap().unwrap();
        if input == "q" {
            return;
        }
        match parser::ast::stmt::Stmt::parse(input.as_str()) {
            Ok(stmt) => {
                println!("{:?}", stmt);
            }
            Err(e) => {
                println!("Error parsing expression: {:?}", e);
            }
        }
    }
}