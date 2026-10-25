//! Escapes a string before it reaches the terminal, when it may come from a device, a hub, or another process
//! (USB product/manufacturer/serial strings, `UserStr`/error text, introspection docs and names from a device's API
//! model, holder/program names, labels, users and hosts): TOOL-19. Left as is otherwise, a device could write C0/C1
//! control characters or Unicode bidi overrides into the string and move the cursor, change colours, rewrite the
//! terminal title, or make the line read back out of order.
//!
//! Escaping style: a byte that needs escaping is shown the same way `{:?}` already shows it in a Rust string
//! literal — `\u{1b}`, `\u{80}`, `\u{202e}` — except `\n`, `\r` and `\t`, which get the short two-character form
//! (`\n`, `\r`, `\t`) so one device string can never make a printed line wrap, overwrite the line above it, or
//! split into two. Everything else, ASCII or not, prints as is. `--json` and other machine-readable output skip
//! this: JSON's own string escaping already covers control characters.
//!
//! `value_fmt`'s `ValueOwned::String(s) => format!("{s:?}")` already goes through `{:?}`, which
//! uses [char::escape_debug] and already escapes all of the above (verified by
//! the test `debug_format_already_escapes_what_escape_does`) — this module exists for the many places that print a
//! device/hub string with `{}` instead, where that protection doesn't apply.
//!
//! Our own ANSI colour codes (`console::style`) must keep working: escape the untrusted piece on its own with
//! [escape] or [Untrusted], then build the styled line around the escaped text — never escape a line that already
//! has colour codes in it, or the codes themselves (which start with the same ESC byte) get escaped away too.

use std::borrow::Cow;
use std::fmt;
use std::fmt::Write as _;

/// Wraps a `&str` that may come from a device, hub or another process, for `{}` instead of printing it directly.
/// See the module docs for the escaping style and why colour codes must be added outside this wrapper, not inside.
pub struct Untrusted<'a>(pub &'a str);

impl fmt::Display for Untrusted<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&escape(self.0))
    }
}

/// `s`, or a copy with every character `needs_escape` marks replaced by a visible `\u{..}` escape (`\n`/`\r`/`\t`
/// by their two-character short form instead). Borrows `s` unchanged, no allocation, when nothing needs escaping.
pub fn escape(s: &str) -> Cow<'_, str> {
    if !s.chars().any(needs_escape) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if needs_escape(c) => _ = write!(out, "\\u{{{:x}}}", c as u32),
            c => out.push(c),
        }
    }
    Cow::Owned(out)
}

/// C0 controls (including ESC) and DEL, C1 controls, and the Unicode bidi override/isolate characters a device
/// could use to move the cursor, start a terminal escape sequence, or make text read out of order.
fn needs_escape(c: char) -> bool {
    matches!(c,
        '\u{00}'..='\u{1f}'
        | '\u{7f}'
        | '\u{80}'..='\u{9f}'
        | '\u{202a}'..='\u{202e}'
        | '\u{2066}'..='\u{2069}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_ascii_is_not_touched_or_allocated() {
        assert!(matches!(
            escape("hello, device 123"),
            Cow::Borrowed("hello, device 123")
        ));
    }

    #[test]
    fn printable_non_ascii_is_not_touched_or_allocated() {
        let s = "b200 · café · \u{65e5}\u{672c}\u{8a9e}";
        assert!(matches!(escape(s), Cow::Borrowed(_)));
        assert_eq!(escape(s), s);
    }

    #[test]
    fn escapes_esc() {
        assert_eq!(escape("\x1b"), "\\u{1b}");
    }

    #[test]
    fn escapes_a_csi_sequence_but_keeps_its_printable_tail() {
        // `\x1b[31m` would turn following text red on a real terminal; only the ESC byte needs hiding
        assert_eq!(escape("\x1b[31mred\x1b[0m"), "\\u{1b}[31mred\\u{1b}[0m");
    }

    #[test]
    fn escapes_newline_cr_and_tab_as_short_forms() {
        assert_eq!(escape("a\nb\rc\td"), "a\\nb\\rc\\td");
    }

    #[test]
    fn escapes_c1_controls() {
        assert_eq!(escape("\u{80}"), "\\u{80}");
        assert_eq!(escape("\u{9f}"), "\\u{9f}");
    }

    #[test]
    fn escapes_bidi_overrides_and_isolates() {
        assert_eq!(escape("\u{202e}evil\u{2069}"), "\\u{202e}evil\\u{2069}");
    }

    #[test]
    fn escapes_del() {
        assert_eq!(escape("\x7f"), "\\u{7f}");
    }

    /// A terminal title-change sequence (`ESC ] 0 ; text BEL`), the kind of thing a malicious product string
    /// could carry: both control bytes come out visible, the printable text between them is untouched.
    #[test]
    fn escapes_an_osc_title_sequence() {
        assert_eq!(escape("\x1b]0;evil\x07"), "\\u{1b}]0;evil\\u{7}");
    }

    #[test]
    fn display_impl_matches_escape() {
        assert_eq!(Untrusted("\x1b[31m").to_string(), escape("\x1b[31m"));
    }

    /// `{:?}` on a `&str` (what [crate::value_fmt] relies on for `ValueOwned::String`) already escapes every
    /// character this module does, so that one site needs no [Untrusted] wrapper of its own.
    #[test]
    fn debug_format_already_escapes_what_escape_does() {
        let s = "evil\x1b]0;x\x07\u{202e}bidi\u{80}";
        let debug = format!("{s:?}");
        for c in s.chars().filter(|c| needs_escape(*c)) {
            assert!(
                !debug.contains(c),
                "{c:?} ({:#x}) was not escaped by {{:?}}: {debug}",
                c as u32
            );
        }
    }
}
