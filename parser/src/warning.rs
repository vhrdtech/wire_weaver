use std::ops::Range;
use codespan_reporting::diagnostic::{Diagnostic, Label};

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ParseWarning {
    pub kind: ParseWarningKind,
    pub rule: crate::lexer::Rule,
    pub span: Range<usize>,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum ParseWarningKind {
    NonCamelCaseTypename,
    CellWithConstRo,
    CellWithRoStream,
    ForcedTyOnPathIndex,
}

impl ParseWarning {
    pub fn to_diagnostic(&self) -> Diagnostic<()> {
        let range = self.span.clone();
        match &self.kind {
            ParseWarningKind::NonCamelCaseTypename => Diagnostic::warning()
                .with_message("non camel case typename")
                .with_labels(vec![
                    Label::primary((), range).with_message("consider renaming to: '{}'")
                ]),
            ParseWarningKind::CellWithConstRo => Diagnostic::warning()
                .with_message("resource containing cell with a constant or read only data")
                .with_labels(vec![
                    Label::primary((), range).with_message("remove this Cell<_>")
                ])
                .with_notes(vec![
                    "const and read only resources are safe to use without a Cell"
                        .to_owned(),
                ]),
            ParseWarningKind::CellWithRoStream => Diagnostic::warning()
                .with_message("resource containing cell with a read only stream")
                .with_labels(vec![
                    Label::primary((), range).with_message("remove this Cell<_>")
                ])
                .with_notes(vec![
                    "read only streams are safe to use without a Cell".to_owned()
                ]),
            ParseWarningKind::ForcedTyOnPathIndex => Diagnostic::warning()
                .with_message("type ascription used in a path segment")
                .with_labels(vec![
                    Label::primary((), range).with_message("remove type from this literal")
                ])
                .with_notes(vec![
                    "index in a path must be a number <= u32::MAX - 1".to_owned()
                ]),
        }
    }
}