use crate::transform::{SynFile, SynItemWithContext};

/// Convert enums and structs to internal AST
pub(crate) struct VisitUserTypes<'i> {
    pub(crate) files: &'i mut [SynFile],
}

impl<'i> VisitUserTypes<'i> {
    pub(crate) fn transform(&mut self) {
        for file in self.files.iter_mut() {
            for item in file.items.iter_mut() {
                match item {
                    SynItemWithContext::Enum {
                        item_enum,
                        transformed,
                        is_lifetime,
                    } => {}
                    SynItemWithContext::Struct {
                        item_struct,
                        transformed,
                        is_lifetime,
                    } => {}
                }
            }
        }
    }
}
