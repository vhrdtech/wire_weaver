use mtoken::{ToTokens, TokenStream, Span, Ident, ext::TokenStreamExt};
use mquote::mquote;
use parser::ast::ty::TyKind;
use std::marker::PhantomData;

pub struct CGTy<'i, 'c> {
    pub inner: &'c TyKind<'i>,
    pub _p: &'i PhantomData<()>
}

impl<'i, 'c> ToTokens for CGTy<'i, 'c> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self.inner {
            TyKind::Boolean => {
                tokens.append(Ident::new("bool", Span::call_site()));
            }
            TyKind::Discrete { is_signed, bits, .. } => {
                let is_signed = if is_native_discrete(*bits) {
                    if *is_signed {
                        "i"
                    } else {
                        "u"
                    }
                } else {
                    if *is_signed {
                        "VI"
                    } else {
                        "VU"
                    }
                };
                let discrete = Ident::new(&format!("{}{}", is_signed, bits), Span::call_site());
                tokens.append_all(mquote!(rust r#"
                    #discrete
                "#));
            }
            TyKind::FixedPoint { .. } => {}
            TyKind::FloatingPoint { .. } => {}
            TyKind::Array { .. } => {}
            TyKind::Tuple(_) => {}
            TyKind::Fn { .. } => {}
            TyKind::Generic { .. } => {}
            TyKind::Char => {}
            TyKind::String => {}
            TyKind::Sequence => {}
            TyKind::UserDefined(_) => {}
            TyKind::AutoNumber(_) => {}
            TyKind::IndexOf(_) => {}
            TyKind::Derive => {}
        }
    }
}

fn is_native_discrete(bits: u32) -> bool {
    bits == 8 || bits == 16 || bits == 32 || bits == 64 || bits == 128
}