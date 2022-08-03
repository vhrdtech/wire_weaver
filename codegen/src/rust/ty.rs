use mtoken::{ToTokens, TokenStream, Span, Ident, ext::TokenStreamExt};
use mquote::mquote;
use parser::ast::ty::Ty;
use std::marker::PhantomData;

pub struct CGTy<'i, 'c> {
    pub inner: &'c Ty<'i>,
    pub _p: &'i PhantomData<()>
}

impl<'i, 'c> ToTokens for CGTy<'i, 'c> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self.inner {
            Ty::Boolean => {
                tokens.append(Ident::new("bool", Span::call_site()));
            }
            Ty::Discrete { is_signed, bits, shift } => {
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
            Ty::FixedPoint { .. } => {}
            Ty::FloatingPoint { .. } => {}
            Ty::Array { .. } => {}
            Ty::Tuple(_) => {}
            Ty::Fn { .. } => {}
            Ty::Generic { .. } => {}
            Ty::Textual(_) => {}
            Ty::Sequence => {}
            Ty::UserDefined(_) => {}
            Ty::AutoNumber(_) => {}
            Ty::IndexOf(_) => {}
            Ty::Derive => {}
        }
    }
}

fn is_native_discrete(bits: u32) -> bool {
    bits == 8 || bits == 16 || bits == 32 || bits == 64 || bits == 128
}