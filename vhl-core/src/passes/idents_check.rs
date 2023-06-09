use ast::{Identifier, IdentifierContext, Visit};
// use crate::error::Error;
use crate::warning::{Warning, WarningKind};

pub struct IdentsCheck<'i> {
    pub warnings: &'i mut Vec<Warning>,
    // errors: &'i mut Vec<Error>,
}

impl<'i> Visit for IdentsCheck<'i> {
    fn visit_identifier(&mut self, i: &Identifier) {
        use IdentifierContext::*;
        match i.context {
            TyAlias => {}
            BuiltinTyName => {}
            FnName | StructFieldName | FnArgName | XpiKeyName | XpiUriSegmentName | PathSegment => {
                if i.symbols.chars().any(|c| c.is_uppercase()) {
                    self.warnings.push(Warning {
                        kind: WarningKind::NonSnakeCaseFnName(i.symbols.clone()),
                        span: i.span.clone(),
                    });
                }
            }
            VariableDefName => {}
            VariableRefName => {}
            StructTyName => {}
            EnumTyName => {}
            EnumFieldName => {}
            GenericName => {}
            MakePath => {}
        }
    }
}
