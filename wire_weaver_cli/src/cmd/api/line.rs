//! `ww api line new` / `retire` (TOOL-33): a new wire line in a crate with several API roots (EVO-37), and the
//! removal of one no device runs any more. Edits `Cargo.toml` with `toml_edit` and `src/lib.rs` as text.

use anyhow::{Context, Result, anyhow, bail};
use clap::Subcommand;
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item, Table, value};
use wire_weaver_evolution::snapshot_files::{CrateFile, crate_files};

#[derive(Subcommand)]
pub enum LineCommand {
    /// Start the next wire line: copy a root's module to a new one at `w(N+1).0` and list it in `roots`
    ///
    /// Copies `src/wire_vN.rs` to `src/wire_vN+1.rs`, adds `pub mod wire_vN+1;` next to the source's in `src/lib.rs`
    /// and `"wire_vN+1::Trait" = "w(N+1).0"` to `[package.metadata.wire_weaver.roots]` in `Cargo.toml`. Then change
    /// the new module: that is the line's breaking change. Run `ww api check` and `ww api save` afterwards.
    New {
        /// Path to the API crate
        #[arg(value_hint = clap::ValueHint::DirPath)]
        path: PathBuf,

        /// The root to copy, `wire_v2::Thermo` or just `wire_v2`; absent: the root on the highest line
        #[arg(long)]
        from: Option<String>,

        /// Name of the new module; absent: the source's with its trailing number replaced by the new line (`wire_v3`)
        #[arg(long)]
        module: Option<String>,
    },
    /// Remove a wire line no device runs any more
    ///
    /// Takes the root out of `roots`, `pub mod` out of `src/lib.rs` and deletes its module file, which has to be tracked and unchanged in git (`--force` skips that check). That
    /// is a breaking crate bump, made in `Cargo.toml` unless the version is already high enough or `--no-bump` is
    /// given. Run `ww api save` afterwards to record the removal.
    Retire {
        /// Path to the API crate
        #[arg(value_hint = clap::ValueHint::DirPath)]
        path: PathBuf,

        /// The root to remove, `wire_v1::Thermo` or just `wire_v1`
        root: String,

        /// Leave the crate version alone
        #[arg(long)]
        no_bump: bool,

        /// Delete the module file even when git doesn't track it or it has changes
        #[arg(long)]
        force: bool,
    },
}

pub(crate) fn line(cmd: LineCommand) -> Result<()> {
    match cmd {
        LineCommand::New { path, from, module } => new(&path, from.as_deref(), module.as_deref()),
        LineCommand::Retire {
            path,
            root,
            no_bump,
            force,
        } => retire(&path, &root, no_bump, force),
    }
}

/// `(module, trait)` of a root key `wire_v2::Thermo`.
fn split_root(key: &str) -> Result<(&str, &str)> {
    let (module, name) = key
        .rsplit_once("::")
        .ok_or_else(|| anyhow!("`{key}` is not a root key, expected `module::Trait`"))?;
    if module.contains("::") {
        bail!(
            "`{key}`: only roots in a top-level module of the crate are supported, not nested modules"
        );
    }
    Ok((module, name))
}

