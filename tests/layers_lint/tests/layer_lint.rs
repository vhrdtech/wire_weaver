//! R9: fails on any crate (in any workspace of this repo) missing from the crate map `layers.toml`, and on any
//! normal, build or dev dependency edge to a crate whose layer the source crate's layer does not allow, unless
//! `layers.toml`'s `[[allow]]` list names it; an allow-list line whose edge no longer exists fails too. R1: an API crate (`[api] crates`) reaches no runtime crate through its normal
//! dependencies. See `/layers.toml` and `docs/evolution/crate_layers.md`. Every member of a workspace with
//! `[workspace.lints]` has a `[lints]` table, whatever the toolchain.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use cargo_metadata::{DependencyKind, Metadata, MetadataCommand, PackageId};
use serde::Deserialize;

/// The crate map (spirit_level reads the same file); `[external]` and the keys spirit_level alone reads are skipped.
#[derive(Deserialize)]
struct LayersFile {
    layers: BTreeMap<String, Layer>,
    crates: BTreeMap<String, CrateEntry>,
    #[serde(default)]
    allow: Vec<AllowEntry>,
    #[serde(default)]
    api: ApiCrates,
}

/// `[layers.<name>]`: the layers a crate of this layer may depend on, its own included when listed.
#[derive(Deserialize)]
struct Layer {
    allow: Vec<String>,
}

/// `[crates]`: the layer of one crate.
#[derive(Deserialize)]
struct CrateEntry {
    layer: String,
}

impl LayersFile {
    fn layer_of(&self, name: &str) -> Option<&str> {
        self.crates.get(name).map(|entry| entry.layer.as_str())
    }
}

/// `[api] crates`: the crates that describe an API and never run one (R1), and `[[api.allow]]`, the runtime
/// crates one of them still reaches, each with the refactor that removes it.
#[derive(Deserialize, Default)]
struct ApiCrates {
    crates: Vec<String>,
    #[serde(default)]
    allow: Vec<ApiAllowEntry>,
}

#[derive(Deserialize)]
struct ApiAllowEntry {
    from: String,
    reaches: String,
    refactor: String,
}

/// The layer of the framework runtime and host: what an API crate's normal closure may not reach.
const RUNTIME_LAYER: &str = "runtime";

/// The crates and edges one or more workspaces' `cargo metadata` found: every local crate, every normal, build or
/// dev edge between two of them, and the normal edges alone (what a `[dependencies]` entry pulls into a build).
#[derive(Default)]
struct Graph {
    local_crates: BTreeSet<String>,
    edges: BTreeSet<(String, String)>,
    normal_edges: BTreeSet<(String, String)>,
}

impl Graph {
    fn extend(&mut self, other: Graph) {
        self.local_crates.extend(other.local_crates);
        self.edges.extend(other.edges);
        self.normal_edges.extend(other.normal_edges);
    }

