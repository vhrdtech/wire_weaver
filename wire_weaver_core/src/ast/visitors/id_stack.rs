use crate::ast::api::{ApiItem, ApiLevel, Multiplicity};
use crate::ast::trait_macro_args::ImplTraitMacroArgs;
use crate::ast::visit::Visit;
use console::style;

#[derive(Default)]
pub struct IdStack {
    /// next id to use on each API level
    stack: Vec<PathSegment>,
    current_item: Option<PathSegment>,
}

#[derive(Clone)]
enum PathSegment {
    Id(u32),
    Array {
        id: u32,
        custom_index_ty: Option<String>,
    },
}

impl PathSegment {
    fn print(&self) {
        match self {
            PathSegment::Id(id) => {
                print!("{id}");
            }
            PathSegment::Array {
                id,
                custom_index_ty,
            } => {
                if let Some(custom_index_ty) = custom_index_ty {
                    print!(
                        "{id}/{}{}{}",
                        style("[").red(),
                        style(custom_index_ty).red(),
                        style("]").red()
                    );
                } else {
                    print!("{id}/{}", style("[u32]").red());
                }
            }
        }
    }
}

impl IdStack {
    pub fn print_indent(&self) {
        for i in 0..self.stack.len() {
            if i == 0 {
                print!("  ");
            } else {
                print!("|  ");
            }
        }
    }

    pub fn print_indent_and_path(&self) {
        self.print_indent();
        for path_segment in &self.stack {
            path_segment.print();
            print!("/");
        }
        if let Some(current) = &self.current_item {
            current.print();
        }
        print!(": ");
    }

    pub fn print_indented<F: Fn(&mut String) -> Result<(), std::fmt::Error>>(&self, f: F) {
        let mut s = String::new();
        f(&mut s).unwrap();
        for line in s.split(['\n', '\r']) {
            self.print_indent();
            println!("  {}", line);
        }
    }
}

impl Visit for IdStack {
    fn visit_api_item(&mut self, item: &ApiItem) {
        let s = if let Multiplicity::Array { index_type } = &item.multiplicity {
            PathSegment::Array {
                id: item.id,
                custom_index_ty: index_type.as_ref().map(|i| i.to_string()),
            }
        } else {
            PathSegment::Id(item.id)
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
