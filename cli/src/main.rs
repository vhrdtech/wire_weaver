mod commands;
mod handlers;
mod util;

use clap::Parser;

use crate::commands::Commands;
use anyhow::{anyhow, Context, Result};
use parser::ast::file::File;
use parser::span::{SourceOrigin, SpanOrigin};
use std::path::PathBuf;
use vhl::ast::file::Definition;

// struct ExprVisitor {}
//
// impl<'ast, 'input> Visit<'ast, 'input> for ExprVisitor {
//     fn visit_expression(&mut self, node: &'ast Expr<'input>) {
//         println!("visiting expr: {}", node);
//
//         parser::ast::visit::visit_expression(self, node);
//     }
// }

fn main() -> Result<()> {
    let cli = commands::Cli::parse();

    match cli.command {
        Some(Commands::Generate { vhl_source }) => {
            let input = std::fs::read_to_string(vhl_source.clone())
                .context(format!("unable to open '{:?}'", vhl_source))?;
            let origin = SpanOrigin::Parser(SourceOrigin::File(vhl_source.clone()));
            let file = match File::parse(&input, origin.clone()) {
                Ok(file) => file,
                Err(e) => {
                    println!("{}", e);
                    return Err(anyhow!("Input contains syntax errors"));
                }
            };

            let ast_core = vhl::ast::file::File::from_parser_ast(file);
            // println!("{:?}", ast_core);

            let mut cg_file = codegen::file::File::new();
            for item in &ast_core.items {
                match item {
                    Definition::Struct(struct_def) => {
                        let cg_struct_def = codegen::rust::struct_def::CGStructDef::new(struct_def);
                        let cg_struct_ser = codegen::rust::serdes::buf::struct_def::StructSer {
                            inner: cg_struct_def.clone(),
                        };
                        let cg_struct_des = codegen::rust::serdes::buf::struct_def::StructDes {
                            inner: cg_struct_def.clone(),
                        };
                        cg_file.push(&cg_struct_def, struct_def.span.clone());
                        cg_file.push(&cg_struct_ser, struct_def.span.clone());
                        cg_file.push(&cg_struct_des, struct_def.span.clone());
                    }
                }
            }
            let rendered_file = cg_file.render()?.0;

            let formatted_file = match util::format_rust(rendered_file.as_str()) {
                Ok(formatted_file) => formatted_file,
                Err(e) => {
                    println!("Failed to format file: {:?}", e);
                    println!("Raw output:\n{}", rendered_file);
                    return Ok(());
                }
            };
            let colorized_file = match util::colorize(formatted_file.as_str()) {
                Ok(colorized_file) => colorized_file,
                Err(e) => {
                    println!("Failed to colorize: {:?}", e);
                    println!("Raw output:\n{}", formatted_file);
                    return Ok(());
                }
            };
            println!("{}", colorized_file);
        }
        Some(Commands::Dev {
            lexer,
            parser,
            definition,
            vhl_source,
        }) => {
            let local_path = PathBuf::from(vhl_source.clone());
            let input = std::fs::read_to_string(local_path.clone())
                .context(format!("unable to open '{:?}'", vhl_source))?;
            let origin = SpanOrigin::Parser(SourceOrigin::File(local_path.clone()));
            let file = match File::parse(&input, origin.clone()) {
                Ok(file) => file,
                Err(e) => {
                    println!("{}", e);
                    return Err(anyhow!("Input contains syntax errors"));
                }
            };
            if !file.warnings.is_empty() {
                println!("\x1b[33mLexer warnings: {:?}\x1b[0m", file.warnings);
            }
            if lexer {
                match definition {
                    Some(name) => {
                        let tree = match File::parse_tree(&input, name.as_str(), origin.clone()) {
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
                        println!("{}", parser::util::pest_file_parse_tree(input.as_str()));
                        // print all definitions in the file
                        for def in file.clone().defs {
                            println!("\x1b[45mD:\x1b[0m\t{:?}\n", def);
                        }
                    }
                }
            } else if parser {
                let ast_core = vhl::ast::file::File::from_parser_ast(file);
                match definition {
                    Some(_name) => {
                        todo!()
                    }
                    None => {
                        println!("{:?}", ast_core);
                    }
                }
            }
        }
        Some(Commands::ReplXpi(repl_xpi)) => {
            return handlers::repl_xpi::repl_xpi_cmd(repl_xpi);
        }
        None => {}
    }

    // // let mut expr_visitor = ExprVisitor {};
    // // expr_visitor.visit_file(&file.0);

    Ok(())
}
