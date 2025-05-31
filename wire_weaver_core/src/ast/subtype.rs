use crate::ast::Type;
use crate::ast::ident::Ident;
use std::ops::RangeInclusive;
use syn::LitInt;

pub struct SubType {
    name: Ident,
    base_ty: Type,
    valid_range: Option<RangeInclusive<LitInt>>,
    valid_list: Vec<LitInt>,
}
