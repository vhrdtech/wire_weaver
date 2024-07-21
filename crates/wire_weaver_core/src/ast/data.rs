use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{Lit, LitInt};

use crate::ast::ident::Ident;
use crate::ast::syn_convert::{
    collect_unknown_attributes, take_default_attr, take_id_attr, take_since_attr,
    SynConversionError, SynConversionWarning,
};
use crate::ast::ty::Type;
use crate::ast::value::Value;
use crate::ast::version::Version;

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
    // attrs
    pub id: u32,
    pub ident: Ident,
    pub ty: Type,
    pub since: Option<Version>,
    pub default: Option<Value>,
}

#[derive(Debug)]
pub struct Variant {
    // attrs
    pub ident: Ident,
    pub fields: Fields,
    pub discriminant: u32,
    pub since: Option<Version>,
}

impl Field {
    pub(crate) fn handle_eob(&self) -> TokenStream {
        match &self.default {
            None => quote!(?),
            Some(value) => {
                let value = value.to_lit();
                quote!(.unwrap_or(#value))
            }
        }
    }
}

impl Variant {
    pub(crate) fn discriminant_lit(&self) -> syn::Lit {
        Lit::Int(LitInt::new(
            format!("{}", self.discriminant).as_str(),
            Span::call_site(),
        ))
    }

    pub fn is_unit(&self) -> bool {
        matches!(self.fields, Fields::Unit)
    }
}
