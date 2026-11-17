//! A crate built as a git dependency of another workspace finds its own dependencies through that workspace's
//! `Cargo.lock`, not through the one in its checkout: Cargo's layout and rustc's view of the build are laid out on
//! disk the way a real build leaves them.

use std::fs;
use std::path::{Path, PathBuf};
use ww_snapshots::cargo::{CargoSources, DepSpec, Dependent};
use ww_snapshots::rustc::RustcBuild;

const USED: &str = "aaaaaaa0000000000000000000000000000000000";
const OWN: &str = "bbbbbbb0000000000000000000000000000000000";
const LIB: &str = "ccccccc0000000000000000000000000000000000";

struct Layout {
    top: PathBuf,
    home: PathBuf,
    lib_host: PathBuf,
    /// `dep_api` at the commit the consumer workspace pins, which Cargo compiled.
    dep_api: PathBuf,
    consumer: PathBuf,
    deps: PathBuf,
}

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn lock(packages: &[(&str, Option<String>, &[&str])]) -> String {
    let mut lock = "version = 4\n".to_owned();
    for (name, source, deps) in packages {
        lock.push_str(&format!(
            "\n[[package]]\nname = \"{name}\"\nversion = \"0.1.0\"\n"
        ));
        if let Some(source) = source {
            lock.push_str(&format!("source = \"{source}\"\n"));
        }
        if !deps.is_empty() {
            let deps: Vec<String> = deps.iter().map(|d| format!("\"{d}\"")).collect();
            lock.push_str(&format!("dependencies = [{}]\n", deps.join(", ")));
        }
    }
    lock
}

fn manifest(name: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n")
}

