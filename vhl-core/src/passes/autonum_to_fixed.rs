use ast::{FixedTy, NumBound, Ty, TyKind, VisitMut};

pub struct AutonumToFixed {}

impl VisitMut for AutonumToFixed {
    fn visit_ty(&mut self, i: &mut Ty) {
        if let TyKind::AutoNumber(_autonum) = &i.kind {
            i.kind = TyKind::Fixed(FixedTy {
                is_signed: false,
                m: 1,
                n: 7,
                num_bound: NumBound::Unbound,
                unit: (),
            });
        }
    }
}
