use super::{api::convert_api_items, util::collect_docs};
use anyhow::{anyhow, Context, Result};
use cargo_toml::{Dependency, DepsSet, Inheritable, Manifest};
use semver::Version;
use shrink_wrap::UNib32;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use syn::{File, Item};
use ww_self::{ApiBundleOwned, ApiLevelLocationOwned, ApiLevelOwned, TypeLocationOwned, TypeOwned};
use ww_version::{FullVersionOwned, VersionOwned};

/// Load API definition and all referenced data types.
///
/// This method will recursively walk (and download if necessary) all the referenced crates:
/// 1. Load `crate_path/Cargo.toml` and parse into cargo_toml::[Manifest].
/// 2. Load `crate_path/src/lib.rs` and parse into syn::[File].
/// 3. Find the trait marked with `#[ww_trait]` and named `trait_name`.
///     3.1 If provided, otherwise the first and only `#[ww_api_root]` or `#[ww_trait]` is used or an error is returned.
/// 4. For each user-defined type referenced, find its definition:
///     4.1 Defined in the same file
///     4.2 Imported using `use another_crate::Ty`
///         4.2.1 Load `another_crate` starting from step #1, skipping #3.
///     4.3 Convert into [ww_self::Type]
/// 5. For each trait referenced via `ww_impl` do similar steps as for types.
/// 6. Assemble all data into [ww_self::ApiBundle]
/// 7. Cache in `~/.wire_weaver/crate_name-sha.ron` if not already.
///
/// If `offline_mode` is true, no attempts to download any crates will be made. Only the ones already
/// found in the local Cargo registry will be used, or an error is returned if they are not found.
///
/// Limitations:
/// * Only types and ww_trait's defined in `src/lib.rs` are supported.
/// * Only crates.io and path dependencies are supported.
pub fn load_v2(
    crate_path: &Path,
    trait_name: Option<String>,
    _offline_mode: bool,
) -> Result<ApiBundleOwned> {
    let mut scratch = Scratch::default();
    let entry = CrateContext::load(crate_path, &mut scratch)?;
    let trait_name = find_trait_if_none(trait_name, &entry.lib_rs_ast)?;
    let item_trait = get_trait(&entry.lib_rs_ast, &trait_name)?;
    let items = convert_api_items(item_trait, &entry, &mut scratch)?;

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
        types: scratch.root_bundle.types,
        traits: scratch.root_bundle.traits,
        ext_crates: scratch.root_bundle.ext_crates,
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
}

/// Scratch space used to hold types and traits
#[derive(Default)]
struct ApiBundleScratch {
    types: Vec<TypeLocationOwned>,
    pub(crate) traits: Vec<ApiLevelLocationOwned>,
    ext_crates: Vec<FullVersionOwned>,
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

    pub(crate) fn push_out_of_line(
        &mut self,
        ty: TypeOwned,
        current_crate: &CrateContext,
    ) -> TypeOwned {
        let type_idx = self.push_out_of_line_idx(ty, current_crate);
        TypeOwned::OutOfLine { type_idx }
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

pub(crate) struct CrateContext {
    manifest: Rc<ManifestContext>,
    version: FullVersionOwned,
    pub(crate) lib_rs_ast: File,
    // crate_path: PathBuf,
    // lib_rs_source: String,
}

/// Cargo.toml manifest together with its file path
struct ManifestContext {
    crate_path: PathBuf,
    manifest: Manifest,
}

impl CrateContext {
    fn load(crate_path: &Path, scratch: &mut Scratch) -> Result<Rc<Self>> {
        let manifest = ManifestContext::load(crate_path)?;
        Self::load_from_manifest(manifest, scratch)
    }

    fn load_from_manifest(
        manifest: Rc<ManifestContext>,
        scratch: &mut Scratch,
    ) -> Result<Rc<Self>> {
        let version = manifest.full_version()?;
        if let Some(crate_cx) = scratch.crates.get(&version) {
            return Ok(crate_cx.clone());
        }
        let lib_rs = load_lib_rs(&manifest.crate_path)?;
        Ok(Rc::new(Self {
            // crate_path: manifest.crate_path.to_path_buf(),
            manifest,
            version,
            lib_rs_ast: lib_rs,
        }))
    }

    pub(crate) fn load_dependent_crate(
        &self,
        crate_name: &String,
        scratch: &mut Scratch,
    ) -> Result<Rc<Self>> {
        let dep_manifest = self.manifest.load_dependent_manifest(crate_name, scratch)?;
        Self::load_from_manifest(dep_manifest, scratch)
    }

    pub(crate) fn err_context(&self) -> String {
        format!("{:?}", self.version)
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
                return Ok(scratch.get_or_load_manifest(possible_path)?);
            }
        }
        Err(anyhow!("not found"))
    }

    fn full_version(&self) -> Result<FullVersionOwned> {
        let package = self
            .manifest
            .package
            .clone()
            .context("No package section found in Cargo.toml")?;
        let Inheritable::Set(package_version) = &package.version else {
            return Err(anyhow!("No version found in Cargo.toml"));
        };
        let version = Version::parse(package_version).context(format!(
            "Failed to parse version in {}/Cargo.toml",
            self.crate_path.display()
        ))?;
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
        crate_name: &String,
        scratch: &mut Scratch,
    ) -> Result<Rc<ManifestContext>> {
        self.load_dependent_inner(&self.manifest.dependencies, crate_name, scratch)
    }

    fn load_dependent_inner(
        &self,
        deps_set: &DepsSet,
        crate_name: &String,
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
                    .ok_or(anyhow!("parent Cargo.toml is not a workspace"))?;
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
                    return Ok(scratch.get_or_load_manifest(dep_crate_path)?);
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
    let mut lib_rs_path = crate_path.to_path_buf();
    lib_rs_path.push("src");
    lib_rs_path.push("lib.rs");
    let contents = fs::read_to_string(&lib_rs_path).context(format!(
        "Failed to read lib.rs from {}",
        lib_rs_path.display()
    ))?;
    syn::parse_file(&contents).context(format!(
        "Failed to parse lib.rs from {}",
        lib_rs_path.display()
    ))
}

fn find_trait_if_none(trait_name: Option<String>, lib_rs: &File) -> Result<String> {
    if let Some(name) = trait_name {
        return Ok(name);
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
