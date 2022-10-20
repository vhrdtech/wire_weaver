use ast::{File, TyKind};
use ast::xpi_def::{AccessMode, XpiKind};
use super::prelude::*;

pub fn xpi_preprocess(file: &mut File, _warnings: &mut Vec<Warning>, errors: &mut Vec<Error>) {
    let mut collect_arrays = CollectArrays { errors };
    collect_arrays.visit_file(file);
}

pub struct CollectArrays<'i> {
    // warnings: &'i mut Vec<Warning>,
    errors: &'i mut Vec<Error>,
}

impl<'i> VisitMut for CollectArrays<'i> {
    fn visit_xpi_kind(&mut self, kind: &mut XpiKind) {
        match kind {
            XpiKind::Property { access, observable, ty } => {
                if *access != AccessMode::ImpliedRo || *observable {
                    self.errors.push(Error {
                        kind: ErrorKind::XpiArrayWithModifier,
                        span: ty.span.clone(),
                    });
                }
                match &ty.kind {
                    TyKind::Array { ty, len_bound } => {
                        match &ty.kind {
                            TyKind::UserDefined(ident) => {
                                if ident.symbols.as_str() == "Self" {
                                    *kind = XpiKind::Array {
                                        num_bound: len_bound.clone()
                                    };
                                    return;
                                }
                            }
                            // TyKind::Generic { id, params } {
                            //
                            // }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }

        // self.visit_xpi_kind(kind);
    }
}