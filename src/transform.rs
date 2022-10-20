use codespan_reporting::diagnostic::Diagnostic;
use codespan_reporting::files::SimpleFile;
use codespan_reporting::term::termcolor::{ColorChoice, StandardStream};
use ast::{File, VisitMut};
use crate::passes::autonum_to_fixed::AutonumToFixed;

/// Do various AST passes that transform things
pub fn transform(file: &mut File) {
    let mut warnings = Vec::new();
    let mut errors = Vec::new();

    let mut autonum_to_discrete = AutonumToFixed {};
    autonum_to_discrete.visit_file(file);

    crate::passes::xpi_preprocess::xpi_preprocess(file, &mut warnings, &mut errors);

    let errors: Vec<Diagnostic<()>> = errors.iter().map(|err| err.report()).collect();
    // let warnings = warnings.iter().map(|warn| warn.report()).collect();
    let writer = StandardStream::stderr(ColorChoice::Always);
    let config = codespan_reporting::term::Config::default();
    let file = SimpleFile::new(file.origin.clone(), &file.input);
    for diagnostic in &errors {
        codespan_reporting::term::emit(&mut writer.lock(), &config, &file, diagnostic).unwrap();
    }
}