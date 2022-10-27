use ast::{NumBound, Path, TryEvaluateInto, Ty, TyKind};
use vhl::project::Project;
use vhl_stdlib::serdes::SerDesSize;
use crate::error::CodegenError;

pub fn size_in_byte_buf(of: &Ty, at_path: &Path, _project: &Project) -> Result<SerDesSize, CodegenError> {
    match &of.kind {
        TyKind::Unit => Ok(SerDesSize::Sized(0)),
        TyKind::Boolean => Ok(SerDesSize::Sized(1)),
        TyKind::Discrete(discrete) => {
            if discrete.is_standard() {
                Ok(SerDesSize::Sized(discrete.bits as usize / 8))
            } else {
                todo!()
            }
        }
        TyKind::Fixed(_) => todo!(),
        TyKind::Float(_) => todo!(),
        TyKind::Array { .. } => todo!(),
        TyKind::Tuple { .. } => todo!(),
        TyKind::Char => Ok(SerDesSize::UnsizedBound(4)), // UTF8 codepoint
        TyKind::String { len_bound } => match len_bound {
            NumBound::Unbound | NumBound::MinBound(_) => Ok(SerDesSize::Unsized),
            NumBound::MaxBound(_max) => todo!(),
            NumBound::Set(set) => match set {
                TryEvaluateInto::Resolved(set) => Ok(SerDesSize::UnsizedBound(set.max_len())),
                TryEvaluateInto::NotResolved(_) | TryEvaluateInto::Error => {
                    return Err(CodegenError::Internal("size_in_byte_buf: not processed AST is given to codegen".to_owned()));
                }
            },
        },
        TyKind::Ref(user_path) => {
            println!("size_in_byte_buf of: {} at: {}", user_path, at_path);
            Ok(SerDesSize::Unsized)
        },
        TyKind::Derive
        | TyKind::Fn { .. }
        | TyKind::AutoNumber(_)
        | TyKind::IndexTyOf(_)
        | TyKind::Generic { .. } => {
            return Err(CodegenError::Internal("size_in_byte_buf: wrong AST is given to codegen".to_owned()));
        },
    }
}
