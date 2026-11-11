//! Every `ww_stdlib` crate checked against its own `api_snapshots/` (docs/evolution/checker_tool.md), one test per
//! crate, from this crate rather than each stdlib crate's own `tests/evolution.rs` (TOOL-31): the
//! `wire_weaver_evolution` dev-dependency turns on wide number features in shared deps (`ww_numeric`, `exact_units`)
//! through cargo's feature unification, which broke a narrow feature-set test of `ww_si` or `ww_numeric` run as
//! `cargo test -p <crate> --no-default-features --features ..` when the dependency sat on the crate itself. Kept
//! here instead, where nothing needs those crates narrow. `WW_API_SAVE=1 cargo test -p stdlib_evolution --test
//! evolution <crate>` saves that crate's current version's snapshot (`ww api save ww_stdlib/<crate>` does the
//! same).

#[test]
fn ww_adc() {
    wire_weaver_evolution::Check::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ww_stdlib/ww_adc"))
        .require_current(true)
        .assert();
}

#[test]
fn ww_block() {
    wire_weaver_evolution::Check::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ww_stdlib/ww_block"))
        .require_current(true)
        .assert();
}

#[test]
fn ww_board_info() {
    wire_weaver_evolution::Check::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ww_stdlib/ww_board_info"))
        .require_current(true)
        .assert();
}

#[test]
fn ww_can_bus() {
    wire_weaver_evolution::Check::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ww_stdlib/ww_can_bus"))
        .require_current(true)
        .assert();
}

#[test]
fn ww_client_server() {
    wire_weaver_evolution::Check::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ww_stdlib/ww_client_server"))
        .require_current(true)
        .assert();
}

#[test]
fn ww_counters() {
    wire_weaver_evolution::Check::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ww_stdlib/ww_counters"))
        .require_current(true)
        .assert();
}

#[test]
fn ww_date_time() {
    wire_weaver_evolution::Check::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ww_stdlib/ww_date_time"))
        .require_current(true)
        .assert();
}

#[test]
fn ww_dfu() {
    wire_weaver_evolution::Check::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ww_stdlib/ww_dfu"))
        .require_current(true)
        .assert();
}

#[test]
fn ww_error() {
    wire_weaver_evolution::Check::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ww_stdlib/ww_error"))
        .require_current(true)
        .assert();
}

#[test]
fn ww_firmware_info() {
    wire_weaver_evolution::Check::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ww_stdlib/ww_firmware_info"))
        .require_current(true)
        .assert();
}

#[test]
fn ww_gpio() {
    wire_weaver_evolution::Check::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ww_stdlib/ww_gpio"))
        .require_current(true)
        .assert();
}

#[test]
fn ww_gpio_hl() {
    wire_weaver_evolution::Check::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ww_stdlib/ww_gpio_hl"))
        .require_current(true)
        .assert();
}

#[test]
fn ww_i2c() {
    wire_weaver_evolution::Check::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ww_stdlib/ww_i2c"))
        .require_current(true)
        .assert();
}

#[test]
fn ww_i2c_hl() {
    wire_weaver_evolution::Check::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ww_stdlib/ww_i2c_hl"))
        .require_current(true)
        .assert();
}

#[test]
fn ww_indication() {
    wire_weaver_evolution::Check::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ww_stdlib/ww_indication"))
        .require_current(true)
        .assert();
}

#[test]
fn ww_interned() {
    wire_weaver_evolution::Check::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ww_stdlib/ww_interned"))
        .require_current(true)
        .assert();
}

#[test]
fn ww_log() {
    wire_weaver_evolution::Check::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ww_stdlib/ww_log"))
        .require_current(true)
        .assert();
}

#[test]
fn ww_logic_analyzer() {
    wire_weaver_evolution::Check::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ww_stdlib/ww_logic_analyzer"))
        .require_current(true)
        .assert();
}

#[test]
fn ww_memory() {
    wire_weaver_evolution::Check::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ww_stdlib/ww_memory"))
        .require_current(true)
        .assert();
}

#[test]
fn ww_numeric() {
    wire_weaver_evolution::Check::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ww_stdlib/ww_numeric"))
        .require_current(true)
        .assert();
}

#[test]
fn ww_onewire() {
    wire_weaver_evolution::Check::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ww_stdlib/ww_onewire"))
        .require_current(true)
        .assert();
}

#[test]
fn ww_pin_diag() {
    wire_weaver_evolution::Check::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ww_stdlib/ww_pin_diag"))
        .require_current(true)
        .assert();
}

#[test]
fn ww_pwm() {
    wire_weaver_evolution::Check::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ww_stdlib/ww_pwm"))
        .require_current(true)
        .assert();
}

#[test]
fn ww_recorder() {
    wire_weaver_evolution::Check::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ww_stdlib/ww_recorder"))
        .require_current(true)
        .assert();
}

#[test]
fn ww_sd() {
    wire_weaver_evolution::Check::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ww_stdlib/ww_sd"))
        .require_current(true)
        .assert();
}

#[test]
fn ww_si() {
    wire_weaver_evolution::Check::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ww_stdlib/ww_si"))
        .require_current(true)
        .assert();
}

#[test]
fn ww_signal_generator() {
    wire_weaver_evolution::Check::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ww_stdlib/ww_signal_generator"))
        .require_current(true)
        .assert();
}

#[test]
fn ww_spi() {
    wire_weaver_evolution::Check::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ww_stdlib/ww_spi"))
        .require_current(true)
        .assert();
}

#[test]
fn ww_timebase() {
    wire_weaver_evolution::Check::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ww_stdlib/ww_timebase"))
        .require_current(true)
        .assert();
}

#[test]
fn ww_uart() {
    wire_weaver_evolution::Check::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ww_stdlib/ww_uart"))
        .require_current(true)
        .assert();
}

#[test]
fn ww_uid() {
    wire_weaver_evolution::Check::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ww_stdlib/ww_uid"))
        .require_current(true)
        .assert();
}

#[test]
fn ww_user_info() {
    wire_weaver_evolution::Check::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ww_stdlib/ww_user_info"))
        .require_current(true)
        .assert();
}

#[test]
fn ww_version() {
    wire_weaver_evolution::Check::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ww_stdlib/ww_version"))
        .require_current(true)
        .assert();
}
