//! Per-resource compatibility between the client's API and the connected device's API.
//!
//! If both sides report the same API hash, the APIs are identical and nothing is checked. Otherwise, both
//! introspection bundles are walked, and every method, property, stream and trait the client knows about is compared
//! with the resource at the same path on the device. Calling a resource that is missing or incompatible then fails
//! locally with a clear error, instead of sending bytes the device would misinterpret.
//!
//! Resources and types are compared by [wire_weaver_evolution::compat], structurally, following [evolution rules](https://github.com/vhrdtech/wire_weaver/blob/master/docs/evolution/rules.md):
//! names don't matter, positions do (but a variant found under another discriminant on the other side moved).
//! Direction matters as well: an `Unsized` struct written by one side can have more trailing fields than the reader
//! knows about (they are skipped), but a reader's extra fields must have `#[default]`, and a variant unknown to the
//! reader needs the reader's `#[future_version]` catch-all.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use wire_weaver::shrink_wrap::UNib32;
use ww_self::visit::Visit;
use ww_self::{ApiBundleOwned, ApiItemKindOwned, ApiItemOwned, ApiLevelOwned};
use ww_version::WireVersion;

use wire_weaver_evolution::compat::{Bounds, compare_items};

/// How the device's API relates to the client's, decided once after connecting.
#[derive(Clone)]
pub(crate) enum ApiMatch {
    /// Client has no API of its own (DynClient), nothing to check against.
    NoClientApi,
    /// Device reported the same API hash as the client was generated with.
    Identical,
    /// API hash is different, every resource was compared with the device's introspection data.
    Checked(Arc<ApiCompat>),
    /// API hash is different, but device's API is unknown (introspection disabled and not in the cache).
    /// Only `#[since]` annotations of the client's resources can be checked against the device's wire version.
    DeviceApiUnknown(Arc<ApiCompat>),
}

/// Outcome of comparing one client resource with the device's resource at the same path.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Verdict {
    Compatible,
    /// Device has no resource at this path.
    Missing,
    /// Device's API has the resource, but it is an `#[optional]` one the device leaves out (API-16).
    Omitted,
    /// Device has a resource at this path, but it can't be used with the client's definition.
    Incompatible(String),
}

/// Client resource tree with a verdict for each resource.
pub(crate) struct ApiCompat {
    root: HashMap<u32, Node>,
}

struct Node {
    ident: String,
    is_array: bool,
    since: Option<Since>,
    verdict: Verdict,
    children: HashMap<u32, Node>,
}

/// Resource found at a request path, see [ApiCompat::resolve].
pub(crate) struct Resolved<'a> {
    /// Human-readable resource path, e.g. `gpio[].set_high`.
    pub(crate) name: String,
    /// First not compatible verdict along the path (a resource inside an incompatible trait is incompatible too),
    /// or the resource's own verdict.
    pub(crate) verdict: &'a Verdict,
    /// Every `#[since]` along the path, outermost first, each with the crate that declares it (EVO-26).
    pub(crate) since: Vec<Since>,
}

/// A `#[since]` marker of a resource and what it is compared with (EVO-26): a marker names a revision of the wire
/// version of the crate that declares the resource (a `ww_stdlib` trait's markers name that crate's revisions, not
/// the API root's).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Since {
    pub(crate) since: WireVersion,
    /// The crate that declares the resource, e.g. `ww_board_info`.
    pub(crate) crate_id: String,
    /// The crate is the client's API root crate: without the device's API, its markers are compared with the
    /// device's root wire version (link setup's, EVO-25).
    pub(crate) root: bool,
    /// The device's wire version of the crate, from its introspection data; `None` while that is unknown or the
    /// device's crate declares none.
    pub(crate) device_wire: Option<WireVersion>,
}

impl ApiCompat {
    /// Compare every resource of the client's API with the device's resource at the same path.
    pub(crate) fn new(client: &ApiBundleOwned, device: &ApiBundleOwned) -> Self {
        let device_items: HashMap<Vec<u32>, (&ApiItemOwned, u32)> = collect(device)
            .into_iter()
            .map(|(path, item, crate_idx)| (path, (item, crate_idx)))
            .collect();
        let omitted = omitted(device);
        // the device's wire version of the crate declaring a resource: of the crate the device's resource at the
        // same path comes from, or for one the device doesn't have, of the device's crate by that name
        let device_wire = |path: &[u32], crate_id: &str| match device_items.get(path) {
            Some((_, crate_idx)) => device.crate_wire(*crate_idx),
            None => device
                .ext_crates
                .iter()
                .position(|c| c.crate_id == crate_id)
                .and_then(|idx| device.crate_wire(idx as u32)),
        };
        Self::build(client, device_wire, |path, item| {
            match device_items.get(path) {
                Some(_) if omitted.contains(path) => Verdict::Omitted,
                Some((device_item, _)) => {
                    // a different max length is not checked here: a longer value is rejected when it is sent
                    match compare_items(
                        (client, item, "client"),
                        (device, device_item, "device"),
                        &mut Bounds::default(),
                    ) {
                        Ok(()) => Verdict::Compatible,
                        Err(reason) => Verdict::Incompatible(reason),
                    }
                }
                None => Verdict::Missing,
            }
        })
    }

    /// Client resource tree only, when the device's API is unknown, to look up `#[since]` of the client's resources.
    pub(crate) fn client_only(client: &ApiBundleOwned) -> Self {
        Self::build(client, |_, _| None, |_, _| Verdict::Compatible)
    }

