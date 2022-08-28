use std::env;
use parser::ast::expr::Expr;
use parser::ast::file::File;
use parser::ast::visit::Visit;

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

    let input = std::fs::read_to_string(filename)?;
    // let s = parser::util::pest_file_parse_tree(&input);
    // println!("{}", s);

    let file = File::parse(&input)?;
    println!("\x1b[33mWarnings: {:?}\x1b[0m", file.1);
    for def in file.0.clone().defs {
        println!("\x1b[45mD:\x1b[0m\t{:?}\n", def);
    }
    // println!("File: {:?}", file.0);
    let file_ast_core: vhl::ast::file::File = file.0.into();
    println!("{:?}", file_ast_core);

    // let mut expr_visitor = ExprVisitor {};
    // expr_visitor.visit_file(&file.0);

    // codegen::fun2();

    Ok(())
}
