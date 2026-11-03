//! Flash attribution by inlined frames (TEST-32): where the bytes of an LTO-merged firmware come from.
//!
//! LTO merges most of a server into a few huge functions, so a symbol list says little. Here every instruction of
//! the ELF is looked up in the DWARF line table with its chain of inlined frames (as `llvm-symbolizer --inlining`
//! does), and its bytes are added to:
//!
//! 1. the crate of the innermost frame (`shrink_wrap`, `core`, `embassy-usb`, ...);
//! 2. the generated dispatcher function it was inlined into (`process_root_*`, `process_request_bytes`, ...);
//! 3. the innermost function;
//! 4. the shrink_wrap serdes impl that owns it (the innermost `<T as SerializeShrinkWrap>` or
//!    `BufWriter::write::<T>` frame), with the number of copies the impl was inlined into and the bytes beyond the
//!    largest copy. With `--gen GENERATED.rs` (a `ww_codegen!` `debug_to_file` dump) each impl also gets the number
//!    of resource arms of the dispatcher that name its type.
//!
//! Instruction boundaries come from `llvm-objdump -d`; a literal pool counts as the code it belongs to. The ELF
//! needs debug info (`debug = 2` keeps the inlined frames, `strip` removes them).

use crate::Res;
use object::{Object, ObjectSymbol, SymbolKind};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

/// One inlined frame of an instruction, innermost first in a chain.
#[derive(Clone, Debug, PartialEq)]
struct Frame {
    name: String,
    file: String,
    line: u32,
    column: u32,
}

/// A function of the ELF: start (thumb bit cleared) and size.
struct Func {
    start: u64,
    size: u64,
}

/// What the instructions of an ELF add up to.
#[derive(Default, Debug)]
pub struct Attribution {
    pub total: u64,
    pub functions: usize,
    /// Bytes under any frame of WireWeaver (`ww_*`, `shrink_wrap`, `wire_weaver*`, `exact_units`) or the dispatcher.
    pub wire_weaver: u64,
    pub by_crate: BTreeMap<String, u64>,
    pub by_dispatcher: BTreeMap<String, u64>,
    pub by_function: BTreeMap<String, u64>,
    pub serdes: BTreeMap<(Dir, String), SerdesImpl>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Dir {
    Ser,
    Des,
}

#[derive(Default, Debug)]
pub struct SerdesImpl {
    /// Bytes of the type's own code, nested types excluded.
    pub own: u64,
    /// Bytes including the types nested in it.
    pub with_nested: u64,
    /// Own bytes by the chain of frames the impl sits in: each key is one copy (inline site or out-of-line function).
    pub copies: BTreeMap<Vec<(String, u32, u32)>, u64>,
    /// Outermost serdes types that contain it.
    pub outer: BTreeSet<String>,
    /// Resource arms of the dispatcher that name the type or one around it (with `--gen`).
    pub resources: usize,
    /// Used by request/event framing, so by every request.
    pub protocol: bool,
}

impl SerdesImpl {
    pub fn beyond_first_copy(&self) -> u64 {
        self.own - self.copies.values().copied().max().unwrap_or(0)
    }
}

/// Function names of a generated dispatcher that are not matched by the name patterns.
#[derive(Default)]
struct Generated {
    fns: BTreeSet<String>,
    /// Text of each resource arm of the dispatcher (`3u32 => match request.kind { ... }`).
    arms: Vec<String>,
}

impl Generated {
    fn parse(text: &str) -> Generated {
        let mut g = Generated::default();
        for line in text.lines() {
            let t = line.trim_start();
            if let Some(rest) = t
                .strip_prefix("fn ")
                .or_else(|| t.strip_prefix("async fn "))
            {
                let name: String = rest.chars().take_while(|c| is_ident(*c)).collect();
                if !name.is_empty() {
                    g.fns.insert(name);
                }
            }
        }
        let body = text
            .find("async fn process_root")
            .map_or(text, |i| &text[i..]);
        for line in body.lines() {
            let t = line.trim_start();
            let digits = t.chars().take_while(char::is_ascii_digit).count();
            let is_arm =
                digits > 0 && t[digits..].starts_with("u32 => match") && t.contains("request.kind");
            if is_arm || g.arms.is_empty() {
                if is_arm {
                    g.arms.push(String::new());
                }
                continue;
            }
            let last = g.arms.last_mut().unwrap();
            last.push_str(line);
            last.push('\n');
        }
        g
    }

