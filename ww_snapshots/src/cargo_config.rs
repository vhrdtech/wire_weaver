//! `[patch]` tables of Cargo's configuration, read the way Cargo reads them: `--config` arguments first, then the
//! `.cargo/config.toml` of each directory from where Cargo runs up, then `CARGO_HOME/config.toml`, each file with
//! the files its `include` names (the including file's own values win, then later includes over earlier ones). A
//! config file's relative path is relative to the directory holding its `.cargo` (the parent of the file's
//! directory), a `--config KEY=VALUE`'s to where Cargo runs. Cargo can't take a `[patch]` from the environment.

use crate::limits::{CARGO_CONFIG_BYTES_MAX, CARGO_CONFIG_INCLUDES_MAX};
use anyhow::{Context, Result};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Where Cargo's configuration for one build is.
pub(crate) struct ConfigSources<'a> {
    /// `--config` arguments of the Cargo command, in order: `KEY=VALUE` or a file.
    pub cli: &'a [String],
    /// Where Cargo runs: the configuration hierarchy starts there, and `--config` paths are relative to it.
    pub cwd: &'a Path,
    /// `CARGO_HOME`, whose `config.toml` comes last.
    pub cargo_home: Option<&'a Path>,
}

/// How many directories, from where Cargo runs up, are looked into for a `.cargo/config.toml`.
const CONFIG_ANCESTORS_MAX: usize = 64;

/// The path Cargo's configuration patches `package` to, made absolute against `sources.cwd` where relative;
/// `reads` is told every file looked at.
pub(crate) fn patch_path(
    sources: &ConfigSources,
    package: &str,
    reads: &dyn Fn(&Path),
) -> Result<Option<PathBuf>> {
    let mut budget = CARGO_CONFIG_INCLUDES_MAX;
    for arg in sources.cli.iter().rev() {
        let file = sources.cwd.join(arg);
        let found = if arg.ends_with(".toml") || file.is_file() {
            in_file(&file, package, reads, &mut budget)?
        } else {
            let table: toml::Table =
                toml::from_str(arg).with_context(|| format!("Failed to parse --config {arg}"))?;
            in_table(
                &table,
                package,
                sources.cwd,
                sources.cwd,
                reads,
                &mut budget,
            )?
        };
        if found.is_some() {
            return Ok(found);
        }
    }
    let mut files: Vec<PathBuf> = sources
        .cwd
        .ancestors()
        .take(CONFIG_ANCESTORS_MAX)
        .map(|dir| dir.join(".cargo"))
        .collect();
    files.extend(sources.cargo_home.map(Path::to_path_buf));
    let mut seen: Vec<PathBuf> = vec![];
    for dir in files {
        // `config` without an extension is the older name, read when `config.toml` isn't there
        let file = [dir.join("config.toml"), dir.join("config")]
            .into_iter()
            .find(|f| {
                reads(f);
                f.is_file()
            });
        let Some(file) = file else { continue };
        let canonical = file.canonicalize().unwrap_or_else(|_| file.clone());
        if seen.contains(&canonical) {
            continue;
        }
        seen.push(canonical);
        if let Some(found) = in_file(&file, package, reads, &mut budget)? {
            return Ok(Some(found));
        }
    }
    Ok(None)
}

/// The path the config file `file` (with its includes) patches `package` to.
fn in_file(
    file: &Path,
    package: &str,
    reads: &dyn Fn(&Path),
    budget: &mut usize,
) -> Result<Option<PathBuf>> {
    if *budget == 0 {
        tracing::debug!(
            "more than {CARGO_CONFIG_INCLUDES_MAX} Cargo config files, {} is not read",
            file.display()
        );
        return Ok(None);
    }
    *budget -= 1;
    reads(file);
    let mut contents = String::new();
    fs::File::open(file)
        .and_then(|f| f.take(CARGO_CONFIG_BYTES_MAX).read_to_string(&mut contents))
        .with_context(|| format!("Failed to read {}", file.display()))?;
    let table: toml::Table =
        toml::from_str(&contents).with_context(|| format!("Failed to parse {}", file.display()))?;
    let dir = file.parent().unwrap_or(Path::new("."));
    let base = dir.parent().unwrap_or(dir);
    in_table(&table, package, base, dir, reads, budget)
}

