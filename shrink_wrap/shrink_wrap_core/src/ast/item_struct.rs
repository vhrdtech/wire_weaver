use crate::ast::docs::Docs;
use crate::ast::object_size::ObjectSize;
use crate::ast::path::Path;
use crate::ast::ty::Type;
use crate::ast::util::{Cfg, CfgAttrDefmt, Version};
use crate::ast::value::Value;
use proc_macro2::{Ident, Span};
use syn::LitStr;

#[derive(Clone, Debug)]
pub struct ItemStruct {
    pub docs: Docs,
    pub derive: Vec<Path>,
    pub size_assumption: Option<ObjectSize>,
    pub ident: Ident,
    pub fields: Vec<Field>,
    pub cfg: Option<Cfg>,
    pub defmt: Option<CfgAttrDefmt>,
}

#[derive(Clone, Debug)]
pub struct Field {
    pub docs: Docs,
    pub id: u32,
    pub ident: Ident,
    pub ty: Type,
    pub since: Option<Version>,
    pub default: Option<Value>,
}

impl ItemStruct {
    pub fn to_owned(&self, feature: LitStr) -> Self {
        let mut owned = self.clone();
        owned.ident = Ident::new(format!("{}Owned", self.ident).as_str(), self.ident.span());
        owned.cfg = Some(Cfg(feature));
        for f in &mut owned.fields {
            f.ty.make_owned();
        }
        owned.defmt = None;
        owned
    }

    pub fn potential_lifetimes(&self) -> bool {
        for field in &self.fields {
            if field.ty.potential_lifetimes() {
                return true;
            }
        }
        false
    }
}

impl Field {
    pub fn new(id: u32, ident: &str, ty: Type) -> Self {
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
