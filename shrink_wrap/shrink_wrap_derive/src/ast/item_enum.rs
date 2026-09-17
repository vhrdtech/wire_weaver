use crate::ast::docs::Docs;
use crate::ast::item_struct::Field;
use crate::ast::ty::Type;
use crate::ast::util::Version;
use proc_macro2::Ident;

#[derive(Clone, Debug)]
pub(crate) struct ItemEnum {
    pub(crate) docs: Docs,
    // pub(crate) ident: Ident,
    pub(crate) variants: Vec<Variant>,
}

#[derive(Clone, Debug)]
pub(crate) enum Fields {
    Named(Vec<Field>),
    Unnamed(Vec<Type>),
    Unit,
}

#[derive(Clone, Debug)]
pub(crate) struct Variant {
    pub(crate) docs: Docs,
    pub(crate) ident: Ident,
    pub(crate) fields: Fields,
    pub(crate) discriminant: u32,
    pub(crate) _since: Option<Version>,
}

impl ItemEnum {
    pub(crate) fn to_discriminants(&mut self) {
        for v in &mut self.variants {
            v.fields = Fields::Unit;
        }
    }

    pub(crate) fn make_owned(&mut self) {
        for v in &mut self.variants {
            match &mut v.fields {
                Fields::Named(named) => {
                    for f in named {
                        f.ty.make_owned();
                    }
                }
                Fields::Unnamed(unnamed) => {
                    for f in unnamed {
                        f.make_owned();
                    }
                }
                Fields::Unit => {}
            }
        }
    }
}
