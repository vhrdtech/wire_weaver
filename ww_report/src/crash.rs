//! The call chain of a crash record as `file:line` frames (CLT-59), from the firmware's ELF and its DWARF.
//!
//! Firmware built with `-Zlocation-detail=none` keeps no `file:line` in its panic messages, so the addresses a
//! crash record holds are the only way back to the panic site. [Symbolizer] turns them into [CrashFrames]:
//!
//! * the program counter of a fault is looked up as it is, its link register and the stack words as return
//!   addresses, at the call instruction before them (a `BL` that ends a function returns past its end);
//! * stack words that are not return addresses (data, stale pointers) are dropped with the rule the firmware uses
//!   (`bedrock_crash::is_return_address`): the Thumb bit set, inside `.text`, right after a `BL` or `BLX`, read from
//!   the ELF's code. A record of newer firmware holds only such words for a panic; a fault's raw words and older
//!   firmware's records are filtered here;
//! * each address gives its inlined frames too, innermost first, with demangled names and shortened paths
//!   ([short_path]);
//! * frames of the panic machinery (`core::panicking`, `unwrap_failed`, the panic handler, `bedrock_crash`) are
//!   marked [FrameKind::Panic], so the first frame past them, the panic site, stands out ([CrashFrames::panic_site]).
//!
//! A stack scan without frame pointers is a heuristic: a stale return address left in unused stack space passes
//! the filter as well, so the chain is a likely one, not an exact one.
//!
//! ```ignore
//! let symbolizer = Symbolizer::load(Path::new("firmware.elf"))?;
//! let frames = symbolizer.symbolize(&CrashAddresses::from_record(&record));
//! print!("{}", frames.render());
//! ```

use crate::info::LastReset;
use crate::symbols::Symbols;
use anyhow::{Context as _, Result};
use object::{Object, ObjectSection, ObjectSymbol, SectionKind};
use serde::Serialize;
use std::borrow::Cow;
use std::fmt::Write;
use std::path::Path;
use ww_firmware_info::{Fault, ResetRecordOwned};

type Reader = gimli::EndianArcSlice<gimli::RunTimeEndian>;

/// Names, files and lines of a firmware ELF's code.
pub struct Symbolizer {
    /// `None` when the ELF has no DWARF: names come from the symbol table then, without files
    dwarf: Option<addr2line::Context<Reader>>,
    symbols: Symbols,
    /// `.text` (`__stext..__etext` when the ELF has them), where return addresses can point
    text: std::ops::Range<u32>,
    /// Executable sections' bytes by address, to read the instruction before a return address
    code: Vec<(u32, Vec<u8>)>,
    /// Thumb code: return addresses have bit 0 set and follow a `BL` / `BLX`
    thumb: bool,
}

/// The addresses of a crash record, in the order the device saved them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CrashAddresses {
    /// Program counter of a fault (`mepc` on RISC-V): where it happened, exactly
    pub pc: Option<u32>,
    /// Link register of a fault (`ra` on RISC-V): the faulting function's caller, if it was not saved yet
    pub lr: Option<u32>,
    /// Stack words, innermost first: return addresses for a panic of newer firmware, raw words otherwise
    pub stack: Vec<u32>,
}

/// Where an address of the record came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// The program counter of a fault
    Pc,
    /// The link register of a fault
    Lr,
    /// A stack word, by its position in the record
    Stack(usize),
}

/// One address of the record and the frames at it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Site {
    pub source: Source,
    /// The address as the record has it (a return address with its Thumb bit)
    pub address: u32,
    /// The address looked up: the call instruction for a return address, the address itself for a program counter
    pub lookup: u32,
    /// Innermost first: the inlined functions at `lookup`, then the function they are inlined into. One frame with
    /// no function when nothing in the ELF covers the address.
    pub frames: Vec<Frame>,
}

/// A function at an address, with its source location.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Frame {
    /// Demangled, without the hash
    pub function: Option<String>,
    /// As the DWARF has it (`/rustc/<hash>/library/core/src/panicking.rs`, `src/main.rs`)
    pub file: Option<String>,
    /// `file` shortened for display ([short_path]): `core/src/panicking.rs`, `embassy-usb-0.6.0/src/lib.rs`
    pub short_file: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
    /// Inlined into the next frame of its site
    pub inlined: bool,
    pub kind: FrameKind,
}

