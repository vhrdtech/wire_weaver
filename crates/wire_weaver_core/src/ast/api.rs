use crate::ast::ident::Ident;
use crate::ast::Type;

#[derive(Debug)]
pub struct ApiLevel {
    pub docs: Vec<String>,
    pub items: Vec<ApiItem>,
}

#[derive(Debug)]
pub struct ApiItem {
    pub id: u16,
    pub multiplicity: Multiplicity,
    pub kind: ApiItemKind,
}

#[derive(Debug)]
pub enum Multiplicity {
    Flat,
    Array { size_bound: u32 },
}

#[derive(Debug)]
pub enum ApiItemKind {
    Method {
        ident: Ident,
        args: Vec<Argument>,
        return_type: Option<Type>,
    },
    Property,
    Stream {
        ident: Ident,
        ty: Type,
        is_up: bool,
    },
    ImplTrait,
    Level(Box<ApiLevel>),
}

#[derive(Debug)]
pub struct Argument {
    pub ident: Ident,
    pub ty: Type,
}
