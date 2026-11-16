//! The root command line: the global flags (device selection, link tap) and the registered subcommands
//! ([`crate::registry`]), with the global flags fitted to each subcommand's help.

use crate::cmd::tap::TapFlags;
use crate::device::DeviceArgs;
use crate::registry::{self, CommandDef, GlobalFlags};
use clap::{Args, Command};
use std::ffi::OsString;

/// The command line, every registered command as a subcommand and any other name a plugin, with the global flags
/// fitted to each subcommand's help. A subcommand shows only the groups it uses; in its `--help` they are one
/// summary line per group, with their help in `-h` and in `ww --help`. Clap's long layout gives every flag two
/// lines or more plus a blank one, so the 15 global flags took ~60 lines of every subcommand's `--help`, even
/// `ww udev --help`.
///
/// Clap copies a global flag into a subcommand only when the subcommand doesn't have it yet, so the fitted copies
/// added here are what the subcommands (and theirs) get; they stay global and parse as before.
pub fn root_command() -> Command {
    let program = registry::program();
    let root = Command::new(program.name)
        .version(program.version)
        .about("WireWeaver command line tool")
        .propagate_version(true)
        .color(clap::ColorChoice::Auto)
        .styles(
            clap::builder::styling::Styles::styled()
                .header(clap::builder::styling::AnsiColor::Blue.on_default())
                .usage(clap::builder::styling::AnsiColor::Cyan.on_default())
                .literal(clap::builder::styling::AnsiColor::Yellow.on_default())
                .placeholder(clap::builder::styling::AnsiColor::Blue.on_default()),
        )
        .subcommand_required(true)
        .arg_required_else_help(true)
        // `ww NAME ARGS..`: the plugin `ww-NAME`
        .allow_external_subcommands(true)
        .external_subcommand_value_parser(clap::value_parser!(OsString));
    let root = DeviceArgs::augment_args(root);
    let root = TapFlags::augment_args(root);
    let root = registry::commands()
        .into_iter()
        .fold(root, |root, def| root.subcommand((def.command)()));
    let tap = TapFlags::augment_args(Command::new("tap"));
    let globals: Vec<(clap::Arg, bool)> = root
        .get_arguments()
        .filter(|a| a.is_global_set())
        .map(|a| {
            (
                a.clone(),
                tap.get_arguments().any(|t| t.get_id() == a.get_id()),
            )
        })
        .collect();
    let header = *root.get_styles().get_header();
    root.mut_subcommands(|sc| {
        let def = registry::find(sc.get_name()).expect("every subcommand is a registered command");
        fit_globals(sc, def, "", GlobalFlags::DeviceAndTap, &globals, header)
    })
}

/// Adds the fitted copies of the global flags (`(flag, is a tap flag)`) to a subcommand at `path` (`rp` under
/// `dfu`) and its own subcommands: hidden where it doesn't use them (by the command's table, else as its parent),
/// out of the long help where it does, a summary standing in for them there. A subcommand's own flag of the same
/// name (`ww dfu flash --product`) wins, as in clap's own propagation.
fn fit_globals(
    cmd: Command,
    def: &CommandDef,
    path: &str,
    parent: GlobalFlags,
    globals: &[(clap::Arg, bool)],
    header: clap::builder::styling::Style,
) -> Command {
    let flags = def.globals_of(path, parent);
    let added: Vec<_> = globals
        .iter()
        .filter(|(a, _)| !cmd.get_arguments().any(|own| own.get_id() == a.get_id()))
        .map(|(a, is_tap)| {
            let shown = match flags {
                GlobalFlags::DeviceAndTap => true,
                GlobalFlags::Device => !is_tap,
                GlobalFlags::Neither => false,
            };
            (a.clone(), shown)
        })
        .collect();
    let summary = global_summary(&added, header);
    let cmd = added.into_iter().fold(cmd, |cmd, (a, shown)| {
        cmd.arg(match shown {
            true => a.hide_long_help(true),
            false => a.hide(true),
        })
    });
    let cmd = match summary.is_empty() {
        true => cmd,
        false => cmd.after_long_help(summary),
    };
    cmd.mut_subcommands(|sc| {
        let path = match path {
            "" => sc.get_name().to_string(),
            path => format!("{path} {}", sc.get_name()),
        };
        fit_globals(sc, def, &path, flags, globals, header)
    })
}