/// What a frame is, to find the panic site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FrameKind {
    /// The panic machinery: `core::panicking`, `unwrap_failed`, `Option::unwrap` and the like, the panic handler,
    /// `bedrock_crash`
    Panic,
    /// Any other code: the application and its libraries
    Code,
}

/// The call chain of a crash, innermost first.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CrashFrames {
    pub sites: Vec<Site>,
    /// Site and frame index of the first frame past the panic machinery: the panic site, or the fault's place
    pub panic_site: Option<(usize, usize)>,
    /// Stack words left out because they are not return addresses
    pub dropped: Vec<u32>,
    /// The ELF has no DWARF: functions come from its symbol table, without files and lines
    pub no_debug_info: bool,
}

impl CrashAddresses {
    /// The addresses of a record as `ww_firmware_info::LastReset::last_reset` returns it.
    pub fn from_record(record: &ResetRecordOwned) -> Self {
        let (pc, lr) = match &record.fault {
            Some(Fault::CortexM(f)) => (Some(f.pc), Some(f.lr)),
            Some(Fault::RiscV(f)) => (Some(f.mepc), Some(f.ra)),
            _ => (None, None),
        };
        Self {
            pc,
            lr,
            stack: record.stack.to_vec(),
        }
    }

    /// The addresses of a record as [crate::info::collect_info] has it.
    pub fn from_last_reset(reset: &LastReset) -> Self {
        Self {
            pc: reset.fault_addresses.first().copied(),
            lr: reset.fault_addresses.get(1).copied(),
            stack: reset.stack.clone(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.pc.is_none() && self.lr.is_none() && self.stack.is_empty()
    }
}

impl Symbolizer {
    pub fn load(path: &Path) -> Result<Self> {
        let bytes =
            std::fs::read(path).with_context(|| format!("Failed to read {}", path.display()))?;
        Self::from_elf(&bytes).with_context(|| path.display().to_string())
    }

    pub fn from_elf(bytes: &[u8]) -> Result<Self> {
        let file = object::File::parse(bytes)?;
        let thumb = matches!(file.architecture(), object::Architecture::Arm);
        let endian = if file.is_little_endian() {
            gimli::RunTimeEndian::Little
        } else {
            gimli::RunTimeEndian::Big
        };
        let has_dwarf = file.section_by_name(".debug_info").is_some();
        let dwarf = if has_dwarf {
            let dwarf = gimli::Dwarf::load(|id| -> Result<Reader> {
                let data = match file.section_by_name(id.name()) {
                    Some(s) => s.uncompressed_data()?,
                    None => Cow::Borrowed(&[][..]),
                };
                Ok(gimli::EndianArcSlice::new(data.as_ref().into(), endian))
            })?;
            Some(addr2line::Context::from_dwarf(dwarf).context("Failed to read the DWARF")?)
        } else {
            None
        };

        let mut code = vec![];
        for s in file.sections() {
            if s.kind() == SectionKind::Text && s.size() > 0 {
                code.push((s.address() as u32, s.data()?.to_vec()));
            }
        }
        let symbol = |name: &str| {
            file.symbols()
                .find(|s| s.name() == Ok(name))
                .map(|s| s.address() as u32)
        };
        let text = match (symbol("__stext"), symbol("__etext")) {
            (Some(start), Some(end)) => start..end,
            _ => {
                let start = code.iter().map(|(a, _)| *a).min().unwrap_or(0);
                let end = code
                    .iter()
                    .map(|(a, b)| a + b.len() as u32)
                    .max()
                    .unwrap_or(0);
                start..end
            }
        };
        Ok(Self {
            dwarf,
            symbols: Symbols::from_elf(bytes)?,
            text,
            code,
            thumb,
        })
    }

    fn halfword(&self, address: u32) -> Option<u16> {
        self.code.iter().find_map(|(start, bytes)| {
            let i = address.checked_sub(*start)? as usize;
            let b = bytes.get(i..i + 2)?;
            Some(u16::from_le_bytes([b[0], b[1]]))
        })
    }

    /// Whether `word` is a return address, and the address of the call instruction it returns past.
    ///
    /// Thumb: the rule of `bedrock_crash::is_return_address`, the Thumb bit set, inside `.text`, right after a
    /// `BLX Rm` (16 bits) or a `BL` / `BLX` immediate (32 bits). Other architectures: inside `.text`, the call
    /// taken as 4 bytes before.
    pub fn call_site(&self, word: u32) -> Option<u32> {
        if !self.thumb {
            return (word > self.text.start && word <= self.text.end).then(|| word - 4);
        }
        if word & 1 == 0 {
            return None;
        }
        let ra = word & !1;
        if ra < self.text.start.saturating_add(4) || ra > self.text.end {
            return None;
        }
        let last = self.halfword(ra - 2)?;
        // BLX Rm: 0100 0111 1mmm m000
        if last & 0xFF87 == 0x4780 {
            return Some(ra - 2);
        }
        // BL imm / BLX imm (T32): 11110 S imm10, then 11 J1 1 J2 imm11 (BL) or 11 J1 0 J2 imm10 H (BLX)
        let first = self.halfword(ra - 4)?;
        (first & 0xF800 == 0xF000 && last & 0xC000 == 0xC000).then_some(ra - 4)
    }

    /// The frames at `lookup`, innermost first.
    pub fn frames(&self, lookup: u32) -> Vec<Frame> {
        let mut frames = vec![];
        if let Some(ctx) = &self.dwarf
            && let Ok(mut iter) = ctx.find_frames(u64::from(lookup)).skip_all_loads()
        {
            while let Ok(Some(f)) = iter.next() {
                let function = f
                    .function
                    .as_ref()
                    .and_then(|n| n.raw_name().ok())
                    .map(|n| format!("{:#}", rustc_demangle::demangle(n.as_ref())));
                let (file, line, column) = match &f.location {
                    Some(l) => (l.file.map(clean_path), l.line, l.column),
                    None => (None, None, None),
                };
                frames.push(Frame {
                    kind: kind_of(function.as_deref()),
                    function,
                    short_file: file.as_deref().map(|f| short_path(f).to_string()),
                    file,
                    line,
                    column,
                    inlined: true,
                });
            }
        }
        if frames.iter().all(|f| f.function.is_none()) {
            // no DWARF for this address: the symbol table still names the function
            let function = self.symbols.locate(lookup).function;
            if frames.is_empty() {
                frames.push(Frame {
                    kind: kind_of(function.as_deref()),
                    function,
                    file: None,
                    short_file: None,
                    line: None,
                    column: None,
                    inlined: false,
                });
            } else {
                let last = frames.last_mut().expect("not empty");
                last.kind = kind_of(function.as_deref());
                last.function = function;
            }
        }
        if let Some(last) = frames.last_mut() {
            last.inlined = false;
        }
        frames
    }

    /// The call chain of a crash record's addresses.
    pub fn symbolize(&self, addresses: &CrashAddresses) -> CrashFrames {
        let mut sites = vec![];
        if let Some(pc) = addresses.pc {
            let lookup = if self.thumb { pc & !1 } else { pc };
            sites.push(self.site(Source::Pc, pc, lookup));
        }
        if let Some(lr) = addresses.lr
            // EXC_RETURN and the like: not code
            && lr & 0xF000_0000 != 0xF000_0000
        {
            // a return address, but kept as the record has it even when no call is before it
            let lookup = self.call_site(lr).unwrap_or(lr & !1);
            sites.push(self.site(Source::Lr, lr, lookup));
        }
        let mut dropped = vec![];
        for (i, &word) in addresses.stack.iter().enumerate() {
            match self.call_site(word) {
                Some(lookup) => sites.push(self.site(Source::Stack(i), word, lookup)),
                None => dropped.push(word),
            }
        }
        let panic_site = sites.iter().enumerate().find_map(|(si, s)| {
            s.frames
                .iter()
                .position(|f| f.kind == FrameKind::Code && f.function.is_some())
                .map(|fi| (si, fi))
        });
        CrashFrames {
            sites,
            panic_site,
            dropped,
            no_debug_info: self.dwarf.is_none(),
        }
    }

    fn site(&self, source: Source, address: u32, lookup: u32) -> Site {
        Site {
            source,
            address,
            lookup,
            frames: self.frames(lookup),
        }
    }
}

impl CrashFrames {
    /// The panic site's frame.
    pub fn panic_frame(&self) -> Option<&Frame> {
        self.panic_site.map(|(s, f)| &self.sites[s].frames[f])
    }

    /// Where it happened, in one line: `Panic site: crash_fw::ratio at src/main.rs:12:22`, `Fault in ..` when the
    /// site is a fault's program counter.
    pub fn summary(&self) -> Option<String> {
        let (si, _) = self.panic_site?;
        let f = self.panic_frame()?;
        let what = match self.sites[si].source {
            Source::Pc => "Fault in",
            _ => "Panic site",
        };
        let function = f.function.as_deref().map(short_function);
        let mut line = format!("{what}: {}", function.as_deref().unwrap_or("??"));
        if let Some(file) = &f.short_file {
            let _ = write!(line, " at {file}");
            if let Some(l) = f.line {
                let _ = write!(line, ":{l}");
                if let Some(c) = f.column {
                    let _ = write!(line, ":{c}");
                }
            }
        }
        Some(line)
    }

    /// One line per frame, innermost first, the panic site marked with `<-`:
    ///
    /// ```text
    /// stack 0x0800007b  crash_fw::ratio at src/main.rs:12:22  <- panic site
    /// ```
    pub fn render_lines(&self) -> Vec<String> {
        let mut lines = vec![];
        for (si, site) in self.sites.iter().enumerate() {
            let what = match site.source {
                Source::Pc => "pc   ",
                Source::Lr => "lr   ",
                Source::Stack(_) => "stack",
            };
            for (fi, f) in site.frames.iter().enumerate() {
                let mut line = if fi == 0 {
                    format!("{what} {:#010x}  ", site.address)
                } else {
                    // an inlined frame's caller, on the same address
                    format!("{:16}    ", "")
                };
                match &f.function {
                    Some(function) => line.push_str(&short_function(function)),
                    None => line.push_str("??"),
                }
                if let Some(file) = &f.short_file {
                    let _ = write!(line, " at {file}");
                    if let Some(l) = f.line {
                        let _ = write!(line, ":{l}");
                        if let Some(c) = f.column {
                            let _ = write!(line, ":{c}");
                        }
                    }
                }
                if f.inlined {
                    line.push_str(" (inlined)");
                }
                if self.panic_site == Some((si, fi)) {
                    line.push_str("  <- panic site");
                }
                lines.push(line);
            }
        }
        if !self.dropped.is_empty() {
            lines.push(format!(
                "({} stack words are not return addresses)",
                self.dropped.len()
            ));
        }
        if self.no_debug_info {
            lines.push("(the ELF has no debug info: functions only, no files)".to_string());
        }
        lines
    }

    /// [Self::render_lines] as one text.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for l in self.render_lines() {
            out.push_str(&l);
            out.push('\n');
        }
        out
    }
}

