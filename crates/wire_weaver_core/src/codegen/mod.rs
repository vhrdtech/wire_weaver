use proc_macro2::TokenStream;
use quote::TokenStreamExt;

use crate::ast::{Context, Item};

// pub mod item;
pub mod item_enum;
// mod op;
pub mod api_client;
mod api_common;
pub mod api_server;
pub mod item_struct;
mod ty;
mod util;

pub fn generate(cx: &Context, no_alloc: bool) -> TokenStream {
    let mut ts = TokenStream::new();
    for module in &cx.modules {
        for item in &module.items {
            match item {
                Item::Struct(item_struct) => {
                    ts.append_all(item_struct::struct_def(item_struct, no_alloc));
                    ts.append_all(item_struct::struct_serdes(item_struct, no_alloc));
                }
                Item::Enum(item_enum) => {
                    ts.append_all(item_enum::enum_def(item_enum, no_alloc));
                    ts.append_all(item_enum::enum_serdes(item_enum, no_alloc));
                }
            }
        }
    }
    ts
}
