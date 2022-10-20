use ast::{File, VisitMut};
use crate::passes::autonum_to_fixed::AutonumToFixed;

/// Do various AST passes that transform things
pub fn transform(file: &mut File) {
    let mut warnings = Vec::new();
    let mut errors = Vec::new();

    let mut autonum_to_discrete = AutonumToFixed {};
    autonum_to_discrete.visit_file(file);

    crate::passes::xpi_preprocess::xpi_preprocess(file, &mut warnings, &mut errors);
}