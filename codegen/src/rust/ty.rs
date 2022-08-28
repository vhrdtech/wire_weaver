use crate::prelude::*;
use vhl::ast::ty::TyKind;
use crate::rust::identifier::Identifier;

#[derive(Clone)]
pub struct Ty {
    pub inner: vhl::ast::ty::Ty
}

impl From<vhl::ast::ty::Ty> for Ty {
    fn from(ty: vhl::ast::ty::Ty) -> Self {
        Ty {
            inner: ty,
        }
    }
}

impl ToTokens for Ty {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match &self.inner.kind {
            TyKind::Boolean => {
                tokens.append(
                    mtoken::Ident::new(
                        Rc::new("bool".to_string()),
                        IdentFlavor::Plain,
                        self.inner.span.clone()
                    )
                );
            }
            TyKind::Discrete(discrete) => {
                let is_signed = if is_native_discrete(discrete.bits) {
                    if discrete.is_signed {
                        "i"
                    } else {
                        "u"
                    }
                } else {
                    if discrete.is_signed {
                        "VI"
                    } else {
                        "VU"
                    }
                };
                let discrete = Identifier {
                    inner: vhl::ast::identifier::Identifier {
                        symbols: Rc::new(format!("{}{}", is_signed, discrete.bits)),
                        context: IdentifierContext::UserTyName,
                        span: self.inner.span.clone()
                    }
                };
                tokens.append_all(mquote!(rust r#"
                    #discrete
                "#));
            }
        }
    }
}

fn is_native_discrete(bits: u32) -> bool {
    bits == 8 || bits == 16 || bits == 32 || bits == 64 || bits == 128
}