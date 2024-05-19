mod item;
mod ty;

use crate::ast::item::Item;
use crate::ast::File;
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt};

pub fn rust_no_std_file(file: &File) -> TokenStream {
    let mut ts = TokenStream::new();
    for item in &file.items {
        match item {
            Item::Enum(item_enum) => {
                ts.append_all(item::enum_def(item_enum, true));
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
