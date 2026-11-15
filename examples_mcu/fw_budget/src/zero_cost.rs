//! Zero-cost check (TEST-34): a snapshot of what a change must leave alone, and a comparison of two snapshots.
//!
//! `--snapshot DIR [--tree PATH]` writes `DIR/manifest.txt`, one line per digest, of the tree at `PATH` (this
//! repository when not given), with that tree's own code:
//!
//! - every API crate (`tests/*_api`, `examples/*_api`, `ww_stdlib/*`, `wire_weaver_evolution/tests/fixtures/*` and
//!   the root crates with `api_snapshots/`): the snapshot `ww api save` writes (every trait and type of the crate as
//!   the walker reads them) and the bundle of each API root `ww api ast` prints (what codegen builds from), both kept
//!   under `DIR/bundles/` for a `diff`;
//! - every firmware build of [BUILDS], at `fw-z` and `fw-3`: size and hash of `.text`, `.rodata` and `.data` of each
//!   bin, the TEST-30 flash budget fixture among them.
//!
//! Everything is built in one staging directory the tree is copied into: v0 symbol names hash the absolute path of
//! a path dependency outside a board's workspace, which orders functions differently from one checkout to another.
//! The copy writes only files whose content differs, so builds after the first are incremental. `--compare A B` fails on any digest that differs or is in one snapshot only, naming the crate or
//! the board, build, profile, bin and section.

use crate::{Res, cargo};
use object::{Object, ObjectSection};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The ELF sections compared: code, read-only data and initialised data.
const SECTIONS: [&str; 3] = [".text", ".rodata", ".data"];
/// The profiles compared, as `opt-level`s; a board with a `[profile.fw-z]` builds at its own profiles.
const PROFILES: [(&str, &str); 2] = [("fw-z", "z"), ("fw-3", "3")];
/// The manifest of a snapshot, in its directory.
const MANIFEST: &str = "manifest.txt";
/// Directories not copied into the staging tree, and kept there: build outputs and tool caches.
const NOT_STAGED: [&str; 6] = [
    "target",
    ".git",
    ".venv",
    "__pycache__",
    ".pytest_cache",
    "site",
];
/// Directories whose crates with a `Cargo.toml` are API crates, and the suffix their names need (`""`: any).
const API_DIRS: [(&str, &str); 4] = [
    ("tests", "_api"),
    ("examples", "_api"),
    ("ww_stdlib", ""),
    ("wire_weaver_evolution/tests/fixtures", ""),
];

/// One firmware build of a board under `examples_mcu/`: its name in the manifest and the cargo arguments, beside
/// `--bins` (unless they name a `--bin`, or the `--bin`s of `flash_budget`).
struct Build {
    board: &'static str,
    name: &'static str,
    args: &'static [&'static str],
    /// The TEST-30 flash budget bins of `mcu_qemu` instead of every bin.
    flash_budget: bool,
}

const fn build(board: &'static str, name: &'static str, args: &'static [&'static str]) -> Build {
    Build {
        board,
        name,
        args,
        flash_budget: false,
    }
}

