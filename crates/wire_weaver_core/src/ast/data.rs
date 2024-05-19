use crate::ast::ident::Ident;
use crate::ast::ty::Type;
use crate::ast::version::Version;
use proc_macro2::Span;
use syn::{Lit, LitInt};

#[derive(Debug)]
pub enum Fields {
    Named(FieldsNamed),
    Unnamed(FieldsUnnamed),
    Unit,
}

#[derive(Debug)]
pub struct FieldsNamed {
    pub named: Vec<Field>,
}

#[derive(Debug)]
pub struct FieldsUnnamed {
    pub unnamed: Vec<Field>,
}

#[derive(Debug)]
pub struct Field {
    pub ident: Ident,
    pub ty: Type,
}

#[derive(Debug)]
pub struct Variant {
    // attrs
    pub ident: Ident,
    pub fields: Fields,
    pub discriminant: u32,
    pub since: Option<Version>,
}

impl Variant {
    pub(crate) fn discriminant_lit(&self) -> syn::Lit {
        Lit::Int(LitInt::new(
            format!("{}", self.discriminant).as_str(),
            Span::call_site(),
        ))
    }
}
