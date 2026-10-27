//! The ELF proof of the units budget (TEST-27): the `plain_*` and `fixed_*` functions of the bench, built for each
//! Cortex-M core at each shipping opt-level, disassembled with `llvm-objdump` and compared instruction by
//! instruction. Identical instructions mean identical cycles on any silicon, so only a pair that differs needs a
//! measurement on a board.
//!
//! What is compared, per function: the mnemonic and operands with addresses removed (a branch keeps its relative
//! offset, a call keeps its callee, a `pc`-relative literal keeps the word it loads). A call to a callee of another
//! name (`PlainStatus::ser_shrink_wrap` against `FixedStatus::ser_shrink_wrap`) is followed and compared the same way.
//! A literal that points into the image (a panic location in read-only data, another function) is compared as "a
//! pointer", not by its value. A function the linker folded into its twin has no symbol of its own and is identical.

use crate::{Res, flash_of_symbol};
use object::{Object, ObjectSection};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

/// One normalized instruction: the text, and the callee when it is a call.
#[derive(Clone, Debug, PartialEq)]
struct Insn {
    text: String,
    call: Option<String>,
}

/// Functions of an ELF by demangled name, as normalized instructions.
pub struct Disassembly {
    funcs: BTreeMap<String, Vec<Insn>>,
}

/// What comparing a pair found.
#[derive(Debug, PartialEq)]
pub enum Verdict {
    Identical {
        insns: usize,
    },
    /// Where it first differs (`plain` / `fixed` side of the first different line, or the pair of callees).
    Differs {
        own: [usize; 2],
        first: String,
    },
}

impl Verdict {
    pub fn identical(&self) -> bool {
        matches!(self, Verdict::Identical { .. })
    }
}

impl Disassembly {
    pub fn of(elf: &Path) -> Res<Disassembly> {
        let data = std::fs::read(elf)?;
        let file = object::File::parse(&*data)?;
        // the address ranges of the image (code, read-only data, data, bss): a literal that falls in one is a pointer
        let ranges: Vec<(u64, u64)> = file
            .sections()
            .filter(|s| s.address() != 0 || s.size() != 0)
            .filter(|s| {
                !matches!(
                    s.kind(),
                    object::SectionKind::Metadata | object::SectionKind::Other
                )
            })
            .map(|s| (s.address(), s.address() + s.size()))
            .collect();
        let text = Command::new("llvm-objdump")
            .args(["-d", "-C", "--no-show-raw-insn"])
            .arg(elf)
            .output()?;
        if !text.status.success() {
            return Err("llvm-objdump failed".into());
        }
        let text = String::from_utf8_lossy(&text.stdout);
        let pool = |addr: u64| -> Option<u32> {
            let section = file
                .sections()
                .find(|s| addr >= s.address() && addr + 4 <= s.address() + s.size())?;
            let bytes = section.data().ok()?;
            let at = (addr - section.address()) as usize;
            Some(u32::from_le_bytes(bytes[at..at + 4].try_into().ok()?))
        };
        let mut funcs: BTreeMap<String, Vec<Insn>> = BTreeMap::new();
        let mut current: Option<(String, Vec<Insn>)> = None;
        for line in text.lines() {
            if let Some(name) = function_header(line) {
                if let Some((n, body)) = current.take() {
                    funcs.insert(n, body);
                }
                current = Some((name.to_string(), Vec::new()));
            } else if let Some((name, body)) = &mut current
                && let Some(insn) = normalize(line, name, &pool, &ranges)
            {
                body.push(insn);
            }
        }
        if let Some((n, body)) = current {
            funcs.insert(n, body);
        }
        Ok(Disassembly { funcs })
    }

    fn get(&self, name: &str) -> Option<&Vec<Insn>> {
        self.funcs.get(name)
    }

    /// Compares two functions by name, following calls whose callees are named differently.
    pub fn compare(&self, a: &str, b: &str) -> Verdict {
        let mut assumed = Vec::new();
        let own = [
            self.get(a).map_or(0, |f| f.len()),
            self.get(b).map_or(0, |f| f.len()),
        ];
        match self.same(a, b, &mut assumed) {
            Ok(()) => Verdict::Identical { insns: own[0] },
            Err(first) => Verdict::Differs { own, first },
        }
    }

