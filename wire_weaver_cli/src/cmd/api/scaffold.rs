//! `ww api scaffold`: the server struct, a stub per handler and the matching `ww_codegen!` call of an API.

use anyhow::{Context, Result, anyhow};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use wire_weaver_core::{ServerScaffoldConfig, UnitHint, gen_server_scaffold, load_with_hints};
use wire_weaver_evolution::unreasoned_message;

pub(crate) fn scaffold(
    crate_path: PathBuf,
    root: Option<String>,
    config: ServerScaffoldConfig,
    output: Option<PathBuf>,
    strict: bool,
) -> Result<()> {
    let Scaffold { code, hints } = render(crate_path, root, &config, strict)?;
    match output {
        Some(path) => {
            let mut file = std::fs::File::create_new(&path)
                .with_context(|| format!("creating {}", path.display()))?;
            file.write_all(code.as_bytes())?;
        }
        None => print!("{code}"),
    }
    // TOOL-22: unit-suffix hints, on stderr so they don't land in redirected/piped generated code.
    for hint in &hints {
        eprintln!("note: {hint}");
    }
    Ok(())
}

/// What `ww api scaffold` writes for an API, and the hints it prints beside it.
#[derive(Debug)]
pub struct Scaffold {
    /// The generated code, formatted by rustfmt.
    pub code: String,
    /// Plain integers whose names end in a unit suffix (TOOL-22); `ww api scaffold` prints them on stderr.
    pub hints: Vec<UnitHint>,
}

/// The code `ww api scaffold` writes for the API crate at `crate_path`: the server struct, a stub per handler and
/// the `ww_codegen!` call that `config` describes. `root` picks one API root of a crate with several, `strict` fails
/// on a change without a written reason. For a program or a test that shows the same code, such as a book's
/// transcript.
pub fn render(
    crate_path: PathBuf,
    root: Option<String>,
    config: &ServerScaffoldConfig,
    strict: bool,
) -> Result<Scaffold> {
    let (api_bundle, hints, unreasoned, crate_paths) = load_with_hints(&crate_path, root, false)?;
    if strict && let Some(message) = unreasoned_message(&unreasoned) {
        return Err(anyhow!("{message}"));
    }
    let code = rustfmt(gen_server_scaffold(&api_bundle, config, &crate_paths)?)?;
    Ok(Scaffold { code, hints })
}

fn rustfmt(code: String) -> Result<String> {
    let mut child = Command::new("rustfmt")
        .args(["--edition", "2024"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context("running rustfmt")?;
    child.stdin.take().unwrap().write_all(code.as_bytes())?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(anyhow!("rustfmt failed on generated code:\n{code}"));
    }
    Ok(String::from_utf8(output.stdout)?)
}
