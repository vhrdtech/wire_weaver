use quote::TokenStreamExt;

// pub mod item;
pub mod item_enum;
// mod op;
mod ty;
mod util;

// pub fn rust_no_std_file(cx: &Context, alloc: bool) -> TokenStream {
//     let mut ts = TokenStream::new();
//     for module in &cx.modules {
//         for item in &module.items {
//             match item {
//                 Item::Struct(_) => {}
//                 Item::Enum(item_enum) => {
//                     ts.append_all(item_enum::)
//                 }
//             }
//         }
//     }
//     ts
// }
