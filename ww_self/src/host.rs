//! Where a host keeps what it learns about APIs: the snapshot cache, the definition cache and the APIs codegen and
//! clients download, all under one root.

use std::path::PathBuf;

/// Environment variable naming the root instead of `~/.wire_weaver`: tests and sandboxes point it at a scratch
/// directory, so they neither read nor change what this machine has.
pub const HOME_ENV: &str = "WIRE_WEAVER_HOME";

/// [HOME_ENV] when set and not empty, else `~/.wire_weaver`; `None` without either.
pub fn root() -> Option<PathBuf> {
    match std::env::var_os(HOME_ENV).filter(|v| !v.is_empty()) {
        Some(root) => Some(PathBuf::from(root)),
        None => Some(std::env::home_dir()?.join(".wire_weaver")),
    }
}