/// `./src/main.rs` as `src/main.rs`.
fn clean_path(path: &str) -> String {
    path.strip_prefix("./").unwrap_or(path).replace("/./", "/")
}

/// A demangled function name without generic arguments, for display: `<donguru::ww::ServerState>::led_on::<Sink<..>>`
/// as `donguru::ww::ServerState::led_on`. A trait's `<T as Trait>` stays.
pub fn short_function(name: &str) -> String {
    let mut out = String::new();
    let mut chars = name.chars().peekable();
    while let Some(c) = chars.next() {
        let generic_args = c == '<'
            && out
                .chars()
                .last()
                .is_some_and(|p| p.is_alphanumeric() || p == '_' || p == ':');
        if !generic_args {
            out.push(c);
            continue;
        }
        // skip to the matching `>`, and the `::` of a turbofish before it
        let mut depth = 1;
        for c in chars.by_ref() {
            match c {
                '<' => depth += 1,
                '>' => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                break;
            }
        }
        if out.ends_with("::") {
            out.truncate(out.len() - 2);
        }
    }
    unwrap_plain_paths(&out)
}

/// `<a::B>::c` as `a::B::c`: angle brackets around a path without `as` are only there for generic types.
fn unwrap_plain_paths(name: &str) -> String {
    let mut out = name.to_string();
    while let Some(start) = out.find('<') {
        let Some(len) = out[start + 1..].find(['<', '>']) else {
            break;
        };
        let end = start + 1 + len;
        let inner = &out[start + 1..end];
        if out.as_bytes()[end] != b'>' || inner.contains(" as ") || inner.contains(' ') {
            break;
        }
        out = format!("{}{}{}", &out[..start], inner, &out[end + 1..]);
    }
    out
}

