use crate::ast::data::Fields;
use crate::ast::item::Item;
use crate::ast::WWFile;
use crate::ast2::{Kind, SerDesPlan};

// TODO: check that no fields and no variants have the same name
// TODO: check that variants fit within chosen repr

pub(crate) fn transform(file: &WWFile) -> Vec<SerDesPlan> {
    let mut plans = vec![];
    for item in &file.items {
        match item {
            Item::Enum(item_enum) => {
                let mut variants = vec![];
                for v in &item_enum.variants {
                    match v.fields {
                        Fields::Named(fields_named) => {}
                        Fields::Unnamed(fields_unnamed) => {}
                        Fields::Unit => {}
                    }
                }
                plans.push(SerDesPlan {
                    ty_name: item_enum.ident.clone(),
                    kind: Kind::Enum {
                        repr: item_enum.repr,
                        variants,
                    },
                })
            }
            Item::Struct(_) => {}
        }
    }
    plans
}
