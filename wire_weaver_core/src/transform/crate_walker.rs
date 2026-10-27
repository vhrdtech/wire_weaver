use super::api::{compact_trait_id, convert_api_items};
use super::cargo_source::{CargoSources, DepSpec, Dependent};
use super::cfg::{
    DepRequest, Features, all_features, dep_requests, enabled_features, expand_cfg_attrs,
};
use super::meta::{MetaSite, get_meta};
use super::util::{UseLeaf, collect_docs, use_tree_leaves, use_tree_path_to};
use anyhow::{Context, Result, anyhow};
use cargo_toml::{Dependency, DepsSet, Inheritable, Manifest};
use shrink_wrap::UNib32;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use syn::{File, Item};
use ww_self::{ApiBundleOwned, ApiLevelLocationOwned, ApiLevelOwned, TypeLocationOwned, TypeOwned};
use ww_version::{FullVersionOwned, VersionOwned};

/// Crates re-exported by `wire_weaver`, usable without depending on them directly.
pub(crate) const WIRE_WEAVER_REEXPORTS: &[&str] = &["shrink_wrap", "ww_error", "ww_version"];

/// Load API definition and all referenced data types from a given crate.
///
/// This method will recursively walk (and download if necessary) all the referenced crates:
/// 1. Load `crate_path/Cargo.toml` and parse into cargo_toml::[Manifest].
/// 2. Load `crate_path/src/lib.rs` and parse into syn::[File].
/// 3. Find the trait marked with `#[ww_trait]` and named `trait_name`.
///    3.1 If provided, otherwise the first and only `#[ww_api_root]` or `#[ww_trait]` is used or an error is returned.
/// 4. For each user-defined type referenced, find its definition:
///    4.1 Defined in the same file
///    4.2 Imported using `use another_crate::Ty`
///    4.2.1 Load `another_crate` starting from step #1, skipping #3.
///    4.3 Convert into [ww_self::Type]
/// 5. For each trait referenced via `ww_impl` do similar steps as for types.
/// 6. Assemble all data into [ww_self::ApiBundle]
/// 7. Cache in `~/.wire_weaver/crate_name-sha.ron` if not already.
///
/// If `offline_mode` is true, no attempts to download any crates will be made. Only the ones already
/// found in the local Cargo registry will be used, or an error is returned if they are not found.
///
/// Limitations:
/// * Only ww_trait's defined in `src/lib.rs` are supported, types can also be in modules (`mod ty;` and `use ty::Ty`).
/// * Path, git and registry dependencies are supported. Git and registry ones are found where Cargo fetched them
///   (under `CARGO_HOME`), at the exact version the nearest `Cargo.lock` above `crate_path` pins.
pub fn load(
    crate_path: &Path,
    trait_name: Option<String>,
    _offline_mode: bool, // TODO: try CARGO_NET_OFFLINE instead
) -> Result<ApiBundleOwned> {
    let mut scratch = Scratch::default();
    let manifest = ManifestContext::load(crate_path)?;
    let features = manifest.default_features();
    load_inner(manifest, features, trait_name, &mut scratch)
}

/// Load API definition and all referenced data types for a given dependency of 'current crate'.
/// Intended use is from a build.rs script, where CARGO_MANIFEST_DIR is available and assumed to be 'current crate'.
pub fn load_dep(
    dep_name: impl AsRef<str>,
    trait_name: Option<impl AsRef<str>>,
) -> Result<ApiBundleOwned> {
    load_dep_with_paths(dep_name, trait_name).map(|(bundle, _)| bundle)
}

/// [load_dep], and how code generated in 'current crate' names the crates of the API's types and traits: pass
/// them to codegen as `crate_paths` (API-60).
pub fn load_dep_with_paths(
    dep_name: impl AsRef<str>,
    trait_name: Option<impl AsRef<str>>,
) -> Result<(ApiBundleOwned, CratePaths)> {
    let mut scratch = Scratch::default();
    let current_crate_path = std::env::var("CARGO_MANIFEST_DIR").map_err(|_|
        anyhow!("CARGO_MANIFEST_DIR not set, cannot determine current crate path. Consider calling this method from a build.rs script.")
    )?;
    let current_crate_path = PathBuf::from(current_crate_path);
    let current_crate_manifest = ManifestContext::load(&current_crate_path)?;
    scratch.sources.entry_dir = Some(current_crate_manifest.crate_path.clone());
    let current_features = current_crate_manifest.default_features();
    let (dep_manifest, dep_features) = current_crate_manifest.load_dependent_manifest(
        dep_name.as_ref(),
        &current_features,
        &mut scratch,
    )?;
    scratch
        .manifests
        .insert(current_crate_path, current_crate_manifest.clone());
    let trait_name = trait_name.map(|t| t.as_ref().to_string());
    let bundle = load_inner(dep_manifest, dep_features, trait_name, &mut scratch)?;
    let paths = CratePaths::find(&current_crate_manifest, &bundle, &scratch);
    Ok((bundle, paths))
}

/// How code generated in a crate names the crates an API's types and traits are defined in (API-60), for those it
/// can't name directly.
///
/// The API model names a type by the crate that defines it (`exact_units::units::Second`), and so does generated
/// code. A crate the generating crate doesn't depend on is still reachable when a crate it does depend on
/// re-exports it at its root (`pub use exact_units;` in `ww_si`): generated code then names it
/// `ww_si::exact_units::units::Second`. Crates the generating crate depends on under their own name aren't listed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CratePaths {
    paths: BTreeMap<String, Vec<String>>,
}

impl CratePaths {
    /// Path segments naming `crate_name` (`["ww_si", "exact_units"]`), `None` when it is named by its own name.
    pub fn get(&self, crate_name: &str) -> Option<&[String]> {
        self.paths.get(crate_name).map(|path| path.as_slice())
    }

