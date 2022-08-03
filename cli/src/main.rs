use parser::ast::def_fn::DefFn;
use parser::ast::definition::Definition;
use parser::ast::expr::Expr;
use parser::ast::file::File;
use parser::ast::stmt::Stmt;
use parser::ast::visit::Visit;

struct ExprVisitor {}

impl<'ast, 'input> Visit<'ast, 'input> for ExprVisitor {
    fn visit_expression(&mut self, node: &'ast Expr<'input>) {
        println!("visiting expr: {}", node);

        parser::ast::visit::visit_expression(self, node);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error + 'static>> {
    let input = std::fs::read_to_string("/Users/roman/git/vhl_hw/led_ctrl/led_ctrl.vhl")?;
    let s = parser::util::pest_file_parse_tree(&input);
    println!("{}", s);

    let file = File::parse(&input)?;
    println!("Warnings: {:?}", file.1);
    println!("File: {:?}", file.0);

    let mut expr_visitor = ExprVisitor {};
    expr_visitor.visit_file(&file.0);

    // codegen::fun2();

    Ok(())
}
