use super::api::convert_api_items;
use super::util::{UseLeaf, collect_docs, use_tree_leaves};
use anyhow::{Context, Result, anyhow};
use cargo_toml::{Dependency, DepsSet, Inheritable, Manifest};
use shrink_wrap::UNib32;
use std::collections::HashMap;
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
/// * Only crates.io and path dependencies are supported.
pub fn load(
    crate_path: &Path,
    trait_name: Option<String>,
    _offline_mode: bool, // TODO: try CARGO_NET_OFFLINE instead
) -> Result<ApiBundleOwned> {
    let mut scratch = Scratch::default();
    load_inner(crate_path, trait_name, &mut scratch)
}

/// Load API definition and all referenced data types for a given dependency of 'current crate'.
/// Intended use is from a build.rs script, where CARGO_MANIFEST_DIR is available and assumed to be 'current crate'.
pub fn load_dep(
    dep_name: impl AsRef<str>,
    trait_name: Option<impl AsRef<str>>,
) -> Result<ApiBundleOwned> {
    let mut scratch = Scratch::default();
    let current_crate_path = std::env::var("CARGO_MANIFEST_DIR").map_err(|_|
        anyhow!("CARGO_MANIFEST_DIR not set, cannot determine current crate path. Consider calling this method from a build.rs script.")
    )?;
    let current_crate_path = PathBuf::from(current_crate_path);
    let current_crate_manifest = ManifestContext::load(&current_crate_path)?;
    let dep_manifest =
        current_crate_manifest.load_dependent_manifest(dep_name.as_ref(), &mut scratch)?;
    scratch
        .manifests
        .insert(current_crate_path, current_crate_manifest);
    let trait_name = trait_name.map(|t| t.as_ref().to_string());
    load_inner(&dep_manifest.crate_path, trait_name, &mut scratch)
}

fn load_inner(
    crate_path: &Path,
    trait_name: Option<String>,
    scratch: &mut Scratch,
) -> Result<ApiBundleOwned> {
    let entry = CrateContext::load(crate_path, scratch)?;
    let trait_name = find_trait_if_none(trait_name, &entry.lib_rs_ast)?;
    let item_trait = get_trait(&entry.lib_rs_ast, &trait_name)?;
    let items = convert_api_items(item_trait, &entry, scratch)?;

    let root = ApiLevelOwned {
        docs: collect_docs(&item_trait.attrs),
        crate_idx: 0.into(),
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
    /// `src/lib.rs`, or a module of the crate when resolving paths through it.
    pub(crate) lib_rs_ast: File,
    /// Directory where files of the child modules of `lib_rs_ast` are.
    mod_dir: PathBuf,
    /// Modules from the crate root to `lib_rs_ast`, each with whether it is `pub`; empty for the crate root.
    mod_path: Vec<ModSegment>,
    // crate_path: PathBuf,
    // lib_rs_source: String,
}

/// A module on the way from the crate root to a [CrateContext].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ModSegment {
    pub(crate) name: String,
    /// `pub mod`, reachable from other crates if the modules before it are.
    pub(crate) public: bool,
}

/// Cargo.toml manifest together with its file path
struct ManifestContext {
    crate_path: PathBuf,
    manifest: Manifest,
}

impl CrateContext {
    pub(crate) fn load(crate_path: &Path, scratch: &mut Scratch) -> Result<Rc<Self>> {
        let manifest = ManifestContext::load(crate_path)?;
        Self::load_from_manifest(manifest, scratch)
    }

