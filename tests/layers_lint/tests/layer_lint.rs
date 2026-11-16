//! R9: fails on any crate (in any workspace of this repo) missing from `layers.toml`, and on any
//! normal, build or dev dependency edge from a lower-layer crate to a higher-layer one that
//! `layers.toml`'s `[[allow]]` list doesn't name; an allow-list line whose edge no longer exists
//! fails too. See `/layers.toml` and
//! `~/git/tpm/prj/P2401-wireweaver/design/architecture-layers.md` section 5.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use cargo_metadata::{MetadataCommand, PackageId};
use serde::Deserialize;

#[derive(Deserialize)]
struct LayersFile {
    layers: BTreeMap<String, u8>,
    #[serde(default)]
    allow: Vec<AllowEntry>,
}

#[derive(Deserialize)]
struct AllowEntry {
    from: String,
    to: String,
    refactor: String,
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("tests/layers_lint is two levels under the repo root")
        .to_path_buf()
}

/// Every workspace in the repo: the root (`tests/*`, `examples/*`, `ww_stdlib/*`, ... are members
/// of it already), `mcu/`, `fuzz/` and `wire_weaver_tool/` (excluded from the root on purpose,
/// AGENTS.md), and every board under `examples_mcu/`, each its own package outside any workspace.
fn workspace_manifests(root: &Path) -> Vec<PathBuf> {
    let mut manifests = vec![
        root.join("Cargo.toml"),
        root.join("mcu/Cargo.toml"),
        root.join("fuzz/Cargo.toml"),
        root.join("wire_weaver_tool/Cargo.toml"),
    ];
    let examples_mcu = root.join("examples_mcu");
    let mut boards: Vec<PathBuf> = std::fs::read_dir(&examples_mcu)
        .unwrap_or_else(|e| panic!("reading {}: {e}", examples_mcu.display()))
        .map(|entry| entry.expect("reading a dir entry of examples_mcu").path())
        .filter(|path| path.is_dir() && path.join("Cargo.toml").is_file())
        .collect();
    boards.sort();
    manifests.extend(boards.into_iter().map(|board| board.join("Cargo.toml")));
    manifests
}

/// Every local (path-sourced, i.e. not registry or git) crate name found in one workspace's
/// `cargo metadata`, and every normal/build/dev dependency edge between two such crates. A
/// dependency the resolver lists more than once (several targets) collapses to one edge: the
/// layer check only cares whether an edge exists, not how many places it comes from.
fn local_crates_and_edges_of(manifest: &Path) -> (BTreeSet<String>, BTreeSet<(String, String)>) {
    // --locked, not --frozen: mcu/, fuzz/ and wire_weaver_tool/ are separate workspaces (AGENTS.md)
    // that CI checks in their own job, so a plain `cargo test` of the root workspace may never have
    // fetched their dependencies; --locked still refuses to re-resolve Cargo.lock, only allowing a
    // fetch of exactly what it already pins.
    let metadata = MetadataCommand::new()
        .manifest_path(manifest)
        .other_options(["--locked".to_string()])
        .exec()
        .unwrap_or_else(|e| panic!("cargo metadata --manifest-path {}: {e}", manifest.display()));
    let local: BTreeSet<String> = metadata
        .packages
        .iter()
        .filter(|package| package.source.is_none())
        .map(|package| package.name.to_string())
        .collect();

    let id_to_name: BTreeMap<PackageId, String> = metadata
        .packages
        .iter()
        .map(|package| (package.id.clone(), package.name.to_string()))
        .collect();
    let resolve = metadata
        .resolve
        .unwrap_or_else(|| panic!("no resolve graph for {}", manifest.display()));
    let mut edges = BTreeSet::new();
    for node in &resolve.nodes {
        let Some(name) = id_to_name.get(&node.id) else {
            continue;
        };
        if !local.contains(name) {
            continue;
        }
        for dep in &node.deps {
            let Some(dep_name) = id_to_name.get(&dep.pkg) else {
                continue;
            };
            // normal, build and dev all count as an edge (R9): only presence matters here.
            if dep_name != name && local.contains(dep_name) {
                edges.insert((name.clone(), dep_name.clone()));
            }
        }
    }
    (local, edges)
}

/// `cargo metadata` of every manifest, one per thread (each is its own process spawn and JSON
/// parse, with nothing shared to synchronize on), merged into one crate set and one edge set.
fn collect_local_crates_and_edges(
    manifests: &[PathBuf],
) -> (BTreeSet<String>, BTreeSet<(String, String)>) {
    std::thread::scope(|scope| {
        let handles: Vec<_> = manifests
            .iter()
            .map(|manifest| scope.spawn(move || local_crates_and_edges_of(manifest)))
            .collect();
        let mut local_crates = BTreeSet::new();
        let mut edges = BTreeSet::new();
        for handle in handles {
            let (local, found_edges) = handle.join().expect("a metadata thread panicked");
            local_crates.extend(local);
            edges.extend(found_edges);
        }
        (local_crates, edges)
    })
}

#[test]
fn no_upward_crate_edges() {
    let root = repo_root();
    let layers_toml = std::fs::read_to_string(root.join("layers.toml"))
        .expect("reading layers.toml at the repo root");
    let layers_file: LayersFile = toml::from_str(&layers_toml).expect("parsing layers.toml");

    let manifests = workspace_manifests(&root);
    let (local_crates, edges) = collect_local_crates_and_edges(&manifests);

    let missing: Vec<&String> = local_crates
        .iter()
        .filter(|name| !layers_file.layers.contains_key(name.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "crate(s) found by `cargo metadata` but missing from layers.toml (every workspace crate \
         needs a layer, 0 to 6): {missing:?}"
    );

    let mut allow_used = vec![false; layers_file.allow.len()];
    let mut violations = Vec::new();
    for (from, to) in &edges {
        let from_layer = layers_file.layers[from];
        let to_layer = layers_file.layers[to];
        if from_layer < to_layer {
            match layers_file.allow.iter().position(|a| &a.from == from && &a.to == to) {
                Some(i) => allow_used[i] = true,
                None => violations.push(format!(
                    "{from} (layer {from_layer}) -> {to} (layer {to_layer}): no layers.toml allow-list entry"
                )),
            }
        }
    }
    assert!(
        violations.is_empty(),
        "upward crate edge(s), a lower layer depending on a higher one:\n{}",
        violations.join("\n")
    );

    let stale: Vec<String> = layers_file
        .allow
        .iter()
        .zip(allow_used.iter())
        .filter(|(_, used)| !**used)
        .map(|(a, _)| {
            format!(
                "{} -> {} ({}): edge no longer exists, drop this layers.toml allow-list line",
                a.from, a.to, a.refactor
            )
        })
        .collect();
    assert!(
        stale.is_empty(),
        "stale layers.toml allow-list entries:\n{}",
        stale.join("\n")
    );
}
