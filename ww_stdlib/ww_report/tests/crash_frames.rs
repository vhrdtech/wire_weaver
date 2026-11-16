//! The crash symbolizer (CLT-59) against a real firmware ELF: `tests/crash_fw.elf`, built from `tests/crash_fw` for
//! thumbv6m as release firmware is (`-Zlocation-detail=none`, `trim-paths`), so its panics carry no `file:line`.
//! The records are synthetic, the addresses taken from `llvm-objdump -d tests/crash_fw.elf` (`just crash-fw-elf`
//! rebuilds it; check them after):
//!
//! ```text
//! 08000000 <Reset>:      8000022 bl ratio         8000026 (return address 0x08000027)
//!                        800003a bl pick          800003e
//!                        800004c bl scaled        8000050
//! 08000056 <pick>:       800005e ldrb r0, [r0, r2]
//!                        8000064 bl core::panicking::panic_bounds_check   8000068 (last instruction)
//! 08000068 <ratio>:      8000076 bl core::option::unwrap_failed           800007a (= the start of scaled)
//! 0800007a <scaled>:     8000086 bl core::panicking::panic_fmt            800008a (= the start of panic_fmt)
//! 08000090 <core::panicking::panic>:              8000094 bl panic_fmt    8000098
//! 08000098 <core::panicking::panic_bounds_check>: 800009c bl panic_fmt    80000a0
//! 080000a0 <core::option::unwrap_failed>:         80000a4 bl panic        80000a8
//! ```

use std::path::{Path, PathBuf};
use ww_report::crash::{CrashAddresses, CrashFrames, FrameKind, Source, Symbolizer};

fn elf(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join(name)
}

fn symbolizer() -> Symbolizer {
    Symbolizer::load(&elf("crash_fw.elf")).unwrap()
}

fn panic(stack: &[u32]) -> CrashFrames {
    symbolizer().symbolize(&CrashAddresses {
        pc: None,
        lr: None,
        stack: stack.to_vec(),
    })
}

/// (function, file:line) of every frame, innermost first
fn chain(frames: &CrashFrames) -> Vec<(String, String)> {
    frames
        .sites
        .iter()
        .flat_map(|s| &s.frames)
        .map(|f| {
            (
                f.function.clone().unwrap_or_default(),
                format!(
                    "{}:{}",
                    f.short_file.as_deref().unwrap_or("?"),
                    f.line.unwrap_or(0)
                ),
            )
        })
        .collect()
}

fn site(frames: &CrashFrames) -> (String, String, u32) {
    let f = frames.panic_frame().expect("a panic site");
    (
        f.function.clone().unwrap(),
        f.short_file.clone().unwrap(),
        f.line.unwrap(),
    )
}

/// `a.checked_div(b).unwrap()` in `ratio`: the panic site is the `unwrap` line, found through the return address of
/// `bl unwrap_failed`, which points past the end of `ratio` into `scaled`.
#[test]
fn unwrap_panic_names_its_line() {
    let frames = panic(&[
        0x0800_0099, // panic -> panic_fmt
        0x2000_1ff0, // a RAM pointer
        0x0800_00a9, // unwrap_failed -> panic
        0x0800_007b, // ratio -> unwrap_failed
        0x0800_0083, // Thumb code, but after a `bhs`, not a call
        0x0800_0026, // a return address without the Thumb bit: data
        0x0800_0027, // Reset -> ratio
    ]);
    assert_eq!(
        site(&frames),
        ("crash_fw::ratio".into(), "src/main.rs".into(), 12)
    );
    assert_eq!(
        chain(&frames),
        [
            ("core::panicking::panic", "core/src/panicking.rs:150"),
            ("core::option::unwrap_failed", "core/src/option.rs:2253"),
            (
                "<core::option::Option<u32>>::unwrap",
                "core/src/option.rs:1016"
            ),
            ("crash_fw::ratio", "src/main.rs:12"),
            ("Reset", "src/main.rs:38"),
        ]
        .map(|(f, l)| (f.to_string(), l.to_string()))
    );
    assert_eq!(frames.dropped, [0x2000_1ff0, 0x0800_0083, 0x0800_0026]);
    assert_eq!(
        frames.summary().as_deref(),
        Some("Panic site: crash_fw::ratio at src/main.rs:12:22")
    );
    let ratio = &frames.sites[2];
    assert_eq!(ratio.source, Source::Stack(3));
    assert_eq!(ratio.lookup, 0x0800_0076, "the `bl` instruction");
    assert!(ratio.frames[0].inlined && !ratio.frames[1].inlined);
    assert_eq!(ratio.frames[0].kind, FrameKind::Panic);
    assert_eq!(
        ratio.frames[1].column,
        Some(22),
        "the column of `.unwrap()`"
    );
    assert_eq!(
        frames.sites[0].frames[0].file.as_deref(),
        Some("/rustc/3e92e1a2d20b991ef23a6c6064c34d57ee8c98d3/library/core/src/panicking.rs"),
        "the DWARF path, kept beside the short one"
    );
}

/// A stale return address between the machinery's words (left in the panic handler's uninitialised frames) is
/// code too, but not the panic site: the site is the caller of the outermost machinery entry.
#[test]
fn stale_word_in_the_machinery_is_not_the_site() {
    let frames = panic(&[
        0x0800_0099, // panic -> panic_fmt
        0x0800_0051, // stale: Reset -> scaled, code from long ago
        0x0800_00a9, // unwrap_failed -> panic
        0x0800_007b, // ratio -> unwrap_failed
        0x0800_0027, // Reset -> ratio
    ]);
    assert_eq!(
        site(&frames),
        ("crash_fw::ratio".into(), "src/main.rs".into(), 12)
    );
    assert_eq!(frames.panic_site, Some((3, 1)));
}

