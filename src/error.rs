use codespan_reporting::diagnostic::{Diagnostic, Label};
use ast::span::Span;

pub struct Error {
    pub kind: ErrorKind,
    pub span: Span,
}

impl Error {
    pub fn new(kind: ErrorKind, span: Span) -> Self {
        Error { kind, span }
    }
}

// impl Display for Error {
//     fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
//         write!(f, "vhl::Error({} {})", self.kind, self.span)
//     }
// }

// impl std::error::Error for Error {}

#[derive(Debug)]
pub enum ErrorKind {
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

impl Error {
    pub fn report(&self) -> Diagnostic<()> {
        let range = self.span.start..self.span.end;
        match &self.kind {
            ErrorKind::XpiArrayWithModifier => {
                Diagnostic::error()
                    .with_code("E0100")
                    .with_message("array of resources with modifier")
                    .with_labels(vec![
                        Label::primary((), range).with_message("array of resources cannot be ro/rw/wo/const, +stream or +observe")
                    ])
                    .with_notes(vec!["consider removing modifiers".to_owned()])
            }

            u => {
                Diagnostic::error()
                    .with_code("Exxxx")
                    .with_message("internal core error (unimplemented)")
                    .with_labels(vec![
                        Label::primary((), range).with_message(format!("{:?}", u))
                    ])
            }
        }
    }
}