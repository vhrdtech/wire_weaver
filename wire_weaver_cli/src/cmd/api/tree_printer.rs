use anyhow::anyhow;
use console::style;
use proc_macro2::{Ident, Span};
use shrink_wrap_core::ast::Type;
use std::collections::HashMap;
use std::path::PathBuf;
use wire_weaver_core::ast::api::{ApiLevel, Argument, PropertyAccess};
use wire_weaver_core::ast::trait_macro_args::{ImplTraitLocation, ImplTraitMacroArgs};
use wire_weaver_core::ast::visit::{Visit, visit_api_level};
use wire_weaver_core::ast::visitors::IdStack;
use wire_weaver_core::transform::load::load_api_level_recursive;

pub fn tree_printer(
    path: PathBuf,
    name: Option<String>,
    skip_reserved: bool,
) -> anyhow::Result<()> {
    // do some gymnastics to point base_dir at crate root (where Cargo.toml is)
    let mut base_dir = path.clone();
    base_dir.pop(); // pop ww.rs
    base_dir.pop(); // pop src

    let parent = path
        .parent()
        .unwrap()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap(); // likely src folder
    let file_name = path.file_name().unwrap().to_str().unwrap(); // likely ww.rs or src.rs

    let mut cache = HashMap::new();
    let level = load_api_level_recursive(
        &ImplTraitLocation::AnotherFile {
            path: format!("{parent}/{file_name}"),
            part_of_crate: Ident::new("crate", Span::call_site()),
        },
        name.map(|n| Ident::new(n.as_str(), Span::call_site())),
        None,
        base_dir.as_path(),
        &mut cache,
    )
    .map_err(|e| anyhow!(e))?;
    println!(
        "{} {}:",
        style("trait").true_color(0xCF, 0x8E, 0x6D),
        style(&level.name).true_color(0x8D, 0x91, 0xDC)
    );
    visit_api_level(
        &level,
        &mut TreePrinter {
            skip_reserved,
            ..Default::default()
        },
    );
    Ok(())
}

#[derive(Default)]
struct TreePrinter {
    id_stack: IdStack,
    skip_reserved: bool,
}

impl Visit for TreePrinter {
    fn visit_method(&mut self, ident: &Ident, _args: &[Argument], _return_type: &Option<Type>) {
        self.id_stack.print_indent();
        println!("{}: {ident}", style("method").blue());
    }

    fn visit_property(
        &mut self,
        ident: &Ident,
        _ty: &Type,
        _access: PropertyAccess,
        _user_result_ty: &Option<Type>,
    ) {
        self.id_stack.print_indent();
        println!(
            "{}: {ident}",
            style("property").true_color(0xC7, 0x7D, 0xBB)
        );
    }

    fn visit_stream(&mut self, ident: &Ident, _ty: &Type, is_up: bool) {
        self.id_stack.print_indent();
        if is_up {
            println!("{}: {ident}", style("stream").true_color(0xA6, 0xBB, 0x77))
        } else {
            println!("{}: {ident}", style("sink").true_color(0x8C, 0xC8, 0xD4))
        }
    }

    fn visit_impl_trait(&mut self, args: &ImplTraitMacroArgs, level: &ApiLevel) {
        self.id_stack.print_indent();
        println!(
            "{} {} {}::{}",
            style("impl").true_color(0xCF, 0x8E, 0x6D),
            args.resource_name,
            style(level.source_location.crate_name()).true_color(0x8D, 0x91, 0xDC),
            style(&args.trait_name).true_color(0x8D, 0x91, 0xDC),
        );
    }

    fn visit_reserved(&mut self) {
        if !self.skip_reserved {
            self.id_stack.print_indent();
            println!("{}", style("reserved").dim());
        }
    }

    fn hook(&mut self) -> Option<&mut dyn Visit> {
        Some(&mut self.id_stack)
    }
}
