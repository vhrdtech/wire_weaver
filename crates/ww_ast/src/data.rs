use crate::ty::Type;
use crate::version::Version;

#[derive(Debug)]
pub enum Fields {
    Named(FieldsNamed),
    Unnamed(FieldsUnnamed),
    // Unit
}

#[derive(Debug)]
pub struct FieldsNamed {
    pub named: Vec<Field>
}

#[derive(Debug)]
pub struct FieldsUnnamed {
    pub unnamed: Vec<Field>
}

#[derive(Debug)]
pub struct Field {
    pub ident: String,
    pub ty: Type,
}

#[derive(Debug)]
pub struct Variant {
    // attrs
    pub ident: String,
    pub fields: Fields,
    pub discriminant: u32,
    pub since: Version,
}