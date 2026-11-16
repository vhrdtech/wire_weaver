fn main() -> anyhow::Result<()> {
    wire_weaver_cli::main(wire_weaver_cli::Program {
        name: "ww",
        version: env!("WW_VERSION"),
    })
}
