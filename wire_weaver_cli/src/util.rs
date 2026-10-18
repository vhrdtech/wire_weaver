//! Shared helpers across `ww` subcommands that work on a [DynResource] (`observe`, `stream`).

use anyhow::{Context, Result, anyhow, bail};
use wire_weaver_client::DynResource;

/// `path`, or "API root" when empty, for a message.
pub(crate) fn display_path(path: &str) -> &str {
    if path.is_empty() { "API root" } else { path }
}

/// Walk `path` (e.g. `motor` or `bank[0].level`) from `root` by names and indices, the same syntax
/// `DynResource::path()` prints.
pub(crate) fn resolve(root: DynResource, path: &str) -> Result<DynResource> {
    let mut res = root;
    if path.is_empty() {
        return Ok(res);
    }
    for segment in path.split('.') {
        let name_end = segment.find('[').unwrap_or(segment.len());
        let (name, mut rest) = (&segment[..name_end], &segment[name_end..]);
        if !name.is_empty() {
            res = res.child(name)?;
        }
        while let Some(after_bracket) = rest.strip_prefix('[') {
            let close = after_bracket
                .find(']')
                .ok_or_else(|| anyhow!("'{segment}': missing ']'"))?;
            let index: u32 = after_bracket[..close]
                .parse()
                .with_context(|| format!("'{segment}': expected an integer index"))?;
            res = res.index(index)?;
            rest = &after_bracket[close + 1..];
        }
        if !rest.is_empty() {
            bail!("'{segment}': unexpected text after the index");
        }
    }
    Ok(res)
}
