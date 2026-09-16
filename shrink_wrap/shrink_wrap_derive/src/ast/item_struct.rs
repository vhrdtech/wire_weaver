use crate::ast::docs::Docs;
use crate::ast::object_size::ObjectSize;
use crate::ast::path::Path;
use crate::ast::ty::Type;
use crate::ast::util::{Cfg, CfgAttrDefmt, CfgAttrSerde, Version};
use crate::ast::value::Value;
use proc_macro2::{Ident, Span};
use syn::LitStr;

#[derive(Clone, Debug)]
pub(crate) struct ItemStruct {
    pub(crate) docs: Docs,
    pub(crate) derive_borrowed: Vec<Path>,
    pub(crate) derive_owned: Vec<Path>,
    pub(crate) size_assumption: Option<ObjectSize>,
    pub(crate) ident: Ident,
    pub(crate) fields: Vec<Field>,
    pub(crate) cfg: Option<Cfg>,
    pub(crate) defmt: Option<CfgAttrDefmt>,
    pub(crate) serde: Option<CfgAttrSerde>,
}

#[derive(Clone, Debug)]
pub(crate) struct Field {
    pub(crate) docs: Docs,
    pub(crate) id: u32,
    pub(crate) ident: Ident,
    pub(crate) ty: Type,
    pub(crate) since: Option<Version>,
    pub(crate) default: Option<Value>,
}

impl ItemStruct {
    pub(crate) fn to_owned(&self, feature: LitStr) -> Self {
        let mut owned = self.clone();
        owned.ident = Ident::new(format!("{}Owned", self.ident).as_str(), self.ident.span());
        owned.cfg = Some(Cfg(feature));
        for f in &mut owned.fields {
            f.ty.make_owned();
        }
        owned.defmt = None;
        owned.derive_owned = core::mem::take(&mut owned.derive_owned);
        owned
    }

    pub(crate) fn potential_lifetimes(&self) -> bool {
        for field in &self.fields {
            if field.ty.potential_lifetimes() {
                return true;
            }
        }
        false
    }
}

impl Field {
    pub(crate) fn new(id: u32, ident: &str, ty: Type) -> Self {
        Self {
            docs: Docs::empty(),
            id,
            ident: Ident::new(ident, Span::call_site()),
            ty,
            since: None,
            default: None,
        }
    }
}
