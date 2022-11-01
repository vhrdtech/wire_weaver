use codespan_reporting::diagnostic::{Diagnostic, Label};
use ast::span::Span;

#[derive(Clone)]
pub struct UserError {
    pub kind: UserErrorKind,
    pub span: Span,
}

impl UserError {
    pub fn new(kind: UserErrorKind, span: Span) -> Self {
        UserError { kind, span }
    }
}

// impl Display for Error {
//     fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
//         write!(f, "vhl::Error({} {})", self.kind, self.span)
//     }
// }

// impl std::error::Error for Error {}

#[derive(Clone, Debug)]
pub enum UserErrorKind {
    //#[error("No serial number provided for a resource")]
    NoSerial,
    //#[error("Const resource cannot be rw, wo, observe or stream")]
    ConstWithMods,
    //#[error("Method resource cannot be const, ro, rw, wo, observe or stream")]
    FnWithMods,
    //#[error("Cell holding const or ro resource is redundant")]
    CellWithConstRo,
    //#[error("Write only resource cannot be observable")]
    WoObserve,
    //#[error("Cell holding ro+stream is redundant, multiple nodes can subscribe to the same screen")]
    CellWithRoStream,
    //#[error("Root resource cannot have a type or serial number")]
    RootWithTyOrSerial,
    //#[error("Root resource uri must be an identifier, not an interpolation")]
    RootWithInterpolatedUri,
    //#[error("Attribute was expected, but not present")]
    AttributeExpected,
    XpiArrayWithModifier,

    //#[error("Exactly one attribute was expected, but several provided")]
    AttributeMustBeUnique,
    //#[error("Expression was expected to be of {} kind but found to be of {}", .0, .1)]
    ExprExpectedToBe(String, String),
    //#[error("Attribute was expected to be of {} kind but found to be of {}", .0, .1)]
    AttrExpectedToBe(String, String),
    //#[error("Resource was expected to be of {} kind but found to be of {}", .0, .1)]
    XpiKindExpectedToBe(String, String),

}

type FileId = usize;

impl UserError {
    pub fn report(&self) -> Diagnostic<FileId> {
        let range = self.span.start..self.span.end;
        match &self.kind {
            UserErrorKind::XpiArrayWithModifier => {
                Diagnostic::error()
                    .with_code("E0100")
                    .with_message("array of resources with modifier")
                    .with_labels(vec![
                        Label::primary(0, range).with_message("array of resources cannot be ro/rw/wo/const, +stream or +observe")
                    ])
                    .with_notes(vec!["consider removing modifiers or changing resource type".to_owned()])
            }

            u => {
                Diagnostic::bug()
                    .with_code("Exxxx")
                    .with_message("internal core error (unimplemented)")
                    .with_labels(vec![
                        Label::primary(0, range).with_message(format!("{:?}", u))
                    ])
            }
        }
    }
}

// impl Errors {
//     pub fn print_report(&self) {
//         let writer = StandardStream::stderr(ColorChoice::Always);
//         let config = codespan_reporting::term::Config::default();
//         let file = SimpleFile::new(self.origin.clone(), &self.input);
//         for diagnostic in self.errors.iter().map(|err| err.report()) {
//             codespan_reporting::term::emit(&mut writer.lock(), &config, &file, &diagnostic).unwrap();
//         }
//     }
// }