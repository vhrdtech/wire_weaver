use crate::ast::ident::Ident;
use crate::ast::syn_convert::{
    collect_unknown_attributes, take_default_attr, take_id_attr, take_since_attr,
    SynConversionError, SynConversionWarning,
};
use crate::ast::ty::Type;
use crate::ast::value::Value;
use crate::ast::version::Version;
use proc_macro2::{Span, TokenStream};
use quote::quote;
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
    pub(crate) fn from_syn(
        def_order_idx: u32,
        mut field: syn::Field,
    ) -> Result<(Self, Vec<SynConversionWarning>), Vec<SynConversionError>> {
        let (ty, mut warnings) = Type::from_syn(field.ty)?;
        let mut errors = vec![];
        let default = take_default_attr(&mut field.attrs, &mut errors);
        if errors.is_empty() {
            collect_unknown_attributes(&mut field.attrs, &mut warnings);
            Ok((
                Field {
                    id: take_id_attr(&mut field.attrs).unwrap_or(def_order_idx),
                    ident: field
                        .ident
                        .map(|i| i.into())
                        .unwrap_or(Ident::new(format!("_{def_order_idx}"))),
                    ty,
                    since: take_since_attr(&mut field.attrs),
                    default,
                },
                warnings,
            ))
        } else {
            Err(errors)
        }
    }

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
