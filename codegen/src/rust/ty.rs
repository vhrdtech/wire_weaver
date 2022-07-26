use mtoken::{ToTokens, TokenStream, Span, Ident, ext::TokenStreamExt};
use mquote::mquote;
use parser::ast::item_type::Type;
use std::marker::PhantomData;

pub struct CGTy<'i, 'c> {
    pub inner: &'c Type<'i>,
    pub _p: &'i PhantomData<()>
}

impl<'i, 'c> ToTokens for CGTy<'i, 'c> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self.inner {
            Type::Boolean => {
                tokens.append(Ident::new("bool", Span::call_site()));
            }
            Type::Discrete { is_signed, bits, shift } => {
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
            Type::FixedPoint { .. } => {}
            Type::FloatingPoint { .. } => {}
            Type::Textual(_) => {}
            Type::Sequence => {}
            Type::UserDefined => {}
            Type::AutoNumber(_) => {}
        }
    }
}

fn is_native_discrete(bits: u32) -> bool {
    bits == 8 || bits == 16 || bits == 32 || bits == 64 || bits == 128
}