/// Every firmware build, the feature sets of `just check-examples-mcu` and the flash budget fixture (TEST-30) on
/// Cortex-M0+, sync and `use_async`, as `just flash-budget` builds it.
const BUILDS: &[Build] = &[
    build("mcu_qemu", "default", &[]),
    build("mcu_qemu", "flash-async", &["--features", "flash-async"]),
    build("mcu_qemu", "streams", &["--features", "streams"]),
    Build {
        board: "mcu_qemu",
        name: "flash-budget-sync",
        args: &["--target", "thumbv6m-none-eabi"],
        flash_budget: true,
    },
    Build {
        board: "mcu_qemu",
        name: "flash-budget-async",
        args: &[
            "--target",
            "thumbv6m-none-eabi",
            "--features",
            "flash-async",
        ],
        flash_budget: true,
    },
    build("mcu_qemu_m4f", "default", &[]),
    build("nucleo_h743zi2", "default", &[]),
    build("nucleo_h743zi2", "streams", &["--features", "streams"]),
    build("nucleo_h743zi2", "settings", &["--features", "settings"]),
    build("nucleo_g0b1re", "usb", &["--features", "usb"]),
    build(
        "nucleo_g0b1re",
        "rtt",
        &[
            "--no-default-features",
            "--features",
            "nucleo_g0b1re,rtt_target",
        ],
    ),
    build(
        "nucleo_g0b1re",
        "rtt-settings",
        &[
            "--no-default-features",
            "--features",
            "nucleo_g0b1re,rtt_target,settings",
        ],
    ),
    build("usb_stm32h725ig", "default", &[]),
    build("usb_stm32h725ig", "streams", &["--features", "streams"]),
    build("rp2", "default", &[]),
    build("rp2", "usb", &["--features", "usb"]),
    build("rp2", "ws_ncm", &["--features", "ws_ncm"]),
    build("rp2", "udp_ncm", &["--features", "udp_ncm"]),
    build(
        "rp2",
        "rtt",
        &[
            "--no-default-features",
            "--features",
            "rtt_target",
            "--bin",
            "rp2_ww_rtt",
        ],
    ),
    build(
        "rp2",
        "usb_log",
        &[
            "--no-default-features",
            "--features",
            "usb_log",
            "--bin",
            "rp2_usb_log",
        ],
    ),
    build("ecc_flash_g0b1", "default", &[]),
    build("ecc_flash_h7", "default", &[]),
];

/// What `--snapshot` covers.
pub struct Scope<'a> {
    /// The tree to snapshot: a checkout of this repository.
    pub tree: &'a Path,
    /// Where the tree is copied to and built: the same directory for every tree compared.
    pub stage: &'a Path,
    /// Only the builds of these boards; empty is all, `none` no firmware.
    pub boards: &'a [String],
}

/// Writes the snapshot of `scope.tree` into `dir`.
pub fn snapshot(dir: &Path, scope: &Scope) -> Res<()> {
    let source = scope.tree.canonicalize()?;
    eprintln!("copying {} to {}", source.display(), scope.stage.display());
    std::fs::create_dir_all(scope.stage)?;
    let tree = scope.stage.canonicalize()?;
    stage(&source, &tree)?;
    let bundles_dir = dir.join("bundles");
    if bundles_dir.exists() {
        std::fs::remove_dir_all(&bundles_dir)?;
    }
    std::fs::create_dir_all(&bundles_dir)?;
    let mut digests: BTreeMap<String, Digest> = BTreeMap::new();
    bundles(&tree, &bundles_dir, &mut digests)?;
    for b in BUILDS {
        let wanted = scope.boards.is_empty() || scope.boards.iter().any(|s| s == b.board);
        if wanted {
            firmware(&tree, b, &mut digests)?;
        }
    }
    let mut text = String::new();
    for (key, d) in &digests {
        writeln!(text, "{key}\t{}\t{:016x}", d.size, d.hash)?;
    }
    std::fs::write(dir.join(MANIFEST), text)?;
    println!(
        "snapshot of {}: {} digests in {}",
        source.display(),
        digests.len(),
        dir.display()
    );
    Ok(())
}

/// The staging directory of the checkout at `repo`: `ww-zero-cost/<hash of its path>` in the user's cache, outside
/// any cargo workspace (a board under a workspace's directory is taken for its member), one per checkout so two
/// sessions don't build in each other's copy.
pub fn default_stage(repo: &Path) -> Res<PathBuf> {
    let cache = match std::env::var_os("XDG_CACHE_HOME") {
        Some(dir) => PathBuf::from(dir),
        None => {
            PathBuf::from(std::env::var_os("HOME").ok_or("neither XDG_CACHE_HOME nor HOME is set")?)
                .join(".cache")
        }
    };
    let hash = Digest::of(repo.as_os_str().as_encoded_bytes()).hash;
    Ok(cache.join("ww-zero-cost").join(format!("{hash:016x}")))
}

