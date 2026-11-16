//! The commands a binary linking `wire_weaver_cli` and this crate has.

use ww_std_cli as _;

/// Every command of `ww`, generic and standard library, in help order: nothing missing, nothing extra.
#[test]
fn generic_and_standard_library_commands_are_registered() {
    let names: Vec<&str> = wire_weaver_cli::registry::commands()
        .into_iter()
        .map(|c| c.name)
        .collect();
    let expected = [
        "list",
        "usb-loopback",
        "api",
        "introspect",
        "explain",
        "observe",
        "stream",
        "monitor",
        "stats",
        "capture",
        "sync",
        "dfu",
        "hub",
        "counters",
        "info",
        "report",
        "board",
        "pins",
        "elf",
        "config",
        "settings",
        #[cfg(target_os = "linux")]
        "udev",
    ];
    assert_eq!(names, expected);
}
