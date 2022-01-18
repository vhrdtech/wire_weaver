pub mod multilang;
pub mod rust;
pub mod ast_wrappers;
pub mod dart;

use parser::ast::item_enum::ItemEnum;
use crate::rust::item_enum::CGItemEnum as RCGItemEnum;
use crate::dart::item_enum::CGItemEnum as DCGItemEnum;
use mquote::mquote;
use mtoken::ToTokens;

pub fn fun(ast_item_enum: &ItemEnum) -> u32 {
    let item_enum = RCGItemEnum::new(ast_item_enum);
    let tokens = mquote! { rust r#"
        #item_enum
    "#};
    // println!("rust: {:?}", tokens);
    println!("rust:\n{}", tokens);

    let item_enum = DCGItemEnum::new(ast_item_enum);
    let tokens = mquote! { rust r#"
        #item_enum
    "#};
    // println!("dart: {:?}", tokens);
    println!("dart:\n{}", tokens);

    0
}

pub fn fun2() {

}