/// `panic!` in `check_gain`, inlined into `scaled`: two frames at one address, the inlined one is the site.
#[test]
fn inlined_panic() {
    let frames = panic(&[0x0800_008b, 0x0800_0051]);
    assert_eq!(
        chain(&frames),
        [
            ("crash_fw::check_gain", "src/main.rs:25"),
            ("crash_fw::scaled", "src/main.rs:31"),
            ("Reset", "src/main.rs:40"),
        ]
        .map(|(f, l)| (f.to_string(), l.to_string()))
    );
    assert_eq!(frames.panic_site, Some((0, 0)));
    let text = frames.render();
    assert!(
        text.contains("crash_fw::check_gain at src/main.rs:25:9 (inlined)  <- panic site"),
        "{text}"
    );
    assert!(
        panic(&[0x0800_007b])
            .render()
            .contains("core::option::Option::unwrap at core/src/option.rs:1016:21 (inlined)"),
        "generic arguments are left out of the text"
    );
}

/// `table[i]` out of bounds in `pick`.
#[test]
fn bounds_check() {
    let frames = panic(&[0x0800_00a1, 0x0800_0069, 0x0800_003f]);
    assert_eq!(
        site(&frames),
        ("crash_fw::pick".into(), "src/main.rs".into(), 18)
    );
    assert_eq!(
        frames.sites[0].frames[0].function.as_deref(),
        Some("core::panicking::panic_bounds_check")
    );
}

/// A fault: the program counter is exact, the link register and the raw stack words are return addresses.
#[test]
fn fault() {
    let frames = symbolizer().symbolize(&CrashAddresses {
        pc: Some(0x0800_005e), // the `ldrb` in pick
        lr: Some(0x0800_003f),
        stack: vec![0x0000_0003, 0x2000_1fe8, 0x0800_003f, 0xffff_fff9],
    });
    assert_eq!(frames.sites[0].source, Source::Pc);
    assert_eq!(frames.sites[0].lookup, 0x0800_005e);
    // the DWARF puts the `ldrb` on the closing brace of `pick`
    assert_eq!(
        site(&frames),
        ("crash_fw::pick".into(), "src/main.rs".into(), 19)
    );
    assert_eq!(
        frames.summary().as_deref(),
        Some("Fault in: crash_fw::pick at src/main.rs:19:2")
    );
    assert_eq!(frames.sites[1].source, Source::Lr);
    assert_eq!(
        chain(&frames)[1..],
        [
            ("Reset".to_string(), "src/main.rs:39".to_string()),
            ("Reset".to_string(), "src/main.rs:39".to_string()),
        ]
    );
    assert_eq!(frames.dropped, [0x0000_0003, 0x2000_1fe8, 0xffff_fff9]);
}

/// An `EXC_RETURN` in the link register (a fault in a handler's caller) is not code.
#[test]
fn exc_return_lr_is_skipped() {
    let frames = symbolizer().symbolize(&CrashAddresses {
        pc: Some(0x0800_005e),
        lr: Some(0xffff_fff9),
        stack: vec![],
    });
    assert_eq!(frames.sites.len(), 1);
}

/// Without DWARF the symbol table still names the functions.
#[test]
fn no_debug_info() {
    let s = Symbolizer::load(&elf("crash_fw_nodebug.elf")).unwrap();
    let frames = s.symbolize(&CrashAddresses {
        pc: None,
        lr: None,
        stack: vec![0x0800_00a9, 0x0800_007b],
    });
    assert!(frames.no_debug_info);
    assert_eq!(
        chain(&frames),
        [
            ("core::option::unwrap_failed", "?:0"),
            ("crash_fw::ratio", "?:0"),
        ]
        .map(|(f, l)| (f.to_string(), l.to_string()))
    );
    assert_eq!(
        frames.panic_frame().and_then(|f| f.function.as_deref()),
        Some("crash_fw::ratio"),
        "{}",
        frames.render()
    );
    assert!(frames.render().contains("no debug info"));
}

/// The JSON a report carries.
#[test]
fn json() {
    let frames = panic(&[0x0800_008b]);
    let v = serde_json::to_value(&frames).unwrap();
    assert_eq!(v["panic_site"], serde_json::json!([0, 0]));
    assert_eq!(v["sites"][0]["source"], serde_json::json!({"stack": 0}));
    assert_eq!(v["sites"][0]["frames"][0]["short_file"], "src/main.rs");
    assert_eq!(v["sites"][0]["frames"][0]["line"], 25);
    assert_eq!(v["sites"][0]["frames"][0]["kind"], "code");
}

/// Words past the entry point (`Reset` here) are what earlier calls left in the stack buffer: kept, but marked.
#[test]
fn words_past_the_entry_point_are_stale() {
    let frames = panic(&[
        0x0800_00a9, // unwrap_failed -> panic
        0x0800_007b, // ratio -> unwrap_failed
        0x0800_0027, // Reset -> ratio
        0x0800_003f, // Reset -> pick, from an earlier call
        0x0800_0051, // Reset -> scaled, from an earlier call
    ]);
    assert_eq!(
        site(&frames),
        ("crash_fw::ratio".into(), "src/main.rs".into(), 12)
    );
    let stale: Vec<bool> = frames.sites.iter().map(|s| s.stale).collect();
    assert_eq!(stale, [false, false, false, true, true]);
    let text = frames.render();
    let marker = text.find("possibly stale").expect(&text);
    assert!(text[..marker].contains("Reset at src/main.rs:38"), "{text}");
    assert_eq!(text[marker..].matches("Reset at").count(), 2, "{text}");
}