    /// Resource arms that name `ty`, or the innermost user type around a generic wrapper.
    fn arms_naming(&self, ty: &str) -> usize {
        let short = short_type_name(ty);
        let wrappers = [
            "RefVec", "Option", "Result", "MaxLen", "u8", "u16", "u32", "bool", "str", "&str",
        ];
        let name = if short.is_empty() || wrappers.contains(&short.as_str()) {
            let inner = ty.split_once('<').map_or("", |(_, r)| r);
            let found = idents(inner).into_iter().rfind(|x| {
                x.starts_with(|c: char| c.is_ascii_uppercase()) && !wrappers.contains(x)
            });
            match found {
                Some(n) => n.to_string(),
                None => return 0,
            }
        } else {
            short
        };
        self.arms.iter().filter(|a| contains_word(a, &name)).count()
    }
}

fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn idents(s: &str) -> Vec<&str> {
    s.split(|c| !is_ident(c))
        .filter(|x| !x.is_empty())
        .collect()
}

fn contains_word(hay: &str, word: &str) -> bool {
    hay.match_indices(word)
        .any(|(i, _)| !hay[..i].ends_with(is_ident) && !hay[i + word.len()..].starts_with(is_ident))
}

/// `ww_client_server::stream::StreamHead<'_>` -> `StreamHead`.
fn short_type_name(ty: &str) -> String {
    let base = ty.split('<').next().unwrap_or(ty);
    base.rsplit("::").next().unwrap_or(base).to_string()
}

/// The crate a source path belongs to: `.../shrink_wrap/src/buf_reader.rs` -> `shrink_wrap`, the standard library
/// -> `core (std library)`, a git checkout's root crate by the checkout's name, a registry crate without its version.
fn crate_of(path: &str) -> String {
    if path.is_empty() {
        return "?".into();
    }
    if path.contains("rustlib/src/rust/library") || path.starts_with("/rustc/") {
        return "core (std library)".into();
    }
    if path.contains("compiler-builtins") || path.contains("compiler_builtins") {
        return "compiler_builtins".into();
    }
    // generated into a build script's OUT_DIR: .../build/embassy-stm32-0123456789abcdef/out/_generated.rs
    if let Some((_, rest)) = path.split_once("/build/")
        && let Some((dir, _)) = rest.split_once("/out/")
        && let Some((name, hash)) = dir.rsplit_once('-')
        && hash.len() == 16
    {
        return name.to_string();
    }
    if let Some(i) = path.rfind("/src/") {
        let dir = path[..i].rsplit('/').next().unwrap_or("");
        let is_rev = dir.len() >= 7 && dir.chars().all(|c| c.is_ascii_hexdigit());
        if is_rev && let Some((_, rest)) = path.split_once("/git/checkouts/") {
            let repo = rest.split('/').next().unwrap_or("");
            return repo.rsplit_once('-').map_or(repo, |(n, _)| n).to_string();
        }
        return strip_version(dir).to_string();
    }
    if path.starts_with("src/") {
        return "(this crate)".into();
    }
    path.rsplit('/').next().unwrap_or(path).to_string()
}

/// `embassy-usb-0.6.0` -> `embassy-usb`.
fn strip_version(dir: &str) -> &str {
    match dir.rsplit_once('-') {
        Some((name, ver)) if ver.starts_with(|c: char| c.is_ascii_digit()) => name,
        _ => dir,
    }
}

fn is_wire_weaver_crate(c: &str) -> bool {
    c.starts_with("ww_") || c.starts_with("wire_weaver") || c == "shrink_wrap" || c == "exact_units"
}

/// The dispatcher function a frame is, by its name: the generated ones, and by pattern the ones every server has.
fn dispatcher_name<'a>(name: &'a str, gen_: &Generated) -> Option<&'a str> {
    idents(name).into_iter().find(|t| {
        t.starts_with("process_root")
            || *t == "process_request_bytes"
            || *t == "poll_streams"
            || t.contains("server_task")
            || t.starts_with("stream_data_ser")
            || (t.ends_with("_send") && *t != "try_send")
            || gen_.fns.contains(*t)
    })
}