/// One line per help heading of the shown global flags, `-s/--serial, --api, ...`, and where their help is.
fn global_summary(globals: &[(clap::Arg, bool)], header: clap::builder::styling::Style) -> String {
    let mut groups: Vec<(&str, Vec<String>)> = Vec::new();
    for (a, _) in globals.iter().filter(|(_, shown)| *shown) {
        let heading = a.get_help_heading().unwrap_or("Options");
        let flag = match (a.get_short(), a.get_long()) {
            (Some(s), Some(l)) => format!("-{s}/--{l}"),
            (None, Some(l)) => format!("--{l}"),
            (Some(s), None) => format!("-{s}"),
            (None, None) => continue,
        };
        match groups.iter_mut().find(|(h, _)| *h == heading) {
            Some((_, flags)) => flags.push(flag),
            None => groups.push((heading, vec![flag])),
        }
    }
    if groups.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for (heading, flags) in groups {
        out += &format!("{header}{heading}:{header:#} {}\n", flags.join(", "));
    }
    out + "Global flags: `-h` shows them with their help, `ww --help` in full."
}

#[cfg(test)]
mod tests {
    use super::*;

    fn help(args: &[&str]) -> String {
        let err = root_command()
            .try_get_matches_from(std::iter::once(&"ww").chain(args))
            .expect_err("help is an early exit");
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
        err.render().to_string()
    }

    /// clap's own checks of the command tree (unique argument ids, ..), which a release build skips.
    #[test]
    fn fitted_command_is_valid() {
        root_command().debug_assert();
    }

    /// Every registered command says which global flags it shows (its `""` entry), and no two take one name.
    #[test]
    fn every_command_says_which_global_flags_it_shows() {
        let mut names = vec![];
        for def in registry::commands() {
            assert!(
                def.globals.iter().any(|(p, _)| p.is_empty()),
                "`ww {}` has no `\"\"` entry in its globals table",
                def.name
            );
            assert!(
                !names.contains(&def.name),
                "`ww {}` registered twice",
                def.name
            );
            names.push(def.name);
        }
    }

    /// The generic commands this crate registers, in help order. The binary's own test adds the standard library's.
    #[test]
    fn generic_commands_are_registered() {
        let names: Vec<&str> = registry::commands().into_iter().map(|c| c.name).collect();
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
            "hub",
            "info",
            "elf",
            "config",
            "settings",
            #[cfg(target_os = "linux")]
            "udev",
        ];
        let generic: Vec<&str> = names
            .iter()
            .copied()
            .filter(|n| expected.contains(n))
            .collect();
        assert_eq!(generic, expected);
    }

    #[test]
    fn global_flags_shown_only_where_used() {
        assert!(!help(&["elf", "--help"]).contains("--serial"));
        assert!(!help(&["elf", "-h"]).contains("--serial"));
        let list = help(&["list", "-h"]);
        assert!(list.contains("--serial") && !list.contains("--monitor"));
        let info = help(&["info", "-h"]);
        assert!(info.contains("[env: WW_SERIAL=]") && info.contains("--monitor"));
    }

    #[test]
    fn long_help_sums_global_flags_up() {
        let info = help(&["info", "--help"]);
        assert!(!info.contains("[env: WW_SERIAL=]"), "{info}");
        assert!(info.contains("-s/--serial, -l/--label"), "{info}");
        assert!(info.contains("--monitor, --monitor-bytes"), "{info}");
        assert!(!help(&["list", "--help"]).contains("--monitor"));
        // in full where they are defined
        assert!(help(&["--help"]).contains("[env: WW_SERIAL=]"));
    }

    /// An unknown command is a plugin: `ww chunky flash f` is `ww-chunky flash f`.
    #[test]
    fn unknown_command_is_a_plugin() {
        let matches = root_command()
            .try_get_matches_from(["ww", "--label", "bench", "chunky", "flash", "-x"])
            .unwrap();
        let (name, sub) = matches.subcommand().unwrap();
        assert_eq!(name, "chunky");
        let args: Vec<&OsString> = sub.get_many::<OsString>("").unwrap().collect();
        assert_eq!(args, ["flash", "-x"]);
    }

    #[test]
    fn hidden_global_flags_still_parse() {
        for args in [
            &["ww", "-s", "x", "elf", "id", "f.elf"][..],
            &["ww", "elf", "id", "f.elf", "-s", "x"],
        ] {
            let matches = root_command().try_get_matches_from(args).unwrap();
            assert!(matches.get_one::<String>("serial").is_some());
        }
    }
}
