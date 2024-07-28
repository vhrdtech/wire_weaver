use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

use pathsearch::find_executable_in_path;
use subprocess::{Exec, Redirection};

use wire_weaver_core::{ast::Source, transform::Transform};

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let client_server_path = format!("{manifest_dir}/ww/client_server.ww");

    let mut transform = Transform::new();
    transform
        .load_and_push(Source::File {
            path: client_server_path,
        })
        .unwrap();

    let cx = transform.transform().unwrap();
    for (source, messages) in transform.messages() {
        for message in messages.messages() {
            println!("cargo:warning={:?} {:?}", source, message);
        }
    }

    let ts = wire_weaver_core::codegen::generate(&cx, false);
    let ts_formatted = format_rust(format!("use crate as wire_weaver;\n{ts}").as_str());
    let manifest_dir_path = &PathBuf::from(manifest_dir.clone());
    File::create(manifest_dir_path.join("src/client_server.rs"))
        .unwrap()
        .write_all(ts_formatted.as_bytes())
        .unwrap();

    let ts = wire_weaver_core::codegen::generate(&cx, true);
    let ts_formatted = format_rust(format!("use crate as wire_weaver;\n{ts}").as_str());
    let manifest_dir_path = &PathBuf::from(manifest_dir);
    File::create(manifest_dir_path.join("src/client_server_no_alloc.rs"))
        .unwrap()
        .write_all(ts_formatted.as_bytes())
        .unwrap();

    // eprintln!("{} bytes", client_server_contents.len());
    println!("cargo:rerun-if-changed=ww");
}

pub fn format_rust(code: &str) -> String {
    let Some(rustfmt_path) = find_executable_in_path("rustfmt") else {
        println!("cargo:warning=rustfmt not found in PATH, skipping formatting");
        return code.to_string();
    };
    let Ok(rustfmt_run) = Exec::cmd(rustfmt_path)
        .stdin(code)
        .stdout(Redirection::Pipe)
        .capture()
    else {
        println!("cargo:warning=rustfmt failed, wrong code?");
        return code.to_string();
    };
    if !rustfmt_run.exit_status.success() {
        println!("cargo:warning=rustfmt failed, wrong code?");
        return code.to_string();
    }
    rustfmt_run.stdout_str()
}