/// Makes `to` a copy of `from` (but [NOT_STAGED]): writes the files whose content differs, removes the ones `from`
/// lacks.
fn stage(from: &Path, to: &Path) -> Res<()> {
    std::fs::create_dir_all(to)?;
    let mut names = Vec::new();
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        if NOT_STAGED.iter().any(|n| name == *n) {
            continue;
        }
        let (src, dst) = (entry.path(), to.join(&name));
        let kind = entry.file_type()?;
        if kind.is_dir() {
            if dst.is_file() {
                std::fs::remove_file(&dst)?;
            }
            stage(&src, &dst)?;
        } else if kind.is_file() {
            if dst.is_dir() {
                std::fs::remove_dir_all(&dst)?;
            }
            let bytes = std::fs::read(&src)?;
            if std::fs::read(&dst).ok().as_deref() != Some(&bytes[..]) {
                std::fs::write(&dst, &bytes)?;
                std::fs::set_permissions(&dst, entry.metadata()?.permissions())?;
            }
        } else {
            return Err(format!("{}: not a file or directory", src.display()).into());
        }
        names.push(name);
    }
    for entry in std::fs::read_dir(to)? {
        let entry = entry?;
        let name = entry.file_name();
        if names.contains(&name) || NOT_STAGED.iter().any(|n| name == *n) {
            continue;
        }
        if entry.file_type()?.is_dir() {
            std::fs::remove_dir_all(entry.path())?;
        } else {
            std::fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

/// Size and FNV-1a hash of some bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Digest {
    size: u64,
    hash: u64,
}

impl Digest {
    fn of(bytes: &[u8]) -> Self {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for b in bytes {
            hash ^= u64::from(*b);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        Self {
            size: bytes.len() as u64,
            hash,
        }
    }
}

/// The API crates of `tree`, as paths relative to it.
fn api_crates(tree: &Path) -> Res<Vec<String>> {
    let mut crates = Vec::new();
    for (dir, suffix) in API_DIRS {
        for entry in std::fs::read_dir(tree.join(dir))? {
            let path = entry?.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.ends_with(suffix) && path.join("Cargo.toml").exists() {
                crates.push(format!("{dir}/{name}"));
            }
        }
    }
    for entry in std::fs::read_dir(tree)? {
        let path = entry?.path();
        if path.join("Cargo.toml").exists() && path.join("api_snapshots").is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            crates.push(name.to_string());
        }
    }
    crates.sort();
    crates.dedup();
    Ok(crates)
}

/// Builds the tree's `ww` and digests the snapshot and root bundles of every API crate.
fn bundles(tree: &Path, out: &Path, digests: &mut BTreeMap<String, Digest>) -> Res<()> {
    eprintln!("building ww in {}", tree.display());
    let status = cargo(tree)
        .args(["build", "-q", "-p", "wire_weaver_cli", "--bin", "ww"])
        .status()?;
    if !status.success() {
        return Err(format!("cargo build of ww in {} failed", tree.display()).into());
    }
    let ww = tree.join("target/debug/ww");
    let crates = api_crates(tree)?;
    eprintln!("digesting the bundles of {} API crates", crates.len());
    for krate in &crates {
        let crate_dir = tree.join(krate);
        let crate_out = out.join(krate);
        // the snapshot: one file, or one per root and wire line of a crate with several roots
        let saved = crate_out.join("saved");
        std::fs::create_dir_all(&saved)?;
        let run = Command::new(&ww)
            .args(["api", "save", "--force", "--snapshots-dir"])
            .arg(&saved)
            .arg(&crate_dir)
            .output()?;
        if run.status.success() {
            let mut files: Vec<PathBuf> = std::fs::read_dir(&saved)?
                .map(|e| e.map(|e| e.path()))
                .collect::<Result<_, _>>()?;
            files.sort();
            for file in files {
                let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("");
                let text = std::fs::read(&file)?;
                let key = format!("bundle {krate} snapshot{}", snapshot_suffix(name));
                digests.insert(key, Digest::of(&text));
            }
        } else {
            let text = failure_text(tree, &run.stderr);
            std::fs::write(crate_out.join("save-error.txt"), &text)?;
            digests.insert(
                format!("bundle {krate} snapshot error"),
                Digest::of(text.as_bytes()),
            );
        }
        // the bundle of the root, or of each trait named when the crate has several
        let run = ast(&ww, &crate_dir, None)?;
        let names = match &run {
            Err(text) => several_traits(text),
            Ok(_) => Vec::new(),
        };
        let mut runs = vec![("root".to_string(), run)];
        for name in names {
            runs.push((name.clone(), ast(&ww, &crate_dir, Some(&name))?));
        }
        for (label, run) in runs {
            let (kind, text) = match run {
                Ok(stdout) => ("ast", strip_tree(tree, &stdout)),
                Err(stderr) => ("ast error", failure_text(tree, &stderr)),
            };
            std::fs::write(crate_out.join(format!("{label}.{kind}.txt")), &text)?;
            digests.insert(
                format!("bundle {krate} {kind} {label}"),
                Digest::of(text.as_bytes()),
            );
        }
    }
    Ok(())
}

/// `methods_api_0_1_0.ron` gives `.ron`, `multi_root_api_0_1_0.wire_v1.Thermo@w1.2.ron` gives
/// `.wire_v1.Thermo@w1.2.ron`: the crate version is left out of the key, a session doesn't bump it.
fn snapshot_suffix(file: &str) -> &str {
    // neither a crate name nor its version has a dot
    file.find('.').map_or(file, |i| &file[i..])
}

/// `ww api ast` of a crate, or of one of its traits: stdout, or stderr when it failed.
fn ast(ww: &Path, crate_dir: &Path, name: Option<&str>) -> Res<Result<Vec<u8>, Vec<u8>>> {
    let mut cmd = Command::new(ww);
    cmd.args(["api", "ast"]).arg(crate_dir);
    if let Some(name) = name {
        cmd.args(["--name", name]);
    }
    let run = cmd.output()?;
    Ok(if run.status.success() {
        Ok(run.stdout)
    } else {
        Err(run.stderr)
    })
}

/// The traits `ww api ast` lists when a crate has several (``Multiple ... found ..: `A`, `B` ``).
fn several_traits(stderr: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(stderr);
    let Some(line) = text.lines().find(|l| l.contains("Multiple #[")) else {
        return Vec::new();
    };
    let Some((_, list)) = line.rsplit_once(": ") else {
        return Vec::new();
    };
    list.split(", ")
        .filter_map(|n| n.strip_prefix('`')?.strip_suffix('`'))
        .map(str::to_string)
        .collect()
}

/// Output of a run with the tree's path replaced, so two worktrees give the same text.
fn strip_tree(tree: &Path, bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).replace(&*tree.to_string_lossy(), "<tree>")
}

