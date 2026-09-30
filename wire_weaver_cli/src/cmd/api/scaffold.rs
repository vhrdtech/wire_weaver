use anyhow::{Context, Result, anyhow};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use wire_weaver_core::{ServerScaffoldConfig, gen_server_scaffold, load};

pub(crate) fn scaffold(
    crate_path: PathBuf,
    trait_name: Option<String>,
    config: ServerScaffoldConfig,
    output: Option<PathBuf>,
) -> Result<()> {
    let api_bundle = load(&crate_path, trait_name, false)?;
    let code = rustfmt(gen_server_scaffold(&api_bundle, &config)?)?;
    match output {
        Some(path) => {
            let mut file = std::fs::File::create_new(&path)
                .with_context(|| format!("creating {}", path.display()))?;
            file.write_all(code.as_bytes())?;
        }
        None => print!("{code}"),
    }
    Ok(())
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
