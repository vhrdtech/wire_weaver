use crate::error::Errors;
use crate::passes::autonum_to_fixed::AutonumToFixed;
use crate::warning::Warnings;
use ast::{File, Visit, VisitMut};
use crate::passes::idents_check::IdentsCheck;

/// Do various AST passes that transform things
pub fn transform(mut file: File) -> Result<(File, Warnings), Errors> {
    let mut warnings = Vec::new();
    let mut errors = Vec::new();

    let mut autonum_to_discrete = AutonumToFixed {};
    autonum_to_discrete.visit_file(&mut file);

    crate::passes::xpi_preprocess::xpi_preprocess(&mut file, &mut warnings, &mut errors);

    let mut idents_check = IdentsCheck { warnings: &mut warnings };
    idents_check.visit_file(&file);

    if errors.is_empty() {
        let warnings = Warnings {
            warnings,
            input: file.input.clone(),
            origin: file.origin.clone(),
        };
        Ok((
            file,
            warnings
        ))
    } else {
        Err(Errors {
            errors,
            input: file.input.clone(),
            origin: file.origin.clone(),
        })
    }
}
