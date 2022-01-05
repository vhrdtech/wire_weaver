pub mod rust;
pub mod ast_wrappers;

use quote::quote;

use parser::ast::item_enum::ItemEnum;
use crate::rust::item_enum::CGItemEnum;

pub fn fun(item_enum: &ItemEnum) -> u32 {
    let item_enum = CGItemEnum::new(item_enum);
    let tokens = quote! {
        #item_enum
    };
    println!("{:?}", tokens);
    println!("{}", tokens);
    0
}