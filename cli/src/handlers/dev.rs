use super::prelude::*;
use crate::commands::DevArgs;
use parser::ast::file::FileParse;
use std::rc::Rc;
use vhl_core::project::Project;

pub fn dev_subcmd(dev_args: DevArgs) -> Result<()> {
    let local_path = PathBuf::from(dev_args.vhl_source.clone());
    let input = std::fs::read_to_string(local_path.clone())
        .context(format!("unable to open '{:?}'", dev_args.vhl_source))?;
    let origin = SpanOrigin::Parser(SourceOrigin::File(Rc::new(local_path.clone())));

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

        let mut project = Project::new(file.ast_file);
        vhl_core::transform::transform(&mut project);
        project.print_report();
        if !project.errors.is_empty() {
            return Err(anyhow!("AST transforms failed due to errors"));
        }
        println!("{:#}", project.root);
    }
    Ok(())
}