/// Panic machinery: what runs between the panic site and the panic handler.
fn kind_of(function: Option<&str>) -> FrameKind {
    let Some(f) = function else {
        return FrameKind::Code;
    };
    // `<core::option::Option<T>>::unwrap` and the like are named with angle brackets
    let f = f.trim_start_matches('<');
    const PANIC: &[&str] = &[
        "core::panicking::",
        "core::panic::",
        "core::option::unwrap_failed",
        "core::option::expect_failed",
        "core::result::unwrap_failed",
        "core::cell::panic_already",
        "core::slice::index::",
        "core::str::slice_error_fail",
        "core::fmt::",
        "rust_begin_unwind",
        "__rustc::rust_begin_unwind",
        "bedrock_crash::",
        // defmt's `panic!` / `unwrap!` and its default panic handler
        "_defmt_panic",
        "__defmt_default_panic",
        "defmt::",
    ];
    const PANICKING_METHODS: &[&str] = &[
        "::unwrap",
        "::expect",
        "::unwrap_err",
        "::expect_err",
        "::index",
        "::index_mut",
    ];
    let std_method = (f.starts_with("core::option::Option")
        || f.starts_with("core::result::Result")
        || f.starts_with("core::slice::")
        || f.starts_with("core::array::"))
        && PANICKING_METHODS.iter().any(|m| f.ends_with(m));
    let slice_index = f.contains("core::slice::index::SliceIndex");
    if PANIC.iter().any(|p| f.starts_with(p))
        || std_method
        || slice_index
        || f.ends_with("::panic_handler")
    {
        FrameKind::Panic
    } else {
        FrameKind::Code
    }
}