/// `'_`, `'static` and lifetime parameters out of a type, so one impl isn't listed per lifetime.
fn clean_type(t: &str) -> String {
    let mut s = t.replace("'_, ", "").replace("'static, ", "");
    while let Some(i) = s.find("<'") {
        let Some(j) = s[i..].find('>') else { break };
        s.replace_range(i..i + j + 1, "");
    }
    s.replace("<>", "")
}

/// The first generic argument of `T, ...>` or `T>`, brackets balanced.
fn first_generic(rest: &str) -> &str {
    let mut depth = 0;
    for (i, c) in rest.char_indices() {
        match c {
            '<' => depth += 1,
            '>' if depth == 0 => return &rest[..i],
            '>' => depth -= 1,
            ',' if depth == 0 => return &rest[..i],
            _ => {}
        }
    }
    rest
}

/// A shrink_wrap serdes frame: `<T as shrink_wrap::traits::SerializeShrinkWrap>::..`, or `BufWriter::write::<T>`
/// (also `BufReader::read` and `read_nested`), as the direction and the type.
fn serdes(name: &str) -> Option<(Dir, String)> {
    if let Some(rest) = name.strip_prefix('<')
        && let Some((ty, tail)) = rest.split_once(" as shrink_wrap::traits::")
    {
        let dir = if tail.starts_with("SerializeShrinkWrap>::") {
            Dir::Ser
        } else if tail.starts_with("DeserializeShrinkWrap>::") {
            Dir::Des
        } else {
            return None;
        };
        return Some((dir, clean_type(ty)));
    }
    for (class, ser) in [
        ("buf_writer::BufWriter", true),
        ("buf_reader::BufReader", false),
    ] {
        let Some((_, tail)) = name.split_once(class) else {
            continue;
        };
        let tail = tail.strip_prefix('>').unwrap_or(tail);
        let methods: &[&str] = if ser {
            &["write"]
        } else {
            &["read_nested", "read"]
        };
        for m in methods {
            let Some(rest) = tail.strip_prefix("::").and_then(|t| t.strip_prefix(m)) else {
                continue;
            };
            let Some(rest) = rest.strip_prefix("::<").or_else(|| rest.strip_prefix('<')) else {
                continue;
            };
            return Some((
                if ser { Dir::Ser } else { Dir::Des },
                clean_type(first_generic(rest)),
            ));
        }
    }
    None
}

/// Protocol types (request/event framing, links, versions) are used by every request, not by one resource.
fn is_protocol(ty: &str) -> bool {
    [
        "ww_client_server",
        "ww_link",
        "ww_version",
        "shrink_wrap::nib32",
        "shrink_wrap::vlq",
    ]
    .iter()
    .any(|p| ty.starts_with(p))
        || ty.starts_with("shrink_wrap::ref_vec::RefVec<shrink_wrap::nib32")
}

/// Function symbols of the ELF with a size, one per address.
fn functions(file: &object::File) -> Vec<Func> {
    let mut seen = BTreeMap::new();
    for s in file.symbols() {
        if s.kind() == SymbolKind::Text && s.size() > 0 {
            seen.entry(s.address() & !1).or_insert(s.size());
        }
    }
    seen.into_iter()
        .map(|(start, size)| Func { start, size })
        .collect()
}

