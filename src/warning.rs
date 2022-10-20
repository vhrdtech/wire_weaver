use codespan_reporting::diagnostic::Diagnostic;
use codespan_reporting::files::SimpleFile;
use codespan_reporting::term::termcolor::{ColorChoice, StandardStream};
use ast::{Span, SpanOrigin};

pub struct Warnings {
    pub warnings: Vec<Warning>,
    pub origin: SpanOrigin,
    pub input: String,
}

pub struct Warning {
    pub kind: WarningKind,
    pub span: Span,
}

pub enum WarningKind {
    XpiArrayWithModifiers,
}

impl Warning {
    pub fn report(&self) -> Diagnostic<()> {
        todo!()
    }
}

impl Warnings {
    pub fn print_report(&self) {
        let writer = StandardStream::stderr(ColorChoice::Always);
        let config = codespan_reporting::term::Config::default();
        let file = SimpleFile::new(self.origin.clone(), &self.input);
        for diagnostic in self.warnings.iter().map(|warn| warn.report()) {
            codespan_reporting::term::emit(&mut writer.lock(), &config, &file, &diagnostic).unwrap();
        }
    }
}