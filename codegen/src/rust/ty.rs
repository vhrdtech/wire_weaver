use mtoken::{ToTokens, TokenStream, Span, Ident};
use mquote::mquote;
use parser::ast::ty::Ty;
use std::marker::PhantomData;

pub struct CGTy<'i, 'c> {
    pub inner: &'c Ty,
    pub _p: &'i PhantomData<()>
}

impl<'i, 'c> ToTokens for CGTy<'i, 'c> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self.inner {
            Ty::Boolean => {
                tokens.append(Ident::new("bool", Span::call_site()));
            }
            Ty::Discrete { is_signed, bits } => {
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
                let discrete = format_ident!("{}{}", is_signed, bits);
                tokens.append_all(mquote!(rust r#"
                    #discrete
                "#));
            }
            Ty::FixedPoint { .. } => {}
            Ty::FloatingPoint { .. } => {}
            Ty::Textual => {}
            Ty::Sequence => {}
            Ty::UserDefined => {}
        }
    }
}

fn is_native_discrete(bits: u32) -> bool {
    bits == 8 || bits == 16 || bits == 32 || bits == 64 || bits == 128
}