    /// Paths for the crates of `bundle` that `current`, the generating crate, doesn't depend on: through the root
    /// re-exports of the crates it does depend on, transitively, among the crates walked for `bundle`.
    fn find(current: &ManifestContext, bundle: &ApiBundleOwned, scratch: &Scratch) -> Self {
        let mut found: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut queue = VecDeque::new();
        let direct = current
            .manifest
            .dependencies
            .iter()
            .chain(&current.manifest.dev_dependencies);
        for (key, dep) in direct {
            let package = dep.package().unwrap_or(key).to_string();
            found.insert(package.clone(), vec![key.clone()]);
            queue.push_back(package);
        }
        while let Some(name) = queue.pop_front() {
            let Some(crate_cx) = scratch.crates.values().find(|c| c.crate_name() == name) else {
                continue;
            };
            for (dep_key, exported_as) in crate_cx.reexported_crates() {
                let Some(dep) = crate_cx.manifest.manifest.dependencies.get(&dep_key) else {
                    continue;
                };
                let package = dep.package().unwrap_or(&dep_key).to_string();
                if found.contains_key(&package) {
                    continue;
                }
                let mut path = found[&name].clone();
                path.push(exported_as);
                found.insert(package.clone(), path);
                queue.push_back(package);
            }
        }
        let paths = bundle
            .ext_crates
            .iter()
            .filter_map(|c| {
                let path = found.get(&c.crate_id)?;
                (path.len() > 1 || path[0] != c.crate_id)
                    .then(|| (c.crate_id.clone(), path.clone()))
            })
            .collect();
        CratePaths { paths }
    }
}

/// The API of the crate of `manifest`, built with `features`.
fn load_inner(
    manifest: Rc<ManifestContext>,
    features: Features,
    trait_name: Option<String>,
    scratch: &mut Scratch,
) -> Result<ApiBundleOwned> {
    let entry = CrateContext::load_from_manifest(manifest, features, scratch)?;
    let trait_name = find_trait_if_none(trait_name, &entry.lib_rs_ast)?;
    let item_trait = get_trait(&entry.lib_rs_ast, &trait_name)?;
    let items = convert_api_items(item_trait, &entry, scratch)?;

    let root = ApiLevelOwned {
        docs: collect_docs(&item_trait.attrs),
        crate_idx: 0.into(),
        compact_id: compact_trait_id(&entry, &trait_name),
        meta: get_meta(&item_trait.attrs, MetaSite::Trait, &entry)?,
        trait_name,
        items,
    };
    Ok(ApiBundleOwned {
        magic: ww_self::MAGIC,
        ww_self_version: ww_self::VERSION,
        root,
        types: scratch.root_bundle.types.clone(),
        traits: scratch.root_bundle.traits.clone(),
        ext_crates: scratch.root_bundle.ext_crates.clone(),
        root_omitted: None,
    })
}

/// Main scratch space and cache used to build ApiBundleOwned
#[derive(Default)]
pub(crate) struct Scratch {
    /// Cached crates
    crates: HashMap<FullVersionOwned, Rc<CrateContext>>,
    /// Cached manifests
    manifests: HashMap<PathBuf, Rc<ManifestContext>>,
    /// Scratch space with types and traits
    pub(crate) root_bundle: ApiBundleScratch,
    /// Reuse already converted traits instead of adding them again, when referenced more than once.
    /// Off for codegen to keep API hashes of existing APIs unchanged.
    pub(crate) dedup_traits: bool,
    /// User-defined types being converted, to stop on self-referential types instead of recursing forever.
    pub(crate) types_in_progress: Vec<TypeInProgress>,
    /// Self-referential types already converted: their definitions refer to their own index, so they are found by
    /// name, not by comparing definitions.
    pub(crate) recursive_types: HashMap<TypeKey, UNib32>,
    /// Type and const parameters of the generic definition whose fields are being converted (API-43), empty
    /// outside of one.
    pub(crate) generic_scope: Vec<ww_self::GenericParamOwned>,
    /// Parameters of the type alias whose right-hand side is being converted (API-45), each with the argument it
    /// stands for, already converted where the alias is used; empty outside of one.
    pub(crate) alias_args: Vec<(String, ww_self::GenericArgOwned)>,
    /// Public paths already found ([CrateContext::public_path]), by crate, module and name of the definition.
    public_paths: HashMap<(FullVersionOwned, Vec<ModSegment>, String), Vec<String>>,
    /// Child modules already parsed ([CrateContext::module]), by crate, parent module and name; `None` for a name
    /// that isn't a module. Resolving every path anew re-parsed the same files hundreds of times per API.
    modules: HashMap<(FullVersionOwned, Vec<ModSegment>, String), Option<Rc<CrateContext>>>,
    /// Cargo.lock and Cargo's fetched sources, for git and registry dependencies.
    pub(crate) sources: CargoSources,
}

/// Crate and module-qualified name (`units::Volt`) of a user-defined type.
pub(crate) type TypeKey = (FullVersionOwned, String);

/// A user-defined type being converted.
pub(crate) struct TypeInProgress {
    pub(crate) key: TypeKey,
    /// Whether the type has type or const parameters.
    pub(crate) generic: bool,
    /// Index reserved for the definition once the type turned out to refer to itself: the references to it while
    /// it is converted point there.
    pub(crate) slot: Option<UNib32>,
}

/// Scratch space used to hold types and traits
#[derive(Default)]
pub(crate) struct ApiBundleScratch {
    pub(crate) types: Vec<TypeLocationOwned>,
    pub(crate) traits: Vec<ApiLevelLocationOwned>,
    pub(crate) ext_crates: Vec<FullVersionOwned>,
}