/// A source path of a firmware ELF, shortened for display: the crate and its version, then the path inside it.
///
/// | DWARF path (`trim-paths`, or an absolute one)                 | shown                          |
/// |---------------------------------------------------------------|--------------------------------|
/// | `/cargo/registry/src/<index>/embassy-usb-0.6.0/src/lib.rs`    | `embassy-usb-0.6.0/src/lib.rs` |
/// | `/cargo/git/checkouts/<repo>/<rev>/ww_device/src/server.rs`   | `ww_device/src/server.rs`      |
/// | `/rustc/<hash>/library/core/src/panicking.rs`                 | `core/src/panicking.rs`        |
/// | `src/main.rs`                                                 | `src/main.rs`                  |
pub fn short_path(path: &str) -> &str {
    let parts: Vec<&str> = path.split('/').collect();
    let at = |i: usize| -> &str {
        // byte offset of component i
        let offset: usize = parts[..i].iter().map(|p| p.len() + 1).sum();
        &path[offset.min(path.len())..]
    };
    // inside cargo's home: `/cargo/..` with trim-paths, `~/.cargo/..` without
    let in_cargo = |i: usize| i > 0 && matches!(parts[i - 1], "cargo" | ".cargo");
    for (i, p) in parts.iter().enumerate().rev() {
        match *p {
            "registry" if in_cargo(i) && i + 2 < parts.len() => {
                // registry/src/<index>/<crate-ver>/.. or registry/<hash>/<crate-ver>/..
                let skip = if parts[i + 1] == "src" { 3 } else { 2 };
                if i + skip < parts.len() {
                    return at(i + skip);
                }
            }
            "git" if in_cargo(i) && i + 3 < parts.len() => {
                // git/checkouts/<repo-hash>/<rev>/.. or git/<hash>/<rev>/..
                let skip = if parts[i + 1] == "checkouts" { 4 } else { 3 };
                if i + skip < parts.len() {
                    return at(i + skip);
                }
            }
            // /rustc/<hash>/library/.. or the toolchain's lib/rustlib/src/rust/library/..
            "library"
                if i + 1 < parts.len()
                    && (parts[..i].contains(&"rustc") || parts[..i].contains(&"rustlib")) =>
            {
                return at(i + 1);
            }
            _ => {}
        }
    }
    if let Some(rest) = path.strip_prefix("library/") {
        return rest;
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_paths() {
        for (path, short) in [
            (
                "/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/embassy-usb-0.6.0/src/lib.rs",
                "embassy-usb-0.6.0/src/lib.rs",
            ),
            (
                "/cargo/registry/1949cf8c6b5b557f/embassy-usb-0.6.0/src/lib.rs",
                "embassy-usb-0.6.0/src/lib.rs",
            ),
            (
                "/home/me/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/heapless-0.9.1/src/vec.rs",
                "heapless-0.9.1/src/vec.rs",
            ),
            (
                "/cargo/git/2f1b1d2a3c4e5f60/8b7b80e/ww_device/src/server.rs",
                "ww_device/src/server.rs",
            ),
            (
                "/home/me/.cargo/git/checkouts/wire_weaver-2f1b1d2a3c4e5f60/8b7b80e/ww_device/src/server.rs",
                "ww_device/src/server.rs",
            ),
            (
                "/rustc/3e92e1a2d20b991ef23a6c6064c34d57ee8c98d3/library/core/src/panicking.rs",
                "core/src/panicking.rs",
            ),
            (
                "/home/me/.rustup/toolchains/nightly/lib/rustlib/src/rust/library/core/src/option.rs",
                "core/src/option.rs",
            ),
            ("library/core/src/panicking.rs", "core/src/panicking.rs"),
            ("src/main.rs", "src/main.rs"),
            ("/home/me/git/fw/src/main.rs", "/home/me/git/fw/src/main.rs"),
            (
                "/home/me/registry/src/a/b/main.rs",
                "/home/me/registry/src/a/b/main.rs",
            ),
        ] {
            assert_eq!(short_path(path), short, "{path}");
        }
    }

    #[test]
    fn short_functions() {
        for (name, short) in [
            ("crash_fw::ratio", "crash_fw::ratio"),
            (
                "<core::option::Option<u32>>::unwrap",
                "core::option::Option::unwrap",
            ),
            (
                "<donguru::ww::ServerState>::led_on::<wire_weaver::context::EventWriter<ww_device::server::Sink<ww_device::server::EmbassyClock>>>",
                "donguru::ww::ServerState::led_on",
            ),
            (
                "<donguru::ww::ServerState>::process_root::<ww_device::Sink<u8>>::{closure#0}",
                "donguru::ww::ServerState::process_root::{closure#0}",
            ),
            (
                "<embassy_executor::raw::TaskPool<donguru::Main, 1>>::spawn",
                "embassy_executor::raw::TaskPool::spawn",
            ),
            (
                "<u32 as core::fmt::Debug>::fmt",
                "<u32 as core::fmt::Debug>::fmt",
            ),
            (
                "<heapless::Vec<u8, 4> as core::ops::Deref>::deref",
                "<heapless::Vec as core::ops::Deref>::deref",
            ),
        ] {
            assert_eq!(short_function(name), short, "{name}");
        }
    }

    #[test]
    fn panic_machinery() {
        for f in [
            "core::panicking::panic_fmt",
            "core::panicking::panic_bounds_check",
            "core::option::unwrap_failed",
            "core::result::unwrap_failed",
            "<core::option::Option<T>>::unwrap",
            "core::option::Option<T>::expect",
            "<core::result::Result<T, E>>::unwrap",
            "bedrock_crash::cortex_m::capture_panic",
            "donguru::panic_handler",
            "_defmt_panic",
            "__defmt_default_panic",
        ] {
            assert_eq!(kind_of(Some(f)), FrameKind::Panic, "{f}");
        }
        for f in [
            "crash_fw::ratio",
            "core::slice::sort::insertion_sort",
            "<ww_device::Server>::handle",
        ] {
            assert_eq!(kind_of(Some(f)), FrameKind::Code, "{f}");
        }
    }
}