/// `(address, size)` of every instruction inside the functions, from `llvm-objdump -d`.
fn instructions(elf: &Path, funcs: &[Func]) -> Res<Vec<(u64, u64)>> {
    let out = Command::new("llvm-objdump")
        .args(["-d", "--no-show-raw-insn"])
        .arg(elf)
        .output()?;
    if !out.status.success() {
        return Err("llvm-objdump failed".into());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let addrs: BTreeSet<u64> = text
        .lines()
        .filter_map(|l| u64::from_str_radix(l.trim_start().split_once(':')?.0, 16).ok())
        .collect();
    let mut insns = Vec::new();
    for f in funcs {
        let end = f.start + f.size;
        let at: Vec<u64> = addrs.range(f.start..end).copied().collect();
        for (i, a) in at.iter().enumerate() {
            let next = at.get(i + 1).copied().unwrap_or(end);
            insns.push((*a, next - a));
        }
    }
    Ok(insns)
}

/// The inlined frames of `addr`, innermost first.
fn frames_of(loader: &addr2line::Loader, addr: u64) -> Vec<Frame> {
    let mut out = Vec::new();
    let Ok(mut iter) = loader.find_frames(addr) else {
        return out;
    };
    while let Ok(Some(f)) = iter.next() {
        let name = f
            .function
            .as_ref()
            .and_then(|n| n.demangle().ok().map(|c| c.into_owned()))
            .unwrap_or_else(|| "?".into());
        let (file, line, column) = f.location.map_or((String::new(), 0, 0), |l| {
            (
                l.file.unwrap_or("").to_string(),
                l.line.unwrap_or(0),
                l.column.unwrap_or(0),
            )
        });
        out.push(Frame {
            name,
            file,
            line,
            column,
        });
    }
    out
}

pub fn attribute(elf: &Path, generated: Option<&Path>) -> Res<(Attribution, usize)> {
    let data = std::fs::read(elf)?;
    let file = object::File::parse(&*data)?;
    let funcs = functions(&file);
    let insns = instructions(elf, &funcs)?;
    let loader =
        addr2line::Loader::new(elf).map_err(|e| format!("DWARF of {}: {e}", elf.display()))?;
    let gen_ = match generated {
        Some(p) => Generated::parse(&std::fs::read_to_string(p)?),
        None => Generated::default(),
    };
    let mut a = Attribution {
        functions: funcs.len(),
        ..Default::default()
    };
    for (addr, n) in insns {
        a.total += n;
        let frames = frames_of(&loader, addr);
        let Some(inner) = frames.first() else {
            *a.by_function.entry("?".into()).or_default() += n;
            *a.by_crate.entry("?".into()).or_default() += n;
            *a.by_dispatcher.entry(NOT_UNDER.into()).or_default() += n;
            continue;
        };
        let inner_crate = crate_of(&inner.file);
        let short: String = inner.name.chars().take(150).collect();
        *a.by_function
            .entry(format!("{short}  [{inner_crate}]"))
            .or_default() += n;
        *a.by_crate.entry(inner_crate).or_default() += n;
        let disp = frames.iter().find_map(|f| dispatcher_name(&f.name, &gen_));
        *a.by_dispatcher
            .entry(disp.unwrap_or(NOT_UNDER).to_string())
            .or_default() += n;
        if disp.is_some()
            || frames
                .iter()
                .any(|f| is_wire_weaver_crate(&crate_of(&f.file)))
        {
            a.wire_weaver += n;
        }
        let sd: Vec<(usize, (Dir, String))> = frames
            .iter()
            .enumerate()
            .filter_map(|(i, f)| Some((i, serdes(&f.name)?)))
            .collect();
        let Some((i, key)) = sd.first() else { continue };
        let entry = a.serdes.entry(key.clone()).or_default();
        entry.own += n;
        let site = frames[i + 1..]
            .iter()
            .map(|f| (f.name.clone(), f.line, f.column))
            .collect();
        *entry.copies.entry(site).or_default() += n;
        entry.outer.insert(sd.last().unwrap().1.1.clone());
        let keys: BTreeSet<&(Dir, String)> = sd.iter().map(|(_, k)| k).collect();
        for k in keys {
            a.serdes.entry(k.clone()).or_default().with_nested += n;
        }
    }
    for ((_, ty), s) in a.serdes.iter_mut() {
        s.protocol = is_protocol(ty) || s.outer.iter().any(|o| is_protocol(o));
        s.resources = std::iter::once(ty)
            .chain(s.outer.iter())
            .filter(|t| !is_protocol(t))
            .map(|t| gen_.arms_naming(t))
            .max()
            .unwrap_or(0);
    }
    Ok((a, gen_.arms.len()))
}

const NOT_UNDER: &str = "(not under the dispatcher)";

fn pct(n: u64, total: u64) -> f64 {
    100.0 * n as f64 / total.max(1) as f64
}

fn print_table(title: &str, rows: &BTreeMap<String, u64>, total: u64, top: usize) {
    println!("\n### by {title}\n\n| bytes | % | what |\n|---:|---:|---|");
    let mut rows: Vec<_> = rows.iter().collect();
    rows.sort_by_key(|(k, v)| (std::cmp::Reverse(**v), (*k).clone()));
    for (k, v) in rows.into_iter().take(top) {
        println!("| {v} | {:.1} | {k} |", pct(*v, total));
    }
}

/// Prints the markdown report for `elf`; `generated` is the dump of the generated server code, to count resources.
pub fn print(elf: &Path, generated: Option<&Path>, top: usize) -> Res<()> {
    let (a, arms) = attribute(elf, generated)?;
    let under_dispatcher = a.total - a.by_dispatcher.get(NOT_UNDER).copied().unwrap_or(0);
    let serdes_own: u64 = a.serdes.values().map(|s| s.own).sum();
    let copies: usize = a.serdes.values().map(|s| s.copies.len()).sum();
    let beyond: u64 = a.serdes.values().map(SerdesImpl::beyond_first_copy).sum();
    println!(
        "{} functions, {} B of code; under the dispatcher: {under_dispatcher} B; WireWeaver code (a frame in a ww_*/shrink_wrap/wire_weaver/exact_units crate or the dispatcher): {} B",
        a.functions, a.total, a.wire_weaver
    );
    println!(
        "serdes: {serdes_own} B of own code in {} impls and {copies} copies, {beyond} B of it beyond the largest copy of each impl",
        a.serdes.len()
    );
    print_table(
        "crate of the innermost frame",
        &a.by_crate,
        a.total,
        top.min(25),
    );
    print_table(
        "dispatcher function it is inlined into",
        &a.by_dispatcher,
        a.total,
        top,
    );
    print_table("innermost function", &a.by_function, a.total, top * 4);
    print_serdes(&a, generated.is_some(), arms, top * 4);
    Ok(())
}

fn print_serdes(a: &Attribution, with_arms: bool, arms: usize, top: usize) {
    let gen_ = (with_arms).then_some(arms);
    println!("\n### serdes impls of a type (innermost one owns the bytes)\n");
    if let Some(n) = gen_ {
        println!("{n} resource arms in the dispatcher\n");
    }
    let resources_col = if with_arms { " resources |" } else { "" };
    println!("| dir | type | own B | incl. nested B | copies | B beyond 1st copy |{resources_col}");
    println!(
        "|---|---|---:|---:|---:|---:|{}",
        if with_arms { "---:|" } else { "" }
    );
    let mut rows: Vec<_> = a.serdes.iter().collect();
    rows.sort_by_key(|((_, t), s)| (std::cmp::Reverse(s.own), t.clone()));
    for ((dir, ty), s) in rows.into_iter().take(top) {
        let d = if *dir == Dir::Ser { "ser" } else { "des" };
        let res = if with_arms {
            format!(" {} |", resources_of(s))
        } else {
            String::new()
        };
        let shown: String = ty.chars().take(110).collect();
        println!(
            "| {d} | `{shown}` | {} | {} | {} | {} |{res}",
            s.own,
            s.with_nested,
            s.copies.len(),
            s.beyond_first_copy()
        );
    }
}

/// "N" arms naming the type or one around it, "N + protocol" or "protocol" for request/event framing.
fn resources_of(s: &SerdesImpl) -> String {
    match (s.resources, s.protocol) {
        (0, true) => "protocol".into(),
        (n, true) => format!("{n} + protocol"),
        (0, false) => "?".into(),
        (n, false) => n.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crates_of_paths() {
        let reg = "/home/u/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/embassy-usb-0.6.0/src/lib.rs";
        assert_eq!(crate_of(reg), "embassy-usb");
        let git = "/home/u/.cargo/git/checkouts/wire_weaver-01e81e7938a16ec4/8b7b80e/shrink_wrap/src/buf_reader.rs";
        assert_eq!(crate_of(git), "shrink_wrap");
        let root = "/home/u/.cargo/git/checkouts/exact_units-0123456789abcdef/6d79d37/src/lib.rs";
        assert_eq!(crate_of(root), "exact_units");
        let core =
            "/home/u/.rustup/toolchains/s/lib/rustlib/src/rust/library/core/src/slice/index.rs";
        assert_eq!(crate_of(core), "core (std library)");
        assert_eq!(
            crate_of("/rustc/abc/library/core/src/fmt.rs"),
            "core (std library)"
        );
        assert_eq!(crate_of("src/main.rs"), "(this crate)");
        let out = "/t/release/build/embassy-stm32-0123456789abcdef/out/_generated.rs";
        assert_eq!(crate_of(out), "embassy-stm32");
    }

    #[test]
    fn serdes_frames() {
        let ser = "<ww_uid::UniqueId as shrink_wrap::traits::SerializeShrinkWrap>::ser_shrink_wrap";
        assert_eq!(serdes(ser), Some((Dir::Ser, "ww_uid::UniqueId".into())));
        let des = "<ww_link::ClientMeta<'_> as shrink_wrap::traits::DeserializeShrinkWrap>::des_shrink_wrap";
        assert_eq!(serdes(des), Some((Dir::Des, "ww_link::ClientMeta".into())));
        let w = "<shrink_wrap::buf_writer::BufWriter>::write::<ww_uid::UniqueId>";
        assert_eq!(serdes(w), Some((Dir::Ser, "ww_uid::UniqueId".into())));
        let r = "shrink_wrap::buf_reader::BufReader::read_nested<a::B<c::D, e::F>, G>";
        assert_eq!(serdes(r), Some((Dir::Des, "a::B<c::D, e::F>".into())));
        assert_eq!(serdes("core::option::Option::map"), None);
    }

    #[test]
    fn dispatcher_frames() {
        let g = Generated::parse("fn time_map_send() {}\nasync fn process_root() {}\n");
        assert_eq!(
            dispatcher_name("a::process_root_clock::{closure#0}", &g),
            Some("process_root_clock")
        );
        assert_eq!(
            dispatcher_name("x::time_map_send", &g),
            Some("time_map_send")
        );
        assert_eq!(dispatcher_name("core::mem::replace", &g), None);
    }

    #[test]
    fn resource_arms() {
        let text = "async fn process_root() {\n 0u32 => match request.kind {\n  Foo(x) => 1,\n }\n 1u32 => match &request.kind {\n  Bar => FooBar,\n }\n}\n";
        let g = Generated::parse(text);
        assert_eq!(g.arms.len(), 2);
        assert_eq!(g.arms_naming("a::Foo"), 1);
        assert_eq!(g.arms_naming("RefVec<a::Bar>"), 1);
        assert_eq!(g.arms_naming("a::Baz"), 0);
    }
}