impl ApiBundleScratch {
    pub(crate) fn find_crate_or_create(&mut self, current_crate: &CrateContext) -> UNib32 {
        if let Some(idx) = self
            .ext_crates
            .iter()
            .position(|c| c == &current_crate.version)
        {
            return (idx as u32).into();
        }
        let idx = self.ext_crates.len() as u32;
        self.ext_crates.push(current_crate.version.clone());
        idx.into()
    }

    pub(crate) fn find_type(&self, ty: &TypeOwned) -> Option<UNib32> {
        self.types
            .iter()
            .enumerate()
            .find(|(_idx, tl)| {
                if let TypeLocationOwned::InLine { ty: t, .. } = tl {
                    t == ty
                } else {
                    false
                }
            })
            .map(|(idx, _)| (idx as u32).into())
    }

    pub(crate) fn push_out_of_line_idx(
        &mut self,
        ty: TypeOwned,
        current_crate: &CrateContext,
    ) -> UNib32 {
        if let TypeOwned::OutOfLine { type_idx } = &ty {
            return *type_idx;
        }
        let type_idx = self.types.len() as u32;
        let crate_idx = self.find_crate_or_create(current_crate);
        self.types.push(TypeLocationOwned::InLine { ty, crate_idx });
        type_idx.into()
    }
}

#[derive(Clone)]
pub(crate) struct CrateContext {
    manifest: Rc<ManifestContext>,
    version: FullVersionOwned,
    /// Features the crate is built with, `#[cfg_attr(..)]` in `lib_rs_ast` are expanded with them (API-60).
    features: Rc<Features>,
    /// `src/lib.rs`, or a module of the crate when resolving paths through it.
    pub(crate) lib_rs_ast: File,
    /// Directory where files of the child modules of `lib_rs_ast` are.
    mod_dir: PathBuf,
    /// Modules from the crate root to `lib_rs_ast`, each with whether it is `pub`; empty for the crate root.
    mod_path: Vec<ModSegment>,
    // crate_path: PathBuf,
    // lib_rs_source: String,
}

impl CrateContext {
    pub(crate) fn crate_name(&self) -> &str {
        &self.version.crate_id
    }
}

/// A module on the way from the crate root to a [CrateContext].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ModSegment {
    pub(crate) name: String,
    /// `pub mod`, reachable from other crates if the modules before it are.
    pub(crate) public: bool,
}

/// Cargo.toml manifest together with its file path
pub(crate) struct ManifestContext {
    crate_path: PathBuf,
    manifest: Manifest,
}

impl CrateContext {
    /// The crate at `crate_path` with every feature enabled, as for a snapshot of everything it can serialize.
    pub(crate) fn load_all_features(crate_path: &Path, scratch: &mut Scratch) -> Result<Rc<Self>> {
        let manifest = ManifestContext::load(crate_path)?;
        let features = all_features(&manifest.manifest);
        Self::load_from_manifest(manifest, features, scratch)
    }

    /// The crate of `manifest` built with `features`. A crate already loaded with fewer features is loaded again
    /// with both sets, as Cargo unifies the features its dependents ask for.
    fn load_from_manifest(
        manifest: Rc<ManifestContext>,
        features: Features,
        scratch: &mut Scratch,
    ) -> Result<Rc<Self>> {
        if scratch.sources.entry_dir.is_none() {
            scratch.sources.entry_dir = Some(manifest.crate_path.clone());
        }
        let version = manifest.full_version(scratch)?;
        let features = match scratch.crates.get(&version) {
            Some(crate_cx) if features.is_subset(&crate_cx.features) => {
                return Ok(crate_cx.clone());
            }
            Some(crate_cx) => {
                let mut features = features;
                features.extend(crate_cx.features.iter().cloned());
                // its modules were parsed with fewer features
                scratch.modules.retain(|(v, _, _), _| v != &version);
                scratch.public_paths.retain(|(v, _, _), _| v != &version);
                features
            }
            None => features,
        };
        let mut lib_rs = load_lib_rs(&manifest.crate_path)?;
        expand_cfg_attrs(&mut lib_rs, &features)
            .with_context(|| format!("{}/src/lib.rs", manifest.crate_path.display()))?;
        let crate_cx = Self {
            // crate_path: manifest.crate_path.to_path_buf(),
            mod_dir: manifest.crate_path.join("src"),
            manifest,
            version,
            features: Rc::new(features),
            lib_rs_ast: lib_rs,
            mod_path: vec![],
        };
        // the root crate is always first in ext_crates, even when no types are referenced from it; the others are
        // added when a type or trait of theirs is, not when a path only goes through them (`wire_weaver::prelude`)
        if scratch.root_bundle.ext_crates.is_empty() {
            scratch.root_bundle.find_crate_or_create(&crate_cx);
        }
        let crate_cx = Rc::new(crate_cx);
        scratch
            .crates
            .insert(crate_cx.version.clone(), crate_cx.clone());
        Ok(crate_cx)
    }

    pub(crate) fn load_dependent_crate(
        &self,
        crate_name: &str,
        scratch: &mut Scratch,
    ) -> Result<Rc<Self>> {
        let (dep_manifest, dep_features) =
            self.manifest
                .load_dependent_manifest(crate_name, &self.features, scratch)?;
        Self::load_from_manifest(dep_manifest, dep_features, scratch)
    }

