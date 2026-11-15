//! Hosts the ww hub and the keyed tests' Dissector device (`tests/keyed`, flows created in shuffled order) in one
//! process, for `wire_weaver_py`'s `test_keyed.py` (CLT-72): Python finds flows by key as `inproc:keyed-device`.
//!
//! Run by a pytest fixture with `WW_RUNTIME_DIR` set to a private temp dir, killed at teardown.

/// Device key clients connect to through the hub (`ClientConfig::hub_device`).
pub const DEVICE_KEY: &str = "inproc:keyed-device";
const DEVICE_PATH: &str = "keyed-device";

fn main() {
    // registered in this process, reached by the hub as `inproc:keyed-device` (ww_hub's device.rs config_for)
    let _device = keyed::device::start_blocking(DEVICE_PATH, keyed::device::shuffled_table());

    let config = ww_hub::HubConfig::from_env().expect("hub config from the environment");
    eprintln!(
        "keyed_pytest_device: runtime dir {}, device {DEVICE_KEY}",
        config.runtime_dir.display()
    );
    match ww_hub::run_blocking_with(config) {
        Ok(exit) => eprintln!("keyed_pytest_device: hub exited: {exit:?}"),
        Err(e) => {
            eprintln!("keyed_pytest_device: {e}");
            std::process::exit(1);
        }
    }
}