    fn same(&self, a: &str, b: &str, assumed: &mut Vec<(String, String)>) -> Result<(), String> {
        if a == b || assumed.iter().any(|(x, y)| x == a && y == b) {
            return Ok(());
        }
        let (Some(fa), Some(fb)) = (self.get(a), self.get(b)) else {
            return Err(format!("no code for {a} or {b}"));
        };
        assumed.push((a.into(), b.into()));
        for (i, (x, y)) in fa.iter().zip(fb).enumerate() {
            match (&x.call, &y.call) {
                (Some(ca), Some(cb))
                    if x.text.split_whitespace().next() == y.text.split_whitespace().next() =>
                {
                    self.same(ca, cb, assumed)
                        .map_err(|e| format!("call {i}: {ca} / {cb}: {e}"))?;
                }
                _ if x == y => {}
                _ => return Err(format!("instruction {i}: `{}` / `{}`", x.text, y.text)),
            }
        }
        if fa.len() != fb.len() {
            return Err(format!("{} against {} instructions", fa.len(), fb.len()));
        }
        Ok(())
    }

    /// `plain_X` / `fixed_X` pairs of the bench: key to the two full names (`None`: folded into the other).
    pub fn pairs(&self) -> BTreeMap<String, [Option<String>; 2]> {
        let mut out: BTreeMap<String, [Option<String>; 2]> = BTreeMap::new();
        for name in self.funcs.keys() {
            let Some((_, tail)) = name.rsplit_once("unit_budget::") else {
                continue;
            };
            if let Some(k) = tail.strip_prefix("plain_") {
                out.entry(k.into()).or_default()[0] = Some(name.clone());
            } else if let Some(k) = tail.strip_prefix("fixed_") {
                out.entry(k.into()).or_default()[1] = Some(name.clone());
            }
        }
        out
    }
}

/// `00000a10 <name>:` to the name.
fn function_header(line: &str) -> Option<&str> {
    let line = line.strip_suffix(">:")?;
    let (addr, name) = line.split_once(" <")?;
    (addr.len() == 8 && addr.bytes().all(|b| b.is_ascii_hexdigit())).then_some(name)
}

/// One disassembly line to an instruction, `None` for blank lines and section headers.
fn normalize(
    line: &str,
    func: &str,
    pool: &dyn Fn(u64) -> Option<u32>,
    ranges: &[(u64, u64)],
) -> Option<Insn> {
    let (addr, rest) = line.trim_start().split_once(':')?;
    if addr.is_empty() || !addr.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let rest = rest.trim();
    if rest.is_empty() {
        return None;
    }
    // `bl 0x2e80 <__aeabi_lmul>   @ imm = #0x243c`, `beq 0xa84 <f+0x74> @ imm = #0x50`, `ldr r2, [pc, #0x5c] @ 0xaa4 <f+0x94>`
    let (insn, comment) = match rest.split_once('@') {
        Some((i, c)) => (i.trim(), Some(c.trim())),
        None => (rest, None),
    };
    // a literal pool word inside the function: `4c 37 00 00 .word 0x0000374c`
    if let Some((_, word)) = insn.split_once(".word")
        && let Ok(v) = u64::from_str_radix(word.trim().trim_start_matches("0x"), 16)
    {
        let value = if is_pointer(v, ranges) {
            "<ptr>".to_string()
        } else {
            format!("{v:#x}")
        };
        return Some(Insn {
            text: format!(".word {value}"),
            call: None,
        });
    }
    let mnemonic = insn.split_whitespace().next()?;
    if let Some(target) = insn.split_once(" <").and_then(|(_, t)| t.strip_suffix('>')) {
        let base = target.rsplit_once("+0x").map_or(target, |(b, _)| b);
        let imm = comment.unwrap_or("");
        let head = insn.split_whitespace().next().unwrap_or("");
        return Some(if base == func {
            Insn {
                text: format!("{head} <self> {imm}"),
                call: None,
            }
        } else {
            Insn {
                text: format!("{head} <call>"),
                call: Some(base.to_string()),
            }
        });
    }
    if insn.contains("[pc,") {
        let word = comment
            .and_then(|c| c.split_whitespace().next())
            .and_then(|a| u64::from_str_radix(a.trim_start_matches("0x"), 16).ok())
            .and_then(pool);
        let value = match word {
            Some(v)
                if ranges
                    .iter()
                    .any(|&(s, e)| (v as u64 & !1) >= s && (v as u64 & !1) < e) =>
            {
                "<ptr>".to_string()
            }
            Some(v) => format!("{v:#x}"),
            None => "?".to_string(),
        };
        let reg = insn.split_whitespace().nth(1).unwrap_or("");
        return Some(Insn {
            text: format!("{mnemonic} {reg} pool {value}"),
            call: None,
        });
    }
    let mut text = insn.split_whitespace().collect::<Vec<_>>().join(" ");
    // `movw r1, #0x55c0` loads the low half of an address on M4 and M33
    if mnemonic.starts_with("movw")
        && let Some((head, imm)) = text.rsplit_once('#')
        && let Ok(v) = u64::from_str_radix(imm.trim_start_matches("0x"), 16)
        && is_pointer(v, ranges)
    {
        text = format!("{head}<ptr>");
    }
    Some(Insn { text, call: None })
}

