//! The wire line of a crate's own `Cargo.toml`, read at compile time: `[package.metadata.wire_weaver] wire = "w1.0"`
//! gives line 1. The foundation crates of the link setup hello (`shrink_wrap`, `ww_framer`, `ww_link`,
//! `ww_client_server`) take their `GENERATION` from it with [generation_from_manifest](crate::generation_from_manifest),
//! so a wire break is bumped in one place, where the evolution checker reads it too (EVO-26).
//!
//! A `const fn` over `include_str!`, no build script: `no_std`, nothing to compile or run before the crate, and
//! cargo rebuilds the crate when the manifest changes. A manifest without the line, or with a malformed one, is a
//! compile error saying what is wrong.

/// `GENERATION` from the wire line of the calling crate's `Cargo.toml`: its line, the major of
/// `[package.metadata.wire_weaver] wire = "wN.M"`, as a `u8`.
///
/// ```
/// pub const GENERATION: u8 = shrink_wrap::generation_from_manifest!();
/// # assert_eq!(GENERATION, 1);
/// ```
#[macro_export]
macro_rules! generation_from_manifest {
    () => {
        $crate::wire_line::line_of_manifest(::core::include_str!(::core::concat!(
            ::core::env!("CARGO_MANIFEST_DIR"),
            "/Cargo.toml"
        )))
    };
}

const TABLE: &[u8] = b"[package.metadata.wire_weaver]";

/// The line of `[package.metadata.wire_weaver] wire = "wN.M"` in `manifest`, the text of a `Cargo.toml`; panics
/// (a compile error in a const) when the table, its `wire` key or a `wN.M` value with `N` in `1..=255` is missing.
pub const fn line_of_manifest(manifest: &str) -> u8 {
    let b = manifest.as_bytes();
    let mut i = match find_table(b) {
        Some(i) => i,
        None => panic!("no `[package.metadata.wire_weaver]` table in Cargo.toml"),
    };
    // the table's lines, up to the next table header
    while i < b.len() {
        let line = skip_spaces(b, i);
        if line < b.len() && b[line] == b'[' {
            break;
        }
        if let Some(value) = value_of_wire(b, line) {
            return parse_line(b, value);
        }
        i = next_line(b, i);
    }
    panic!("no `wire = \"wN.M\"` in `[package.metadata.wire_weaver]` of Cargo.toml")
}

/// Start of the line after the `[package.metadata.wire_weaver]` header, a header alone on its line.
const fn find_table(b: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i < b.len() {
        let at = skip_spaces(b, i);
        if starts_with(b, at, TABLE) {
            let end = skip_spaces(b, at + TABLE.len());
            if end >= b.len() || b[end] == b'\n' || b[end] == b'\r' || b[end] == b'#' {
                return Some(next_line(b, at));
            }
        }
        i = next_line(b, i);
    }
    None
}

/// Where the value of `wire = ` starts, when the line at `at` is that key.
const fn value_of_wire(b: &[u8], at: usize) -> Option<usize> {
    if !starts_with(b, at, b"wire") {
        return None;
    }
    let eq = skip_spaces(b, at + 4);
    if eq >= b.len() || b[eq] != b'=' {
        return None;
    }
    Some(skip_spaces(b, eq + 1))
}

/// `"wN.M"` at `at`: `N`, with `M` checked to be there.
const fn parse_line(b: &[u8], at: usize) -> u8 {
    if !starts_with(b, at, b"\"w") {
        panic!("the wire version in Cargo.toml is written \"wN.M\", like \"w1.0\"");
    }
    let mut i = at + 2;
    let mut line: u32 = 0;
    let start = i;
    while i < b.len() && b[i].is_ascii_digit() && line <= 255 {
        line = line * 10 + (b[i] - b'0') as u32;
        i += 1;
    }
    if i == start || line > 255 || i >= b.len() || b[i] != b'.' {
        panic!(
            "the wire version in Cargo.toml is written \"wN.M\" with a line N of 1 to 255, like \"w1.0\""
        );
    }
    let revision = i + 1;
    i = revision;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i == revision || i >= b.len() || b[i] != b'"' {
        panic!(
            "the wire version in Cargo.toml is written \"wN.M\" with a revision M, like \"w1.0\""
        );
    }
    if line == 0 {
        panic!("a generation counts from 1: the wire line in Cargo.toml is w1 or later");
    }
    line as u8
}

const fn starts_with(b: &[u8], at: usize, prefix: &[u8]) -> bool {
    if at + prefix.len() > b.len() {
        return false;
    }
    let mut i = 0;
    while i < prefix.len() {
        if b[at + i] != prefix[i] {
            return false;
        }
        i += 1;
    }
    true
}

const fn skip_spaces(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
        i += 1;
    }
    i
}

const fn next_line(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && b[i] != b'\n' {
        i += 1;
    }
    i + 1
}

#[cfg(test)]
mod tests {
    use super::line_of_manifest;

    #[test]
    fn line_of_a_manifest() {
        let m = "[package]\nname = \"x\"\n\n[package.metadata.wire_weaver]\nwire = \"w3.12\"\n\n[dependencies]\n";
        assert_eq!(line_of_manifest(m), 3);
        // cargo's normalized form in a published crate, CRLF, comments and spacing
        let m = "[package]\r\nname = \"x\"\r\n\r\n[package.metadata.wire_weaver]  # the wire\r\n  wire=\"w255.0\"\r\n";
        assert_eq!(line_of_manifest(m), 255);
        // a `wire` key of another table doesn't count
        let m = "[package.metadata.wire_weaver]\nroots = 1\n[other]\nwire = \"w2.0\"\n";
        assert!(std::panic::catch_unwind(|| line_of_manifest(m)).is_err());
    }

    #[test]
    fn malformed_lines_are_refused() {
        for wire in [
            "\"1.0\"",
            "\"w1\"",
            "\"w1.\"",
            "\"w256.0\"",
            "\"w0.3\"",
            "\"w.1\"",
            "1",
        ] {
            let m = format!("[package.metadata.wire_weaver]\nwire = {wire}\n");
            assert!(
                std::panic::catch_unwind(|| line_of_manifest(&m)).is_err(),
                "{wire} accepted"
            );
        }
        assert!(std::panic::catch_unwind(|| line_of_manifest("[package]\n")).is_err());
    }

    #[test]
    fn this_crates_generation() {
        assert_eq!(crate::GENERATION, 1);
    }
}
