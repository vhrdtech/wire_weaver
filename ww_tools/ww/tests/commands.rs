//! The commands the installed `ww` has: every one of `wire_weaver_cli` and `ww_std_cli`, in help order.

use std::process::Command;

/// The names in the `Commands:` block of `ww --help`.
fn listed_commands() -> Vec<String> {
    let out = Command::new(env!("CARGO_BIN_EXE_ww"))
        .arg("--help")
        .output()
        .expect("ww --help runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let help = String::from_utf8(out.stdout).expect("help is UTF-8");
    help.lines()
        .skip_while(|l| *l != "Commands:")
        .skip(1)
        .take_while(|l| !l.is_empty())
        .map(|l| {
            l.split_whitespace()
                .next()
                .expect("a command line")
                .to_owned()
        })
        .collect()
}

/// TOOL-36: the generic commands and the standard library's, registered from two crates, make one `ww`.
#[test]
fn help_lists_every_command_in_order() {
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
        "help",
    ];
    assert_eq!(listed_commands(), expected);
}