/// The error lines of a failed run, without warnings (which name paths and change with unrelated work).
fn failure_text(tree: &Path, stderr: &[u8]) -> String {
    strip_tree(tree, stderr)
        .lines()
        .filter(|l| !l.starts_with("warning:"))
        .map(|l| format!("{l}\n"))
        .collect()
}

/// Builds `b` at both profiles and digests the sections of every bin.
fn firmware(tree: &Path, b: &Build, digests: &mut BTreeMap<String, Digest>) -> Res<()> {
    let dir = tree.join("examples_mcu").join(b.board);
    let manifest = std::fs::read_to_string(dir.join("Cargo.toml"))?;
    let own_profiles = manifest.contains("[profile.fw-z]");
    let remap = format!(
        "target.'cfg(target_os = \"none\")'.rustflags=[\"--remap-path-prefix={}=/ww\"]",
        tree.display()
    );
    for (profile, opt_level) in PROFILES {
        eprintln!("building {} {} at {profile}", b.board, b.name);
        let target_dir = dir
            .join("target/zero-cost")
            .join(format!("{}-{profile}", b.name));
        let mut cmd = cargo(&dir);
        // a bin that doesn't fit its board's flash at one profile fails alone: the others are still compared
        cmd.args([
            "build",
            "--keep-going",
            "--message-format=json-render-diagnostics",
            "--config",
            &remap,
        ])
        .env("CARGO_TARGET_DIR", &target_dir);
        if own_profiles {
            cmd.args(["--profile", profile]);
        } else {
            cmd.arg("--release")
                .env("CARGO_PROFILE_RELEASE_OPT_LEVEL", opt_level);
        }
        cmd.args(b.args);
        if b.flash_budget {
            for bin in crate::flash::bins() {
                cmd.args(["--bin", &bin]);
            }
        } else if !b.args.contains(&"--bin") {
            cmd.arg("--bins");
        }
        let run = cmd.stderr(Stdio::inherit()).output()?;
        let elfs = executables(&String::from_utf8_lossy(&run.stdout));
        if elfs.is_empty() {
            return Err(format!("{} {} at {profile}: no bin built", b.board, b.name).into());
        }
        if !run.status.success() {
            eprintln!(
                "warning: {} {} at {profile}: some bins failed to build, the others are compared",
                b.board, b.name
            );
            // the bins that did build, so the same failure on both sides compares equal
            let mut built: Vec<&str> = elfs
                .iter()
                .filter_map(|e| e.file_name()?.to_str())
                .collect();
            built.sort_unstable();
            let names = built.join("\n");
            let key = format!("firmware {} {} {profile} build failed", b.board, b.name);
            digests.insert(key, Digest::of(names.as_bytes()));
        }
        for elf in elfs {
            let bin = elf.file_name().and_then(|n| n.to_str()).unwrap_or("?");
            let data = std::fs::read(&elf)?;
            let file = object::File::parse(&*data)?;
            for section in SECTIONS {
                let key = format!("firmware {} {} {profile} {bin} {section}", b.board, b.name);
                let digest = match file.section_by_name(section) {
                    Some(s) => Digest::of(s.data()?),
                    None => Digest { size: 0, hash: 0 },
                };
                digests.insert(key, digest);
            }
        }
    }
    Ok(())
}

