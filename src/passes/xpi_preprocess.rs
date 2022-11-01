use ast::{make_path, TyKind};
use ast::generics::GenericParam;
use ast::xpi_def::{AccessMode, XpiKind};
use crate::project::Project;
use super::prelude::*;

pub fn xpi_preprocess(project: &mut Project) {
    let mut collect_arrays = CollectArrays { errors: &mut project.errors };
    collect_arrays.visit_file(&mut project.root);
}

pub struct CollectArrays<'i> {
    // warnings: &'i mut Vec<Warning>,
    errors: &'i mut Vec<UserError>,
}

impl<'i> VisitMut for CollectArrays<'i> {
    fn visit_xpi_kind(&mut self, kind: &mut XpiKind) {
        match kind {
            XpiKind::Property { access, observable, ty } => {
                let span = ty.span.clone();
                match &ty.kind {
                    TyKind::Array { ty, len_bound } => {
                        let self_path = make_path!(Self);
                        match &ty.kind {
                            TyKind::Ref(path) => {
                                if *path == self_path {
                                    if *access != AccessMode::ImpliedRo || *observable {
                                        self.errors.push(UserError {
                                            kind: UserErrorKind::XpiArrayWithModifier,
                                            span,
                                        });
                                    }
                                    *kind = XpiKind::Array {
                                        num_bound: len_bound.clone(),
                                        is_celled: false,
                                    };
                                    return;
                                }
                            }
                            TyKind::Generic { path, params } => {
                                if path.as_string() == "Cell" && params.params.len() == 1 {
                                    match &params.params[0] {
                                        GenericParam::Ty(ty) => {
                                            match &ty.kind {
                                                TyKind::Ref(path) => {
                                                    if *path == self_path {
                                                        if *access != AccessMode::ImpliedRo || *observable {
                                                            self.errors.push(UserError {
                                                                kind: UserErrorKind::XpiArrayWithModifier,
                                                                span,
                                                            });
                                                        }
                                                        *kind = XpiKind::Array {
                                                            num_bound: len_bound.clone(),
                                                            is_celled: true,
                                                        };
                                                        return;
                                                    }
                                                }
                                                _ => {}
                                            }
                                        }
                                        GenericParam::Expr(_) => {}
                                    }
                                }
                            }
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