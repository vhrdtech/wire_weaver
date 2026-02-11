use crate::ast::api::{ApiItem, ApiLevel, Multiplicity};
use crate::ast::trait_macro_args::ImplTraitMacroArgs;
use crate::ast::visit::Visit;
use convert_case::{Case, Casing};

/// Visitor that keeps a stack of trait names (level names).
/// Additionally, for each array of traits, index type (or u32) is tracked as well.
#[derive(Default)]
pub struct IndexChain {
    stack: Vec<ChainSegment>,
    current_item: Option<ChainSegment>,
}

#[derive(Debug)]
pub struct Index {
    pub name: String,
    pub kind: IndexKind,
}

#[derive(Debug)]
pub enum IndexKind {
    OneDU32,
    OneDUser { user_ty: String },
}

impl IndexChain {
    /// Returns a name like: `gpio_bank_gpio_set_high`, where `gpio_bank` is created from `GpioBank`, `gpio` from `Gpio` trait names,
    /// and `set_high` is a method name at the last level.
    pub fn flattened_name(&self) -> String {
        let mut name = String::new();
        for s in &self.stack {
            name += s.to_snake_case().as_str();
            name += "_";
        }
        if let Some(current) = &self.current_item {
            name += &current.to_snake_case();
        }
        name
    }

    /// Returns all the array indices that lead to the current item.
    ///
    /// For example if there is an array `banks` of `GpioBank`'s with a custom index type `MyIndex` and inside,
    /// there is another array `pin` of `Gpio`'s and this method is called for one of the `Gpio` trait resources:
    /// * `[Index{name: "banks_index", kind: OneDUser("MyIndex")}, Index{name: "pin_index", kind: OneDU32}]`
    pub fn array_indices(&self) -> Vec<Index> {
        self.stack
            .iter()
            .filter_map(|s| {
                if let ChainSegment::Array {
                    ident,
                    custom_index_ty,
                    ..
                } = s
                {
                    let level_name = ident.clone().unwrap_or(String::new()) + "_index";
                    if let Some(user_ty) = custom_index_ty.clone() {
                        Some(Index {
                            name: level_name,
                            kind: IndexKind::OneDUser { user_ty },
                        })
                    } else {
                        Some(Index {
                            name: level_name,
                            kind: IndexKind::OneDU32,
                        })
                    }
                } else {
                    None
                }
            })
            .collect()
    }
}

#[derive(Clone)]
enum ChainSegment {
    Id(Option<String>),
    Array {
        ident: Option<String>,
        // trait_name: Option<Ident>,
        custom_index_ty: Option<String>,
    },
}

impl ChainSegment {
    fn to_snake_case(&self) -> String {
        match self {
            ChainSegment::Id(ident) | ChainSegment::Array { ident, .. } => {
                if let Some(ident) = ident {
                    ident.to_string().to_case(Case::Snake)
                } else {
                    "reserved".to_string()
                }
            }
        }
    }
}

impl Visit for IndexChain {
    fn visit_api_item(&mut self, item: &ApiItem) {
        let s = if let Multiplicity::Array { index_type } = &item.multiplicity {
            // let mut trait_name = None;
            // if let ApiItemKind::ImplTrait { level, .. } = &item.kind {
            //     trait_name = Some(level.as_ref().expect("").name.clone());
            // }
            ChainSegment::Array {
                ident: item.ident().map(|i| i.to_string()),
                // trait_name,
                custom_index_ty: index_type.as_ref().map(|i| i.to_string()),
            }
        } else {
            ChainSegment::Id(item.ident().map(|i| i.to_string()))
        };
        self.current_item = Some(s);
    }

    fn after_visit_impl_trait(&mut self, _args: &ImplTraitMacroArgs, _level: &ApiLevel) {
        if let Some(s) = &self.current_item {
            self.stack.push(s.clone());
        }
    }

    fn after_visit_api_item(&mut self, _item: &ApiItem) {
        self.current_item = None;
    }

    fn after_visit_level(&mut self, _level: &ApiLevel) {
        self.stack.pop();
    }
}
