use std::rc::Rc;
use codespan_reporting::diagnostic::{Diagnostic, Label};
use ast::Span;

#[derive(Clone)]
pub struct Warning {
    pub kind: WarningKind,
    pub span: Span,
}

#[derive(Clone)]
pub enum WarningKind {
    NonSnakeCaseFnName(Rc<String>)
}

type FileId = usize;

impl Warning {
    pub fn report(&self) -> Diagnostic<FileId> {
        let range = self.span.start..self.span.end;
        match &self.kind {
            WarningKind::NonSnakeCaseFnName(name) => {
                let snake_case = name.to_lowercase();
                Diagnostic::warning()
                    .with_code("W0001")
                    .with_message("non snake case function name")
                    .with_labels(vec![
                        Label::primary(0, range).with_message("function names are snake case by convention")
                    ])
                    .with_notes(vec![format!("consider renaming to: '{}'", snake_case)])
            }
        }
    }
}

// impl Warnings {
//     pub fn print_report(&self) {
//         let writer = StandardStream::stderr(ColorChoice::Always);
//         let config = codespan_reporting::term::Config::default();
//         let file = SimpleFile::new(self.origin.clone(), &self.input);
//         for diagnostic in self.warnings.iter().map(|warn| warn.report(&self.input)) {
//             codespan_reporting::term::emit(&mut writer.lock(), &config, &file, &diagnostic).unwrap();
//         }
//     }
// }