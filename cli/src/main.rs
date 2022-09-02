mod commands;
use clap::Parser;

use std::env;
use std::io::{Read, Write};
use std::path::Path;
use std::process::Stdio;
use std::rc::Rc;
use anyhow::{
    Context, Result};
use subprocess::{Exec, Redirection};
use parser::ast::expr::Expr;
use parser::ast::file::File;
use parser::ast::visit::Visit;
use vhl::ast::file::Definition;
use vhl::span::{SourceOrigin, SpanOrigin};
use crate::commands::Commands;

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

    match &cli.command {
        Some(Commands::Generate { vhl_source }) => {
            let input = std::fs::read_to_string(vhl_source)
                .context(format!("unable to open '{:?}'", vhl_source))?;
            let file = File::parse(&input)?;
            println!("\x1b[33mWarnings: {:?}\x1b[0m", file.warnings);
            for def in file.clone().defs {
                println!("\x1b[45mD:\x1b[0m\t{:?}\n", def);
            }
            // println!("File: {:?}", file.0);

            let origin = SpanOrigin::Parser(SourceOrigin::File(Rc::new(vhl_source.clone())));
            let ast_core = vhl::ast::file::File::from_parser_ast(file, origin);
            println!("{:?}", ast_core);

            let mut cg_file = codegen::file::File::new();
            for item in &ast_core.items {
                match item {
                    Definition::Struct(struct_def) => {
                        let cg_struct_def = codegen::rust::struct_def::CGStructDef::new(struct_def);
                        let cg_struct_ser = codegen::rust::serdes::buf::struct_def::StructSer { inner: cg_struct_def.clone() };
                        let cg_struct_des = codegen::rust::serdes::buf::struct_def::StructDes { inner: cg_struct_def.clone() };
                        cg_file.push(&cg_struct_def, struct_def.span.clone());
                        cg_file.push(&cg_struct_ser, struct_def.span.clone());
                        cg_file.push(&cg_struct_des, struct_def.span.clone());
                    }
                }
            }
            let rendered_file = cg_file.render()?.0;
            // let mut fmt = std::process::Command::new("rustfmt")
            //     .stdin(Stdio::piped())
            //     .stdout(Stdio::piped())
            //     .spawn()?;
            // fmt.stdin.as_mut().unwrap().write(rendered_file.as_bytes())?;
            // let fmt_result = fmt.wait_with_output()?;
            // println!("{}", String::from_utf8(fmt_result.stdout)?);
            let formatted = Exec::cmd("rustfmt")
                .stdin(rendered_file.as_str())
                .stdout(Redirection::Pipe)
                .capture().context("failed to rustfmt")?
                .stdout_str();
            // let colorized = Exec::cmd("/usr/local/bin/highlight")
            //     .args(&[
            //         "--syntax-by-name", "rust",
            //         "--out-format", "truecolor",
            //         // "--out-format", "xterm256",
            //         "--style", "moria",
            //     ])
            //     .stdin(formatted.as_str())
            //     .stdout(Redirection::Pipe)
            //     .capture()?
            //     .stdout_str();
            let colorized = Exec::cmd("/usr/local/bin/pygmentize")
                .args(&[
                    "-l", "rust",
                    "-O", "style=monokai"
                ])
                .stdin(formatted.as_str())
                .stdout(Redirection::Pipe)
                .capture().context("failed to highlight with pygmentize")?
                .stdout_str();

            println!("{}", colorized);
        }
        None => {}
    }

    // // let mut expr_visitor = ExprVisitor {};
    // // expr_visitor.visit_file(&file.0);

    Ok(())
}
