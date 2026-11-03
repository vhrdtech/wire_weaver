//! Instructions per request of a server (TEST-30): builds `mcu_qemu`'s `qemu-dispatch-requests` (the server of
//! `flash_budget_api::BaseOnly`, the same as `qemu-flash-base-only`) for the Cortex-M3 at the shipped profiles,
//! sync and `use_async`, runs it under `qemu_icount` and prints the instructions each request kind takes from
//! `process_request_bytes` to its reply, and each reply's length and hash (a dispatcher change that changes a
//! reply shows here; TEST-31 is the proof).

use crate::{Filter, Res, cargo, parse_icount, qemu_icount};
use std::path::Path;

const BIN: &str = "qemu-dispatch-requests";
const TARGET: &str = "thumbv7m-none-eabi";
/// The profiles firmware ships with (`mcu_qemu/Cargo.toml`).
const PROFILES: [&str; 2] = ["fw-z", "fw-3"];

pub fn run(root: &Path, filter: &Filter, qemu_dir: &Path, qemu_host: Option<&str>) -> Res<()> {
    for (profile, async_) in filter.pairs(&PROFILES) {
        let mode = if async_ { "use_async" } else { "sync" };
        eprintln!("building {BIN} for {TARGET} at {profile}, {mode}");
        let mut cmd = cargo(qemu_dir);
        cmd.args([
            "build",
            "--profile",
            profile,
            "--target",
            TARGET,
            "--bin",
            BIN,
        ]);
        if async_ {
            cmd.args(["--features", "flash-async"]);
        }
        if !cmd.status()?.success() {
            return Err(format!("cargo build of {BIN} failed").into());
        }
        let elf = qemu_dir.join("target").join(TARGET).join(profile).join(BIN);
        eprintln!("running {BIN} under QEMU");
        let text = qemu_icount(root, &elf, qemu_host)?;
        let costs = parse_icount(&text)?;
        println!(
            "\n{TARGET} (Cortex-M3 under QEMU), {profile}, {mode}: instructions per request, warm run (cold in brackets)"
        );
        for c in &costs {
            let reply = text
                .lines()
                .find_map(|l| l.strip_prefix(&format!("{} | ", c.label)))
                .and_then(|l| l.split_once(" reply=").map(|(head, _)| head))
                .unwrap_or("");
            println!(
                "  {:<34} {:>6} ({:>6})  {reply}",
                c.label,
                c.warm / c.ops,
                c.cold / c.ops
            );
        }
    }
    Ok(())
}