    /// Resolve the leading segments of a path (e.g. `a::b` in `a::b::Ty`): `crate`, `self`, modules of this crate or
    /// dependent crates, followed by modules.
    pub(crate) fn resolve_path<'s>(
        &self,
        segments: impl IntoIterator<Item = &'s syn::Ident>,
        scratch: &mut Scratch,
    ) -> Result<Rc<Self>> {
        // None is self, to avoid cloning it
        let mut resolved: Option<Rc<Self>> = None;
        for (i, segment) in segments.into_iter().enumerate() {
            let cx = resolved.as_deref().unwrap_or(self);
            let name = segment.to_string();
            let next = match name.as_str() {
                "self" if i == 0 => continue,
                "crate" if i == 0 => cx.crate_root(scratch)?,
                _ => match cx.module(&name, scratch)? {
                    Some(module) => module,
                    None => match cx.imported_module(&name, scratch) {
                        Some(module) => module,
                        None if i == 0 => cx.load_dependent_crate(&name, scratch)?,
                        None => {
                            return Err(
                                anyhow!("Module {name} not found").context(cx.err_context())
                            );
                        }
                    },
                },
            };
            resolved = Some(next);
        }
        Ok(resolved.unwrap_or_else(|| Rc::new(self.clone())))
    }

    /// Crates re-exported at this module's root (`pub use dep;`, `pub use dep as name;`, `pub extern crate dep;`),
    /// by the dependency key and the name they are exported under.
    fn reexported_crates(&self) -> Vec<(String, String)> {
        let mut crates = vec![];
        for item in &self.lib_rs_ast.items {
            match item {
                Item::Use(item_use) if matches!(item_use.vis, syn::Visibility::Public(_)) => {
                    for (path, leaf) in use_tree_leaves(&item_use.tree) {
                        match (path.is_empty(), leaf) {
                            (true, UseLeaf::Name(name)) => {
                                crates.push((name.to_string(), name.to_string()))
                            }
                            (true, UseLeaf::Rename { name, rename }) => {
                                crates.push((name.to_string(), rename.to_string()))
                            }
                            _ => {}
                        }
                    }
                }
                Item::ExternCrate(extern_crate)
                    if matches!(extern_crate.vis, syn::Visibility::Public(_)) =>
                {
                    let name = extern_crate.ident.to_string();
                    let exported = extern_crate
                        .rename
                        .as_ref()
                        .map(|(_, rename)| rename.to_string())
                        .unwrap_or_else(|| name.clone());
                    crates.push((name, exported));
                }
                _ => {}
            }
        }
        crates
    }

    /// The module or crate a `use` of this module names `name`: `use dep as name;` (a crate under another name, API-60),
    /// `use dep::units;` or `use self::inner::units as name;`. `None` when no `use` names it, or it doesn't resolve
    /// to a module or crate.
    fn imported_module(&self, name: &str, scratch: &mut Scratch) -> Option<Rc<Self>> {
        for item in &self.lib_rs_ast.items {
            let Item::Use(item_use) = item else {
                continue;
            };
            let Some((path, real_name)) = use_tree_path_to(&item_use.tree, name) else {
                continue;
            };
            if path.is_empty() {
                return self
                    .load_dependent_crate(&real_name.to_string(), scratch)
                    .ok();
            }
            return self
                .resolve_path(path.into_iter().chain([real_name]), scratch)
                .ok();
        }
        None
    }

    /// The root of the crate this module is in.
    fn crate_root(&self, scratch: &mut Scratch) -> Result<Rc<Self>> {
        Self::load_from_manifest(self.manifest.clone(), (*self.features).clone(), scratch)
    }

    /// Whether `name` is a child module of this crate or module.
    pub(crate) fn has_module(&self, name: &str) -> bool {
        self.lib_rs_ast
            .items
            .iter()
            .any(|item| matches!(item, Item::Mod(item_mod) if item_mod.ident == name))
    }

    /// Child module `name`, either in-line or in its own file.
    fn module(&self, name: &str, scratch: &mut Scratch) -> Result<Option<Rc<Self>>> {
        let key = (
            self.version.clone(),
            self.mod_path.clone(),
            name.to_string(),
        );
        if let Some(module) = scratch.modules.get(&key) {
            return Ok(module.clone());
        }
        let module = self.parse_module(name)?.map(Rc::new);
        scratch.modules.insert(key, module.clone());
        Ok(module)
    }

    fn parse_module(&self, name: &str) -> Result<Option<Self>> {
        let Some(item_mod) = self.lib_rs_ast.items.iter().find_map(|item| match item {
            Item::Mod(item_mod) if item_mod.ident == name => Some(item_mod),
            _ => None,
        }) else {
            return Ok(None);
        };
        let mod_dir = self.mod_dir.join(name);
        let ast = match &item_mod.content {
            Some((_, items)) => {
                let mut file: File = syn::parse_quote! {};
                file.items = items.clone();
                file
            }
            None => {
                let file_rs = self.mod_dir.join(format!("{name}.rs"));
                let path = if file_rs.exists() {
                    file_rs
                } else {
                    mod_dir.join("mod.rs")
                };
                let mut file = parse_rs(&path)?;
                expand_cfg_attrs(&mut file, &self.features)
                    .with_context(|| path.display().to_string())?;
                file
            }
        };
        let mut mod_path = self.mod_path.clone();
        mod_path.push(ModSegment {
            name: name.to_string(),
            public: matches!(item_mod.vis, syn::Visibility::Public(_)),
        });
        Ok(Some(Self {
            manifest: self.manifest.clone(),
            version: self.version.clone(),
            features: self.features.clone(),
            lib_rs_ast: ast,
            mod_dir,
            mod_path,
        }))
    }

    pub(crate) fn err_context(&self) -> String {
        format!("{:?}", self.version)
    }

    /// Where and under which name other crates find `ident`, a struct or enum defined in this module (API-46): the
    /// modules of its [public path](Self::public_path), `None` at the crate root, and its name there (`Renamed` for
    /// `pub use inner::Inner as Renamed`).
    pub(crate) fn public_name(
        &self,
        ident: &str,
        scratch: &mut Scratch,
    ) -> Result<(Option<Vec<String>>, String)> {
        let mut path = self.public_path(ident, scratch)?;
        let name = path.pop().expect("a public path ends with the name");
        Ok(((!path.is_empty()).then_some(path), name))
    }

    /// `ident` qualified with the modules it is in from the crate root: `units::Volt`.
    pub(crate) fn qualified_name(&self, ident: &str) -> String {
        let mut name: String = self
            .mod_path
            .iter()
            .map(|m| format!("{}::", m.name))
            .collect();
        name.push_str(ident);
        name
    }

    /// Path after the crate name by which other crates name `ident`, a struct or enum defined in this module
    /// (API-46): the shortest one through `pub` modules and `pub use` re-exports of this crate. `["Ty"]` for a type
    /// defined or re-exported at the crate root, `["units", "Volt"]` for one in `pub mod units` that isn't.
    pub(crate) fn public_path(&self, ident: &str, scratch: &mut Scratch) -> Result<Vec<String>> {
        let key = (
            self.version.clone(),
            self.mod_path.clone(),
            ident.to_string(),
        );
        if let Some(path) = scratch.public_paths.get(&key) {
            return Ok(path.clone());
        }
        let mut cx = self.crate_root(scratch)?;
        let mut path = vec![];
        for segment in self.mod_path.iter().map(Some).chain([None]) {
            if let Some(name) = cx.exported_name(self, ident, scratch, 0) {
                path.push(name);
                scratch.public_paths.insert(key, path.clone());
                return Ok(path);
            }
            let Some(segment) = segment.filter(|s| s.public) else {
                break;
            };
            path.push(segment.name.clone());
            cx = cx
                .module(&segment.name, scratch)?
                .ok_or_else(|| anyhow!("internal: module {} not found", segment.name))?;
        }
        Err(anyhow!(
            "{} is not reachable from other crates, so generated code can't name it: make the modules on its way \
             `pub` or re-export it with `pub use`",
            self.qualified_name(ident)
        )
        .context(self.err_context()))
    }

    /// The name under which this module makes `ident`, defined in `def_cx`, public: `ident` when this is `def_cx`,
    /// the name of a `pub use` of it (by name, renamed or through a glob) otherwise. Only re-exports within the crate
    /// are followed, `depth` bounds the chain.
    fn exported_name(
        &self,
        def_cx: &Self,
        ident: &str,
        scratch: &mut Scratch,
        depth: u8,
    ) -> Option<String> {
        if self.version == def_cx.version && self.mod_path == def_cx.mod_path {
            return Some(ident.to_string());
        }
        if depth >= 8 {
            return None;
        }
        for item in &self.lib_rs_ast.items {
            let Item::Use(item_use) = item else {
                continue;
            };
            if !matches!(item_use.vis, syn::Visibility::Public(_)) {
                continue;
            }
            for (path, leaf) in use_tree_leaves(&item_use.tree) {
                let is_local = path.first().is_some_and(|first| {
                    *first == "self" || *first == "crate" || self.has_module(&first.to_string())
                });
                if !is_local {
                    continue;
                }
                let Ok(module) = self.resolve_path(path, scratch) else {
                    continue;
                };
                if module.mod_path == self.mod_path {
                    // `pub use self::*`
                    continue;
                }
                let exported = module.exported_name(def_cx, ident, scratch, depth + 1);
                let name = match (leaf, exported) {
                    (_, None) => continue,
                    (UseLeaf::Name(name), Some(exported)) if *name == exported => exported,
                    (UseLeaf::Rename { name, rename }, Some(exported)) if *name == exported => {
                        rename.to_string()
                    }
                    (UseLeaf::Glob, Some(exported)) => exported,
                    _ => continue,
                };
                return Some(name);
            }
        }
        None
    }

    pub(crate) fn version(&self) -> &FullVersionOwned {
        &self.version
    }
}

