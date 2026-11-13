//! Finished loads kept across the proc-macro invocations of one compile (EVO-52).
//!
//! `#[ww_trait]`, `#[ww_api_root]` and `ww_codegen!` each load an API, and a crate with several of them walks the
//! same crates again in every invocation. rustc loads the proc-macro library once for a crate's whole compile, and
//! rust-analyzer's proc-macro server keeps it loaded for as long as it runs, so a static of this library outlives
//! one invocation. Parsed source can't be kept there: its spans are valid for one invocation only. A finished load
//! has no spans, so it is kept with the files and directories its walk read ([Reads]), and used again only while
//! all of them are unchanged.

use super::stamps::{self, Reads};
use anyhow::Result;
use std::any::Any;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

/// What a load is asked: which entry point, from which crate, for which root or dependency.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum MacroLoad {
    /// [load_with_hints](super::load_with_hints) of a crate's API root or trait (`#[ww_trait]`).
    Api {
        crate_path: PathBuf,
        root: Option<String>,
    },
    /// [load_own_with_paths](super::load_own_with_paths) (`ww_codegen!(crate :: ..)`).
    Own {
        crate_path: PathBuf,
        root: Option<String>,
    },
    /// [load_dep_with_paths](super::load_dep_with_paths) of `dep` from the crate being compiled.
    Dep {
        current: PathBuf,
        dep: String,
        root: Option<String>,
    },
}

impl MacroLoad {
    /// The path as a key: the same crate however it is named.
    pub(crate) fn path(path: &Path) -> PathBuf {
        path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
    }
}

struct Kept {
    value: Arc<dyn Any + Send + Sync>,
    reads: Reads,
}

/// Loads kept while what they read is unchanged.
#[derive(Default)]
pub(crate) struct KeptLoads {
    loads: Mutex<HashMap<MacroLoad, Kept>>,
    /// Walks run, for tests to tell a load kept from a load walked.
    #[cfg(test)]
    walks: std::sync::atomic::AtomicUsize,
}

/// The loads of this proc-macro library, for as long as it is loaded.
static IN_MACROS: LazyLock<KeptLoads> = LazyLock::new(KeptLoads::default);

/// `load` as asked by `key`: inside a proc macro the one kept from an earlier invocation while what it read is
/// unchanged, else walked (and kept when it succeeds). Outside a proc macro always walked: tools keep their loads in
/// a [CrateCache](super::CrateCache).
pub(crate) fn load_in_macro<T: Clone + Send + Sync + 'static>(
    key: MacroLoad,
    load: impl FnOnce() -> Result<T>,
) -> Result<T> {
    if proc_macro::is_available() {
        IN_MACROS.get_or_load(key, load)
    } else {
        load()
    }
}

impl KeptLoads {
    pub(crate) fn get_or_load<T: Clone + Send + Sync + 'static>(
        &self,
        key: MacroLoad,
        load: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        if let Ok(loads) = self.loads.lock()
            && let Some(kept) = loads.get(&key)
            && let Some(value) = kept.value.downcast_ref::<T>()
            && kept.reads.unchanged()
        {
            return Ok(value.clone());
        }
        #[cfg(test)]
        self.walks
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // not holding the lock while walking: another thread's invocation may load something else meanwhile
        let (loaded, reads) = stamps::record(load);
        let value = loaded?;
        if let Ok(mut loads) = self.loads.lock() {
            let kept = Kept {
                value: Arc::new(value.clone()),
                reads,
            };
            loads.insert(key, kept);
        }
        Ok(value)
    }

    #[cfg(test)]
    pub(crate) fn walks(&self) -> usize {
        self.walks.load(std::sync::atomic::Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transform::crate_walker::load_with_hints_walked;

    /// A load is kept for the next invocation while the crate is unchanged, and walked again after an edit, which
    /// it then sees; another root of the same crate is a load of its own.
    #[test]
    fn kept_until_an_edit() {
        let dir = std::env::temp_dir()
            .join("ww_core_macro_cache_tests")
            .join(format!("edit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"kept_api\"\nversion = \"0.1.0\"\nedition = \"2024\"\npublish = false\n",
        )
        .unwrap();
        let lib_rs = |methods: &str| {
            let src = format!(
                "#[ww_api_root(wire = \"w1.0\")] pub trait Api {{ {methods} }}\n\
                 #[ww_trait] pub trait Other {{ fn other(); }}\n"
            );
            std::fs::write(dir.join("src/lib.rs"), src).unwrap();
        };
        lib_rs("fn one();");
        let kept = KeptLoads::default();
        let load = |root: &str| {
            let key = MacroLoad::Api {
                crate_path: MacroLoad::path(&dir),
                root: Some(root.to_string()),
            };
            let (bundle, _hints, _unreasoned, _paths) = kept
                .get_or_load(key, || load_with_hints_walked(&dir, Some(root.to_string())))
                .unwrap();
            bundle
                .root
                .items
                .iter()
                .map(|item| item.ident.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(load("Api"), ["one"]);
        assert_eq!(load("Api"), ["one"]);
        assert_eq!(
            kept.walks(),
            1,
            "the second invocation reuses the first one's load"
        );
        assert_eq!(load("Other"), ["other"]);
        assert_eq!(kept.walks(), 2, "another root is walked");

        lib_rs("fn one(); fn two();");
        assert_eq!(load("Api"), ["one", "two"]);
        assert_eq!(kept.walks(), 3, "an edit between invocations walks again");
        assert_eq!(load("Api"), ["one", "two"]);
        assert_eq!(kept.walks(), 3);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
