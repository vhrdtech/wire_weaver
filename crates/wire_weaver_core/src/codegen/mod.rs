use proc_macro2::TokenStream;
use quote::TokenStreamExt;

use crate::ast::item::Item;
use crate::ast::WWFile;

pub mod item;
pub mod item_enum;
mod op;
mod ty;

pub fn rust_no_std_file(file: &WWFile) -> TokenStream {
    let mut ts = TokenStream::new();
    for item in &file.items {
        match item {
            Item::Enum(item_enum) => {
                ts.append_all(item::enum_def(item_enum, true));
                ts.append_all(item::enum_discriminant(item_enum));
                ts.append_all(item::enum_serdes(item_enum, true));
            }
            Item::Struct(item_struct) => {
                ts.append_all(item::struct_def(item_struct, true));
                ts.append_all(item::struct_serdes(item_struct, true));
            }
        }
    }
    ts
}
