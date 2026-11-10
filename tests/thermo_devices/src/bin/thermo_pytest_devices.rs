//! Hosts the ww hub and simulated firmware of every `thermo_api` line (EVO-28) in one process, for
//! `wire_weaver_py`'s pytest of one scenario against three wire lines: no hardware needed. Python reaches each device
//! through the hub as `inproc:thermo-w1`, `inproc:thermo-w2` and `inproc:thermo-w3`.
//!
//! Run by a pytest fixture with `WW_RUNTIME_DIR` set to a private temp dir, killed at teardown, like
//! `hub_pytest_device`.

fn main() {
    // registered in this process, reached by the hub as `inproc:<path>` (ww_hub's device.rs config_for)
    let _w1 = thermo_devices::w1::start("thermo-w1", thermo_api::wire_v1::THERMO_WIRE_VERSION);
    let _w2 = thermo_devices::w2::start("thermo-w2", thermo_api::wire_v2::THERMO_WIRE_VERSION);
    let _w3 = thermo_devices::w3::start("thermo-w3", thermo_api::wire_v3::THERMO_WIRE_VERSION);

    let config = ww_hub::HubConfig::from_env().expect("hub config from the environment");
    eprintln!(
        "thermo_pytest_devices: runtime dir {}, devices inproc:thermo-w1, -w2, -w3",
        config.runtime_dir.display()
    );
    match ww_hub::run_blocking_with(config) {
        Ok(exit) => eprintln!("thermo_pytest_devices: hub exited: {exit:?}"),
        Err(e) => {
            eprintln!("thermo_pytest_devices: {e}");
            std::process::exit(1);
        }
    }
}