impl Scratch {
    fn get_or_load_manifest(&mut self, crate_path: PathBuf) -> Result<Rc<ManifestContext>> {
        if let Some(manifest_cx) = self.manifests.get(&crate_path) {
            return Ok(manifest_cx.clone());
        }
        let manifest_cx = ManifestContext::load(&crate_path)?;
        self.manifests.insert(crate_path, manifest_cx.clone());
        Ok(manifest_cx)
    }
}

impl ManifestContext {
    fn load(crate_path: &Path) -> Result<Rc<Self>> {
        let crate_path = crate_path.canonicalize()?;
        //println!("loading manifest for {}", crate_path.display());
        let mut cargo_toml_path = crate_path.to_path_buf();
        cargo_toml_path.push("Cargo.toml");
        let contents = fs::read_to_string(&cargo_toml_path).context(format!(
            "Failed to read Cargo.toml from {}",
            cargo_toml_path.display()
        ))?;
        let manifest = Manifest::from_str(&contents).context(format!(
            "Failed to parse Cargo.toml from {}",
            cargo_toml_path.display()
        ))?;
        Ok(Rc::new(ManifestContext {
            crate_path: crate_path.to_path_buf(),
            manifest,
        }))
    }

    /// Look for a parent Cargo.toml that is a workspace and load it.
    fn load_parent_manifest(&self, scratch: &mut Scratch) -> Result<Rc<ManifestContext>> {
        let mut possible_path = self.crate_path.to_path_buf();
        loop {
            let parent_is_some = possible_path.pop();
            if !parent_is_some {
                break;
            }
            let possible_cargo_toml = possible_path.join("Cargo.toml");
            //println!("trying {possible_cargo_toml:?}");
            if possible_cargo_toml.exists() {
                return scratch.get_or_load_manifest(possible_path);
            }
        }
        Err(anyhow!("not found"))
    }

