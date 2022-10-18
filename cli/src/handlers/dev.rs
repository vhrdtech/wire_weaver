use super::prelude::*;
use parser::ast::file::FileParse;
use crate::commands::DevArgs;

pub fn dev_subcmd(dev_args: DevArgs) -> Result<()> {
    let local_path = PathBuf::from(dev_args.vhl_source.clone());
    let input = std::fs::read_to_string(local_path.clone())
        .context(format!("unable to open '{:?}'", dev_args.vhl_source))?;
    let origin = SpanOrigin::Parser(SourceOrigin::File(local_path.clone()));

    if dev_args.lexer {
        match dev_args.definition {
            Some(name) => {
                let tree = match FileParse::parse_tree(&input, &name, origin.clone()) {
                    Ok(t) => t,
                    Err(e) => {
                        println!("{}", e);
                        return Err(anyhow!("Input contains syntax errors"));
                    }
                };
                match tree {
                    Some(tree) => {
                        println!("{}", tree);
                    }
                    None => {
                        println!("Definition with name '{}' not found", name);
                    }
                }
            }
            None => {
                // print parse tree for the whole file
                match parser::util::pest_file_parse_tree(input.as_str()) {
                    Ok(tree) => println!("{}", tree),
                    Err(e) => {
                        e.print_report();
                    }
                }
            }
        }
    } else if dev_args.parser {
        let file = match FileParse::parse(&input, origin.clone()) {
            Ok(file) => file,
            Err(e) => {
                e.print_report();
                return Err(anyhow!("Input contains syntax errors"));
            }
        };
        if !file.warnings.is_empty() {
            println!("\x1b[33mLexer warnings: {:?}\x1b[0m", file.warnings);
        }
        println!("{}", file.ast_file);
        // match dev_args.definition {
        //     Some(_name) => {
        //         todo!()
        //     }
        //     None => {
        //         println!("{:?}", ast_core);
        //     }
        // }
        // if dev_args.process {
        //     println!("Processing AST...");
        //     vhl::process(&mut ast_core);
        //     println!("{:#?}", ast_core);
        // }
    }
    Ok(())
}