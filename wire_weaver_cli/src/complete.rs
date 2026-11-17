//! Shell completions, produced by the `ww` binary itself when the shell calls it with `COMPLETE=<shell>` set
//! (see [CompleteEnv]).
//!
//! Values of device selection flags are taken from connected devices, listed live without opening them.
//! Bash uses its own adapter (`Bash`), other shells use the clap_complete ones.

use clap::Command;
use clap_complete::CompleteEnv;
use clap_complete::CompletionCandidate;
use clap_complete::env::{Elvish, EnvCompleter, Fish, Powershell, Shells, Zsh};
use std::ffi::OsString;
use std::io::Write;
use std::path::Path;
pub use ww_plugin::complete::{
    apis, candidates, device_keys, labels, manufacturers, probes, products, rtt_chips, serials,
    serials_and_labels, usb_paths, vid_pids,
};

/// The command line for completion: `factory`'s, one subcommand per plugin found on `PATH` (`ww-NAME`), and the
/// subcommands that forward to another command ([`crate::registry::Forward`]: `dfu flash`, which `chunky flash`
/// answers). A forward to a registered command completes as that command does; a plugin completes its own arguments: it is asked for the words of the command line being completed,
/// and what it answers is put into the command as its flags and values, which the completion engine then filters
/// by the word being completed.
pub fn with_plugins(factory: impl Fn() -> Command) -> Command {
    let mut cmd = factory();
    // the global flags are the root's: a plugin offering them would be a second flag of the same name
    let taken: Vec<String> = cmd
        .get_arguments()
        .filter_map(|a| a.get_long().map(str::to_string))
        .chain(["help".into(), "version".into()])
        .collect();
    for def in crate::registry::commands() {
        for forward in def.forwards {
            let prefix: Vec<&str> = std::iter::once(def.name)
                .chain(forward.path.iter().copied())
                .collect();
            cmd = at_path(cmd, &prefix, |stub| match forward.registered() {
                Some(target) => registered_stub(stub, def.name_at(forward), target, forward.lead),
                None => plugin_stub(stub, &prefix, forward.plugin, forward.lead, &taken),
            });
        }
    }
    for (name, path) in crate::plugin::list() {
        if cmd.find_subcommand(&name).is_some() {
            continue;
        }
        // a completion run is short, the name lives until it ends
        let name: &'static str = Box::leak(name.into_boxed_str());
        let stub = Command::new(name)
            .about(format!("plugin {}", path.display()))
            .disable_help_flag(true);
        cmd = cmd.subcommand(plugin_stub(stub, &[name], name, &[], &taken));
    }
    cmd
}

/// `cmd` with the subcommand at `path` (`["dfu", "flash"]`) changed by `f`.
fn at_path(cmd: Command, path: &[&str], f: impl FnOnce(Command) -> Command) -> Command {
    match path {
        [] => f(cmd),
        [first, rest @ ..] => cmd.mut_subcommand(*first, |sc| at_path(sc, rest, f)),
    }
}

/// The subcommand of the registered command `target` at `lead` (`chunky flash`), in place of `stub` (`dfu flash`):
/// its arguments and help, under the stub's name `name`.
fn registered_stub(
    stub: Command,
    name: &'static str,
    target: &crate::registry::CommandDef,
    lead: &[&str],
) -> Command {
    let mut found = (target.command)();
    for word in lead {
        let Some(sub) = found.find_subcommand(word) else {
            return stub;
        };
        found = sub.clone();
    }
    found.name(name)
}