/// The root a name stands for: a full key, or a module name with exactly one root in it.
fn resolve_root(roots: &[(String, u32)], name: &str) -> Result<String> {
    if roots.iter().any(|(k, _)| k == name) {
        return Ok(name.to_string());
    }
    let prefix = format!("{name}::");
    let mut found = roots.iter().filter(|(k, _)| k.starts_with(&prefix));
    match (found.next(), found.next()) {
        (Some((k, _)), None) => Ok(k.clone()),
        _ => Err(anyhow!(
            "no API root `{name}`; the crate has: {}",
            roots
                .iter()
                .map(|(k, _)| k.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

fn roots_of(path: &Path) -> Result<Vec<(String, u32)>> {
    let roots = wire_weaver_core::api_roots(path)?;
    if roots.is_empty() {
        bail!(
            "{} has no `[package.metadata.wire_weaver.roots]` table: lines are kept as one root per module in a crate \
             with several (docs/evolution/rules.md#several-lines-in-one-crate)",
            path.display()
        );
    }
    Ok(roots.into_iter().map(|(k, w)| (k, w.line.0)).collect())
}

/// `wire_v2` and line 3 -> `wire_v3`: the trailing number replaced.
fn next_module_name(module: &str, line: u32) -> Option<String> {
    let stem = module.trim_end_matches(|c: char| c.is_ascii_digit());
    (stem.len() < module.len()).then(|| format!("{stem}{line}"))
}

fn module_file(path: &Path, module: &str) -> Result<PathBuf> {
    let file = path.join("src").join(format!("{module}.rs"));
    if file.exists() {
        return Ok(file);
    }
    if path.join("src").join(module).join("mod.rs").exists() {
        bail!(
            "module `{module}` is a directory (src/{module}/mod.rs); only single-file modules are handled, copy or remove it by hand"
        );
    }
    bail!("module file {} not found", file.display())
}

fn lib_rs(path: &Path) -> Result<PathBuf> {
    let lib = path.join("src").join("lib.rs");
    if lib.exists() {
        Ok(lib)
    } else {
        Err(anyhow!("{} not found", lib.display()))
    }
}

fn read_manifest(path: &Path) -> Result<(PathBuf, DocumentMut)> {
    let file = path.join("Cargo.toml");
    let text =
        std::fs::read_to_string(&file).with_context(|| format!("reading {}", file.display()))?;
    let doc = text
        .parse::<DocumentMut>()
        .with_context(|| format!("parsing {}", file.display()))?;
    Ok((file, doc))
}

fn roots_table(doc: &mut DocumentMut) -> Result<&mut Table> {
    doc["package"]["metadata"]["wire_weaver"]["roots"]
        .as_table_mut()
        .ok_or_else(|| anyhow!("`[package.metadata.wire_weaver.roots]` is not a table"))
}

/// The line declaring `mod <module>;` (any visibility), as `(line index, prefix up to `mod`)`.
fn find_mod_line(lib: &str, module: &str) -> Option<(usize, String)> {
    let needle = format!("mod {module};");
    lib.lines().enumerate().find_map(|(i, l)| {
        let t = l.trim_start();
        let rest = t.strip_prefix("pub ").unwrap_or(t);
        (rest == needle).then(|| (i, l[..l.len() - needle.len()].to_string()))
    })
}

/// `src/lib.rs` with `mod <new>;` added after the source's declaration, in the same visibility.
fn add_mod(lib: &str, source: &str, new: &str) -> Result<String> {
    let (at, prefix) = find_mod_line(lib, source).ok_or_else(|| {
        anyhow!("`mod {source};` not found in src/lib.rs, add `pub mod {new};` there by hand")
    })?;
    let mut lines: Vec<&str> = lib.lines().collect();
    let added = format!("{prefix}mod {new};");
    lines.insert(at + 1, &added);
    Ok(lines.join("\n") + if lib.ends_with('\n') { "\n" } else { "" })
}

/// `src/lib.rs` without `mod <module>;`.
fn remove_mod(lib: &str, module: &str) -> Result<String> {
    let (at, _) = find_mod_line(lib, module)
        .ok_or_else(|| anyhow!("`mod {module};` not found in src/lib.rs, remove it by hand"))?;
    let mut lines: Vec<&str> = lib.lines().collect();
    lines.remove(at);
    Ok(lines.join("\n") + if lib.ends_with('\n') { "\n" } else { "" })
}

fn new(path: &Path, from: Option<&str>, module: Option<&str>) -> Result<()> {
    let roots = roots_of(path)?;
    let source = match from {
        Some(name) => resolve_root(&roots, name)?,
        None => {
            let max = roots.iter().map(|(_, l)| *l).max().unwrap_or(0);
            let mut top = roots.iter().filter(|(_, l)| *l == max);
            match (top.next(), top.next()) {
                (Some((k, _)), None) => k.clone(),
                _ => bail!("several roots are on line {max}, name the one to copy with --from"),
            }
        }
    };
    let (source_module, trait_name) = split_root(&source)?;
    let source_line = roots.iter().find(|(k, _)| *k == source).unwrap().1;
    let new_line = roots
        .iter()
        .map(|(_, l)| *l)
        .max()
        .unwrap_or(0)
        .max(source_line)
        + 1;
    let new_module = match module {
        Some(m) => m.to_string(),
        None => next_module_name(source_module, new_line).ok_or_else(|| {
            anyhow!("can't derive a module name from `{source_module}`, pass --module")
        })?,
    };
    let key = format!("{new_module}::{trait_name}");
    if roots.iter().any(|(k, _)| *k == key) {
        bail!("`{key}` is already a root");
    }
    let from_file = module_file(path, source_module)?;
    let to_file = path.join("src").join(format!("{new_module}.rs"));
    if to_file.exists() {
        bail!("{} exists already", to_file.display());
    }
    let lib_path = lib_rs(path)?;
    let lib = add_mod(
        &std::fs::read_to_string(&lib_path)?,
        source_module,
        &new_module,
    )?;
    let (manifest_path, mut doc) = read_manifest(path)?;
    let wire = format!("w{new_line}.0");
    roots_table(&mut doc)?.insert(&key, value(wire.as_str()));

    std::fs::copy(&from_file, &to_file)
        .with_context(|| format!("copying {}", from_file.display()))?;
    std::fs::write(&lib_path, lib)?;
    std::fs::write(&manifest_path, doc.to_string())?;
    println!(
        "{key} at {wire}: copied {} to {}, listed in Cargo.toml and src/lib.rs",
        from_file.display(),
        to_file.display()
    );
    println!(
        "next: make the line's breaking change in {new_module}, then `ww api check` and `ww api save`"
    );
    Ok(())
}

/// Refuses a file git can't bring back: not tracked, or changed since the last commit (or no git at all).
fn check_recoverable(file: &Path) -> Result<()> {
    let dir = file.parent().unwrap_or(Path::new("."));
    let name = file.file_name().unwrap_or_default();
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .arg("--")
            .arg(name)
            .current_dir(dir)
            .output()
    };
    let refuse = |why: &str| {
        Err(anyhow!(
            "{} {why}, deleting it would lose it: commit it first, or pass --force",
            file.display()
        ))
    };
    let tracked = git(&["ls-files", "--error-unmatch"]);
    match tracked {
        Err(_) => return refuse("can't be checked, git is not available"),
        Ok(o) if !o.status.success() => return refuse("is not tracked by git"),
        Ok(_) => {}
    }
    let status = git(&["status", "--porcelain"]).context("running git status")?;
    if !status.status.success() || !status.stdout.is_empty() {
        return refuse("has uncommitted changes");
    }
    Ok(())
}

fn retire(path: &Path, root: &str, no_bump: bool, force: bool) -> Result<()> {
    let roots = roots_of(path)?;
    let key = resolve_root(&roots, root)?;
    if roots.len() == 1 {
        bail!("`{key}` is the crate's only root, a crate keeps at least one line");
    }
    let (module, _) = split_root(&key)?;
    // the module may hold other roots, then it stays
    let module_shared = roots
        .iter()
        .any(|(k, _)| *k != key && k.starts_with(&format!("{module}::")));
    let (manifest_path, mut doc) = read_manifest(path)?;
    roots_table(&mut doc)?.remove(&key);

    let lib_path = lib_rs(path)?;
    let lib = if module_shared {
        None
    } else {
        Some(remove_mod(&std::fs::read_to_string(&lib_path)?, module)?)
    };
    let file = if module_shared {
        None
    } else {
        Some(module_file(path, module)?)
    };
    if let (Some(file), false) = (&file, force) {
        check_recoverable(file)?;
    }

    // the removal is a breaking bump of the last version that shipped the root
    let package = &doc["package"];
    let crate_id = package["name"]
        .as_str()
        .ok_or_else(|| anyhow!("Cargo.toml has no package name"))?
        .replace('-', "_");
    let current = package["version"].as_str().map(semver::Version::parse);
    let shipped = crate_files(&path.join("api_snapshots"), &crate_id)
        .map_err(|e| anyhow!(e))?
        .into_iter()
        .filter(|f| matches!(&f.kind, CrateFile::Root { root, .. } if *root == key))
        .map(|f| f.version)
        .max();
    let mut bump_note = "no bump needed: the root was never saved".to_string();
    if let Some((major, minor, patch)) = shipped {
        let target = if major >= 1 {
            semver::Version::new(major as u64 + 1, 0, 0)
        } else if minor >= 1 || patch == 0 {
            semver::Version::new(0, minor as u64 + 1, 0)
        } else {
            semver::Version::new(0, 0, patch as u64 + 1)
        };
        bump_note = match current {
            _ if no_bump => format!("crate version left alone, it needs {target} or later"),
            Some(Ok(current)) if current >= target => {
                format!("crate version {current} is high enough (needs {target})")
            }
            Some(Ok(_)) => {
                doc["package"]["version"] = Item::Value(target.to_string().into());
                format!("crate version bumped to {target} (breaking)")
            }
            _ => format!(
                "crate version is not a plain string in Cargo.toml: bump it to {target} or later by hand"
            ),
        };
    }

    std::fs::write(&manifest_path, doc.to_string())?;
    if let Some(lib) = lib {
        std::fs::write(&lib_path, lib)?;
    }
    if let Some(file) = &file {
        std::fs::remove_file(file).with_context(|| format!("removing {}", file.display()))?;
    }
    println!("{key} retired: {bump_note}");
    println!(
        "next: fix what still uses the module, then `ww api save` records the removal ({module}.rs is in git if needed)"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_names() {
        assert_eq!(next_module_name("wire_v2", 3).as_deref(), Some("wire_v3"));
        assert_eq!(next_module_name("v9", 10).as_deref(), Some("v10"));
        assert_eq!(next_module_name("current", 2), None);
    }

    #[test]
    fn lib_edits() {
        let lib = "#![no_std]\n\npub mod common;\npub mod wire_v1;\npub mod wire_v2;\n";
        let added = add_mod(lib, "wire_v2", "wire_v3").unwrap();
        assert_eq!(
            added,
            "#![no_std]\n\npub mod common;\npub mod wire_v1;\npub mod wire_v2;\npub mod wire_v3;\n"
        );
        let removed = remove_mod(&added, "wire_v1").unwrap();
        assert_eq!(
            removed,
            "#![no_std]\n\npub mod common;\npub mod wire_v2;\npub mod wire_v3;\n"
        );
        assert!(add_mod(lib, "nope", "x").is_err());
    }

    #[test]
    fn root_names() {
        let roots = vec![
            ("wire_v1::Thermo".to_string(), 1),
            ("wire_v2::Thermo".to_string(), 2),
        ];
        assert_eq!(resolve_root(&roots, "wire_v1").unwrap(), "wire_v1::Thermo");
        assert_eq!(
            resolve_root(&roots, "wire_v2::Thermo").unwrap(),
            "wire_v2::Thermo"
        );
        assert!(resolve_root(&roots, "wire_v3").is_err());
    }

    fn copy_dir(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_dir(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), target).unwrap();
            }
        }
    }

    /// `tests/multi_root_api` copied: a new line, then the retirement of the oldest with a saved snapshot.
    #[test]
    fn new_then_retire() {
        let dir = std::env::temp_dir().join(format!("ww_line_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/multi_root_api");
        copy_dir(&source.join("src"), &dir.join("src"));
        let manifest = std::fs::read_to_string(source.join("Cargo.toml")).unwrap();
        std::fs::write(dir.join("Cargo.toml"), manifest).unwrap();

        new(&dir, None, None).unwrap();
        assert!(dir.join("src/wire_v3.rs").exists());
        assert!(
            std::fs::read_to_string(dir.join("src/lib.rs"))
                .unwrap()
                .contains("pub mod wire_v3;")
        );
        let roots = wire_weaver_core::api_roots(&dir).unwrap();
        assert_eq!(roots["wire_v3::Thermo"].to_string(), "w3.0");
        assert!(
            new(&dir, None, Some("wire_v3")).is_err(),
            "a module that exists is refused"
        );

        // the oldest line was saved at 0.1.0: retiring it is a breaking bump
        std::fs::create_dir_all(dir.join("api_snapshots")).unwrap();
        std::fs::write(
            dir.join("api_snapshots/multi_root_api_0_1_0.wire_v1.Thermo@w1.2.ron"),
            "",
        )
        .unwrap();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(["-c", "user.name=t", "-c", "user.email=t@t"])
                .args(args)
                .current_dir(&dir)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}: {out:?}");
        };
        git(&["init", "-q"]);
        // untracked: refused, nothing changed
        let err = retire(&dir, "wire_v1", false, false)
            .unwrap_err()
            .to_string();
        assert!(err.contains("not tracked"), "{err}");
        assert!(dir.join("src/wire_v1.rs").exists());
        git(&["add", "-A"]);
        git(&["commit", "-qm", "x"]);
        // modified: refused
        std::fs::write(dir.join("src/wire_v1.rs"), "// edited\n").unwrap();
        let err = retire(&dir, "wire_v1", false, false)
            .unwrap_err()
            .to_string();
        assert!(err.contains("uncommitted changes"), "{err}");
        assert!(dir.join("src/wire_v1.rs").exists());
        assert!(
            std::fs::read_to_string(dir.join("Cargo.toml"))
                .unwrap()
                .contains("wire_v1::Thermo")
        );
        git(&["commit", "-qam", "y"]);
        retire(&dir, "wire_v1", false, false).unwrap();
        assert!(!dir.join("src/wire_v1.rs").exists());
        let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap();
        assert!(manifest.contains("version = \"0.2.0\""), "{manifest}");
        assert!(!manifest.contains("wire_v1::Thermo"));
        assert!(
            !std::fs::read_to_string(dir.join("src/lib.rs"))
                .unwrap()
                .contains("mod wire_v1;")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
