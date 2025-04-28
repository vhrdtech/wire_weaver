use proc_macro2::TokenStream;
use quote::TokenStreamExt;
use syn::{File, Item};
use wire_weaver_core::ast::Source;
use wire_weaver_core::codegen::item_enum::{enum_discriminant, enum_lifetime, enum_serdes};
use wire_weaver_core::codegen::item_struct::struct_serdes;
use wire_weaver_core::transform::syn_util::{take_repr_attr, take_shrink_wrap_attr};
use wire_weaver_core::transform::{Messages, Transform};

pub fn shrink_wrap(item: proc_macro::TokenStream) -> TokenStream {
    let item: TokenStream = item.into();
    let file = syn::parse2::<File>(item).unwrap();

    let mut messages = Messages::default();
    let mut common_attrs = Vec::new();
    for item in &file.items {
        match item {
            Item::Enum(item_enum) => {
                let mut attrs = item_enum.attrs.clone();
                let repr = take_repr_attr(&mut attrs, &mut messages);
                if repr.is_none() {
                    panic!("enums must be #[repr(u8/u16/u32)]")
                }
                common_attrs = attrs;
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

    let mut transform = Transform::new();
    transform.push_file(Source::String("shrink_wrap_derive".into()), file);
    let add_derives = [];
    let cx = transform.transform(&add_derives);
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
                    let ts = struct_serdes(item_struct, no_alloc);
                    codegen_ts.append_all(ts);
                }
                wire_weaver_core::ast::Item::Enum(item_enum) => {
                    let lifetime = enum_lifetime(item_enum, no_alloc);
                    let ts = enum_discriminant(item_enum, lifetime);
                    codegen_ts.append_all(ts);

                    let ts = enum_serdes(item_enum, no_alloc);
                    codegen_ts.append_all(ts);
                }
                _ => {}
            }
        }
    }
    codegen_ts
}
