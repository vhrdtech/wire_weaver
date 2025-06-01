use proc_macro2::TokenStream;
use quote::TokenStreamExt;
use syn::{File, Item};
use wire_weaver_core::ast::Source;
use wire_weaver_core::codegen::item_enum::{enum_def, enum_serdes};
use wire_weaver_core::codegen::item_struct::{struct_def, struct_serdes};
use wire_weaver_core::transform::syn_util::{take_owned_attr, take_shrink_wrap_attr};
use wire_weaver_core::transform::{Messages, Transform};

pub fn shrink_wrap(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let file = syn::parse2::<File>(item).unwrap();

    let mut messages = Messages::default();
    let mut common_attrs = Vec::new();
    for item in &file.items {
        match item {
            Item::Enum(item_enum) => {
                common_attrs = item_enum.attrs.clone();
            }
            Item::Struct(item_struct) => {
                common_attrs = item_struct.attrs.clone();
            }
            _ => {}
        }
    }

    let mut no_alloc = false;
    let attr = take_shrink_wrap_attr(&mut common_attrs, &mut messages);
    if let Some(attr) = attr {
        if attr == "no_alloc" {
            no_alloc = true;
        }
    }
    let generate_owned = take_owned_attr(&mut common_attrs, &mut messages);

    let mut transform = Transform::new();
    transform.push_file(Source::String("shrink_wrap_derive".into()), file);
    let add_derives = [];
    let cx = transform.transform(&add_derives, true);
    for (source, messages) in transform.messages() {
        for message in messages.messages() {
            eprintln!("{:?} {:?}", source, message);
        }
    }
    for message in messages.messages() {
        eprintln!("{:?}", message);
    }
    let cx = cx.expect("ww transform failed");

    let mut codegen_ts = TokenStream::new();
    for module in &cx.modules {
        for item in &module.items {
            match item {
                wire_weaver_core::ast::Item::Struct(item_struct) => {
                    codegen_ts.append_all(struct_def(item_struct, no_alloc));
                    codegen_ts.append_all(struct_serdes(item_struct, no_alloc));
                    if let Some(feature) = &generate_owned {
                        let struct_owned = item_struct.to_owned(feature.clone());
                        codegen_ts.append_all(struct_def(&struct_owned, false));
                        codegen_ts.append_all(struct_serdes(&struct_owned, false));
                    }
                }
                wire_weaver_core::ast::Item::Enum(item_enum) => {
                    codegen_ts.append_all(enum_def(item_enum, no_alloc));
                    codegen_ts.append_all(enum_serdes(item_enum, no_alloc));
                    if let Some(feature) = &generate_owned {
                        let enum_owned = item_enum.to_owned(feature.clone());
                        codegen_ts.append_all(enum_def(&enum_owned, false));
                        codegen_ts.append_all(enum_serdes(&enum_owned, false));
                    }
                }
                _ => {}
            }
        }
    }
    codegen_ts
}