    fn full_version(&self, scratch: &mut Scratch) -> Result<FullVersionOwned> {
        let (name, version) = self.package_version(scratch)?;
        Ok(FullVersionOwned::new(
            name,
            VersionOwned::new(
                version.major as u32,
                version.minor as u32,
                version.patch as u32,
            ),
        ))
    }

    /// Package name and its full version (with pre-release and build metadata).
    fn package_version(&self, scratch: &mut Scratch) -> Result<(String, semver::Version)> {
        let package = self
            .manifest
            .package
            .clone()
            .context("No package section found in Cargo.toml")?;
        let version = match &package.version {
            Inheritable::Set(version) => version.clone(),
            // `version.workspace = true`, e.g. `wire_weaver` itself, walked for its `prelude`
            Inheritable::Inherited => self
                .load_parent_manifest(scratch)?
                .manifest
                .workspace
                .as_ref()
                .and_then(|w| w.package.as_ref())
                .and_then(|p| p.version.clone())
                .ok_or_else(|| {
                    anyhow!("version.workspace = true, but the workspace has no package.version")
                        .context(self.err_context())
                })?,
        };
        Ok((package.name, version))
    }

    /// Features of this crate when nobody asks for any: its default ones.
    fn default_features(&self) -> Features {
        enabled_features(&self.manifest, [], true)
    }

    /// Manifest of the dependency `crate_name` of this crate (built with `features`), and the features this crate
    /// asks of it: in its dependency entry, and through `crate_name/feature` in its own `[features]`.
    fn load_dependent_manifest(
        &self,
        crate_name: &str,
        features: &Features,
        scratch: &mut Scratch,
    ) -> Result<(Rc<ManifestContext>, Features)> {
        // e.g. `use wire_weaver::prelude::*;` brings in `ww_version` without a direct dependency on it
        if !self.manifest.dependencies.contains_key(crate_name)
            && WIRE_WEAVER_REEXPORTS.contains(&crate_name)
            && self.manifest.dependencies.contains_key("wire_weaver")
        {
            let (wire_weaver, wire_weaver_features) =
                self.load_dependent_manifest("wire_weaver", features, scratch)?;
            return wire_weaver.load_dependent_manifest(crate_name, &wire_weaver_features, scratch);
        }
        // a `ww_codegen!` in test code (e.g. a test server) sees dev-dependencies too
        let deps = if !self.manifest.dependencies.contains_key(crate_name)
            && self.manifest.dev_dependencies.contains_key(crate_name)
        {
            &self.manifest.dev_dependencies
        } else {
            &self.manifest.dependencies
        };
        let (dep, request) = self.load_dependent_inner(deps, crate_name, self, scratch)?;
        let requested =
            request
                .features
                .into_iter()
                .chain(dep_requests(&self.manifest, features, crate_name));
        let dep_features = enabled_features(&dep.manifest, requested, request.default_features);
        Ok((dep, dep_features))
    }

    /// Manifest of `crate_name` from `deps_set`, a dependency table of this manifest, and what its entry asks for;
    /// `dependent` is the package asking for it (this one, or a member of this workspace for
    /// `[workspace.dependencies]`).
    fn load_dependent_inner(
        &self,
        deps_set: &DepsSet,
        crate_name: &str,
        dependent: &ManifestContext,
        scratch: &mut Scratch,
    ) -> Result<(Rc<ManifestContext>, DepRequest)> {
        let dep = deps_set.get(crate_name).ok_or(
            anyhow!("Dependency not found")
                .context(crate_name.to_owned())
                .context(self.err_context()),
        )?;
        let request = DepRequest::of(dep);
        let manifest = match dep {
            // crate_name = "version"
            Dependency::Simple(version) => self.load_locked(
                dependent,
                crate_name,
                &DepSpec::Registry {
                    version_req: Some(version),
                },
                scratch,
            ),
            Dependency::Inherited(_) => {
                // crate_name.workspace = true
                let manifest = self.load_parent_manifest(scratch)?;
                let workspace = manifest
                    .manifest
                    .workspace
                    .as_ref()
                    .ok_or(anyhow!(
                        "{} load_dependent({crate_name}): parent Cargo.toml ({}) is not a workspace",
                        self.crate_path.display(),
                        manifest.crate_path.display()
                    ))
                    .context(self.err_context())?;
                let (manifest, mut workspace_request) = manifest.load_dependent_inner(
                    &workspace.dependencies,
                    crate_name,
                    dependent,
                    scratch,
                )?;
                // `{ workspace = true, features = [..] }` adds to the workspace entry's features
                workspace_request.features.extend(request.features);
                return Ok((manifest, workspace_request));
            }
            Dependency::Detailed(detailed) => {
                // crate_name = { path = "" }
                if let Some(path) = &detailed.path {
                    let path = Path::new(path);
                    let dep_crate_path = if path.is_relative() {
                        self.crate_path.join(path)
                    } else {
                        path.to_path_buf()
                    };
                    return Ok((scratch.get_or_load_manifest(dep_crate_path)?, request));
                }
                let package = detailed.package.as_deref().unwrap_or(crate_name);
                let spec = match &detailed.git {
                    // crate_name = { git = "" }
                    Some(url) => DepSpec::Git { url },
                    // crate_name = { version = "" }
                    None => DepSpec::Registry {
                        version_req: detailed.version.as_ref(),
                    },
                };
                self.load_locked(dependent, package, &spec, scratch)
            }
        }?;
        Ok((manifest, request))
    }