/// `stub` (the command a plugin answers for), with the flags and values `plugin` offers for the command line being
/// completed, when it is the one that line is in.
fn plugin_stub(
    stub: Command,
    prefix: &[&str],
    plugin: &str,
    lead: &[&str],
    taken: &[String],
) -> Command {
    let mut stub = stub;
    if stub.get_arguments().all(|a| a.get_id() != "args") {
        stub = stub.arg(
            clap::Arg::new("args")
                .num_args(0..)
                .trailing_var_arg(true)
                .allow_hyphen_values(true),
        );
    }
    let Some(words) = words_after(prefix) else {
        return stub;
    };
    // the plugin's own command line starts with the subcommand the stub stands for
    let words: Vec<std::ffi::OsString> = lead
        .iter()
        .map(std::ffi::OsString::from)
        .chain(words)
        .collect();
    let mut values = vec![];
    for c in crate::plugin::complete_words(plugin, &words) {
        let value = c.get_value().to_string_lossy().into_owned();
        let help = c.get_help().cloned();
        match value.strip_prefix("--") {
            Some(long) if !long.is_empty() && !taken.iter().any(|t| t == long) => {
                // names live until the completion run ends
                let long: &'static str = Box::leak(long.to_string().into_boxed_str());
                let mut arg = clap::Arg::new(long)
                    .long(long)
                    .action(clap::ArgAction::SetTrue);
                if let Some(help) = help {
                    arg = arg.help(help);
                }
                stub = stub.arg(arg);
            }
            Some(_) => {}
            _ => {
                let value: &'static str = Box::leak(value.into_boxed_str());
                let mut possible = clap::builder::PossibleValue::new(value);
                if let Some(help) = help {
                    possible = possible.help(help);
                }
                values.push(possible);
            }
        }
    }
    if !values.is_empty() {
        stub = stub.mut_arg("args", |a| a.value_parser(values));
    }
    stub
}

/// The words of the command line being completed after `prefix` (the word being completed last, empty if there is
/// none yet), `None` when the command line is not in `prefix`'s command. The command line is in the environment
/// bash's adapter passes (`Bash`) or after `--` in the arguments of the other shells.
fn words_after(prefix: &[&str]) -> Option<Vec<std::ffi::OsString>> {
    let words: Vec<String> = match std::env::var("_WW_COMP_LINE") {
        Ok(line) => {
            let wordbreaks = std::env::var("_WW_COMP_WORDBREAKS").unwrap_or_default();
            split_line(&line, &wordbreaks).0
        }
        Err(_) => std::env::args().skip_while(|a| a != "--").skip(2).collect(),
    };
    let at = words
        .windows(prefix.len())
        .position(|w| w.iter().map(String::as_str).eq(prefix.iter().copied()))?;
    let mut rest: Vec<std::ffi::OsString> = words[at + prefix.len()..]
        .iter()
        .map(std::ffi::OsString::from)
        .collect();
    if rest.is_empty() {
        rest.push(std::ffi::OsString::new());
    }
    Some(rest)
}

/// Answers a shell completion request and exits if there is one, otherwise does nothing.
pub fn complete(factory: impl Fn() -> Command) {
    CompleteEnv::with_factory(factory)
        .shells(Shells(&[&Bash, &Elvish, &Fish, &Powershell, &Zsh]))
        .complete();
}

/// Bash adapter that quotes completions.
///
/// Bash inserts completions verbatim and splits the word being completed on `COMP_WORDBREAKS` (quotes, `@`, `:`,
/// `=`, ...), while clap_complete's adapter passes neither quoting nor these splits on, so values with spaces
/// broke into several words and values like `--api name@^0.1` could not be completed. Here the raw command line is
/// split by `ww` instead, and each candidate is turned into the text bash should put in place of its current word:
/// only the part after the last word break, escaped for an already open quote or single-quoted when needed.
struct Bash;