/// Whether a word is an address of the image rather than a constant (values below 0x100 never are).
fn is_pointer(v: u64, ranges: &[(u64, u64)]) -> bool {
    v >= 0x100 && ranges.iter().any(|&(s, e)| (v & !1) >= s && (v & !1) < e)
}

/// Prints the table of one build: every pair, identical or not.
pub fn print(label: &str, elf: &Path) -> Res<Vec<(String, Verdict)>> {
    let dis = Disassembly::of(elf)?;
    println!("\n{label}");
    println!(
        "{:<20} {:>8} {:>8}  verdict",
        "function", "plain B", "Fixed B"
    );
    let mut out = Vec::new();
    for (key, [plain, fixed]) in dis.pairs() {
        let (verdict, text) = match (&plain, &fixed) {
            (Some(p), Some(f)) => {
                let v = dis.compare(p, f);
                let t = match &v {
                    Verdict::Identical { insns } => format!("identical ({insns} instructions)"),
                    Verdict::Differs { own, first } => {
                        format!("DIFFERS ({} / {} instructions): {first}", own[0], own[1])
                    }
                };
                (v, t)
            }
            _ => (
                Verdict::Identical { insns: 0 },
                "identical (folded by the linker)".into(),
            ),
        };
        let size = |n: &Option<String>| {
            n.as_deref()
                .and_then(|n| flash_of_symbol(elf, n))
                .map_or("folded".to_string(), |s| s.to_string())
        };
        println!(
            "{:<20} {:>8} {:>8}  {}",
            key,
            size(&plain),
            size(&fixed),
            text
        );
        out.push((key, verdict));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: u64) -> Option<u32> {
        None
    }

    #[test]
    fn addresses_drop_out_of_a_branch() {
        let a = normalize(
            "     a30:      \tbeq\t0xa84 <f+0x74> @ imm = #0x50",
            "f",
            &none,
            &[],
        );
        let b = normalize(
            "     b52:      \tbeq\t0xba2 <g+0x6e> @ imm = #0x50",
            "g",
            &none,
            &[],
        );
        assert_eq!(a, b);
    }

    #[test]
    fn a_call_keeps_its_callee() {
        let i = normalize(
            "     a40:      \tbl\t0x2e80 <__aeabi_lmul>   @ imm = #0x243c",
            "f",
            &none,
            &[],
        )
        .unwrap();
        assert_eq!(i.call.as_deref(), Some("__aeabi_lmul"));
    }

    #[test]
    fn a_literal_that_points_into_the_image_is_a_pointer() {
        let pool = |_: u64| Some(0x0000_0b40u32);
        let i = normalize(
            "     a44:      \tldr\tr2, [pc, #0x5c]         @ 0xaa4 <f+0x94>",
            "f",
            &pool,
            &[(0xb00, 0xb80)],
        )
        .unwrap();
        assert_eq!(i.text, "ldr r2, pool <ptr>");
    }
}
