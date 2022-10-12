use crate::ast::{FixedTy, Ty, VisitMut};
use crate::ast::ty::TyKind;

pub struct AutonumToFixed {}

impl VisitMut for AutonumToFixed {
    fn visit_ty(&mut self, i: &mut Ty) {
        if let TyKind::AutoNumber(_autonum) = &i.kind {
            i.kind = TyKind::Fixed(FixedTy {
                is_signed: false,
                m: 1,
                n: 7,
                shift: 0,
            });
        }
    }
}