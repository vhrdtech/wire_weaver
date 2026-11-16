//! R9: fails on any crate (in any workspace of this repo) missing from `layers.toml`, and on any
//! normal, build or dev dependency edge from a lower-layer crate to a higher-layer one that
//! `layers.toml`'s `[[allow]]` list doesn't name; an allow-list line whose edge no longer exists
//! fails too. R1: an API crate (`[api] crates`) reaches no runtime crate through its normal
//! dependencies. See `/layers.toml` and `docs/evolution/crate_layers.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use cargo_metadata::{DependencyKind, MetadataCommand, PackageId};
use serde::Deserialize;

#[derive(Deserialize)]
struct LayersFile {
    layers: BTreeMap<String, u8>,
    #[serde(default)]
    allow: Vec<AllowEntry>,
    #[serde(default)]
    api: ApiCrates,
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
const RUNTIME_LAYER: u8 = 5;

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

/// Every local crate name found in one workspace's `cargo metadata` (path-sourced, i.e. not registry or git, and
/// inside this repo: a `[patch]` to a checkout beside it is another repo's crate), and every normal/build/dev
/// dependency edge between two such crates. A
/// dependency the resolver lists more than once (several targets) collapses to one edge: the
/// layer check only cares whether an edge exists, not how many places it comes from.
fn local_crates_and_edges_of(manifest: &Path) -> Graph {
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
        .filter(|package| {
            package.source.is_none() && package.manifest_path.as_std_path().starts_with(repo_root())
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
fn collect_graph(manifests: &[PathBuf]) -> Graph {
    std::thread::scope(|scope| {
        let handles: Vec<_> = manifests
            .iter()
            .map(|manifest| scope.spawn(move || local_crates_and_edges_of(manifest)))
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

/// R1: a crate named in `layers.toml`'s `[api] crates` describes an API and never runs one, so its normal
/// dependencies, direct or transitive, reach no layer-5 crate (`wire_weaver` first of all) beyond what `ww_api`'s
/// own closure holds. Dev dependencies are free: a test may drive the API through the runtime.
#[test]
fn api_crates_are_free_of_the_runtime() {
    let root = repo_root();
    let layers_file = layers_file(&root);
    let graph = collect_graph(&[root.join("Cargo.toml")]);
    let allowed = graph.normal_closure("ww_api");
    assert!(
        allowed.contains("ww_version"),
        "ww_api's normal closure should hold ww_version, found {allowed:?}"
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
                        || layers_file.layers.get(*reached) == Some(&RUNTIME_LAYER))
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
    } = collect_graph(&manifests);

    let missing: Vec<&String> = local_crates
        .iter()
        .filter(|name| !layers_file.layers.contains_key(name.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "crate(s) found by `cargo metadata` but missing from layers.toml (every workspace crate \
         needs a layer, 2 to 6): {missing:?}"
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
