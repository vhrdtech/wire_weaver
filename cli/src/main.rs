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
    let input = std::fs::read_to_string("/Users/roman/git/vhl_hw/led_ctrl/led_ctrl.vhl")?;
    // let s = parser::util::pest_file_parse_tree(&input);
    // println!("{}", s);

    let file = File::parse(&input)?;
    println!("\x1b[33mWarnings: {:?}\x1b[0m", file.1);
    for def in file.0.defs {
        println!("\x1b[45mD:\x1b[0m\t{:?}\n", def);
    }
    // println!("File: {:?}", file.0);

    // let mut expr_visitor = ExprVisitor {};
    // expr_visitor.visit_file(&file.0);

    // codegen::fun2();

    Ok(())
}