/// The path the config `table` patches `package` to: its own `[patch]` first (relative paths against `base`), then
/// its `include`s, last first (relative to `include_dir`).
fn in_table(
    table: &toml::Table,
    package: &str,
    base: &Path,
    include_dir: &Path,
    reads: &dyn Fn(&Path),
    budget: &mut usize,
) -> Result<Option<PathBuf>> {
    if let Some(path) = patch_in(table, package) {
        return Ok(Some(base.join(path)));
    }
    for include in includes(table).iter().rev() {
        // `..` resolved as the file system does, symbolic links included
        let file = include_dir.join(&include.path);
        let file = file.canonicalize().unwrap_or(file);
        if include.optional && !file.is_file() {
            reads(&file);
            continue;
        }
        if let Some(found) = in_file(&file, package, reads, budget)? {
            return Ok(Some(found));
        }
    }
    Ok(None)
}

struct Include {
    path: String,
    optional: bool,
}

/// `include = ["a.toml", ..]` or `[{ path = "a.toml", optional = true }, ..]`.
fn includes(table: &toml::Table) -> Vec<Include> {
    let plain = |path: &str| Include {
        path: path.to_owned(),
        optional: false,
    };
    match table.get("include") {
        Some(toml::Value::Array(items)) => items
            .iter()
            .filter_map(|item| match item {
                toml::Value::String(path) => Some(plain(path)),
                toml::Value::Table(t) => Some(Include {
                    path: t.get("path")?.as_str()?.to_owned(),
                    optional: t
                        .get("optional")
                        .and_then(toml::Value::as_bool)
                        .unwrap_or(false),
                }),
                _ => None,
            })
            .collect(),
        _ => vec![],
    }
}

/// The path the `[patch]` tables of `table` (a manifest or a config) give `package`, if any.
pub(crate) fn patch_in(table: &toml::Table, package: &str) -> Option<String> {
    let patch = table.get("patch")?.as_table()?;
    for deps in patch.values().filter_map(|deps| deps.as_table()) {
        for (name, dep) in deps {
            let Some(dep) = dep.as_table() else { continue };
            let renamed = dep.get("package").and_then(|p| p.as_str());
            if renamed.unwrap_or(name) != package {
                continue;
            }
            if let Some(dir) = dep.get("path").and_then(|p| p.as_str()) {
                return Some(dir.to_owned());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, contents: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    const PATCH: &str = "[patch.\"https://example.org/units\"]\n";

    #[test]
    fn a_patch_in_an_included_file_counts_and_the_including_file_wins() {
        let top = std::env::temp_dir().join(format!("ww_cargo_config_{}", std::process::id()));
        let _ = fs::remove_dir_all(&top);
        let ws = top.join("ws");
        write(
            &ws.join(".cargo/config.toml"),
            &format!(
                "include = [\"dev-local.toml\", {{ path = \"missing.toml\", optional = true }}]\n{PATCH}\
                 units = {{ path = \"own\" }}\n"
            ),
        );
        write(
            &ws.join(".cargo/dev-local.toml"),
            &format!(
                "include = [\"../../shared/more.toml\"]\n{PATCH}si = {{ path = \"../si\" }}\nunits = {{ path = \"included\" }}\n"
            ),
        );
        // an include's own paths are relative to the directory above its directory
        write(
            &top.join("shared/more.toml"),
            &format!("{PATCH}tape = {{ path = \"tape\" }}\n"),
        );
        let home = top.join("home");
        write(
            &home.join("config.toml"),
            &format!("{PATCH}crc = {{ path = \"/abs/crc\" }}\n"),
        );
        let cwd = ws.join("member");
        fs::create_dir_all(&cwd).unwrap();
        let cli = ["patch.'https://example.org/units'.si.path = \"cli_si\"".to_owned()];
        let find = |cli: &[String], package: &str| {
            let sources = ConfigSources {
                cli,
                cwd: &cwd,
                cargo_home: Some(&home),
            };
            patch_path(&sources, package, &|_| {}).unwrap()
        };
        let units = find(&[], "units");
        let si = find(&[], "si");
        let si_cli = find(&cli, "si");
        let tape = find(&[], "tape");
        let crc = find(&[], "crc");
        let none = find(&[], "none");
        let canonical_top = top.canonicalize().unwrap();
        fs::remove_dir_all(&top).unwrap();
        assert_eq!(units, Some(ws.join("own")));
        assert_eq!(si, Some(ws.join("../si")));
        assert_eq!(si_cli, Some(cwd.join("cli_si")));
        assert_eq!(tape, Some(canonical_top.join("tape")));
        assert_eq!(crc, Some(PathBuf::from("/abs/crc")));
        assert_eq!(none, None);
    }
}
