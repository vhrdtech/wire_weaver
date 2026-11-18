//! `ww` with the generic commands only, for this repository's own recipes (`just save-snapshots`,
//! `just refresh-fixture-pins`): it builds with the root workspace alone, never needs the `ww` of vhrdtech/ww_tools,
//! and is never installed.
//!
//! ```sh
//! cargo run -p wire_weaver_cli --example ww_generic -- api save ww_global
//! ```

fn main() -> anyhow::Result<()> {
    wire_weaver_cli::main(wire_weaver_cli::Program {
        name: "ww",
        version: env!("CARGO_PKG_VERSION"),
        // records nothing: eventfull's capture host is ww_tools' `ww`'s
        hub: Default::default(),
    })
}
