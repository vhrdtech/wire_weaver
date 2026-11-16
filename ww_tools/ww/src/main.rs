// the standard library commands register themselves when their crate is linked
use ww_std_cli as _;

/// The API snapshots of every crate in the build's lock (ww_stdlib, ww_global, ww_tap, exact_units): a device built
/// with them is understood with nothing on disk and no network (EVO-80).
static STDLIB: ww_snapshots::Set = include!(concat!(env!("OUT_DIR"), "/stdlib_set.rs"));

fn main() -> anyhow::Result<()> {
    ww_snapshots::embed(&STDLIB);
    wire_weaver_cli::main(wire_weaver_cli::Program {
        name: "ww",
        version: env!("WW_VERSION"),
    })
}
