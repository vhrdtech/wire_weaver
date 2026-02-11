use crate::util;
use console::{StyledObject, style};
use proc_macro2::Ident;
use shrink_wrap_core::ast::Type;
use std::fmt::Write;
use std::path::PathBuf;
use wire_weaver_core::ast::api::{ApiItem, ApiItemKind, ApiLevel, Argument, PropertyAccess};
use wire_weaver_core::ast::trait_macro_args::ImplTraitMacroArgs;
use wire_weaver_core::ast::visit::{Visit, visit_api_level};
use wire_weaver_core::ast::visitors::IdStack;

pub fn tree_printer(
    path: PathBuf,
    name: Option<String>,
    skip_reserved: bool,
    skip_docs: bool,
) -> anyhow::Result<()> {
    let level = util::load_level(path, name)?;
    println!(
        "{} {}:",
        style("trait").true_color(0xCF, 0x8E, 0x6D),
        style(&level.name).true_color(0x8D, 0x91, 0xDC)
    );
    visit_api_level(
        &level,
        &mut TreePrinter {
            skip_reserved,
            skip_docs,
            ..Default::default()
        },
    );
    Ok(())
}

#[derive(Default)]
struct TreePrinter {
    id_stack: IdStack,
    skip_reserved: bool,
    skip_docs: bool,
}

impl Visit for TreePrinter {
    fn hook(&mut self) -> Option<&mut dyn Visit> {
        Some(&mut self.id_stack)
    }

    fn visit_method(&mut self, ident: &Ident, args: &[Argument], return_type: &Option<Type>) {
        self.id_stack.print_indent_and_path();
        print!("{} {ident}(", style("fn").blue());
        for (idx, arg) in args.iter().enumerate() {
            print!(
                "{}: {}",
                arg.ident,
                style(arg.ty.arg_pos_def2(true).to_string()).true_color(0xA6, 0xBB, 0x77)
            );
            if idx + 1 < args.len() {
                print!(", ");
            }
        }
        print!(")");
        if let Some(ret) = return_type {
            print!(
                " -> {}",
                style(ret.arg_pos_def2(true).to_string()).true_color(0xA6, 0xBB, 0x77)
            );
        }
        println!();
    }

    fn visit_property(
        &mut self,
        ident: &Ident,
        ty: &Type,
        access: PropertyAccess,
        user_result_ty: &Option<Type>,
    ) {
        self.id_stack.print_indent_and_path();
        let access = match access {
            PropertyAccess::Const => "const",
            PropertyAccess::ReadOnly => "ro",
            PropertyAccess::ReadWrite => "rw",
            PropertyAccess::WriteOnly => "wo",
        };
        print!(
            "{} {} {ident}: {}",
            style(access).true_color(0xC7, 0x7D, 0xBB),
            style("property").true_color(0xC7, 0x7D, 0xBB),
            style_ty(ty),
        );
        if let Some(ty) = user_result_ty {
            print!(", on_set_err: Result<(), {}>", style_ty(ty))
        }
        println!();
    }

    fn visit_stream(&mut self, ident: &Ident, ty: &Type, is_up: bool) {
        self.id_stack.print_indent_and_path();
        if is_up {
            println!(
                "{} {ident}: {}",
                style("stream").true_color(0x8C, 0xC8, 0xD4),
                style_ty(ty)
            )
        } else {
            println!(
                "{} {ident}: {}",
                style("sink").true_color(0x8C, 0xC8, 0xD4),
                style_ty(ty)
            )
        }
    }

    fn visit_impl_trait(&mut self, args: &ImplTraitMacroArgs, level: &ApiLevel) {
        self.id_stack.print_indent_and_path();
        println!(
            "{} {} {}::{}",
            style("impl").true_color(0xCF, 0x8E, 0x6D),
            args.resource_name,
            style(level.source_location.crate_name()).true_color(0x8D, 0x91, 0xDC),
            style(&args.trait_name).true_color(0x8D, 0x91, 0xDC),
        );
    }

    fn after_visit_impl_trait(&mut self, _args: &ImplTraitMacroArgs, level: &ApiLevel) {
        if self.skip_docs {
            return;
        }
        if level.docs.is_empty() {
            return;
        }
        self.id_stack.print_indented(|w| {
            write!(w, "{}", style(level.docs.to_string()).dim())?;
            Ok(())
        });
    }

    fn visit_reserved(&mut self) {
        if !self.skip_reserved {
            self.id_stack.print_indent_and_path();
            println!("{}", style("reserved").dim());
        }
    }

    fn after_visit_api_item(&mut self, item: &ApiItem) {
        if self.skip_docs {
            return;
        }
        if matches!(item.kind, ApiItemKind::ImplTrait { .. }) {
            // printed in after_visit_impl_trait instead, if printed here - it appears after all child levels
            return;
        }
        if self.skip_reserved && matches!(item.kind, ApiItemKind::Reserved) {
            return;
        }
        if item.docs.is_empty() {
            return;
        }
        self.id_stack.print_indented(|w| {
            write!(w, "{}", style(item.docs.to_string()).dim())?;
            Ok(())
        });
    }
}

fn style_ty(ty: &Type) -> StyledObject<String> {
    style(ty.arg_pos_def2(false).to_string()).true_color(0xA6, 0xBB, 0x77)
}
