//! Hosts the ww hub and a simulated bench supply in one process, for `wire_weaver_py`'s pytest tests
//! (`examples/bench_supply_py`): no real hardware needed (TOOL-18, SRV-29, same pattern as
//! `tests/hub_pytest_device`).
//!
//! Run directly by a pytest fixture (`WW_RUNTIME_DIR` set to a private temp dir, killed at teardown): every
//! client connects with `connect(hub_device=DEVICE_KEY)`, never starting a hub of its own.

use bench_supply::{DEVICE_KEY, DEVICE_PATH, Server};

fn main() {
    // registered in this process, reached by the hub as `inproc:bench-supply` (ww_hub's device.rs config_for)
    let _device = tests_common::start_device_async(
        DEVICE_PATH,
        Server::new(),
        bench_supply_api::BENCH_SUPPLY_FULL_GID,
        bench_supply_api::BENCH_SUPPLY_WIRE_VERSION,
        tests_common::api!(bench_supply::api_impl),
    );

    let config = ww_hub::HubConfig::from_env().expect("hub config from the environment");
    eprintln!(
        "bench_supply_hub_device: runtime dir {}, device {DEVICE_KEY}",
        config.runtime_dir.display()
    );
    match ww_hub::run_blocking_with(config) {
        Ok(exit) => eprintln!("bench_supply_hub_device: hub exited: {exit:?}"),
        Err(e) => {
            eprintln!("bench_supply_hub_device: {e}");
            std::process::exit(1);
        }
    }
}
