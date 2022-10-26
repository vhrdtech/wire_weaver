use ast::{Identifier, IdentifierContext, Visit};
// use crate::error::Error;
use crate::warning::{Warning, WarningKind};

pub struct IdentsCheck<'i> {
    pub warnings: &'i mut Vec<Warning>,
    // errors: &'i mut Vec<Error>,
}

impl<'i> Visit for IdentsCheck<'i> {
    fn visit_identifier(&mut self, i: &Identifier) {
        match i.context {
            IdentifierContext::TyAlias => {}
            IdentifierContext::BuiltinTyName => {}
            IdentifierContext::PathSegment => {}
            IdentifierContext::XpiUriSegmentName => {}
            IdentifierContext::XpiKeyName => {}
            IdentifierContext::FnName => {
                if i.symbols.chars().find(|c| c.is_uppercase()).is_some() {
                    self.warnings.push(Warning {
                        kind: WarningKind::NonSnakeCaseFnName,
                        span: i.span.clone(),
                    });
                }
            }
            IdentifierContext::FnArgName => {}
            IdentifierContext::VariableDefName => {}
            IdentifierContext::VariableRefName => {}
            IdentifierContext::StructTyName => {}
            IdentifierContext::StructFieldName => {}
            IdentifierContext::EnumTyName => {}
            IdentifierContext::EnumFieldName => {}
            IdentifierContext::GenericName => {}
            IdentifierContext::MakePath => {}
        }
    }
}