    /// Manifest of `package`, a git or registry dependency of `dependent`, found where Cargo fetched it.
    fn load_locked(
        &self,
        dependent: &ManifestContext,
        package: &str,
        spec: &DepSpec,
        scratch: &mut Scratch,
    ) -> Result<Rc<ManifestContext>> {
        let (name, version) = dependent.package_version(scratch)?;
        let dependent_id = Dependent {
            name: &name,
            version: &version,
            dir: &dependent.crate_path,
        };
        let dir = scratch
            .sources
            .locate(&dependent_id, package, spec)
            .context(dependent.err_context())?;
        scratch.get_or_load_manifest(dir)
    }

    fn err_context(&self) -> String {
        format!("{}/Cargo.toml", self.crate_path.display())
    }
}

fn load_lib_rs(crate_path: &Path) -> Result<File> {
    parse_rs(&crate_path.canonicalize()?.join("src").join("lib.rs"))
}

fn parse_rs(path: &Path) -> Result<File> {
    let contents =
        fs::read_to_string(path).context(format!("Failed to read {}", path.display()))?;
    syn::parse_file(&contents).context(format!("Failed to parse {}", path.display()))
}

fn find_trait_if_none(trait_name: Option<String>, lib_rs: &File) -> Result<String> {
    if let Some(name) = trait_name {
        return Ok(name.to_string());
    }
    let mut ww_api_root = vec![];
    let mut ww_trait = vec![];
    for item in &lib_rs.items {
        let Item::Trait(item_trait) = item else {
            continue;
        };
        let attrs = &item_trait.attrs;
        if attrs.iter().any(|attr| attr.path().is_ident("ww_api_root")) {
            ww_api_root.push(item_trait.ident.to_string());
        }
        if attrs.iter().any(|attr| attr.path().is_ident("ww_trait")) {
            ww_trait.push(item_trait.ident.to_string());
        }
    }
    match (ww_api_root.len(), ww_trait.len()) {
        (0, 0) => Err(anyhow!(
            "No #[ww_api_root] or #[ww_trait] marked traits found in lib.rs"
        )),
        (0, 1) => Ok(ww_trait[0].clone()),
        (1, _) => Ok(ww_api_root[0].clone()),
        _ => Err(anyhow!(
            "Multiple #[ww_api_root] or #[ww_trait] marked traits found in lib.rs"
        )),
    }
}