impl EnvCompleter for Bash {
    fn name(&self) -> &'static str {
        "bash"
    }

    fn is(&self, name: &str) -> bool {
        name == "bash"
    }

    fn write_registration(
        &self,
        var: &str,
        name: &str,
        bin: &str,
        completer: &str,
        buf: &mut dyn Write,
    ) -> Result<(), std::io::Error> {
        let script = r#"
_ww_complete_NAME() {
    local IFS=$'\013'
    if compopt +o nospace 2> /dev/null; then
        local space=false
    else
        local space=true
    fi
    COMPREPLY=( $( \
        _WW_COMP_LINE="${COMP_LINE:0:COMP_POINT}" \
        _WW_COMP_WORDBREAKS="$COMP_WORDBREAKS" \
        _CLAP_IFS="$IFS" \
        VAR="bash" \
        'COMPLETER' -- "$1" \
    ) )
    if [[ $? != 0 ]]; then
        unset COMPREPLY
    elif [[ $space == false ]] && [[ "${COMPREPLY-}" =~ [=/:]\'?$ ]]; then
        compopt -o nospace
    fi
}
if [[ "${BASH_VERSINFO[0]}" -eq 4 && "${BASH_VERSINFO[1]}" -ge 4 || "${BASH_VERSINFO[0]}" -gt 4 ]]; then
    complete -o nospace -o bashdefault -o nosort -F _ww_complete_NAME BIN
else
    complete -o nospace -o bashdefault -F _ww_complete_NAME BIN
fi
"#
        .replace("NAME", &name.replace('-', "_"))
        .replace("BIN", bin)
        .replace("COMPLETER", &completer.replace('\'', r"'\''"))
        .replace("VAR", var);
        writeln!(buf, "{script}")
    }

    fn write_complete(
        &self,
        cmd: &mut Command,
        _args: Vec<OsString>,
        current_dir: Option<&Path>,
        buf: &mut dyn Write,
    ) -> Result<(), std::io::Error> {
        let line = std::env::var("_WW_COMP_LINE").unwrap_or_default();
        let wordbreaks = std::env::var("_WW_COMP_WORDBREAKS").unwrap_or_default();
        let ifs = std::env::var("_CLAP_IFS").unwrap_or_else(|_| "\n".into());
        let (words, current) = split_line(&line, &wordbreaks);
        let index = words.len() - 1;
        let args = words.into_iter().map(OsString::from).collect();
        let completions = clap_complete::engine::complete(cmd, args, index, current_dir)?;
        let replies: Vec<_> = completions
            .iter()
            .filter_map(|c| current.reply(&c.get_value().to_string_lossy()))
            .collect();
        write!(buf, "{}", replies.join(&ifs))
    }
}

/// The word being completed, as seen by bash.
#[derive(Debug, PartialEq)]
struct CurrentWord {
    /// The word without quotes and escapes.
    value: String,
    /// Quote left open, if any.
    open_quote: Option<char>,
    /// Length of the start of `value` that bash keeps: before the open quote or the last word break.
    kept: usize,
}

/// Splits a command line (up to the cursor) into shell words, the last one being the word being completed.
fn split_line(line: &str, wordbreaks: &str) -> (Vec<String>, CurrentWord) {
    let mut words = vec![];
    let mut word = String::new();
    let mut in_word = false;
    let mut quote = None;
    let mut kept = 0;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => {
                quote = None;
                // bash treats the closing quote as a word break
                kept = word.len();
            }
            (Some('"'), '\\') => match chars.next() {
                Some(e @ ('"' | '\\' | '$' | '`')) => word.push(e),
                Some(e) => {
                    word.push('\\');
                    word.push(e);
                }
                None => word.push('\\'),
            },
            (Some(_), c) => word.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                in_word = true;
                kept = word.len();
            }
            (None, '\\') => {
                in_word = true;
                if let Some(e) = chars.next() {
                    word.push(e);
                }
            }
            (None, c) if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
                kept = 0;
            }
            (None, c) => {
                in_word = true;
                if wordbreaks.contains(c) {
                    // bash keeps `@` and `$` (its rl_special_prefixes) as the start of the next word
                    kept = word.len()
                        + if matches!(c, '@' | '$') {
                            0
                        } else {
                            c.len_utf8()
                        };
                }
                word.push(c);
            }
        }
    }
    words.push(word.clone());
    let current = CurrentWord {
        value: word,
        open_quote: quote,
        kept,
    };
    (words, current)
}

impl CurrentWord {
    /// Text to put in place of bash's current word to complete `candidate`, `None` if it doesn't continue the word.
    fn reply(&self, candidate: &str) -> Option<String> {
        let rest = candidate.strip_prefix(&self.value[..self.kept])?;
        Some(match self.open_quote {
            Some('"') => rest
                .chars()
                .flat_map(|c| match c {
                    '"' | '\\' | '$' | '`' => vec!['\\', c],
                    c => vec![c],
                })
                .collect(),
            Some(_) => rest.replace('\'', r"'\''"),
            None => quote(rest),
        })
    }
}

/// Single-quotes a value containing characters the shell would interpret (spaces, quotes, `$`, ...).
fn quote(v: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || "-_.,:/@%+=^~".contains(c);
    if v.chars().all(plain) {
        v.to_string()
    } else {
        format!("'{}'", v.replace('\'', r"'\''"))
    }
}

/// `ww usb-loopback --packet-size`: `max` or a few common sizes.
pub fn packet_sizes() -> Vec<CompletionCandidate> {
    [
        ("max", "largest message both the device and the host accept"),
        ("64", "small messages, rate of messages counts"),
        ("512", "medium messages"),
        ("1024", "large messages, rate of bytes counts"),
    ]
    .into_iter()
    .map(|(v, help)| CompletionCandidate::new(v).help(Some(help.into())))
    .collect()
}