    fn build(
        client: &ApiBundleOwned,
        device_wire: impl Fn(&[u32], &str) -> Option<WireVersion>,
        mut verdict: impl FnMut(&[u32], &ApiItemOwned) -> Verdict,
    ) -> Self {
        let mut root: HashMap<u32, Node> = HashMap::new();
        let root_crate = client
            .crate_version(client.root.crate_idx.0)
            .map(|v| v.crate_id.as_str())
            .unwrap_or_default();
        // parents are collected before their children
        for (path, item, crate_idx) in collect(client) {
            let since = item.since.map(|since| {
                let crate_id = client
                    .crate_version(crate_idx)
                    .map(|v| v.crate_id.clone())
                    .unwrap_or_default();
                Since {
                    since,
                    root: crate_id == root_crate,
                    device_wire: device_wire(&path, &crate_id),
                    crate_id,
                }
            });
            let node = Node {
                ident: item.ident.clone(),
                is_array: item.is_array(),
                since,
                verdict: verdict(&path, item),
                children: HashMap::new(),
            };
            let (last, parents) = path.split_last().expect("item path is never empty");
            let mut level = &mut root;
            for id in parents {
                level = &mut level
                    .get_mut(id)
                    .expect("parent is collected first")
                    .children;
            }
            level.insert(*last, node);
        }
        ApiCompat { root }
    }

    /// Find a resource by a request path (item ids interleaved with array indices, as sent to the device).
    /// Returns None for paths not in the client's API.
    pub(crate) fn resolve(&self, path: &[UNib32]) -> Option<Resolved<'_>> {
        let mut level = &self.root;
        let mut name = String::new();
        let mut since = vec![];
        let mut path = path.iter();
        let mut resolved = None;
        while let Some(id) = path.next() {
            let node = level.get(&id.0)?;
            if !name.is_empty() {
                name.push('.');
            }
            name.push_str(&node.ident);
            if node.is_array {
                name.push_str("[]");
                // array index, absent when reading valid indices of the array itself
                path.next();
            }
            since.extend(node.since.clone());
            resolved = match resolved {
                Some(verdict) if verdict != &Verdict::Compatible => Some(verdict),
                _ => Some(&node.verdict),
            };
            level = &node.children;
        }
        Some(Resolved {
            name,
            verdict: resolved?,
            since,
        })
    }

    /// Resources that are missing or incompatible, not descending into ones already reported. Optional resources the
    /// device leaves out are not listed, leaving them out is the device's choice.
    pub(crate) fn mismatches(&self) -> Vec<(String, &Verdict)> {
        fn walk<'a>(
            level: &'a HashMap<u32, Node>,
            prefix: &str,
            out: &mut Vec<(String, &'a Verdict)>,
        ) {
            let mut ids: Vec<_> = level.keys().collect();
            ids.sort();
            for id in ids {
                let node = &level[id];
                let array = if node.is_array { "[]" } else { "" };
                let name = format!("{prefix}{}{array}", node.ident);
                match node.verdict {
                    Verdict::Compatible => walk(&node.children, &format!("{name}."), out),
                    Verdict::Omitted => {}
                    _ => out.push((name, &node.verdict)),
                }
            }
        }
        let mut out = vec![];
        walk(&self.root, "", &mut out);
        out
    }
}

/// Walks an API bundle from the root, descending into traits, and collects every item with its id path and the
/// crate of the trait (or root) that declares it.
struct ItemCollector<'ast> {
    bundle: &'ast ApiBundleOwned,
    path: Vec<u32>,
    trait_stack: Vec<u32>,
    crate_stack: Vec<u32>,
    items: Vec<(Vec<u32>, &'ast ApiItemOwned, u32)>,
}

impl<'ast> Visit<'ast> for ItemCollector<'ast> {
    fn visit_api_level(&mut self, node: &'ast ApiLevelOwned) {
        self.crate_stack.push(node.crate_idx.0);
        ww_self::visit::visit_api_level(self, node);
        self.crate_stack.pop();
    }

    fn visit_api_item(&mut self, node: &'ast ApiItemOwned) {
        self.path.push(node.id.0);
        let crate_idx = self.crate_stack.last().copied().unwrap_or_default();
        self.items.push((self.path.clone(), node, crate_idx));
        // traits are referred to by index, follow them to get the resource tree as the device sees it
        if let ApiItemKindOwned::Trait { trait_idx, .. } = &node.kind
            && !self.trait_stack.contains(&trait_idx.0)
            && let Some(location) = self.bundle.traits.get(trait_idx.0 as usize)
        {
            self.trait_stack.push(trait_idx.0);
            self.visit_api_level_location(location);
            self.trait_stack.pop();
        }
        self.path.pop();
    }
}

/// Paths of the optional items a device leaves out (API-16): `root_omitted` and `omitted` of its trait items.
fn omitted(device: &ApiBundleOwned) -> HashSet<Vec<u32>> {
    let mut paths: HashSet<Vec<u32>> = device
        .root_omitted
        .iter()
        .flatten()
        .map(|id| vec![id.0])
        .collect();
    for (path, item, _) in collect(device) {
        if let ApiItemKindOwned::Trait {
            omitted: Some(omitted),
            ..
        } = &item.kind
        {
            paths.extend(omitted.iter().map(|id| {
                let mut path = path.clone();
                path.push(id.0);
                path
            }));
        }
    }
    paths
}

fn collect(bundle: &ApiBundleOwned) -> Vec<(Vec<u32>, &ApiItemOwned, u32)> {
    let mut collector = ItemCollector {
        bundle,
        path: vec![],
        trait_stack: vec![],
        crate_stack: vec![],
        items: vec![],
    };
    collector.visit_api_level(&bundle.root);
    collector.items
}

