//! Embeds the API snapshots of every crate in the workspace's lock (ww_stdlib, ww_global, ww_tap, exact_units), so
//! the wheel understands a device built with them without their sources at hand (EVO-80).

fn main() {
    ww_snapshots::build::workspace_set("stdlib_set.rs")
        .unwrap_or_else(|e| panic!("embedding API snapshots: {e:#}"));
}