/// The `"executable"` paths of cargo's JSON messages.
fn executables(json: &str) -> Vec<PathBuf> {
    const KEY: &str = "\"executable\":\"";
    json.lines()
        .filter_map(|line| {
            let (_, rest) = line.split_once(KEY)?;
            let (path, _) = rest.split_once('"')?;
            Some(PathBuf::from(path))
        })
        .collect()
}

/// Compares two snapshots and fails on any difference.
pub fn compare(a: &Path, b: &Path) -> Res<()> {
    let (da, db) = (read_manifest(a)?, read_manifest(b)?);
    let diffs = differences(&da, &db);
    for line in &diffs {
        println!("{line}");
    }
    if diffs.is_empty() {
        println!("identical: {} digests", da.len());
        return Ok(());
    }
    if diffs.iter().any(|d| d.contains("bundle ")) {
        println!(
            "bundle texts: diff -r {} {}",
            a.join("bundles").display(),
            b.join("bundles").display()
        );
    }
    Err(format!(
        "{} of {} digests differ",
        diffs.len(),
        da.len().max(db.len())
    )
    .into())
}

fn read_manifest(dir: &Path) -> Res<BTreeMap<String, Digest>> {
    let path = dir.join(MANIFEST);
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("{}: {e}, run --snapshot first", path.display()))?;
    let mut digests = BTreeMap::new();
    for line in text.lines() {
        let mut parts = line.split('\t');
        let (Some(key), Some(size), Some(hash), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(format!("{}: bad line {line:?}", path.display()).into());
        };
        let digest = Digest {
            size: size.parse()?,
            hash: u64::from_str_radix(hash, 16)?,
        };
        digests.insert(key.to_string(), digest);
    }
    Ok(digests)
}