/// Whether a device whose API is at wire version `device` has an item marked `#[since = "since"]` (EVO-24): when
/// `device >= since`, by line, then revision. On one line this is the revision that added the item; a new line
/// (`w2.5` -> `w3.0`) counts every older marker as present, so markers never need rewriting. A device on an older
/// line than the marker doesn't have the item. Whether the item survived the new line is the device's introspection
/// data's to say (`Verdict::Missing`), not the marker's.
pub(crate) fn has_since(device: WireVersion, since: WireVersion) -> bool {
    device >= since
}

/// One side of a comparison: bundle, resource in it, and how to call that side in messages.
#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::path::PathBuf;
    use wire_weaver::shrink_wrap::ElementSize;
    use wire_weaver_evolution::compat::TypeCheck;
    use ww_self::signature;
    use ww_self::{
        ApiLevelLocationOwned, FieldsOwned, ItemStructOwned, TypeLocationOwned, TypeOwned,
    };
    use ww_version::FullVersionOwned;

    /// [bundle] of an API crate with `dependencies`, lines of its `[dependencies]` section; written under the
    /// workspace, so the walker finds its Cargo.lock and `[patch]`es for dependencies of those dependencies.
    pub(crate) fn bundle_with(
        name: &str,
        version: &str,
        lib_rs: &str,
        dependencies: &str,
    ) -> ApiBundleOwned {
        let dir: PathBuf = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/ww_client_compat_tests")
            .join(format!(
                "{name}-{version}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            format!(
                "[package]\nname = \"test_api\"\nversion = \"{version}\"\nedition = \"2024\"\npublish = false\n\n[dependencies]\n{dependencies}\n"
            ),
        )
        .unwrap();
        let lib_rs = format!("use wire_weaver::prelude::*;\n\n{lib_rs}");
        std::fs::write(dir.join("src/lib.rs"), lib_rs).unwrap();
        let bundle = wire_weaver_core::load(&dir, None, true).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        bundle
    }

    /// Build an API bundle from source, as codegen would, by writing a throwaway API crate.
    pub(crate) fn bundle(name: &str, version: &str, lib_rs: &str) -> ApiBundleOwned {
        let dir: PathBuf = std::env::temp_dir()
            .join("ww_client_compat_tests")
            .join(format!("{name}-{version}-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            format!(
                "[package]\nname = \"test_api\"\nversion = \"{version}\"\nedition = \"2024\"\npublish = false\n\n[dependencies]\n"
            ),
        )
        .unwrap();
        let lib_rs = format!("use wire_weaver::prelude::*;\n\n{lib_rs}");
        std::fs::write(dir.join("src/lib.rs"), lib_rs).unwrap();
        let bundle = wire_weaver_core::load(&dir, None, true).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        bundle
    }

    fn compat(client: &str, device: &str) -> ApiCompat {
        let client = bundle("client", "0.1.1", client);
        let device = bundle("device", "0.1.0", device);
        ApiCompat::new(&client, &device)
    }

    fn path(ids: &[u32]) -> Vec<UNib32> {
        ids.iter().map(|id| UNib32(*id)).collect()
    }

    fn verdict(compat: &ApiCompat, ids: &[u32]) -> Verdict {
        compat.resolve(&path(ids)).unwrap().verdict.clone()
    }

    fn assert_incompatible(compat: &ApiCompat, ids: &[u32], reason_contains: &str) {
        match verdict(compat, ids) {
            Verdict::Incompatible(reason) => assert!(
                reason.contains(reason_contains),
                "'{reason}' does not contain '{reason_contains}'"
            ),
            v => panic!("expected incompatible, got {v:?}"),
        }
    }

    /// Definitions a device leaves out, from another revision of a crate than the host's snapshot: the
    /// host finds the device's definition by signature in the definition cache and compares it by the evolution
    /// rules; without it, the error says what happened.
    mod other_revision {
        use super::*;
        use crate::client::introspect::inline_known_in;
        use ww_self::definition_cache::{self, Kind};
        use ww_self::signature::{canonical_type_bytes, signature_of};

        /// `read`, `write` and `supply` (returns `ww_si::Volt`, `Quantity<units::Volt>`). The fixture is the marker
        /// `units::Volt`, a plain definition: one from another revision is compared by the evolution rules. A generic
        /// definition (`Quantity`) is compared by its signature only (CLT-32).
        const SUPPLY: u32 = 2;

        fn full() -> ApiBundleOwned {
            let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/cfg_variants_api");
            wire_weaver_core::load(&dir, None, false).unwrap()
        }

        fn marker_idx(bundle: &ApiBundleOwned) -> usize {
            bundle
                .types
                .iter()
                .position(|l| {
                    matches!(l, TypeLocationOwned::InLine { ty: TypeOwned::Struct(s), .. } if s.qualified_name() == "units::Volt")
                })
                .expect("units::Volt in-line")
        }

        fn cache_dir(name: &str) -> PathBuf {
            let dir = std::env::temp_dir()
                .join("ww_client_compat_tests")
                .join(format!("definitions-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            dir
        }

        /// What the host has (`units::Volt` from its snapshot), and what a device built from another revision of
        /// `ww_si` sends: `units::Volt` left out, under the signature of `older` `units::Volt`, which is stored in
        /// `cache` when given.
        fn host_and_device(
            older: impl FnOnce(&mut ItemStructOwned),
            cache: Option<&std::path::Path>,
        ) -> (ApiBundleOwned, ApiBundleOwned, Vec<u8>, Vec<u8>) {
            let full = full();
            let sent = wire_weaver_core::transform::skip_known(&full).unwrap();
            let mut older_full = full.clone();
            let idx = marker_idx(&older_full);
            let TypeLocationOwned::InLine {
                ty: TypeOwned::Struct(marker),
                ..
            } = &mut older_full.types[idx]
            else {
                unreachable!()
            };
            older(marker);
            let marker_crate_idx = marker.crate_idx.0;
            let resolve = |v: &_| crate::snapshots::get(v);
            let bytes = canonical_type_bytes(&older_full, idx as u32, &resolve).unwrap();
            let older_signature = signature_of(&bytes);
            let version = older_full.crate_version(marker_crate_idx).unwrap().clone();
            if let Some(dir) = cache {
                definition_cache::store(dir, Kind::Type, &version, "units::Volt", &bytes).unwrap();
            }

            let mut device = sent.clone();
            let mut host_signature = vec![];
            for location in &mut device.types {
                if let TypeLocationOwned::SkippedFullVersion {
                    type_name,
                    signature,
                    ..
                } = location
                    && type_name == "units::Volt"
                {
                    host_signature = signature.clone();
                    *signature = older_signature.clone();
                }
            }
            assert!(!host_signature.is_empty(), "units::Volt is left out");
            assert_ne!(host_signature, older_signature);
            let mut host = sent;
            inline_known_in(&mut host, None);
            (host, device, host_signature, older_signature)
        }

        #[test]
        fn compatible_definition_from_the_cache() {
            let dir = cache_dir("compatible");
            let (host, mut device, host_signature, older_signature) = host_and_device(
                |marker| marker.docs = vec!["Docs of an older revision".into()],
                Some(&dir),
            );
            let inlined = inline_known_in(&mut device, Some(&dir));
            assert!(inlined.not_inlined.is_empty(), "{:?}", inlined.not_inlined);
            assert_eq!(inlined.from_cache.len(), 1);
            let from_cache = &inlined.from_cache[0];
            assert_eq!(from_cache.name, "units::Volt");
            assert_eq!(from_cache.signature, older_signature);
            assert_eq!(from_cache.snapshot_signature, Some(host_signature));
            let compat = ApiCompat::new(&host, &device);
            assert!(compat.mismatches().is_empty());
            assert_eq!(verdict(&compat, &[SUPPLY]), Verdict::Compatible);
            std::fs::remove_dir_all(&dir).unwrap();
        }

        #[test]
        fn incompatible_definition_from_the_cache() {
            let dir = cache_dir("incompatible");
            let (host, mut device, _, _) = host_and_device(
                // a final structure can't be read as an evolvable one
                |marker| marker.size = ElementSize::UnsizedFinalStructure,
                Some(&dir),
            );
            inline_known_in(&mut device, Some(&dir));
            let compat = ApiCompat::new(&host, &device);
            assert_incompatible(&compat, &[SUPPLY], "struct Volt");
            std::fs::remove_dir_all(&dir).unwrap();
        }

        #[test]
        fn definition_not_in_the_cache() {
            let dir = cache_dir("missing");
            let (host, mut device, host_signature, older_signature) = host_and_device(
                |marker| marker.docs = vec!["Docs of an older revision".into()],
                None,
            );
            let inlined = inline_known_in(&mut device, Some(&dir));
            assert_eq!(inlined.not_inlined.len(), 1);
            let compat = ApiCompat::new(&host, &device);
            let Verdict::Incompatible(reason) = verdict(&compat, &[SUPPLY]) else {
                panic!("expected incompatible");
            };
            for part in [
                "exact_units::units::Volt 0.1.0",
                &hex::encode(host_signature),
                &hex::encode(older_signature),
                "different revisions of exact_units",
                "Rebuild (reflash) both from one revision",
            ] {
                assert!(
                    reason.contains(part),
                    "'{reason}' does not contain '{part}'"
                );
            }
        }
    }

    const BLINKY: &str = r#"
        #[ww_api_root(wire = "w1.0")]
        pub trait Blinky {
            fn led_on();
            fn led_off();
        }
    "#;

    #[test]
    fn self_referential_types_written_out_differently() {
        // A { x: Box<A> }, written out a level further at every other step on each side, so that out-of-line
        // references never meet: the host's A is `A { x: Box<A { x: Box<A> }> }`, the device's argument is an
        // in-line `A { x: Box<A> }` around that
        let mut host = bundle(
            "recursive",
            "0.1.0",
            "#[ww_api_root] pub trait Api { fn f(a: A<'i>); } pub struct A<'i> { x: RefBox<'i, A<'i>> }",
        );
        let TypeLocationOwned::InLine { ty: a, .. } = host.types[0].clone() else {
            panic!("in-line");
        };
        let TypeOwned::Struct(mut outer) = a.clone() else {
            panic!("struct");
        };
        let FieldsOwned::Named(fields) = &mut outer.fields else {
            panic!("named");
        };
        fields[0].ty = TypeOwned::Box(Box::new(a.clone()));
        if let TypeLocationOwned::InLine { ty, .. } = &mut host.types[0] {
            *ty = TypeOwned::Struct(outer);
        }
        let mut device = host.clone();
        let ApiItemKindOwned::Method { args, .. } = &mut device.root.items[0].kind else {
            panic!("method");
        };
        args[0].ty = a;
        let ApiItemKindOwned::Method {
            args: host_args, ..
        } = &host.root.items[0].kind
        else {
            panic!("method");
        };
        let ApiItemKindOwned::Method {
            args: device_args, ..
        } = &device.root.items[0].kind
        else {
            panic!("method");
        };
        let mut check = TypeCheck::new(&host, &device, "client", "device");
        assert_eq!(check.check(&host_args[0].ty, &device_args[0].ty), Ok(()));
        let mut check = TypeCheck::new(&device, &host, "device", "client");
        assert_eq!(check.check(&device_args[0].ty, &host_args[0].ty), Ok(()));
    }

    /// Generated bundles (self-referential ones too, see `ww_self::fuzz_bundle`) compared in pairs, as a client
    /// compares its API with a device's: a verdict for each, on the stack of a spawned thread.
    #[test]
    fn generated_bundles_in_pairs() {
        std::thread::Builder::new()
            .stack_size(2 << 20)
            .spawn(|| {
                let mut state = 0x9e37_79b9_7f4a_7c15u64;
                let mut next = || {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    state
                };
                let mut bundles = vec![];
                while bundles.len() < 60 {
                    let data: Vec<u8> = (0..next() % 160).map(|_| next() as u8).collect();
                    let mut bundle = ww_self::fuzz_bundle::gen_bundle(&data, next() % 4 != 0);
                    if ww_self::generics::prepare(&mut bundle).is_ok() {
                        bundles.push(bundle);
                    }
                }
                for a in &bundles {
                    for b in &bundles {
                        let compat = ApiCompat::new(a, b);
                        for item in &a.root.items {
                            let _ = compat.resolve(&[item.id]);
                        }
                    }
                }
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn identical_apis() {
        let compat = compat(BLINKY, BLINKY);
        assert!(compat.mismatches().is_empty());
        assert_eq!(verdict(&compat, &[0]), Verdict::Compatible);
        assert!(
            compat.resolve(&path(&[5])).is_none(),
            "unknown path is not checked"
        );
    }

    #[test]
    fn newer_client_older_device() {
        let client = r#"
            #[ww_api_root(wire = "w1.1")]
            pub trait Blinky {
                fn led_on();
                fn led_off();
                #[since = "w1.1"]
                fn led_toggle();
            }
        "#;
        let compat = compat(client, BLINKY);
        let toggle = compat.resolve(&path(&[2])).unwrap();
        assert_eq!(toggle.name, "led_toggle");
        assert_eq!(toggle.verdict, &Verdict::Missing);
        let since: Vec<_> = toggle.since.iter().map(|m| (m.since, m.root)).collect();
        assert_eq!(since, [(WireVersion::new(1, 1), true)]);
        assert_eq!(toggle.since[0].device_wire, Some(WireVersion::new(1, 0)));
        assert_eq!(compat.mismatches().len(), 1);

        // resources the client doesn't know about are irrelevant
        let compat = ApiCompat::new(
            &bundle("client", "0.1.0", BLINKY),
            &bundle("device", "0.1.1", client),
        );
        assert!(compat.mismatches().is_empty());
    }

    #[test]
    fn method_signatures() {
        let client = r#"
            #[ww_api_root(wire = "w1.0")]
            pub trait Api {
                fn set(x: u8);
                fn get() -> u8;
                fn two(a: u8, b: u8);
                fn was_method();
                fn one(a: u8);
                fn one_more(a: u8);
            }
        "#;
        let device = r#"
            #[ww_api_root(wire = "w1.0")]
            pub trait Api {
                fn set(x: u16);
                fn get() -> u8;
                fn two(a: u8);
                property!(rw was_method: u8);
                fn one(a: u8, b: u8);
                fn one_more(a: u8, b: Option<u8>);
            }
        "#;
        let compat = compat(client, device);
        assert_incompatible(&compat, &[0], "argument `x`");
        assert_eq!(verdict(&compat, &[1]), Verdict::Compatible);
        // the arguments are an Unsized struct (EVO-22): an older device skips a newer client's trailing ones
        assert_eq!(verdict(&compat, &[2]), Verdict::Compatible);
        assert_incompatible(
            &compat,
            &[3],
            "a method on the client, a property on the device",
        );
        // a newer device reads the default of an argument an older client leaves out, if it has one
        assert_incompatible(
            &compat,
            &[4],
            "argument `b` is not sent by the client and has no #[default]",
        );
        assert_eq!(verdict(&compat, &[5]), Verdict::Compatible);
    }

    #[test]
    fn evolved_struct() {
        let v1 = r#"
            #[ww_api_root(wire = "w1.0")]
            pub trait Api {
                fn set(c: Coord);
                fn get() -> Coord;
            }

            #[derive_shrink_wrap]
            struct Coord {
                x: u8,
                y: u8,
            }
        "#;
        let v2_default = r#"
            #[ww_api_root(wire = "w1.0")]
            pub trait Api {
                fn set(c: Coord);
                fn get() -> Coord;
            }

            #[derive_shrink_wrap]
            struct Coord {
                x: u8,
                y: u8,
                #[default = None]
                z: Option<u8>,
            }
        "#;
        // device reads a newer struct with a default, client reads a newer struct and skips the extra field
        let compat = compat(v1, v2_default);
        assert!(compat.mismatches().is_empty(), "{:?}", compat.mismatches());
        // same the other way around
        let compat = self::compat(v2_default, v1);
        assert!(compat.mismatches().is_empty(), "{:?}", compat.mismatches());

        let v2_no_default = v2_default.replace("#[default = None]", "");
        // device can't read what an older client sends
        let compat = self::compat(v1, &v2_no_default);
        assert_incompatible(
            &compat,
            &[0],
            "field `z` is not sent by the client and has no #[default]",
        );
        assert_eq!(verdict(&compat, &[1]), Verdict::Compatible);
        // client can't read what an older device returns
        let compat = self::compat(&v2_no_default, v1);
        assert_eq!(verdict(&compat, &[0]), Verdict::Compatible);
        assert_incompatible(
            &compat,
            &[1],
            "return type: struct Coord: field `z` is not sent by the device",
        );

        let v1_final = v1.replace(
            "#[derive_shrink_wrap]",
            "#[derive_shrink_wrap(final_structure)]",
        );
        let v2_final = v2_default.replace(
            "#[derive_shrink_wrap]",
            "#[derive_shrink_wrap(final_structure)]",
        );
        let compat = self::compat(&v1_final, &v2_final);
        assert_incompatible(&compat, &[1], "only Unsized types can gain new fields");
    }

    #[test]
    fn enum_variants() {
        let v1 = r#"
            #[ww_api_root(wire = "w1.0")]
            pub trait Api {
                fn get() -> Mode;
                fn set(m: Mode);
            }

            #[derive_shrink_wrap(ww_repr = u4)]
            enum Mode {
                A,
                B(u8),
            }
        "#;
        let v2 = r#"
            #[ww_api_root(wire = "w1.0")]
            pub trait Api {
                fn get() -> Mode;
                fn set(m: Mode);
            }

            #[derive_shrink_wrap(ww_repr = u4)]
            enum Mode {
                A,
                B(u8),
                C,
            }
        "#;
        // newer device can return a variant the client doesn't know, the client reads it as Mode::Unknown(2)
        // (EVO-10), and the device can read all the client sends
        let compat = self::compat(v1, v2);
        assert_eq!(verdict(&compat, &[0]), Verdict::Compatible);
        assert_eq!(verdict(&compat, &[1]), Verdict::Compatible);

        // without the catch-all the client can't read it
        let off = |api: &str| api.replace("ww_repr = u4)", "ww_repr = u4, no_future_version)");
        let compat = self::compat(&off(v1), &off(v2));
        assert_incompatible(
            &compat,
            &[0],
            "variant C is unknown to the client, which has no catch-all for it",
        );
        assert_eq!(verdict(&compat, &[1]), Verdict::Compatible);

        // a variant inserted before B: the client would read the device's B as C
        let inserted = v2.replace(
            "A,\n                B(u8),\n                C,",
            "A,\n                C,\n                B(u8),",
        );
        let compat = self::compat(v1, &inserted);
        assert_incompatible(&compat, &[0], "variant B moved");
        assert_incompatible(&compat, &[1], "variant B moved");
    }

    #[test]
    fn properties_and_streams() {
        let client = r#"
            #[ww_api_root(wire = "w1.0")]
            pub trait Api {
                property!(rw a: u8);
                property!(ro b: u8);
                stream!(s: u8);
                sink!(k: u8);
            }
        "#;
        let device = r#"
            #[ww_api_root(wire = "w1.0")]
            pub trait Api {
                property!(ro a: u8);
                property!(rw b: u8);
                sink!(s: u8);
                sink!(k: u32);
            }
        "#;
        let compat = compat(client, device);
        assert_incompatible(&compat, &[0], "ReadWrite");
        assert_eq!(verdict(&compat, &[1]), Verdict::Compatible);
        assert_incompatible(&compat, &[2], "stream on one side, sink on the other");
        assert_incompatible(&compat, &[3], "u8 on the client");
    }

    #[test]
    fn traits_and_arrays() {
        let client = r#"
            #[ww_api_root(wire = "w1.0")]
            pub trait Api {
                ww_impl!(periph[]: Peripheral);
                ww_impl!(sub: Subgroup);
            }

            #[ww_trait]
            trait Peripheral {
                ww_impl!(channel[]: Channel);
            }

            #[ww_trait]
            trait Channel {
                property!(rw gain: f32);
                fn run();
            }

            #[ww_trait]
            trait Subgroup {
                fn m1();
            }
        "#;
        let device = client
            .replace("fn run();", "fn run(fast: bool);")
            .replace("trait Subgroup", "trait Other")
            .replace("sub: Subgroup", "sub: Other");
        let compat = compat(client, &device);

        // periph[3].channel[1].gain, periph[3].channel[1].run
        assert_eq!(verdict(&compat, &[0, 3, 0, 1, 0]), Verdict::Compatible);
        let run = compat.resolve(&path(&[0, 3, 0, 1, 1])).unwrap();
        assert_eq!(run.name, "periph[].channel[].run");
        assert!(matches!(run.verdict, Verdict::Incompatible(_)));
        // reading valid indices of the array itself (no index at the end of the path)
        assert_eq!(verdict(&compat, &[0, 3, 0]), Verdict::Compatible);

        // everything inside a different trait is incompatible as well
        assert_incompatible(&compat, &[1], "trait test_api::Subgroup vs test_api::Other");
        assert_incompatible(&compat, &[1, 0], "trait test_api::Subgroup");

        let mismatches: Vec<_> = compat
            .mismatches()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(mismatches, ["periph[].channel[].run", "sub"]);
    }

    /// Device bundle with the `Subgroup` trait definition left out, as it would be when it's in a snapshot.
    fn skip_subgroup(mut device: ApiBundleOwned) -> ApiBundleOwned {
        let idx = device
            .traits
            .iter()
            .position(|l| matches!(l, ApiLevelLocationOwned::InLine { level, .. } if level.trait_name == "Subgroup"))
            .unwrap();
        let signature =
            signature::trait_signature(&device, idx as u32, &signature::no_resolve).unwrap();
        let ApiLevelLocationOwned::InLine { crate_idx, .. } = device.traits[idx] else {
            unreachable!()
        };
        device.traits[idx] = ApiLevelLocationOwned::SkippedFullVersion {
            crate_idx,
            trait_name: "Subgroup".into(),
            signature,
        };
        device
    }

    #[test]
    fn skipped_trait_changed_without_version_bump() {
        let source = r#"
            #[ww_api_root(wire = "w1.0")]
            pub trait Api {
                fn m0();
                ww_impl!(sub: Subgroup);
            }

            #[ww_trait]
            trait Subgroup {
                fn m1();
            }
        "#;
        let client = bundle("client", "0.1.0", source);

        let device = skip_subgroup(bundle("device", "0.1.0", source));
        let compat = ApiCompat::new(&client, &device);
        assert_eq!(verdict(&compat, &[1]), Verdict::Compatible);

        // same version, but Subgroup gained a method (a doc change would be caught the same way)
        let changed = source.replace("fn m1();", "fn m1();\nfn m2();");
        let device = skip_subgroup(bundle("device", "0.1.0", &changed));
        let compat = ApiCompat::new(&client, &device);
        assert_incompatible(&compat, &[1], "changed without a version bump");
    }

    #[test]
    fn client_only_tracks_since() {
        let client = r#"
            #[ww_api_root(wire = "w1.2")]
            pub trait Api {
                fn a();
                #[since = "w1.2"]
                ww_impl!(sub: Subgroup);
            }

            #[ww_trait]
            trait Subgroup {
                #[since = "w1.1"]
                fn m1();
            }
        "#;
        let compat = ApiCompat::client_only(&bundle("client", "0.1.2", client));
        let m1 = compat.resolve(&path(&[1, 0])).unwrap();
        assert_eq!(m1.verdict, &Verdict::Compatible);
        let since: Vec<_> = m1.since.iter().map(|m| (m.since, m.device_wire)).collect();
        assert_eq!(
            since,
            [
                (WireVersion::new(1, 2), None),
                (WireVersion::new(1, 1), None)
            ],
            "every since along the path, the device's wire versions unknown"
        );
    }

    /// A `ww_stdlib` trait's `#[since]` names a revision of its own crate's wire version, not the API root's (EVO-26):
    /// `ww_board_info` (w1.2) marks `set_reworks` `w1.2` under an API root at `w1.0`. The device's `ww_board_info` at
    /// w1.2 has it, one at w1.1 doesn't, and the error names the crate.
    #[test]
    fn since_of_a_stdlib_crate() {
        use crate::config::IntrospectBundle;
        use crate::{Commander, Error};
        use ww_client_server::PathKind;
        use ww_version::{ApiHashOwned, ApiHashPairOwned, VersionOwned};

        let board_info = format!(
            "ww_board_info = {{ path = {:?} }}",
            concat!(env!("CARGO_MANIFEST_DIR"), "/../ww_stdlib/ww_board_info")
        );
        let src = r#"
            #[ww_api_root(wire = "w1.0")]
            pub trait Api {
                ww_impl!(board: ww_board_info::BoardInfo);
            }
        "#;
        let client = bundle_with("client", "0.1.1", src, &board_info);
        let device = bundle_with("device", "0.1.0", src, &board_info);
        let board = |b: &ApiBundleOwned| {
            b.ext_crates
                .iter()
                .position(|c| c.crate_id == "ww_board_info")
                .unwrap() as u32
        };
        assert_eq!(
            client.crate_wire(board(&client)),
            Some(WireVersion::new(1, 2))
        );
        assert_eq!(client.wire, Some(WireVersion::new(1, 0)));
        let ApiItemKindOwned::Trait { .. } = &client.root.items[0].kind else {
            panic!()
        };
        let level = client.root.items[0].get_as_level(&client).unwrap();
        let set_reworks = level
            .items
            .iter()
            .find(|i| i.ident == "set_reworks")
            .unwrap()
            .id
            .0;

        let introspect = |bundle, hash| {
            IntrospectBundle::from_sent(
                bundle,
                ApiHashPairOwned {
                    no_docs: ApiHashOwned { hash: vec![hash] },
                    with_docs: ApiHashOwned { hash: vec![] },
                },
                0,
            )
            .unwrap()
        };
        let connect = |device: ApiBundleOwned| {
            let (tx, _rx) = tokio::sync::mpsc::channel(1);
            let mut cmd = Commander::new(tx);
            cmd.connected_device.user_api_version =
                FullVersionOwned::new("test_api".into(), VersionOwned::new(0, 1, 0));
            cmd.connected_device.user_api_hash = introspect(device.clone(), 2).api_hash;
            cmd.set_client_introspect(introspect(client.clone(), 1));
            cmd.set_device_introspect(introspect(device, 2));
            cmd.resolve_api_match();
            cmd
        };
        let call = |cmd: &Commander| {
            cmd.prepare_call::<(), Error>(
                PathKind::absolute(&[UNib32(0), UNib32(set_reworks)]),
                Ok(vec![]),
                "",
            )
            .postpone_err
        };

        let cmd = connect(device.clone());
        assert_eq!(
            cmd.connected_device.user_api_wire,
            Some(WireVersion::new(1, 0))
        );
        assert!(call(&cmd).is_ok(), "{:?}", call(&cmd));

        let mut older = device;
        let idx = board(&older);
        older.set_crate_wire(idx, Some(WireVersion::new(1, 1)));
        let cmd = connect(older);
        match call(&cmd) {
            Err(e @ Error::OlderProtocol { .. }) => {
                let Error::OlderProtocol {
                    crate_id,
                    device_wire,
                    required,
                    ..
                } = &e
                else {
                    unreachable!()
                };
                assert_eq!(crate_id, "ww_board_info");
                assert_eq!(*device_wire, WireVersion::new(1, 1));
                assert_eq!(*required, WireVersion::new(1, 2));
                assert!(
                    e.to_string().contains("has ww_board_info at wire w1.1"),
                    "{e}"
                );
            }
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn commander_fails_locally() {
        use crate::config::IntrospectBundle;
        use crate::{Commander, Error};
        use ww_client_server::PathKind;
        use ww_version::{ApiHashOwned, ApiHashPairOwned, VersionOwned};

        let client = r#"
            #[ww_api_root(wire = "w1.1")]
            pub trait Api {
                fn a(x: u8);
                #[since = "w1.1"]
                fn b();
                fn c();
            }
        "#;
        let device = r#"
            #[ww_api_root(wire = "w1.0")]
            pub trait Api {
                fn a(x: u16);
            }
        "#;
        let introspect = |bundle, hash| {
            IntrospectBundle::from_sent(
                bundle,
                ApiHashPairOwned {
                    no_docs: ApiHashOwned { hash: vec![hash] },
                    with_docs: ApiHashOwned { hash: vec![] },
                },
                0,
            )
            .unwrap()
        };
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let mut cmd = Commander::new(tx);
        cmd.connected_device.user_api_version =
            FullVersionOwned::new("test_api".into(), VersionOwned::new(0, 1, 0));
        cmd.connected_device.user_api_hash = introspect(bundle("d", "0.1.0", device), 2).api_hash;
        cmd.set_client_introspect(introspect(bundle("c", "0.1.1", client), 1));
        cmd.set_device_introspect(introspect(bundle("d", "0.1.0", device), 2));
        cmd.resolve_api_match();

        fn call(cmd: &Commander, id: u32) -> Result<(), Error> {
            cmd.prepare_call::<(), Error>(PathKind::absolute(&[UNib32(id)]), Ok(vec![]), "")
                .postpone_err
        }
        assert!(
            matches!(call(&cmd, 0), Err(Error::IncompatibleResource { resource, .. }) if resource == "a")
        );
        assert!(matches!(
            call(&cmd, 1),
            Err(Error::OlderProtocol { device_wire, required, .. })
                if device_wire == WireVersion::new(1, 0) && required == WireVersion::new(1, 1)
        ));
        assert!(
            matches!(call(&cmd, 2), Err(Error::NotImplementedByDevice { resource, .. }) if resource == "c")
        );

        // same hash: nothing is checked
        cmd.connected_device.user_api_hash.no_docs.hash = vec![1];
        cmd.resolve_api_match();
        assert!(call(&cmd, 2).is_ok());

        // device API unknown: only since is checked, against the wire version the device's introspection data gave
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let mut cmd2 = Commander::new(tx);
        cmd2.connected_device = cmd.connected_device.clone();
        assert_eq!(
            cmd2.connected_device.user_api_wire,
            Some(WireVersion::new(1, 0))
        );
        cmd2.connected_device.user_api_hash.no_docs.hash = vec![2];
        cmd2.set_client_introspect(introspect(bundle("c", "0.1.1", client), 1));
        cmd2.resolve_api_match();
        assert!(call(&cmd2, 0).is_ok());
        assert!(matches!(call(&cmd2, 1), Err(Error::OlderProtocol { .. })));

        // device API and wire version unknown (a device from before wire versions): nothing to compare with
        cmd2.connected_device.user_api_wire = None;
        cmd2.resolve_api_match();
        assert!(call(&cmd2, 1).is_ok());
    }

    #[test]
    fn since_by_wire_version() {
        let w = WireVersion::new;
        // one line: the revision that added the item
        assert!(has_since(w(2, 3), w(2, 3)));
        assert!(has_since(w(2, 4), w(2, 3)));
        assert!(!has_since(w(2, 2), w(2, 3)));
        assert!(has_since(w(2, 10), w(2, 9)));
        // a new line counts every older marker as present
        assert!(has_since(w(3, 0), w(2, 3)));
        assert!(has_since(w(3, 0), w(1, 9)));
        // the device is on an older line than the marker
        assert!(!has_since(w(2, 9), w(3, 0)));
        assert!(!has_since(w(2, 9), w(3, 1)));
    }

    /// The device's API moved to a new wire line after the client's `#[since]` markers were written (w1.2 -> w2.0):
    /// items from the older line are callable, items newer than the device are refused.
    #[test]
    fn commander_since_across_breaking_bump() {
        use crate::config::IntrospectBundle;
        use crate::{Commander, Error};
        use ww_client_server::PathKind;
        use ww_version::{ApiHashOwned, ApiHashPairOwned, VersionOwned};

        let client = r#"
            #[ww_api_root(wire = "w2.1")]
            pub trait Api {
                fn a();
                #[since = "w1.1"]
                fn b();
                #[since = "w1.2"]
                fn c();
                #[since = "w2.1"]
                fn d();
            }
        "#;
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let mut cmd = Commander::new(tx);
        cmd.connected_device.user_api_version =
            FullVersionOwned::new("test_api".into(), VersionOwned::new(0, 6, 0));
        cmd.connected_device.user_api_wire = Some(WireVersion::new(2, 0));
        cmd.connected_device.user_api_hash = ApiHashPairOwned {
            no_docs: ApiHashOwned { hash: vec![2] },
            with_docs: ApiHashOwned { hash: vec![] },
        };
        let client = IntrospectBundle::from_sent(
            bundle("c", "0.6.1", client),
            ApiHashPairOwned {
                no_docs: ApiHashOwned { hash: vec![1] },
                with_docs: ApiHashOwned { hash: vec![] },
            },
            0,
        )
        .unwrap();
        cmd.set_client_introspect(client);
        // device API unknown: only since is checked
        cmd.resolve_api_match();

        let call = |id: u32| -> Result<(), Error> {
            cmd.prepare_call::<(), Error>(PathKind::absolute(&[UNib32(id)]), Ok(vec![]), "")
                .postpone_err
        };
        assert!(call(0).is_ok());
        assert!(call(1).is_ok(), "#[since = \"w1.1\"] on a w2.0 device");
        assert!(call(2).is_ok(), "#[since = \"w1.2\"] on a w2.0 device");
        assert!(matches!(call(3), Err(Error::OlderProtocol { .. })));
    }
}
