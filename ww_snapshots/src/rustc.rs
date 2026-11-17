//! The Cargo build a proc macro runs in, as the rustc process hosting it tells: which crate it compiles, where
//! Cargo put the crates it compiled before (rustc's `--out-dir` and `-L dependency=`), the exact files of the direct
//! dependencies (`--extern`), and the dep-info (`.d`) Cargo writes beside each, naming the sources it was compiled
//! from. [crate::cargo::CargoSources] checks a `Cargo.lock` against these: a crate built as a dependency of another
//! workspace is pinned by that workspace's lock, not by the one next to it.

use crate::limits::{DEP_INFO_BYTES_MAX, DEP_INFO_FILES_MAX};
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

/// What rustc's arguments and environment say about the build of one crate.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RustcBuild {
    /// Directory of the crate being compiled (`CARGO_MANIFEST_DIR`).
    pub manifest_dir: PathBuf,
    /// Directory rustc runs in: Cargo runs it in the workspace root for a workspace member, in the package's own
    /// directory for a fetched one.
    pub cwd: Option<PathBuf>,
    /// The `PWD` Cargo was started with, passed on to rustc: usually inside the workspace being built.
    pub pwd: Option<PathBuf>,
    /// Where the Cargo that started rustc was asked to build: its working directory and the directory of its
    /// `--manifest-path` (Linux: the parent process; empty elsewhere or behind a rustc wrapper).
    pub cargo_dirs: Vec<PathBuf>,
    /// The working directory of the Cargo that started rustc, as [Self::cargo_dirs] learns it.
    pub cargo_cwd: Option<PathBuf>,
    /// Its `--config` arguments, in order (`KEY=VALUE` or a file).
    pub cargo_configs: Vec<String>,
    /// `--out-dir` and every `-L dependency=`: where the crates compiled for this build are, with their dep-info.
    pub dep_dirs: Vec<PathBuf>,
    /// The file of each `--extern` dependency (`lib<name>-<hash>.rmeta` or `.rlib`).
    pub externs: Vec<PathBuf>,
}

impl RustcBuild {
    /// The build of this process when it is rustc started by Cargo (a proc macro runs inside it), `None` otherwise
    /// (a CLI, a test, a build script).
    pub fn from_process() -> Option<Self> {
        let manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR")?;
        let mut build = Self::from_args(std::env::args_os().skip(1), PathBuf::from(manifest_dir))?;
        build.cwd = std::env::current_dir().ok();
        build.pwd = std::env::var_os("PWD").map(PathBuf::from);
        let cargo = cargo_command();
        build.cargo_dirs = cargo.dirs;
        build.cargo_cwd = cargo.cwd;
        build.cargo_configs = cargo.configs;
        Some(build)
    }

