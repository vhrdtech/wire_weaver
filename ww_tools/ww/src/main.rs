// the standard library commands register themselves when their crate is linked
use ww_std_cli as _;

fn main() -> anyhow::Result<()> {
    wire_weaver_cli::main(wire_weaver_cli::Program {
        name: "ww",
        version: env!("WW_VERSION"),
    })
}
