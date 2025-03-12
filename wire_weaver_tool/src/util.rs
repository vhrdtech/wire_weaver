use pathsearch::find_executable_in_path;
use subprocess::{Exec, Redirection};

pub fn format_rust(code: String) -> String {
    let Some(rustfmt_path) = find_executable_in_path("rustfmt") else {
        println!("cargo:warning=rustfmt not found in PATH, skipping formatting");
        return code;
    };
    let Ok(rustfmt_run) = Exec::cmd(rustfmt_path)
        .args(&["--edition", "2021"])
        .stdin(code.as_str())
        .stdout(Redirection::Pipe)
        .capture()
    else {
        println!("cargo:warning=rustfmt failed, wrong code?");
        return code;
    };
    if !rustfmt_run.exit_status.success() {
        println!("cargo:warning=rustfmt failed, wrong code?");
        return code;
    }
    rustfmt_run.stdout_str()
}
