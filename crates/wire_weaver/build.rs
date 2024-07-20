use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use wire_weaver_core::ast::file::{FileSource, WWFile};

fn main() {
    let manifest_dir = &PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let client_server_path = manifest_dir.join("ww/client_server.ww");
    let client_server_contents = std::fs::read_to_string(&client_server_path).unwrap();
    let (client_server_ww, warnings) = WWFile::from_str(
        FileSource::File("ww/client_server.ww".into()),
        client_server_contents,
    )
    .unwrap();
    for w in warnings {
        println!("cargo:warning={:?}", w);
    }
    let ts = wire_weaver_core::codegen::rust_no_std_file(&client_server_ww);
    let ts_formatted = format_rust(format!("{ts}").as_str());
    File::create(manifest_dir.join("src/client_server.rs"))
        .unwrap()
        .write_all(ts_formatted.as_bytes())
        .unwrap();

    // eprintln!("{} bytes", client_server_contents.len());
    println!("cargo:rerun-if-changed=ww");
}

use pathsearch::find_executable_in_path;
use subprocess::{Exec, Redirection};
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