/// One line per key that differs: changed (with both sizes), or in one snapshot only.
fn differences(a: &BTreeMap<String, Digest>, b: &BTreeMap<String, Digest>) -> Vec<String> {
    let mut diffs = Vec::new();
    for (key, da) in a {
        match b.get(key) {
            None => diffs.push(format!("only in A: {key}")),
            Some(db) if db != da => diffs.push(format!(
                "changed: {key}: {} -> {} B ({:+})",
                da.size,
                db.size,
                db.size as i64 - da.size as i64
            )),
            Some(_) => {}
        }
    }
    for key in b.keys().filter(|k| !a.contains_key(*k)) {
        diffs.push(format!("only in B: {key}"));
    }
    diffs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_key_leaves_the_version_out() {
        assert_eq!(snapshot_suffix("methods_api_0_1_0.ron"), ".ron");
        assert_eq!(
            snapshot_suffix("multi_root_api_0_1_0.wire_v1.Thermo@w1.2.ron"),
            ".wire_v1.Thermo@w1.2.ron"
        );
        assert_eq!(snapshot_suffix("ww_si_0_5_0.ron"), ".ron");
        assert_eq!(snapshot_suffix("i2c_v2_api_0_12_3.ron"), ".ron");
    }

    #[test]
    fn differences_name_the_key() {
        let d = |size, hash| Digest { size, hash };
        let a = BTreeMap::from([
            (
                "firmware rp2 default fw-z blinky .text".to_string(),
                d(10, 1),
            ),
            ("bundle tests/a_api snapshot.ron".to_string(), d(5, 2)),
        ]);
        let mut b = a.clone();
        assert!(differences(&a, &b).is_empty());
        b.insert(
            "firmware rp2 default fw-z blinky .text".to_string(),
            d(12, 3),
        );
        b.insert("bundle tests/b_api snapshot.ron".to_string(), d(1, 1));
        assert_eq!(
            differences(&a, &b),
            [
                "changed: firmware rp2 default fw-z blinky .text: 10 -> 12 B (+2)",
                "only in B: bundle tests/b_api snapshot.ron"
            ]
        );
    }

    #[test]
    fn executables_of_cargo_json() {
        let json = "{\"reason\":\"compiler-artifact\",\"executable\":null}\n\
                    {\"reason\":\"compiler-artifact\",\"filenames\":[\"/t/x\"],\"executable\":\"/t/x\",\"fresh\":true}\n";
        assert_eq!(executables(json), [PathBuf::from("/t/x")]);
    }

    #[test]
    fn stage_copies_and_prunes() {
        let root = std::env::temp_dir().join(format!("fw-budget-stage-{}", std::process::id()));
        let (from, to) = (root.join("from"), root.join("to"));
        std::fs::create_dir_all(from.join("a/target")).unwrap();
        std::fs::write(from.join("a/x.rs"), "x").unwrap();
        std::fs::write(from.join("a/target/out"), "built").unwrap();
        std::fs::create_dir_all(to.join("a/target")).unwrap();
        std::fs::write(to.join("a/target/kept"), "").unwrap();
        std::fs::write(to.join("gone.rs"), "").unwrap();
        stage(&from, &to).unwrap();
        assert_eq!(std::fs::read_to_string(to.join("a/x.rs")).unwrap(), "x");
        assert!(to.join("a/target/kept").exists());
        assert!(!to.join("a/target/out").exists());
        assert!(!to.join("gone.rs").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn traits_of_a_crate_with_several() {
        let err = b"Error: Multiple #[ww_api_root] marked traits found in crate x, name one by its module path (`ww_codegen!(api::wire_v2::Thermo ..)`, EVO-37): `wire_v1::Thermo`, `Floor`\n";
        assert_eq!(several_traits(err), ["wire_v1::Thermo", "Floor"]);
        assert!(several_traits(b"Error: No #[ww_api_root] found").is_empty());
    }
}
