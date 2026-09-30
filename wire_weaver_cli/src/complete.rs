//! Shell completions, produced by the `ww` binary itself when the shell calls it with `COMPLETE=<shell>` set
//! (see [CompleteEnv]).
//!
//! Values of device selection flags are taken from connected devices, listed live without opening them.
//! Bash uses its own adapter ([Bash]), other shells use the clap_complete ones.

use clap::Command;
use clap_complete::CompleteEnv;
use clap_complete::CompletionCandidate;
use clap_complete::env::{Elvish, EnvCompleter, Fish, Powershell, Shells, Zsh};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Write;
use std::path::Path;
use wire_weaver_client::DeviceInfo;

/// Answers a shell completion request and exits if there is one, otherwise does nothing.
pub(crate) fn complete(factory: fn() -> Command) {
    CompleteEnv::with_factory(factory)
        .shells(Shells(&[&Bash, &Elvish, &Fish, &Powershell, &Zsh]))
        .complete();
}

/// Devices reporting a WireWeaver API id, empty on any error (completion must never fail loudly).
fn connected_devices() -> Vec<DeviceInfo> {
    let Ok(rt) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return vec![];
    };
    rt.block_on(wire_weaver_client::list_usb_devices())
        .unwrap_or_default()
}

/// One candidate per distinct non-empty value, with the product (and label) of the devices it came from as help.
fn candidates(value: impl Fn(&DeviceInfo) -> Vec<String>) -> Vec<CompletionCandidate> {
    let mut values: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for device in connected_devices() {
        let help = if device.user_label.is_empty() {
            device.product.clone()
        } else {
            format!("{} ({})", device.product, device.user_label)
        };
        for v in value(&device).into_iter().filter(|v| !v.is_empty()) {
            let helps = values.entry(v).or_default();
            if !helps.contains(&help) {
                helps.push(help.clone());
            }
        }
    }
    values
        .into_iter()
        .map(|(v, helps)| CompletionCandidate::new(v).help(Some(helps.join(", ").into())))
        .collect()
}

pub(crate) fn serials() -> Vec<CompletionCandidate> {
    candidates(|d| d.serials.clone())
}

pub(crate) fn labels() -> Vec<CompletionCandidate> {
    candidates(|d| vec![d.user_label.clone()])
}

pub(crate) fn products() -> Vec<CompletionCandidate> {
    candidates(|d| vec![d.product.clone()])
}

pub(crate) fn manufacturers() -> Vec<CompletionCandidate> {
    candidates(|d| vec![d.manufacturer.clone()])
}

pub(crate) fn apis() -> Vec<CompletionCandidate> {
    candidates(|d| {
        d.api
            .iter()
            .flat_map(|api| [api.gid.clone(), format!("{}@^{}", api.gid, api.version)])
            .collect()
    })
}

pub(crate) fn vid_pids() -> Vec<CompletionCandidate> {
    candidates(|d| {
        d.usb
            .iter()
            .map(|u| format!("{:04x}:{:04x}", u.vid, u.pid))
            .collect()
    })
}

pub(crate) fn usb_paths() -> Vec<CompletionCandidate> {
    candidates(|d| {
        d.usb
            .iter()
            .map(|u| {
                let ports: Vec<_> = u.port_chain.iter().map(|p| p.to_string()).collect();
                format!("{}-{}", u.bus_id, ports.join("."))
            })
            .collect()
    })
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

#[cfg(test)]
mod tests {
    use super::*;

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