    fn load_from_manifest(
        manifest: Rc<ManifestContext>,
        scratch: &mut Scratch,
    ) -> Result<Rc<Self>> {
        let version = manifest.full_version(scratch)?;
        if let Some(crate_cx) = scratch.crates.get(&version) {
            return Ok(crate_cx.clone());
        }
        let lib_rs = load_lib_rs(&manifest.crate_path)?;
        let crate_cx = Self {
            // crate_path: manifest.crate_path.to_path_buf(),
            mod_dir: manifest.crate_path.join("src"),
            manifest,
            version,
            lib_rs_ast: lib_rs,
            mod_path: vec![],
        };
        // the root crate is always first in ext_crates, even when no types are referenced from it; the others are
        // added when a type or trait of theirs is, not when a path only goes through them (`wire_weaver::prelude`)
        if scratch.root_bundle.ext_crates.is_empty() {
            scratch.root_bundle.find_crate_or_create(&crate_cx);
        }
        Ok(Rc::new(crate_cx))
    }

    pub(crate) fn load_dependent_crate(
        &self,
        crate_name: &str,
        scratch: &mut Scratch,
    ) -> Result<Rc<Self>> {
        let dep_manifest = self.manifest.load_dependent_manifest(crate_name, scratch)?;
        Self::load_from_manifest(dep_manifest, scratch)
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
                "crate" if i == 0 => Self::load_from_manifest(cx.manifest.clone(), scratch)?,
                _ => match cx.module(&name)? {
                    Some(module) => Rc::new(module),
                    None if i == 0 => cx.load_dependent_crate(&name, scratch)?,
                    None => {
                        return Err(anyhow!("Module {name} not found").context(cx.err_context()));
                    }
                },
            };
            resolved = Some(next);
        }
        Ok(resolved.unwrap_or_else(|| Rc::new(self.clone())))
    }

    /// Whether `name` is a child module of this crate or module.
    pub(crate) fn has_module(&self, name: &str) -> bool {
        self.lib_rs_ast
            .items
            .iter()
            .any(|item| matches!(item, Item::Mod(item_mod) if item_mod.ident == name))
    }

    /// Child module `name`, either in-line or in its own file.
    fn module(&self, name: &str) -> Result<Option<Self>> {
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
                parse_rs(&path)?
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
        let mut cx = Self::load_from_manifest(self.manifest.clone(), scratch)?;
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
            cx = Rc::new(
                cx.module(&segment.name)?
                    .ok_or_else(|| anyhow!("internal: module {} not found", segment.name))?,
            );
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
        Ok(FullVersionOwned::new(
            package.name,
            VersionOwned::new(
                version.major as u32,
                version.minor as u32,
                version.patch as u32,
            ),
        ))
    }

    fn load_dependent_manifest(
        &self,
        crate_name: &str,
        scratch: &mut Scratch,
    ) -> Result<Rc<ManifestContext>> {
        // e.g. `use wire_weaver::prelude::*;` brings in `ww_version` without a direct dependency on it
        if !self.manifest.dependencies.contains_key(crate_name)
            && WIRE_WEAVER_REEXPORTS.contains(&crate_name)
            && self.manifest.dependencies.contains_key("wire_weaver")
        {
            let wire_weaver =
                self.load_dependent_inner(&self.manifest.dependencies, "wire_weaver", scratch)?;
            return wire_weaver.load_dependent_manifest(crate_name, scratch);
        }
        self.load_dependent_inner(&self.manifest.dependencies, crate_name, scratch)
    }

    fn load_dependent_inner(
        &self,
        deps_set: &DepsSet,
        crate_name: &str,
        scratch: &mut Scratch,
    ) -> Result<Rc<ManifestContext>> {
        let dep = deps_set.get(crate_name).ok_or(
            anyhow!("Dependency not found")
                .context(crate_name.to_owned())
                .context(self.err_context()),
        )?;
        match dep {
            Dependency::Simple(_version) => {
                // crate_name = "version"
                todo!()
            }
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
                manifest.load_dependent_inner(&workspace.dependencies, crate_name, scratch)
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
                    return scratch.get_or_load_manifest(dep_crate_path);
                }
                // crate_name = { git = "" }
                todo!();
                // crate_name = { version = "" }
                // todo!();
            }
        }
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
