use super::prelude::*;
use parser::ast::file::File;
use vhl::ast::file::Definition;
use crate::commands::GenerateArgs;

pub fn generate_subcmd(generate_args: GenerateArgs) -> Result<()> {
    let input = std::fs::read_to_string(generate_args.vhl_source.clone())
        .context(format!("unable to open '{:?}'", generate_args.vhl_source))?;
    let origin = SpanOrigin::Parser(SourceOrigin::File(generate_args.vhl_source.clone()));
    let file = match File::parse(&input, origin.clone()) {
        Ok(file) => file,
        Err(e) => {
            println!("{}", e);
            return Err(anyhow!("Input contains syntax errors"));
        }
    };

    let ast_core = vhl::ast::file::File::from_parser_ast(file);
    // println!("{:?}", ast_core);

    let mut cg_file = codegen::file::File::new();
    for item in &ast_core.items {
        match item {
            Definition::Struct(struct_def) => {
                let cg_struct_def = codegen::rust::struct_def::CGStructDef::new(struct_def);
                let cg_struct_ser = codegen::rust::serdes::buf::struct_def::StructSer {
                    inner: cg_struct_def.clone(),
                };
                let cg_struct_des = codegen::rust::serdes::buf::struct_def::StructDes {
                    inner: cg_struct_def.clone(),
                };
                cg_file.push(&cg_struct_def, struct_def.span.clone());
                cg_file.push(&cg_struct_ser, struct_def.span.clone());
                cg_file.push(&cg_struct_des, struct_def.span.clone());
            }
            Definition::Xpi(xpi_def) => {
                let cg_xpi_def = codegen::rust::xpi::vlu4::dispatch::DispatchCall { xpi_def };
                cg_file.push_cg(&cg_xpi_def, xpi_def.span.clone())?;
            }
        }
    }
    let rendered_file = cg_file.render()?.0;

    let formatted_file = match util::format_rust(rendered_file.as_str()) {
        Ok(formatted_file) => formatted_file,
        Err(e) => {
            println!("Failed to format file: {:?}", e);
            rendered_file
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