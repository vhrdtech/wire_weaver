//! Every `ww_stdlib` crate checked against its own `api_snapshots/` (docs/evolution/checker_tool.md), one test per
//! crate, from this crate rather than each stdlib crate's own `tests/evolution.rs` (TOOL-31): the
//! `wire_weaver_evolution` dev-dependency turns on wide number features in shared deps (`ww_numeric`, `exact_units`)
//! through cargo's feature unification, which broke a narrow feature-set test of `ww_si` or `ww_numeric` run as
//! `cargo test -p <crate> --no-default-features --features ..` when the dependency sat on the crate itself. Kept
//! here instead, where nothing needs those crates narrow. `WW_API_SAVE=1 cargo test -p stdlib_evolution --test
//! evolution <crate>` saves that crate's current version's snapshot (`ww api save ww_stdlib/<crate>` does the
//! same).

use std::path::Path;
use wire_weaver_evolution::Check;

fn check(name: &str) {
    Check::new(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../ww_stdlib")
            .join(name),
    )
    .require_current(true)
    .assert();
}

#[test]
fn ww_adc() {
    check("ww_adc");
}

#[test]
fn ww_block() {
    check("ww_block");
}

#[test]
fn ww_board_info() {
    check("ww_board_info");
}

#[test]
fn ww_can_bus() {
    check("ww_can_bus");
}

#[test]
fn ww_client_server() {
    check("ww_client_server");
}

#[test]
fn ww_counters() {
    check("ww_counters");
}

#[test]
fn ww_date_time() {
    check("ww_date_time");
}

#[test]
fn ww_dfu() {
    check("ww_dfu");
}

#[test]
fn ww_error() {
    check("ww_error");
}

#[test]
fn ww_firmware_info() {
    check("ww_firmware_info");
}

#[test]
fn ww_gpio() {
    check("ww_gpio");
}

#[test]
fn ww_gpio_hl() {
    check("ww_gpio_hl");
}

#[test]
fn ww_i2c() {
    check("ww_i2c");
}

#[test]
fn ww_i2c_hl() {
    check("ww_i2c_hl");
}

#[test]
fn ww_indication() {
    check("ww_indication");
}

#[test]
fn ww_interned() {
    check("ww_interned");
}

#[test]
fn ww_log() {
    check("ww_log");
}

#[test]
fn ww_logic_analyzer() {
    check("ww_logic_analyzer");
}

#[test]
fn ww_memory() {
    check("ww_memory");
}

#[test]
fn ww_numeric() {
    check("ww_numeric");
}

#[test]
fn ww_onewire() {
    check("ww_onewire");
}

#[test]
fn ww_pin_diag() {
    check("ww_pin_diag");
}

#[test]
fn ww_pwm() {
    check("ww_pwm");
}

#[test]
fn ww_recorder() {
    check("ww_recorder");
}

#[test]
fn ww_sd() {
    check("ww_sd");
}

#[test]
fn ww_si() {
    check("ww_si");
}

#[test]
fn ww_signal_generator() {
    check("ww_signal_generator");
}

#[test]
fn ww_spi() {
    check("ww_spi");
}

#[test]
fn ww_timebase() {
    check("ww_timebase");
}

#[test]
fn ww_uart() {
    check("ww_uart");
}

#[test]
fn ww_uid() {
    check("ww_uid");
}

#[test]
fn ww_user_info() {
    check("ww_user_info");
}

#[test]
fn ww_version() {
    check("ww_version");
}
