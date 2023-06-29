use super::prelude::*;
use crate::commands::GenerateArgs;
use crate::config;
use ast::Definition;
use log::{debug, info};
use parser::ast::file::FileParse;
use std::rc::Rc;
use vhl_core::project::Project;

pub fn generate_subcmd(generate_args: GenerateArgs) -> Result<()> {
    match generate_args.src {
        Some(vhl_src_filename) => {
            debug!("got filename: {vhl_src_filename:?}");
        }
        None => {
            debug!("loading Vhl.toml from working directory");
            let config = std::fs::read_to_string("Vhl.toml")?;
            // let config: toml::Value = toml::from_str(&config)?;
            let config: config::Config = toml::from_str(&config)?;
            // println!("{config:#?}");

            let main_filename = config.info.src.clone().unwrap_or("main.vhl".to_owned());
            let mut main_path: PathBuf = std::env::current_dir()?;
            main_path.push(main_filename);
            debug!("loading: {:?}", main_path);
            let input = std::fs::read_to_string(&main_path)
                .context(format!("unable to open '{:?}'", &main_path))?;
            let origin = SpanOrigin::Parser(SourceOrigin::File(Rc::new(main_path)));
            let file = match FileParse::parse(input, origin) {
                Ok(file) => file,
                Err(e) => {
                    e.print_report();
                    return Err(anyhow!("Input contains syntax errors"));
                }
            };
            let mut project = Project::new(file.ast_file);
            debug!("Processing AST");
            vhl_core::transform::transform(&mut project);
            project.print_report();

            if let Some(targets) = config.gen {
                if let Some(target_rust) = targets.rust {
                    info!("Generating Rust sources");
                    generate_rust(&project, &config.info, &target_rust)?;
                }
            }
        }
    }

    Ok(())
}

fn generate_rust(
    project: &Project,
    _info: &config::Info,
    target: &config::TargetRust,
) -> Result<()> {
    for core in &target.core {
        let mut cg_file = codegen::file::CGFile::new();
        for (_id, def) in &project.root.defs {
            match def {
                Definition::Struct(struct_def) => {
                    let cg_struct_def = codegen::rust::struct_def::CGStructDef::new(struct_def);
                    // let cg_struct_ser = codegen::rust::serdes::buf::struct_def::StructSer {
                    //     inner: cg_struct_def.clone(),
                    // };
                    // let cg_struct_des = codegen::rust::serdes::buf::struct_def::StructDes {
                    //     inner: cg_struct_def.clone(),
                    // };
                    match cg_struct_def.codegen(&core.add_derives) {
                        Ok(piece) => {
                            cg_file.push(piece);
                        }
                        Err(e) => {
                            e.print_report();
                            return Err(anyhow!("DispatchCall codegen err"));
                        }
                    }
                }
                _ => {}
            }
        }
        // let cg_xpi_def = codegen::rust::xpi::vlu4::dispatch::DispatchCall {
        //     project: &project,
        //     xpi_def_path: make_path!(crate::main),
        // };
        // match cg_xpi_def.codegen() {
        //     Ok(piece) => {
        //         cg_file.push(piece);
        //     }
        //     Err(e) => {
        //         e.print_report();
        //         return Err(anyhow!("DispatchCall codegen err"));
        //     }
        // }
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

        let mut dest = std::env::current_dir()?;
        dest.push(&core.target_crate);
        dest.push("src");
        std::fs::create_dir_all(dest.clone())?;
        let mut dest = std::fs::canonicalize(dest.clone()).with_context(|| format!("construct destination file path for rust::core codegen, path = {:?}", dest))?;
        dest.push("core.rs");
        debug!("Writing to {:?}", dest);
        std::fs::write(dest, formatted_file)?;
        // let colorized_file = match util::colorize(formatted_file.as_str()) {
        //     Ok(colorized_file) => colorized_file,
        //     Err(e) => {
        //         warning!("Failed to colorize: {:?}", e);
        //         println!("Raw output:\n{}", formatted_file);
        //         return Ok(());
        //     }
        // };
        // println!("{}", colorized_file);
    }
    Ok(())
}