    /// The build rustc's arguments `args` describe, `None` when they aren't rustc's (no `--crate-name`).
    pub fn from_args(
        args: impl IntoIterator<Item = OsString>,
        manifest_dir: PathBuf,
    ) -> Option<Self> {
        let mut build = Self {
            manifest_dir,
            ..Default::default()
        };
        let mut is_rustc = false;
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let Some(arg) = arg.to_str() else { continue };
            // `--flag value` and `--flag=value` alike
            let (flag, value) = match arg.split_once('=') {
                Some((flag, value)) if flag.starts_with("--") => (flag, Some(value.to_owned())),
                _ => (arg, None),
            };
            let mut value = || value.clone().or_else(|| args.next()?.into_string().ok());
            match flag {
                "--crate-name" => is_rustc = true,
                "--out-dir" => build.dep_dirs.extend(value().map(PathBuf::from)),
                "-L" => {
                    if let Some(dir) = value()
                        .as_deref()
                        .and_then(|v| v.strip_prefix("dependency="))
                    {
                        build.dep_dirs.push(PathBuf::from(dir));
                    }
                }
                "--extern" => {
                    // `name=path`; a bare `name` is a crate of the sysroot
                    if let Some((_, path)) = value().as_deref().and_then(|v| v.split_once('=')) {
                        build.externs.push(PathBuf::from(path));
                    }
                }
                _ => {}
            }
        }
        build.dep_dirs.sort();
        build.dep_dirs.dedup();
        is_rustc.then_some(build)
    }

    /// Whether this build compiles the crate in `dir` (both canonical).
    pub fn compiles(&self, dir: &Path) -> bool {
        self.manifest_dir.canonicalize().is_ok_and(|own| own == dir)
    }

    /// The package directory each `--extern` dependency was compiled from, canonical; one whose dep-info is
    /// missing or names a relative source (a path package, given relative to the workspace root) is left out.
    pub fn extern_dirs(&self) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = self
            .externs
            .iter()
            .filter_map(|file| {
                let stem = file.file_stem()?.to_str()?;
                let d =
                    file.with_file_name(format!("{}.d", stem.strip_prefix("lib").unwrap_or(stem)));
                package_dir_of(&first_source(&d)?)
            })
            .collect();
        dirs.sort();
        dirs.dedup();
        dirs
    }

    /// Package directories of the crates compiled into [Self::dep_dirs] by crate name (`-` as `_`), each
    /// canonical, from their dep-info. A directory used for many builds keeps older ones too: a crate may have more
    /// than one.
    pub fn compiled(&self) -> HashMap<String, Vec<PathBuf>> {
        let mut compiled: HashMap<String, Vec<PathBuf>> = HashMap::new();
        let mut budget = DEP_INFO_FILES_MAX;
        for dir in &self.dep_dirs {
            let Ok(entries) = fs::read_dir(dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_none_or(|e| e != "d") {
                    continue;
                }
                if budget == 0 {
                    tracing::debug!(
                        "more than {DEP_INFO_FILES_MAX} dep-info files, the rest is not read"
                    );
                    return compiled;
                }
                budget -= 1;
                // `<crate>-<16 hex digits>.d`
                let Some(name) = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.rsplit_once('-'))
                    .map(|(name, _)| name.to_owned())
                else {
                    continue;
                };
                if let Some(dir) = first_source(&path).and_then(|src| package_dir_of(&src)) {
                    let dirs = compiled.entry(name).or_default();
                    if !dirs.contains(&dir) {
                        dirs.push(dir);
                    }
                }
            }
        }
        compiled
    }
}

/// What the command line of the Cargo that started this process says.
#[derive(Default)]
struct CargoCommand {
    /// [RustcBuild::cargo_dirs]
    dirs: Vec<PathBuf>,
    /// [RustcBuild::cargo_cwd]
    cwd: Option<PathBuf>,
    /// [RustcBuild::cargo_configs]
    configs: Vec<String>,
}

/// The [CargoCommand] of this process's parent.
#[cfg(target_os = "linux")]
fn cargo_command() -> CargoCommand {
    let parent = Path::new("/proc").join(std::os::unix::process::parent_id().to_string());
    let cwd = fs::read_link(parent.join("cwd")).ok();
    let mut cmdline = Vec::new();
    let read = fs::File::open(parent.join("cmdline"))
        .and_then(|f| f.take(DEP_INFO_BYTES_MAX).read_to_end(&mut cmdline));
    let args: Vec<&str> = if read.is_ok() {
        cmdline
            .split(|&b| b == 0)
            .filter_map(|a| std::str::from_utf8(a).ok())
            .collect()
    } else {
        vec![]
    };
    command_of(cwd, &args)
}

/// The [CargoCommand] of this process's parent: nothing outside Linux.
#[cfg(not(target_os = "linux"))]
fn cargo_command() -> CargoCommand {
    CargoCommand::default()
}

