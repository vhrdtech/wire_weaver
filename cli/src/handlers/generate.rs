use std::collections::HashMap;
use std::rc::Rc;
use ast::{make_path};
use codegen::Codegen;
use parser::ast::file::FileParse;
use vhl::project::Project;
use super::prelude::*;
use crate::commands::GenerateArgs;

pub fn generate_subcmd(generate_args: GenerateArgs) -> Result<()> {
    let input = std::fs::read_to_string(generate_args.vhl_source.clone())
        .context(format!("unable to open '{:?}'", generate_args.vhl_source))?;
    let origin = SpanOrigin::Parser(SourceOrigin::File(Rc::new(generate_args.vhl_source.clone())));
    let file = match FileParse::parse(&input, origin.clone()) {
        Ok(file) => file,
        Err(e) => {
            e.print_report();
            return Err(anyhow!("Input contains syntax errors"));
        }
    };
    let project = Project {
        root: file.ast_file,
        local: HashMap::new(),
        deps: HashMap::new(),

    };

    let mut cg_file = codegen::file::CGFile::new();
    // for item in &project.root.defs {
    // match item {
    //     Definition::Struct(struct_def) => {
    //         let cg_struct_def = codegen::rust::struct_def::CGStructDef::new(&struct_def);
    //         let cg_struct_ser = codegen::rust::serdes::buf::struct_def::StructSer {
    //             inner: cg_struct_def.clone(),
    //         };
    //         let cg_struct_des = codegen::rust::serdes::buf::struct_def::StructDes {
    //             inner: cg_struct_def.clone(),
    //         };
    //         cg_file.push(&cg_struct_def, struct_def.span.clone());
    //         cg_file.push(&cg_struct_ser, struct_def.span.clone());
    //         cg_file.push(&cg_struct_des, struct_def.span.clone());
    //     }
    //     Definition::Xpi(xpi_def) => {
    //         let cg_xpi_def = codegen::rust::xpi::vlu4::dispatch::DispatchCall { xpi_def: &xpi_def };
    //     }
    //     _ => todo!()
    // }
    // }
    let cg_xpi_def = codegen::rust::xpi::vlu4::dispatch::DispatchCall {
        project: &project,
        xpi_def_path: make_path!(crate::main),
    };
    match cg_xpi_def.codegen() {
        Ok(piece) => {
            cg_file.push(piece);
        }
        Err(e) => {
            e.print_report();
            return Err(anyhow!("DispatchCall codegen err"));
        }
    }
    let rendered_file = match cg_file.render() {
        Ok(file) => file,
        Err(e) => {
            e.print_report();
            return Err(anyhow!("Render codegen err"));
        }
    };

    let formatted_file = match util::format_rust(rendered_file.0.as_str()) {
        Ok(formatted_file) => formatted_file,
        Err(e) => {
            println!("Failed to format file: {:?}", e);
            rendered_file.0
        }
    };
    let colorized_file = match util::colorize(formatted_file.as_str()) {
        Ok(colorized_file) => colorized_file,
        Err(e) => {
            println!("Failed to colorize: {:?}", e);
            println!("Raw output:\n{}", formatted_file);
            return Ok(());
        }
    };
    println!("{}", colorized_file);
    Ok(())
}