/// `ww settings dump --word-size`: the flash word sizes `ww_settings::Store` supports, with a chip example each.
pub fn settings_word_sizes() -> Vec<CompletionCandidate> {
    [
        ("1", "no ECC, e.g. SPI NOR flash (W25Q128JV)"),
        ("4", "STM32F4"),
        ("8", "STM32G0"),
        ("16", ""),
        ("32", "STM32H7"),
    ]
    .into_iter()
    .map(|(v, help)| CompletionCandidate::new(v).help((!help.is_empty()).then(|| help.into())))
    .collect()
}

/// `ww settings dump --page-size`: the flash page (erase unit) sizes `ww_settings::Store` supports.
pub fn settings_page_sizes() -> Vec<CompletionCandidate> {
    [
        ("2048", "e.g. STM32H7A3/7B3"),
        ("4096", ""),
        ("8192", "e.g. STM32G0B1"),
        ("16384", "e.g. STM32F405/407 small sectors"),
        ("131072", "e.g. STM32F4/H7 large sectors"),
    ]
    .into_iter()
    .map(|(v, help)| CompletionCandidate::new(v).help((!help.is_empty()).then(|| help.into())))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target_command() -> Command {
        Command::new("chunky").subcommand(
            Command::new("flash")
                .about("Update a chunky device")
                .arg(clap::Arg::new("fw_version").long("fw-version")),
        )
    }

    fn target_run(
        _: crate::registry::Run<'_>,
    ) -> crate::registry::BoxFuture<'_, anyhow::Result<crate::registry::Ran>> {
        Box::pin(async { Ok(crate::registry::Ran::DONE) })
    }

    /// A forward to a registered command completes with that command's subcommand, under the forward's name.
    #[test]
    fn a_registered_forward_completes_as_its_target() {
        let target = crate::registry::CommandDef::new("chunky", 0, target_command, target_run);
        let stub = registered_stub(Command::new("flash"), "flash", &target, &["flash"]);
        assert_eq!(stub.get_name(), "flash");
        assert!(
            stub.get_arguments()
                .any(|a| a.get_long() == Some("fw-version"))
        );
        // a lead the target doesn't have leaves the stub as it was
        let stub = registered_stub(Command::new("flash"), "flash", &target, &["nope"]);
        assert!(stub.get_arguments().next().is_none());
    }

    const WORDBREAKS: &str = "\"'><=;|&(:@";

    fn reply(line: &str, candidate: &str) -> Option<String> {
        split_line(line, WORDBREAKS).1.reply(candidate)
    }

    #[test]
    fn splits_words() {
        let (words, current) = split_line(r#"ww  --label 'a b' "c\"d" e\ f g"#, WORDBREAKS);
        assert_eq!(words, ["ww", "--label", "a b", "c\"d", "e f", "g"]);
        assert_eq!(current.value, "g");
        let (words, current) = split_line("ww --label ", WORDBREAKS);
        assert_eq!(words, ["ww", "--label", ""]);
        assert_eq!(current.value, "");
    }

    #[test]
    fn quotes_values_needing_it() {
        assert_eq!(
            reply("ww --label N", "Nucleo on the desk").unwrap(),
            "'Nucleo on the desk'"
        );
        assert_eq!(reply("ww --label ", "it's").unwrap(), r"'it'\''s'");
        assert_eq!(reply("ww --serial 21", "2100AB").unwrap(), "2100AB");
    }

    #[test]
    fn escapes_for_open_quote() {
        assert_eq!(
            reply("ww --label 'Nu", "Nucleo on the desk").unwrap(),
            "Nucleo on the desk"
        );
        assert_eq!(
            reply(r#"ww --label "Nu"#, r#"Nu "x" $y"#).unwrap(),
            r#"Nu \"x\" \$y"#
        );
        assert_eq!(
            reply("ww --label 'a b' --label 'it", "it's").unwrap(),
            r"it'\''s"
        );
    }

    #[test]
    fn replaces_after_word_break() {
        assert_eq!(
            reply("ww --api blinky_api@", "blinky_api@^0.1.0").unwrap(),
            "@^0.1.0"
        );
        assert_eq!(reply("ww --vid-pid c0de:", "c0de:cafe").unwrap(), "cafe");
        assert_eq!(
            reply("ww --label=Nu", "--label=Nucleo on the desk").unwrap(),
            "'Nucleo on the desk'"
        );
        assert_eq!(reply("ww --api blinky_api@", "other"), None);
    }
}
