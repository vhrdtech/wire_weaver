use crate::ast::def_fn::DefFn;
use crate::ast::definition::Definition;
use crate::ast::expr::Expr;
use crate::ast::file::File;
use crate::ast::stmt::Stmt;

pub trait Visit<'ast, 'input> {
    fn visit_file(&mut self, i: &'ast File<'input>) {
        visit_file(self, i);
    }

    fn visit_definition(&mut self, i: &'ast Definition<'input>) {
        visit_definition(self, i);
    }

    fn visit_function(&mut self, i: &'ast DefFn<'input>) {
        visit_function(self, i);
    }

    fn visit_statement(&mut self, i: &'ast Stmt<'input>) {
        visit_statement(self, i);
    }

    fn visit_expression(&mut self, i: &'ast Expr<'input>) {
        visit_expression(self, i);
    }
}

pub fn visit_file<'ast, 'input, V>(v: &mut V, node: &'ast File<'input>)
    where V: Visit<'ast, 'input> + ?Sized
{
    for d in &node.defs {
        v.visit_definition(d);
    }
}

pub fn visit_definition<'ast, 'input, V>(v: &mut V, node: &'ast Definition<'input>)
    where V: Visit<'ast, 'input> + ?Sized
{
    match &node {
        Definition::Const(_) => {},
        Definition::Enum(_) => {},
        Definition::Struct(_) => {},
        Definition::Function(fun) => v.visit_function(fun),
        Definition::TypeAlias(_) => {},
        Definition::XpiBlock(_) => {},
    }
}

pub fn visit_function<'ast, 'input, V>(v: &mut V, node: &'ast DefFn<'input>)
    where V: Visit<'ast, 'input> + ?Sized
{
    for stmt in &node.statements.stmts {
        v.visit_statement(stmt);
    }
}

pub fn visit_statement<'ast, 'input, V>(v: &mut V, node: &'ast Stmt<'input>)
    where V: Visit<'ast, 'input> + ?Sized
{
    match &node {
        Stmt::Let(_) => {},
        Stmt::Expr(ex) => v.visit_expression(ex),
    }
}

pub fn visit_expression<'ast, 'input, V>(_v: &mut V, _node: &'ast Expr<'input>)
    where V: Visit<'ast, 'input> + ?Sized
{

}