    /// Every crate `start`'s normal dependencies reach, `start` excluded.
    fn normal_closure(&self, start: &str) -> BTreeSet<String> {
        let mut reached = BTreeSet::new();
        let mut pending = vec![start.to_owned()];
        while let Some(from) = pending.pop() {
            for (edge_from, to) in &self.normal_edges {
                if *edge_from == from && *to != start && reached.insert(to.clone()) {
                    pending.push(to.clone());
                }
            }
        }
        reached
    }
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

/// Every workspace in the repo: the root (`tests/*`, `examples/*`,
/// `ww_settings/*`, ... are members of it already), `mcu/`, `fuzz/`, `wire_weaver_tool/` (excluded from the root on
/// purpose, AGENTS.md), and every board under `examples_mcu/`, each its own package outside any workspace.
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

/// `cargo metadata` of one workspace with `options`, run in the workspace's own directory so its
/// `.cargo/config.toml` applies, as in a build there.
fn metadata_of(manifest: &Path, options: &[&str]) -> Result<Metadata, cargo_metadata::Error> {
    let dir = manifest.parent().expect("a manifest is in a directory");
    MetadataCommand::new()
        .manifest_path(manifest)
        .current_dir(dir)
        .other_options(
            options
                .iter()
                .map(|option| (*option).to_owned())
                .collect::<Vec<_>>(),
        )
        .exec()
}

/// Every local crate name of one workspace, read with `--locked`: mcu/, fuzz/ and wire_weaver_tool/ are separate
/// workspaces (AGENTS.md) that CI checks in their own job, so a plain `cargo test` of the root workspace may never
/// have fetched their dependencies; --locked refuses to re-resolve Cargo.lock and fetches only what it pins.
fn local_crates_and_edges_of(root: &Path, manifest: &Path) -> Graph {
    let metadata = metadata_of(manifest, &["--locked"])
        .unwrap_or_else(|e| panic!("cargo metadata --manifest-path {}: {e}", manifest.display()));
    graph_of(root, manifest, metadata)
}

/// Every local crate name (path-sourced, i.e. not registry or git, and inside the repo: a `[patch]` to a
/// sibling checkout is another repository) of one workspace's `metadata`, every normal/build/dev dependency edge
/// between two such crates. A dependency the resolver lists more than
/// once (several targets) collapses to one edge: the layer check only cares whether an edge exists.
fn graph_of(root: &Path, manifest: &Path, metadata: Metadata) -> Graph {
    let local: BTreeSet<String> = metadata
        .packages
        .iter()
        .filter(|package| {
            package.source.is_none() && package.manifest_path.as_std_path().starts_with(root)
        })
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
    let mut graph = Graph {
        local_crates: local,
        ..Graph::default()
    };
    for node in &resolve.nodes {
        let Some(name) = id_to_name.get(&node.id) else {
            continue;
        };
        if !graph.local_crates.contains(name) {
            continue;
        }
        for dep in &node.deps {
            let Some(dep_name) = id_to_name.get(&dep.pkg) else {
                continue;
            };
            if dep_name == name || !graph.local_crates.contains(dep_name) {
                continue;
            }
            // normal, build and dev all count as an edge (R9): only presence matters here.
            graph.edges.insert((name.clone(), dep_name.clone()));
            if dep
                .dep_kinds
                .iter()
                .any(|kind| kind.kind == DependencyKind::Normal)
            {
                graph.normal_edges.insert((name.clone(), dep_name.clone()));
            }
        }
    }
    graph
}

/// `cargo metadata` of every manifest, one per thread (each is its own process spawn and JSON
/// parse, with nothing shared to synchronize on), merged into one graph.
fn collect_graph(root: &Path, manifests: &[PathBuf]) -> Graph {
    std::thread::scope(|scope| {
        let handles: Vec<_> = manifests
            .iter()
            .map(|manifest| scope.spawn(move || local_crates_and_edges_of(root, manifest)))
            .collect();
        let mut graph = Graph::default();
        for handle in handles {
            graph.extend(handle.join().expect("a metadata thread panicked"));
        }
        graph
    })
}

fn layers_file(root: &Path) -> LayersFile {
    let layers_toml = std::fs::read_to_string(root.join("layers.toml"))
        .expect("reading layers.toml at the repo root");
    toml::from_str(&layers_toml).expect("parsing layers.toml")
}

fn read_toml(manifest: &Path) -> toml::Table {
    let text = std::fs::read_to_string(manifest)
        .unwrap_or_else(|e| panic!("reading {}: {e}", manifest.display()));
    toml::from_str(&text).unwrap_or_else(|e| panic!("parsing {}: {e}", manifest.display()))
}

/// Every member of a workspace that has `[workspace.lints]` has a `[lints]` table (`workspace = true`, or its own
/// tables with a comment why). Cargo warns about a missing one only from 1.100 on; this catches it on any toolchain.
#[test]
fn every_crate_inherits_workspace_lints() {
    let root = repo_root();
    let mut missing = Vec::new();
    for manifest in workspace_manifests(&root) {
        let has_workspace_lints = read_toml(&manifest)
            .get("workspace")
            .and_then(|workspace| workspace.get("lints"))
            .is_some();
        if !has_workspace_lints {
            continue;
        }
        let metadata = metadata_of(&manifest, &["--no-deps"]).unwrap_or_else(|e| {
            panic!("cargo metadata --manifest-path {}: {e}", manifest.display())
        });
        for package in &metadata.packages {
            if !read_toml(package.manifest_path.as_std_path()).contains_key("lints") {
                missing.push(package.manifest_path.to_string());
            }
        }
    }
    assert!(
        missing.is_empty(),
        "crate(s) without a [lints] table in a workspace with [workspace.lints]; add `[lints]\\nworkspace = true`: \
         {missing:?}"
    );
}

/// `ww_plugin` is what an executable plugin (`ww-NAME`) takes its device arguments from, so it must not link the
/// hub: no `ww_hub` in its closure over normal, build and dev edges (`cargo tree -p ww_plugin -i ww_hub` finds
/// nothing).
#[test]
fn ww_plugin_is_free_of_the_hub() {
    let root = repo_root();
    let graph = collect_graph(&root, &[root.join("Cargo.toml")]);
    assert!(
        graph.local_crates.contains("ww_plugin"),
        "ww_plugin is a crate of the root workspace"
    );
    let mut reached = BTreeSet::new();
    let mut pending = vec!["ww_plugin".to_owned()];
    while let Some(from) = pending.pop() {
        for (edge_from, to) in &graph.edges {
            if *edge_from == from && reached.insert(to.clone()) {
                pending.push(to.clone());
            }
        }
    }
    assert!(
        !reached.contains("ww_hub"),
        "ww_plugin reaches ww_hub through its dependencies: {reached:?}"
    );
}

/// `ww_json` is a codec that settings files and MCP servers take without a transport, so its normal dependencies
/// reach neither the client runtime nor codegen nor the snapshot fetch (`ureq`, `rustls`, `ring`): its quantities come
/// from `ww_quantity`.
#[test]
fn ww_json_is_free_of_the_client() {
    let root = repo_root();
    let graph = collect_graph(&root, &[root.join("Cargo.toml")]);
    assert!(
        graph.local_crates.contains("ww_json"),
        "ww_json is a crate of the root workspace"
    );
    let reached = graph.normal_closure("ww_json");
    for heavy in ["wire_weaver_client", "wire_weaver_core", "ww_snapshots"] {
        assert!(
            !reached.contains(heavy),
            "ww_json reaches {heavy} through its normal dependencies: {reached:?}"
        );
    }
}

/// R1: a crate named in `layers.toml`'s `[api] crates` describes an API and never runs one, so its normal
/// dependencies, direct or transitive, reach no layer-5 crate (`wire_weaver` first of all) beyond what `ww_api`'s
/// own closure holds. Dev dependencies are free: a test may drive the API through the runtime.
#[test]
fn api_crates_are_free_of_the_runtime() {
    let root = repo_root();
    let layers_file = layers_file(&root);
    let graph = collect_graph(&root, &[root.join("Cargo.toml")]);
    let allowed = graph.normal_closure("ww_api");
    assert!(
        allowed.contains("wire_weaver_derive"),
        "ww_api's normal closure should hold wire_weaver_derive, found {allowed:?}"
    );

    let mut allow_used = vec![false; layers_file.api.allow.len()];
    let mut violations = Vec::new();
    for api_crate in &layers_file.api.crates {
        assert!(
            graph.local_crates.contains(api_crate),
            "{api_crate} is in layers.toml [api] crates but not a crate of the root workspace"
        );
        // another API crate is fine wherever the layer table puts it (`ww_timebase` is framework by content)
        let runtime: Vec<String> = graph
            .normal_closure(api_crate)
            .difference(&allowed)
            .filter(|reached| {
                !layers_file.api.crates.contains(reached)
                    && (*reached == "wire_weaver"
                        || layers_file.layer_of(reached) == Some(RUNTIME_LAYER))
            })
            .filter(|reached| {
                match layers_file
                    .api
                    .allow
                    .iter()
                    .position(|a| &a.from == api_crate && a.reaches == **reached)
                {
                    Some(i) => {
                        allow_used[i] = true;
                        false
                    }
                    None => true,
                }
            })
            .cloned()
            .collect();
        if !runtime.is_empty() {
            violations.push(format!(
                "{api_crate} reaches the runtime through its normal dependencies: {runtime:?}"
            ));
        }
    }
    assert!(
        violations.is_empty(),
        "API crate(s) with the runtime in their normal dependency closure (take ww_api, not wire_weaver):\n{}",
        violations.join("\n")
    );

    let stale: Vec<String> = layers_file
        .api
        .allow
        .iter()
        .zip(allow_used.iter())
        .filter(|(_, used)| !**used)
        .map(|(a, _)| {
            format!(
                "{} -> {} ({}): no longer reached, drop this layers.toml [[api.allow]] entry",
                a.from, a.reaches, a.refactor
            )
        })
        .collect();
    assert!(
        stale.is_empty(),
        "stale layers.toml [[api.allow]] entries:\n{}",
        stale.join("\n")
    );
}

#[test]
fn no_upward_crate_edges() {
    let root = repo_root();
    let layers_file = layers_file(&root);

    let manifests = workspace_manifests(&root);
    let Graph {
        local_crates,
        edges,
        ..
    } = collect_graph(&root, &manifests);

    let missing: Vec<&String> = local_crates
        .iter()
        .filter(|name| layers_file.layer_of(name).is_none())
        .collect();
    assert!(
        missing.is_empty(),
        "crate(s) found by `cargo metadata` but missing from layers.toml (every workspace crate \
         needs an entry under [crates] with its layer): {missing:?}"
    );
    let unknown: Vec<(&String, &String)> = layers_file
        .crates
        .iter()
        .filter(|(_, entry)| !layers_file.layers.contains_key(&entry.layer))
        .map(|(name, entry)| (name, &entry.layer))
        .collect();
    assert!(
        unknown.is_empty(),
        "crate(s) in layers.toml with a layer that [layers] does not declare: {unknown:?}"
    );

    let mut allow_used = vec![false; layers_file.allow.len()];
    let mut violations = Vec::new();
    for (from, to) in &edges {
        let from_layer = layers_file.crates[from].layer.as_str();
        let to_layer = layers_file.crates[to].layer.as_str();
        if !layers_file.layers[from_layer]
            .allow
            .iter()
            .any(|allowed| allowed == to_layer)
        {
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
        "crate edge(s) to a layer the source crate's layer does not allow:\n{}",
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
