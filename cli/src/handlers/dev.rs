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
            file.print_report();
        }
        // println!("{:#}", file.ast_file);
        // match dev_args.definition {
        //     Some(_name) => {
        //         todo!()
        //     }
        //     None => {
        //         println!("{:?}", ast_core);
        //     }
        // }
        let ast_file = file.ast_file;
        let ast_file = match vhl::transform::transform(ast_file) {
            Ok((file, warnings)) => {
                warnings.print_report();
                file
            }
            Err(errors) => {
                errors.print_report();
                return Err(anyhow!("AST transforms failed due to errors"));
            }
        };
        println!("{:#}", ast_file);
    }
    Ok(())
}