fn get_trait<'i>(lib_rs: &'i File, trait_name: &str) -> Result<&'i syn::ItemTrait> {
    for item in &lib_rs.items {
        let Item::Trait(item_trait) = item else {
            continue;
        };
        if item_trait.ident == trait_name {
            return Ok(item_trait);
        }
    }
    Err(anyhow!("Trait with name {} not found", trait_name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn write(path: &Path, contents: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("ww_core_crate_walker_tests")
            .join(format!("{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// `#[ww_api_root]` of crate `api` in `dir` using `Info` from `dep_api`, asked for with `dep`.
    fn write_api_crate(dir: &Path, dep: &str) {
        write(
            &dir.join("Cargo.toml"),
            &format!(
                "[package]\nname = \"api\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\n\n\
                 [dependencies]\ndep_api = {dep}\n"
            ),
        );
        write(
            &dir.join("src/lib.rs"),
            "#[ww_api_root]\npub trait Api {\n    ww_impl!(info: dep_api::Info);\n}\n",
        );
    }

    fn write_dep_api(dir: &Path, version: &str) {
        write(
            &dir.join("Cargo.toml"),
            &format!("[package]\nname = \"dep_api\"\nversion = {version}\nedition = \"2024\"\n"),
        );
        write(
            &dir.join("src/lib.rs"),
            "#[ww_trait]\npub trait Info {\n    fn name() -> u8;\n}\n",
        );
    }

    fn load_with_cargo_home(api: &Path, cargo_home: &Path) -> Result<ApiBundleOwned> {
        let mut scratch = Scratch::default();
        scratch.sources.cargo_home = Some(cargo_home.to_path_buf());
        let manifest = ManifestContext::load(api)?;
        let features = manifest.default_features();
        load_inner(manifest, features, None, &mut scratch)
    }

    fn crate_names(bundle: &ApiBundleOwned) -> Vec<(String, u32)> {
        bundle
            .ext_crates
            .iter()
            .map(|c| (c.crate_id.clone(), c.version.minor.0))
            .collect()
    }

    fn run(cmd: &mut Command) {
        let out = cmd.output().unwrap();
        assert!(
            out.status.success(),
            "{cmd:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// `{ git = .. }`: the member crate of a git workspace, from where a real `cargo fetch` put it (a local repository
    /// and a throwaway `CARGO_HOME`, no network).
    #[test]
    fn git_dependency() {
        let dir = scratch_dir("git");
        let repo = dir.join("dep_repo");
        write(
            &repo.join("Cargo.toml"),
            "[workspace]\nmembers = [\"dep_api\"]\n\n[workspace.package]\nversion = \"0.3.0\"\n",
        );
        write_dep_api(&repo.join("dep_api"), "{ workspace = true }");
        let git = |args: &[&str]| {
            run(Command::new("git")
                .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c"])
                .args(["commit.gpgsign=false", "-c", "init.defaultBranch=main"])
                .args(args)
                .current_dir(&repo)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1"))
        };
        git(&["init", "-q"]);
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "dep_api"]);

        let api = dir.join("api");
        let url = format!("file://{}", repo.display());
        write_api_crate(&api, &format!("{{ git = \"{url}\" }}"));
        let cargo_home = dir.join("cargo_home");
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let err = load_with_cargo_home(&api, &cargo_home).unwrap_err();
        assert!(format!("{err:#}").contains("no Cargo.lock"), "{err:#}");
        for args in [["generate-lockfile", "-q"], ["fetch", "-q"]] {
            run(Command::new(&cargo)
                .args(args)
                .current_dir(&api)
                .env("CARGO_HOME", &cargo_home)
                .env("CARGO_NET_GIT_FETCH_WITH_CLI", "false"));
        }

        let bundle = load_with_cargo_home(&api, &cargo_home).unwrap();
        assert_eq!(
            crate_names(&bundle),
            [("api".into(), 1), ("dep_api".into(), 3)]
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    /// `name = "version"`: the exact version Cargo.lock pins, from `registry/src/<index>/`; a clear error when it
    /// hasn't been fetched.
    #[test]
    fn registry_dependency() {
        let dir = scratch_dir("registry");
        let api = dir.join("api");
        write_api_crate(&api, "\"1.2\"");
        let crates_io = "registry+https://github.com/rust-lang/crates.io-index";
        // another package uses an older dep_api, so the name alone doesn't say which one
        write(
            &api.join("Cargo.lock"),
            &format!(
                "version = 4\n\n\
                 [[package]]\nname = \"api\"\nversion = \"0.1.0\"\ndependencies = [\"dep_api 1.2.3\", \"other\"]\n\n\
                 [[package]]\nname = \"dep_api\"\nversion = \"0.9.0\"\nsource = \"{crates_io}\"\n\n\
                 [[package]]\nname = \"dep_api\"\nversion = \"1.2.3\"\nsource = \"{crates_io}\"\n\n\
                 [[package]]\nname = \"other\"\nversion = \"1.0.0\"\nsource = \"{crates_io}\"\n\
                 dependencies = [\"dep_api 0.9.0\"]\n"
            ),
        );
        let cargo_home = dir.join("cargo_home");
        let index = cargo_home.join("registry/src/index.crates.io-1949cf8c6b5b557f");
        write_dep_api(&index.join("dep_api-0.9.0"), "\"0.9.0\"");

        let err = load_with_cargo_home(&api, &cargo_home).unwrap_err();
        let err = format!("{err:#}");
        assert!(
            err.contains("dep_api 1.2.3 not found") && err.contains("cargo fetch"),
            "{err}"
        );

        write_dep_api(&index.join("dep_api-1.2.3"), "\"1.2.3\"");
        let bundle = load_with_cargo_home(&api, &cargo_home).unwrap();
        assert_eq!(
            crate_names(&bundle),
            [("api".into(), 1), ("dep_api".into(), 2)]
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    /// A crate that isn't a ww API crate (no `wire_weaver`, no `#[ww_trait]`), re-exported by the API crate: its
    /// types are walked like any other, and code generated in a crate depending only on the API crate names them
    /// through the re-export (API-60).
    #[test]
    fn standalone_crate_through_a_reexport() {
        let dir = scratch_dir("standalone");
        write(
            &dir.join("units/Cargo.toml"),
            "[package]\nname = \"units\"\nversion = \"0.2.0\"\nedition = \"2024\"\n",
        );
        write(
            &dir.join("units/src/lib.rs"),
            "pub mod scale {\n    #[derive_shrink_wrap(sized)]\n    pub struct Milli { pub raw: i32 }\n}\n",
        );
        write(
            &dir.join("api/Cargo.toml"),
            "[package]\nname = \"api\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n\
             [dependencies]\nunits_core = { path = \"../units\", package = \"units\" }\n",
        );
        write(
            &dir.join("api/src/lib.rs"),
            "pub use units_core as units;\n#[ww_api_root]\npub trait Api {\n    fn get() -> units::scale::Milli;\n}\n",
        );
        write(
            &dir.join("server/Cargo.toml"),
            "[package]\nname = \"server\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n\
             [workspace]\n\n[dependencies]\napi = { path = \"../api\" }\n",
        );
        let current = ManifestContext::load(&dir.join("server")).unwrap();
        let mut scratch = Scratch::default();
        let (api, features) = current
            .load_dependent_manifest("api", &current.default_features(), &mut scratch)
            .unwrap();
        let bundle = load_inner(api, features, None, &mut scratch).unwrap();
        assert_eq!(
            crate_names(&bundle),
            [("api".into(), 1), ("units".into(), 2)]
        );
        let paths = CratePaths::find(&current, &bundle, &scratch);
        assert_eq!(
            paths.get("units"),
            Some(&["api".to_string(), "units".to_string()][..])
        );
        assert_eq!(paths.get("api"), None);
        fs::remove_dir_all(&dir).unwrap();
    }

    /// An API crate Cargo fetched itself (`#[ww_trait]` of a dependency): its registry dependencies are the newest
    /// matching versions Cargo has, the publisher's `Cargo.lock` next to it pins what this build may not use.
    #[test]
    fn registry_dependency_of_a_fetched_crate() {
        let dir = scratch_dir("fetched");
        let cargo_home = dir.join("cargo_home");
        let index = cargo_home.join("registry/src/index.crates.io-1949cf8c6b5b557f");
        let api = index.join("api-0.1.0");
        write_api_crate(&api, "\"1.2\"");
        write(
            &api.join("Cargo.lock"),
            "version = 4\n\n[[package]]\nname = \"api\"\nversion = \"0.1.0\"\ndependencies = [\"dep_api\"]\n\n\
             [[package]]\nname = \"dep_api\"\nversion = \"1.2.0\"\n\
             source = \"registry+https://github.com/rust-lang/crates.io-index\"\n",
        );
        for version in ["1.2.3", "1.4.0", "2.0.0"] {
            write_dep_api(
                &index.join(format!("dep_api-{version}")),
                &format!("\"{version}\""),
            );
        }
        let bundle = load_with_cargo_home(&api, &cargo_home).unwrap();
        assert_eq!(
            crate_names(&bundle),
            [("api".into(), 1), ("dep_api".into(), 4)]
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
