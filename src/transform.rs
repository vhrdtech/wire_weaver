use crate::passes::autonum_to_fixed::AutonumToFixed;
use ast::{Visit, VisitMut};
use crate::passes::idents_check::IdentsCheck;
use crate::project::Project;

/// Do various AST passes that transform things
pub fn transform(project: &mut Project) {
    let mut autonum_to_discrete = AutonumToFixed {};
    autonum_to_discrete.visit_file(&mut project.root);

    crate::passes::xpi_preprocess::xpi_preprocess(project);

    let mut idents_check = IdentsCheck { warnings: &mut project.warnings };
    idents_check.visit_file(&project.root);
}
