use anyhow::{anyhow, Context, Result};
use pathsearch::find_executable_in_path;
use subprocess::{Exec, Redirection};

pub fn format_rust(code: &str) -> Result<String> {
    let rustfmt_path =
        find_executable_in_path("rustfmt").context("Failed to find rustfmt in PATH")?;
    let rustfmt_run = Exec::cmd(rustfmt_path)
        .stdin(code)
        .stdout(Redirection::Pipe)
        .capture()
        .context("Failed to run rustfmt, most likely incorrect code?")?;
    if !rustfmt_run.exit_status.success() {
        return Err(anyhow!("rustfmt exited with an error"));
    }
    Ok(rustfmt_run.stdout_str())
}

pub fn colorize(code: &str /*, _language: Language*/) -> Result<String> {
    let pygmentize_path =
        find_executable_in_path("pygmentize").context("Failed to find pygmentize in PATH")?;
    let colorized = Exec::cmd(pygmentize_path)
        .args(&["-l", "rust", "-O", "style=monokai"])
        .stdin(code)
        .stdout(Redirection::Pipe)
        .capture()
        .context("Failed to colorize with pygmentize")?
        .stdout_str();
    // let colorized = Exec::cmd(highlight_path)
    //     .args(&[
    //         "--syntax-by-name", "rust",
    //         "--out-format", "truecolor",
    //         // "--out-format", "xterm256",
    //         "--style", "moria",
    //     ])
    //     .stdin(formatted.as_str())
    //     .stdout(Redirection::Pipe)
    //     .capture()?
    //     .stdout_str();
    Ok(colorized)
}
