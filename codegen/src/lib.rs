pub mod rust;
pub mod ast_wrappers;
pub mod dart;

use quote::quote;

use parser::ast::item_enum::ItemEnum;
use crate::rust::item_enum::CGItemEnum as RCGItemEnum;
use crate::dart::item_enum::CGItemEnum as DCGItemEnum;

pub fn fun(ast_item_enum: &ItemEnum) -> u32 {
    let item_enum = RCGItemEnum::new(ast_item_enum);
    let tokens = quote! {
        #item_enum
    };
    println!("{:?}", tokens);
    println!("{}", tokens);

    let item_enum = DCGItemEnum::new(ast_item_enum);
    let tokens = quote! {
        #item_enum
    };
    println!("{:?}", tokens);
    println!("{}", tokens);

    0
}

pub fn fun2() {
    use mquote::mquote;
    let ts = mquote!(rust r#"
        struct #{name.field}
    "#);
    println!("{}", ts);
}