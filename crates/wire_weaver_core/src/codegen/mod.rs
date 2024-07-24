use proc_macro2::TokenStream;
use quote::TokenStreamExt;

use crate::ast::{Context, Item};

// pub mod item;
pub mod item_enum;
// mod op;
mod ty;
mod util;

pub fn generate(cx: &Context, no_alloc: bool) -> TokenStream {
    let mut ts = TokenStream::new();
    for module in &cx.modules {
        for item in &module.items {
            match item {
                Item::Struct(_) => {}
                Item::Enum(item_enum) => {
                    ts.append_all(item_enum::enum_def(item_enum, no_alloc));
                    ts.append_all(item_enum::enum_serdes(item_enum, no_alloc));
                }
            }
        }
    }
    ts
}
