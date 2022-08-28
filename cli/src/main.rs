use std::env;
use std::path::Path;
use std::rc::Rc;
use parser::ast::expr::Expr;
use parser::ast::file::File;
use parser::ast::visit::Visit;
use vhl::span::{SourceOrigin, SpanOrigin};

struct ExprVisitor {}

impl<'ast, 'input> Visit<'ast, 'input> for ExprVisitor {
    fn visit_expression(&mut self, node: &'ast Expr<'input>) {
        println!("visiting expr: {}", node);

        parser::ast::visit::visit_expression(self, node);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error + 'static>> {
    let args: Vec<String> = env::args().collect();
    let filename = &args[1];
    let filepath = Path::new(filename);

    let input = std::fs::read_to_string(filepath)?;
    // let s = parser::util::pest_file_parse_tree(&input);
    // println!("{}", s);

    let file = File::parse(&input)?;
    println!("\x1b[33mWarnings: {:?}\x1b[0m", file.warnings);
    for def in file.clone().defs {
        println!("\x1b[45mD:\x1b[0m\t{:?}\n", def);
    }
    // println!("File: {:?}", file.0);
    let origin = SpanOrigin::Parser(SourceOrigin::File(Rc::new(filepath.to_path_buf())));
    let ast_core = vhl::ast::file::File::from_parser_ast(file, origin);
    println!("{:?}", ast_core);

    // let mut expr_visitor = ExprVisitor {};
    // expr_visitor.visit_file(&file.0);

    // codegen::fun2();

    Ok(())
}
