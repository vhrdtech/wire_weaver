use ast::{NumBound, TryEvaluateInto, Ty, TyKind};
use vhl_stdlib::serdes::SerDesSize;

pub fn size_in_buf(of: &Ty) -> SerDesSize {
    match &of.kind {
        TyKind::Unit => SerDesSize::Sized(0),
        TyKind::Boolean => SerDesSize::Sized(1),
        TyKind::Discrete(discrete) => {
            if discrete.is_standard() {
                SerDesSize::Sized(discrete.bits as usize / 8)
            } else {
                todo!()
            }
        }
        TyKind::Fixed(_) => todo!(),
        TyKind::Float(_) => todo!(),
        TyKind::Array { .. } => todo!(),
        TyKind::Tuple { .. } => todo!(),
        TyKind::Char => SerDesSize::UnsizedBound(4), // UTF8 codepoint
        TyKind::String { len_bound } => match len_bound {
            NumBound::Unbound | NumBound::MinBound(_) => SerDesSize::Unsized,
            NumBound::MaxBound(_max) => todo!(),
            NumBound::Set(set) => match set {
                TryEvaluateInto::Resolved(set) => SerDesSize::UnsizedBound(set.max_len()),
                TryEvaluateInto::NotResolved(_) | TryEvaluateInto::Error => {
                    panic!("internal: not processed AST is given to codegen")
                }
            },
        },
        TyKind::UserDefined(_) => todo!(), // need to resolve first
        TyKind::Derive
        | TyKind::Fn { .. }
        | TyKind::AutoNumber(_)
        | TyKind::IndexTyOf(_)
        | TyKind::Generic { .. } => panic!("internal: invalid AST is given to codegen"),
    }
}