/// `lib_host` comes from the repository `libr` at [LIB], whose own lock pins `dep_api` at [OWN], never fetched; the
/// consumer workspace (in `consumer`) pins it at [USED].
fn layout(name: &str, consumer: impl FnOnce(&Path) -> PathBuf) -> Layout {
    let top = std::env::temp_dir().join(format!("ww_build_lock_{name}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&top);
    let home = top.join("cargo_home");
    let base = home.join("git/checkouts/base-1111").join(&USED[..7]);
    let dep_api = base.join("dep_api");
    write(&dep_api.join("Cargo.toml"), &manifest("dep_api"));
    write(&dep_api.join("src/lib.rs"), "");
    let libr = home.join("git/checkouts/libr-2222").join(&LIB[..7]);
    write(
        &libr.join("Cargo.toml"),
        "[workspace]\nmembers = [\"lib_host\"]\n",
    );
    let lib_host = libr.join("lib_host");
    write(&lib_host.join("Cargo.toml"), &manifest("lib_host"));
    write(
        &libr.join("Cargo.lock"),
        &lock(&[
            (
                "dep_api",
                Some(format!("git+file:///repos/base#{OWN}")),
                &[],
            ),
            ("lib_host", None, &["dep_api"]),
        ]),
    );
    let consumer = consumer(&top);
    write(&consumer.join("Cargo.toml"), &manifest("consumer"));
    write(
        &consumer.join("Cargo.lock"),
        &lock(&[
            ("consumer", None, &["lib_host"]),
            (
                "dep_api",
                Some(format!("git+file:///repos/base#{USED}")),
                &[],
            ),
            (
                "lib_host",
                Some(format!("git+file:///repos/libr#{LIB}")),
                &["dep_api"],
            ),
        ]),
    );
    // the build directory is outside the workspace (Cargo's `build.build-dir`)
    let deps = top.join("build/debug/deps");
    write(
        &deps.join("dep_api-0123456789abcdef.d"),
        &format!(
            "{d}: {s}\n\n{d}: {s}\n",
            d = deps.join("dep_api-0123456789abcdef.d").display(),
            s = dep_api.join("src/lib.rs").display()
        ),
    );
    write(&deps.join("libdep_api-0123456789abcdef.rmeta"), "");
    Layout {
        top: top.canonicalize().unwrap(),
        home: home.canonicalize().unwrap(),
        lib_host: lib_host.canonicalize().unwrap(),
        dep_api: dep_api.canonicalize().unwrap(),
        consumer,
        deps,
    }
}

impl Layout {
    /// rustc compiling `lib_host` for the consumer: `--extern dep_api` when `extern_dep`, Cargo started in `pwd`.
    fn build(&self, pwd: Option<&Path>, extern_dep: bool) -> RustcBuild {
        RustcBuild {
            manifest_dir: self.lib_host.clone(),
            // Cargo runs rustc in a fetched package's own directory
            cwd: Some(self.lib_host.clone()),
            pwd: pwd.map(Path::to_path_buf),
            cargo_dirs: vec![],
            dep_dirs: vec![self.deps.clone()],
            externs: if extern_dep {
                vec![self.deps.join("libdep_api-0123456789abcdef.rmeta")]
            } else {
                vec![]
            },
        }
    }

    fn locate(&self, build: Option<RustcBuild>) -> anyhow::Result<PathBuf> {
        let mut sources = CargoSources::default();
        sources.entry_dir = Some(self.lib_host.clone());
        sources.cargo_home = Some(self.home.clone());
        sources.rustc = build;
        let version = semver::Version::new(0, 1, 0);
        let dependent = Dependent {
            name: "lib_host",
            version: &version,
            dir: &self.lib_host,
        };
        let spec = DepSpec::Git {
            url: "file:///repos/base",
        };
        sources.locate(&dependent, "dep_api", &spec)
    }
}

#[test]
fn a_git_dependency_is_walked_at_the_commit_the_consumer_lock_pins() {
    let layout = layout("pwd", |top| top.join("consumer"));
    // its own lock: a checkout that isn't there, or the wrong commit when it happens to be
    let own = layout.locate(None).unwrap_err();
    let found = layout.locate(Some(layout.build(Some(&layout.consumer), true)));
    let cargo_dir = layout.locate(Some(RustcBuild {
        cargo_dirs: vec![layout.consumer.join("src")],
        ..layout.build(None, true)
    }));
    let lost = layout
        .locate(Some(layout.build(None, true)))
        .unwrap_err()
        .to_string();
    // without the `--extern`, the dep-info of what was compiled still rules the lib's own lock out
    let transitive = layout
        .locate(Some(layout.build(None, false)))
        .unwrap_err()
        .to_string();
    fs::remove_dir_all(&layout.top).unwrap();
    assert!(own.to_string().contains("no checkout"), "{own:#}");
    assert_eq!(found.unwrap(), layout.dep_api);
    assert_eq!(cargo_dir.unwrap(), layout.dep_api);
    for err in [&lost, &transitive] {
        assert!(
            err.contains("built as a dependency of another workspace"),
            "{err}"
        );
        assert!(err.contains("libr-2222/ccccccc/Cargo.lock"), "{err}");
        assert!(err.contains(&layout.dep_api.display().to_string()), "{err}");
    }
    assert!(lost.contains("pins no dep_api at"), "{lost}");
    assert!(
        transitive.contains(&format!(
            "pins dep_api 0.1.0 from git+file:///repos/base#{OWN}"
        )),
        "{transitive}"
    );
}

#[test]
fn a_workspace_installed_from_a_checkout_is_found_among_the_checkouts() {
    // `cargo install --git`: the workspace is a checkout itself, the build directory a temporary one
    let layout = layout("install", |top| {
        top.join("cargo_home/git/checkouts/consumer-3333/ddddddd")
    });
    let found = layout.locate(Some(layout.build(None, false)));
    fs::remove_dir_all(&layout.top).unwrap();
    assert_eq!(found.unwrap(), layout.dep_api);
}

/// A workspace crate built as a path dependency of a workspace around it (trybuild's under `target/tests/`): the
/// outer lock or the crate's own both pin the git source Cargo compiled, and a registry crate the lock pins in two
/// versions, of which the build compiled one, rules neither out.
#[test]
fn a_path_dependency_of_an_outer_workspace_takes_a_lock_pinning_its_git_sources() {
    let top = std::env::temp_dir().join(format!("ww_build_lock_path_{}", std::process::id()));
    let _ = fs::remove_dir_all(&top);
    let home = top.join("cargo_home");
    let dep_api = home
        .join("git/checkouts/base-1111")
        .join(&USED[..7])
        .join("dep_api");
    write(&dep_api.join("Cargo.toml"), &manifest("dep_api"));
    write(&dep_api.join("src/lib.rs"), "");
    let flags = home.join("registry/src/index.crates.io-abc/flags-2.0.0");
    write(
        &flags.join("Cargo.toml"),
        "[package]\nname = \"flags\"\nversion = \"2.0.0\"\n",
    );
    write(&flags.join("src/lib.rs"), "");
    let git = format!("git+file:///repos/base#{USED}");
    let registry = "registry+https://github.com/rust-lang/crates.io-index";
    let repo = top.join("repo");
    let timebase = repo.join("timebase");
    write(
        &repo.join("Cargo.toml"),
        "[workspace]\nmembers = [\"timebase\"]\n",
    );
    write(&timebase.join("Cargo.toml"), &manifest("timebase"));
    write(
        &repo.join("Cargo.lock"),
        &format!(
            "version = 4\n\n[[package]]\nname = \"dep_api\"\nversion = \"0.1.0\"\nsource = \"{git}\"\n\n\
             [[package]]\nname = \"flags\"\nversion = \"1.3.2\"\nsource = \"{registry}\"\n\n\
             [[package]]\nname = \"flags\"\nversion = \"2.0.0\"\nsource = \"{registry}\"\n\n\
             [[package]]\nname = \"timebase\"\nversion = \"0.1.0\"\n\
             dependencies = [\"dep_api\", \"flags 1.3.2\", \"flags 2.0.0\"]\n"
        ),
    );
    let outer = repo.join("target/tests/trybuild/client");
    write(&outer.join("Cargo.toml"), &manifest("client-tests"));
    write(
        &outer.join("Cargo.lock"),
        &format!(
            "version = 4\n\n[[package]]\nname = \"client-tests\"\nversion = \"0.1.0\"\ndependencies = [\"timebase\"]\n\n\
             [[package]]\nname = \"dep_api\"\nversion = \"0.1.0\"\nsource = \"{git}\"\n\n\
             [[package]]\nname = \"flags\"\nversion = \"2.0.0\"\nsource = \"{registry}\"\n\n\
             [[package]]\nname = \"timebase\"\nversion = \"0.1.0\"\ndependencies = [\"dep_api\", \"flags\"]\n"
        ),
    );
    let deps = repo.join("target/tests/trybuild/target/debug/deps");
    for (name, src) in [("dep_api", &dep_api), ("flags", &flags)] {
        write(
            &deps.join(format!("{name}-0123456789abcdef.d")),
            &format!("{}: {}\n", deps.display(), src.join("src/lib.rs").display()),
        );
        write(&deps.join(format!("lib{name}-0123456789abcdef.rmeta")), "");
    }
    let timebase = timebase.canonicalize().unwrap();
    let build = |cargo_dirs: Vec<PathBuf>| RustcBuild {
        manifest_dir: timebase.clone(),
        // a path package outside the workspace root: Cargo runs rustc in its own directory
        cwd: Some(timebase.clone()),
        pwd: None,
        cargo_dirs,
        dep_dirs: vec![deps.clone()],
        externs: vec![
            deps.join("libdep_api-0123456789abcdef.rmeta"),
            deps.join("libflags-0123456789abcdef.rmeta"),
        ],
    };
    let locate = |build: RustcBuild| {
        let mut sources = CargoSources::default();
        sources.entry_dir = Some(timebase.clone());
        sources.cargo_home = Some(home.canonicalize().unwrap());
        sources.rustc = Some(build);
        let version = semver::Version::new(0, 1, 0);
        let dependent = Dependent {
            name: "timebase",
            version: &version,
            dir: &timebase,
        };
        let spec = DepSpec::Git {
            url: "file:///repos/base",
        };
        sources.locate(&dependent, "dep_api", &spec)
    };
    let outer_lock = locate(build(vec![outer.clone()]));
    let own_lock = locate(build(vec![]));
    let dep_api = dep_api.canonicalize().unwrap();
    let outer_lock = outer_lock.map(|dir| dir.canonicalize().unwrap());
    let own_lock = own_lock.map(|dir| dir.canonicalize().unwrap());
    fs::remove_dir_all(&top).unwrap();
    assert_eq!(outer_lock.unwrap(), dep_api);
    assert_eq!(own_lock.unwrap(), dep_api);
}