/// The [CargoCommand] of a Cargo started in `cwd` with `args`.
#[cfg_attr(
    not(target_os = "linux"),
    expect(dead_code, reason = "only Linux tells the parent's command line")
)]
fn command_of(cwd: Option<PathBuf>, args: &[&str]) -> CargoCommand {
    let mut command = CargoCommand {
        dirs: cwd.iter().cloned().collect(),
        cwd,
        configs: vec![],
    };
    let base = command.cwd.clone().unwrap_or_default();
    let mut args = args.iter().copied();
    while let Some(arg) = args.next() {
        // everything after `--` is for the program Cargo runs
        if arg == "--" {
            break;
        }
        let (flag, value) = match arg.split_once('=') {
            Some((flag, value)) if flag.starts_with("--") => (flag, Some(value)),
            _ => (arg, None),
        };
        match flag {
            "--manifest-path" => {
                if let Some(manifest) = value.or_else(|| args.next()) {
                    command
                        .dirs
                        .extend(base.join(manifest).parent().map(Path::to_path_buf));
                }
            }
            "--config" => command
                .configs
                .extend(value.or_else(|| args.next()).map(str::to_owned)),
            _ => {}
        }
    }
    command
}

/// The first source an absolute dep-info file `d` names, the crate's root file; `None` for a relative one.
fn first_source(d: &Path) -> Option<PathBuf> {
    let file = fs::File::open(d).ok()?;
    let mut line = String::new();
    BufReader::new(file.take(DEP_INFO_BYTES_MAX))
        .read_line(&mut line)
        .ok()?;
    // `<output>: <source> <source> ..`, a space in a path escaped as `\ `
    let (_, sources) = line.split_once(": ")?;
    let mut source = String::new();
    let mut chars = sources.trim_end().chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => source.extend(chars.next()),
            ' ' => break,
            c => source.push(c),
        }
    }
    let source = PathBuf::from(source);
    source.is_absolute().then_some(source)
}

/// The nearest directory above `source` with a `Cargo.toml`, canonical.
fn package_dir_of(source: &Path) -> Option<PathBuf> {
    source
        .ancestors()
        .skip(1)
        .find(|dir| dir.join("Cargo.toml").is_file())
        .and_then(|dir| dir.canonicalize().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rustc_arguments_name_the_dependency_dirs_and_externs() {
        let args = [
            "--crate-name",
            "lib_host",
            "--edition=2024",
            "/src/lib.rs",
            "--out-dir",
            "/build/debug/deps",
            "-L",
            "dependency=/build/debug/deps",
            "-L",
            "dependency=/build/host/deps",
            "--extern",
            "api=/build/debug/deps/libapi-0123.rmeta",
            "--extern",
            "proc_macro",
        ];
        let build = RustcBuild::from_args(args.map(OsString::from), PathBuf::from("/src")).unwrap();
        assert_eq!(
            build.dep_dirs,
            [
                PathBuf::from("/build/debug/deps"),
                PathBuf::from("/build/host/deps")
            ]
        );
        assert_eq!(
            build.externs,
            [PathBuf::from("/build/debug/deps/libapi-0123.rmeta")]
        );
        assert!(RustcBuild::from_args(["--test"].map(OsString::from), PathBuf::new()).is_none());
    }

    #[test]
    fn cargo_arguments_name_the_manifest_and_the_configs() {
        let args = [
            "cargo",
            "build",
            "--config",
            "a.toml",
            "--manifest-path=sub/Cargo.toml",
            "--config=patch.crates-io.x.path='x'",
            "--",
            "--config",
            "not_cargo",
        ];
        let command = command_of(Some(PathBuf::from("/ws")), &args);
        assert_eq!(
            command.dirs,
            [PathBuf::from("/ws"), PathBuf::from("/ws/sub")]
        );
        assert_eq!(command.cwd, Some(PathBuf::from("/ws")));
        assert_eq!(command.configs, ["a.toml", "patch.crates-io.x.path='x'"]);
    }
}
