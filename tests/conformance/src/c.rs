//! C as a consumer, where C exists for the case's type:
//!
//! * a single number, bool, string or byte list: written and read by the C runtime (`shrink_wrap/c`, SW-43)
//!   through `shrink_wrap_c`'s FFI, the calls chosen from the type in the API model (as generated C would make
//!   them);
//! * a `fixed_layout` type (SW-44, SW-45): the generated C reader (`FixedLayout::write_c`), compiled with `cc`
//!   and run on the golden bytes, every field compared with the case's value;
//! * a `head_tail` type (SW-46): its generated C reader and tail walker, the same way.
//!
//! The requantize cases (STD-63) run through `sw_requantize()` (`shrink_wrap/c/sw_requantize.c`, CLT-54) in one
//! generated program: [requantize].
//!
//! The generated C readers have no writers yet, so those cases are decode only (their gap mark says so). Every
//! other type waits for C generated from the model (API-26).

use crate::corpus::{Case, Direction, RequantizeCase};
use crate::dynamic::{bundle, property_ty};
use crate::json::int;
use conformance_api::*;
use serde_json::Value as J;
use shrink_wrap_c::CReader;
use shrink_wrap_c::model::{Op, Val, c_bytes, c_read, expected};
use shrink_wrap_c::requantize::{CFactor, c_overflow, c_rounding, c_storage};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;
use wire_weaver::shrink_wrap::fixed_layout::FixedLayout;
use wire_weaver::shrink_wrap::head_tail::HeadTail;
use ww_numeric::{NumericAnyTypeOwned, NumericBaseType as B};
use ww_self::{FieldsOwned, TypeOwned};
use ww_si::exact_units::Repr;

const PREFIX: &str = "t_";

type WriteC = fn(&str, &mut dyn std::fmt::Write) -> std::fmt::Result;

/// The C readers of the fixed-layout types, by property name.
fn fixed_types() -> Vec<(&'static str, WriteC)> {
    vec![
        ("fixed_numbers", <FixedNumbers as FixedLayout>::write_c),
        ("fixed_bits", <FixedBits as FixedLayout>::write_c),
        ("fixed_nested", <FixedOuter as FixedLayout>::write_c),
        ("fixed_maybe", <FixedMaybe as FixedLayout>::write_c),
        ("fixed_op", <FixedOp as FixedLayout>::write_c),
    ]
}

/// Field kinds of the `head_tail` types (they are not in the API model: their own format).
#[derive(Clone, Copy)]
enum Ht {
    U8,
    Bit,
    OptionU16,
    ListU16,
    Str,
}

fn head_tail_types() -> Vec<(&'static str, &'static [(&'static str, Ht)])> {
    vec![(
        "HtRecord",
        &[
            ("ch", Ht::U8),
            ("maybe", Ht::OptionU16),
            ("list", Ht::ListU16),
            ("name", Ht::Str),
            ("on", Ht::Bit),
        ],
    )]
}

pub struct C;

impl crate::Consumer for C {
    const NAME: &'static str = "c";

    fn encode(&self, case: &Case) -> Option<Result<Vec<u8>, String>> {
        let ops = runtime_ops(case)?;
        Some(ops.and_then(|ops| c_bytes(&ops, 256).map_err(|e| format!("C error {e}"))))
    }

    fn decode(&self, case: &Case) -> Option<crate::DecodeResult> {
        if let Some(result) = generated_readers().get(&case.name) {
            return Some(result.clone());
        }
        let ops = runtime_ops(case)?;
        let golden = case.golden();
        let ops = match ops {
            Ok(ops) => ops,
            // a rejected value has no ops: read with those of its type (the value doesn't matter)
            Err(_) if case.direction == Direction::Reject => match reject_ops(case) {
                Some(ops) => ops,
                None => return Some(Ok(Err("no ops to read with".into()))),
            },
            Err(e) => return Some(Ok(Err(e))),
        };
        let mut rd = CReader::new(&golden);
        let mut got = vec![];
        c_read(&mut rd, &ops, &mut got);
        if rd.error() != 0 {
            return Some(Err(format!("C error {}", rd.error())));
        }
        if let Some(bound) = max_len(case)
            && let [Val::B(b)] = got.as_slice()
            && str_or_bytes_len(case, b) > bound
        {
            return Some(Err(format!("{} elements, at most {bound}", b.len())));
        }
        if case.direction == Direction::Reject {
            return Some(Ok(Err(format!("read {got:?}"))));
        }
        let mut want = vec![];
        expected(&ops, &mut want);
        Some(Ok(if got == want {
            Ok(())
        } else {
            Err(format!("read {got:?}, expected {want:?}"))
        }))
    }
}

// ---------------------------------------------------------------------------------------------------------------
// the C runtime

fn base_of(ty: &TypeOwned) -> Option<&B> {
    match ty {
        TypeOwned::NumericAny(NumericAnyTypeOwned::Base(b)) => Some(b),
        _ => None,
    }
}

/// `MaxLen` bound of the case's type.
fn max_len(case: &Case) -> Option<usize> {
    match property_ty(&case.ty)? {
        TypeOwned::MaxLen { max_len, .. } => max_len.fixed().ok().map(|n| n as usize),
        _ => None,
    }
}

fn str_or_bytes_len(case: &Case, b: &[u8]) -> usize {
    match property_ty(&case.ty) {
        Some(TypeOwned::MaxLen { ty, .. }) if matches!(**ty, TypeOwned::String) => {
            String::from_utf8_lossy(b).chars().count().max(b.len())
        }
        _ => b.len(),
    }
}

/// The C runtime calls for the case's type, `None` when it isn't a single number, bool, string or byte list.
fn runtime_ops(case: &Case) -> Option<Result<Vec<Op>, String>> {
    let ty = property_ty(&case.ty)?;
    let (ty, bound) = match ty {
        TypeOwned::MaxLen { max_len, ty } => (&**ty, max_len.fixed().ok().map(|n| n as usize)),
        ty => (ty, None),
    };
    let op = |j: &J| -> Option<Result<Op, String>> {
        let num = || int(j).ok_or_else(|| format!("{j} is not an integer"));
        Some(match ty {
            TypeOwned::Bool => j
                .as_bool()
                .map(Op::Bool)
                .ok_or_else(|| format!("{j} is not a bool")),
            TypeOwned::String => match j.as_str() {
                Some(s) if bound.is_some_and(|b| s.len() > b) => {
                    Err(format!("over the bound of {bound:?}"))
                }
                Some(s) => Ok(Op::Str(s.into())),
                None => Err(format!("{j} is not a string")),
            },
            TypeOwned::Vec(inner) if base_of(inner) == Some(&B::U8) => {
                let bytes: Option<Vec<u8>> = j
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .map(|b| int(b).and_then(|b| u8::try_from(b).ok()))
                            .collect()
                    })
                    .unwrap_or(None);
                match bytes {
                    Some(b) if bound.is_some_and(|n| b.len() > n) => {
                        Err(format!("over the bound of {bound:?}"))
                    }
                    Some(b) => Ok(Op::Bytes(b)),
                    None => Err(format!("{j} is not a byte list")),
                }
            }
            ty => {
                let base = base_of(ty)?;
                let f = || j.as_f64().ok_or_else(|| format!("{j} is not a float"));
                match base {
                    B::U8 => num().map(|v| Op::U8(v as u8)),
                    B::U16 => num().map(|v| Op::U16(v as u16)),
                    B::U32 => num().map(|v| Op::U32(v as u32)),
                    B::U64 => num().map(|v| Op::U64(v as u64)),
                    B::I8 => num().map(|v| Op::I8(v as i8)),
                    B::I16 => num().map(|v| Op::I16(v as i16)),
                    B::I32 => num().map(|v| Op::I32(v as i32)),
                    B::I64 => num().map(|v| Op::I64(v as i64)),
                    B::F32 => f().map(|v| Op::F32((v as f32).to_bits())),
                    B::F64 => f().map(|v| Op::F64(v.to_bits())),
                    B::UNib32 => num().map(|v| Op::UNib32(v as u32)),
                    B::UVlq32 => num().map(|v| Op::UVlq32(v as u32)),
                    B::UVlq64 => num().map(|v| Op::UVlq64(v as u64)),
                    B::Nibble => num().map(|v| Op::Nib(v as u8)),
                    B::UB(_) | B::IB(_) => {
                        let n = match base {
                            B::UB(bits) => bits.0,
                            B::IB(bits) => bits.0,
                            _ => unreachable!(),
                        };
                        num().map(|v| {
                            let raw = (v as u64) & (u64::MAX >> (64 - n as u32));
                            match n {
                                0..=8 => Op::Un8(n, raw as u8),
                                9..=16 => Op::Un16(n, raw as u16),
                                17..=32 => Op::Un32(n, raw as u32),
                                _ => Op::Un64(n, raw),
                            }
                        })
                    }
                    // no 128-bit numbers in the C runtime
                    _ => return None,
                }
            }
        })
    };
    if case.direction == Direction::Reject && case.value.is_null() {
        return Some(Err("rejected case".into()));
    }
    Some(op(&case.value)?.map(|op| vec![op]))
}

/// The read of a rejected case's type.
fn reject_ops(case: &Case) -> Option<Vec<Op>> {
    let ty = match property_ty(&case.ty)? {
        TypeOwned::MaxLen { ty, .. } => &**ty,
        ty => ty,
    };
    Some(vec![match ty {
        TypeOwned::String => Op::Str(String::new()),
        TypeOwned::Vec(_) => Op::Bytes(vec![]),
        _ => return None,
    }])
}

// ---------------------------------------------------------------------------------------------------------------
// generated readers: one C program for every case of a fixed-layout or head_tail type

/// Result of every case with a generated C reader, by case name: built once per test process.
fn generated_readers() -> &'static HashMap<String, crate::DecodeResult> {
    static RESULTS: OnceLock<HashMap<String, crate::DecodeResult>> = OnceLock::new();
    RESULTS.get_or_init(run_generated)
}

fn run_generated() -> HashMap<String, crate::DecodeResult> {
    let corpus = crate::corpus::load();
    let fixed = fixed_types();
    let ht = head_tail_types();
    let mut header = String::new();
    wire_weaver::shrink_wrap::fixed_layout::write_c_header(
        &mut header,
        "CONFORMANCE_FIXED_H",
        PREFIX,
        &std::iter::once(<FixedInner as FixedLayout>::write_c as WriteC)
            .chain(fixed.iter().map(|(_, f)| *f))
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let mut ht_header = String::new();
    wire_weaver::shrink_wrap::head_tail::write_c_header(
        &mut ht_header,
        "CONFORMANCE_HT_H",
        PREFIX,
        &[<HtRecord as HeadTail>::write_c_ht],
    )
    .unwrap();
    // the head_tail header repeats the fixed prelude under its own guard
    let mut program = String::from("#include <stdio.h>\n#include <string.h>\n");
    program.push_str(&header);
    program.push_str(&ht_header);
    let mut names = vec![];
    for case in &corpus.cases {
        let is_fixed = fixed.iter().any(|(k, _)| *k == case.ty);
        let ht_fields = ht.iter().find(|(k, _)| *k == case.ty).map(|(_, f)| *f);
        if !is_fixed && ht_fields.is_none() || case.direction == Direction::RejectValue {
            continue;
        }
        let golden = case.golden();
        let idx = names.len();
        names.push(case.name.clone());
        let mut checks = vec![];
        let result = if let Some(fields) = ht_fields {
            head_tail_checks(&case.ty, fields, &case.value, golden.len(), &mut checks)
        } else {
            let ty = property_ty(&case.ty).expect("fixed types are in the API model");
            fixed_checks(ty, &case.value, "p", Some(golden.len()), &mut checks)
        };
        if let Err(e) = result {
            panic!("case '{}': no C checks for it: {e}", case.name);
        }
        let bytes: Vec<String> = golden.iter().map(|b| format!("0x{b:02x}")).collect();
        let _ = writeln!(
            program,
            "static const char *case_{idx}(void) {{\n    static const uint8_t buf[] = {{{}0}};\n    const uint8_t *p = buf;\n    (void)p;",
            bytes.iter().map(|b| format!("{b}, ")).collect::<String>()
        );
        for (pre, check) in checks {
            if !pre.is_empty() {
                let _ = writeln!(program, "    {pre}");
            }
            let _ = writeln!(
                program,
                "    if (!({check})) {{ return \"{}\"; }}",
                check.replace('\\', "\\\\").replace('"', "\\\"")
            );
        }
        let _ = writeln!(program, "    return NULL;\n}}");
    }
    let _ = writeln!(program, "int main(void) {{\n    const char *e;");
    for idx in 0..names.len() {
        let _ = writeln!(
            program,
            "    e = case_{idx}();\n    printf(\"%d %s\\n\", {idx}, e ? e : \"ok\");"
        );
    }
    let _ = writeln!(program, "    return 0;\n}}");
    let output = compile_and_run("conformance", &program, &[]);
    let mut results = HashMap::new();
    for line in output.lines() {
        let (idx, msg) = line.split_once(' ').expect("index and message");
        let name = names[idx.parse::<usize>().expect("index")].clone();
        let r = if msg == "ok" {
            Ok(Ok(()))
        } else {
            Ok(Err(format!("C check failed: {msg}")))
        };
        results.insert(name, r);
    }
    assert_eq!(results.len(), names.len(), "every case printed: {output}");
    results
}

/// `program` built as `name` (a name of its own per test: nextest runs tests in parallel processes) with the C
/// runtime's `sources` (in `shrink_wrap/c`, which is on the include path), and run.
fn compile_and_run(name: &str, program: &str, sources: &[&str]) -> String {
    // target/conformance_c, next to the build of this crate
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/conformance_c");
    std::fs::create_dir_all(&dir).unwrap();
    let runtime = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../shrink_wrap/c");
    let src = dir.join(format!("{name}.c"));
    let exe = dir.join(name);
    std::fs::write(&src, program).unwrap();
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".into());
    let out = Command::new(&cc)
        .args([
            "-std=c99",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-pedantic",
            "-Wconversion",
            "-Wsign-conversion",
            "-o",
        ])
        .arg(&exe)
        .arg("-I")
        .arg(&runtime)
        .arg(&src)
        .args(sources.iter().map(|s| runtime.join(s)))
        .output()
        .unwrap_or_else(|e| panic!("{cc}: {e} (the C consumer needs a C compiler, `CC` or `cc`)"));
    assert!(
        out.status.success(),
        "the generated C doesn't build ({}):\n{}",
        src.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    let run = Command::new(&exe).output().unwrap();
    assert!(run.status.success(), "{} failed", exe.display());
    String::from_utf8(run.stdout).unwrap()
}

fn snake(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i != 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// A C literal of a number of `base`.
fn literal(j: &J, base: &B) -> Result<String, String> {
    if matches!(base, B::F32 | B::F64) {
        let v = j.as_f64().ok_or_else(|| format!("{j} is not a float"))?;
        return Ok(if matches!(base, B::F32) {
            format!("{v:?}f")
        } else {
            format!("{v:?}")
        });
    }
    let v = int(j).ok_or_else(|| format!("{j} is not an integer"))?;
    Ok(match base {
        B::I64 if v == i64::MIN as i128 => "INT64_MIN".into(),
        B::I64 | B::I32 | B::I16 | B::I8 | B::IB(_) => format!("{v}"),
        _ => format!("{v}u"),
    })
}

/// Checks of a fixed-layout value `j` of type `ty` at C pointer `p`: `(statement before, condition)` pairs;
/// `len` is the golden length for the top-level value.
fn fixed_checks(
    ty: &TypeOwned,
    j: &J,
    p: &str,
    len: Option<usize>,
    out: &mut Vec<(String, String)>,
) -> Result<(), String> {
    let ty = ty.get_in_line(bundle()).map_err(|e| format!("{e:#}"))?;
    match ty {
        TypeOwned::Struct(s) => {
            let c = format!("{PREFIX}{}", snake(&s.ident));
            let FieldsOwned::Named(fields) = &s.fields else {
                return Err("named fields expected".into());
            };
            let o = j.as_object().ok_or("an object expected")?;
            let mut variable = false;
            for f in fields.iter().filter(|f| !f.is_flag()) {
                let name = f.ident.as_deref().unwrap_or_default();
                let v = o.get(name).ok_or_else(|| format!("field {name} missing"))?;
                let acc = format!("{c}_{name}");
                variable |= field_checks(&f.ty, v, &acc, p, out)?;
            }
            if let Some(len) = len {
                let macro_ = format!("{}_LEN", c.to_uppercase());
                if variable {
                    out.push((String::new(), format!("{macro_} >= {len}u")));
                } else {
                    out.push((String::new(), format!("{macro_} == {len}u")));
                }
            }
        }
        TypeOwned::Enum(e) => {
            let c = format!("{PREFIX}{}", snake(&e.ident));
            let (variant, payload) = match j {
                J::String(v) => (v.clone(), None),
                J::Object(o) if o.len() == 1 => {
                    let (k, v) = o.iter().next().unwrap();
                    (k.clone(), Some(v))
                }
                _ => return Err(format!("{j} is not a variant")),
            };
            let def = e
                .variants
                .iter()
                .find(|v| v.ident == variant)
                .ok_or("no such variant")?;
            let vc = snake(&variant);
            out.push((
                String::new(),
                format!("{c}_tag({p}) == {}_{}", c.to_uppercase(), vc.to_uppercase()),
            ));
            if let Some(len) = len {
                out.push((String::new(), format!("{c}_len({c}_tag({p})) == {len}u")));
            }
            if let (Some(payload), FieldsOwned::Named(fields)) = (payload, &def.fields) {
                let o = payload.as_object().ok_or("variant fields expected")?;
                for f in fields.iter().filter(|f| !f.is_flag()) {
                    let name = f.ident.as_deref().unwrap_or_default();
                    let v = o.get(name).ok_or_else(|| format!("field {name} missing"))?;
                    field_checks(&f.ty, v, &format!("{c}_{vc}_{name}"), p, out)?;
                }
            }
        }
        other => return Err(format!("no fixed layout for {other:?}")),
    }
    Ok(())
}

/// Checks of one field read with accessor `acc` (the generated `<type>_<field>`); `true` for a variable one.
fn field_checks(
    ty: &TypeOwned,
    v: &J,
    acc: &str,
    p: &str,
    out: &mut Vec<(String, String)>,
) -> Result<bool, String> {
    let ty = ty.get_in_line(bundle()).map_err(|e| format!("{e:#}"))?;
    match ty {
        TypeOwned::Bool => {
            let b = v.as_bool().ok_or("a bool expected")?;
            out.push((String::new(), format!("{acc}({p}) == {}u", b as u8)));
        }
        TypeOwned::NumericAny(NumericAnyTypeOwned::Base(base)) => {
            out.push((
                String::new(),
                format!("{acc}({p}) == {}", literal(v, base)?),
            ));
        }
        TypeOwned::Array { ty: inner, .. } => {
            let items = v.as_array().ok_or("a list expected")?;
            let base = base_of(inner.get_in_line(bundle()).map_err(|e| format!("{e:#}"))?)
                .ok_or("arrays of numbers only")?;
            for (i, item) in items.iter().enumerate() {
                let read = if *base == B::U8 {
                    format!("{acc}({p})[{i}]")
                } else {
                    format!("{acc}({p}, {i}u)")
                };
                out.push((String::new(), format!("{read} == {}", literal(item, base)?)));
            }
        }
        TypeOwned::Struct(_) | TypeOwned::Enum(_) => {
            fixed_checks(ty, v, &format!("{acc}({p})"), None, out)?;
        }
        TypeOwned::Option { some_ty } => {
            out.push((
                String::new(),
                format!("{acc}_flag({p}) == {}u", !v.is_null() as u8),
            ));
            if !v.is_null() {
                let some = some_ty
                    .get_in_line(bundle())
                    .map_err(|e| format!("{e:#}"))?;
                match base_of(some) {
                    // a number payload is read by the field's accessor
                    Some(base) => out.push((
                        String::new(),
                        format!("{acc}({p}) == {}", literal(v, base)?),
                    )),
                    None => fixed_checks(some, v, &format!("{acc}({p})"), None, out)?,
                }
            }
            return Ok(true);
        }
        other => return Err(format!("no C reader check for {other:?}")),
    }
    Ok(false)
}

/// Checks of a `head_tail` value: the head's fields, the tail walked to the end of the golden bytes.
fn head_tail_checks(
    ty: &str,
    fields: &[(&str, Ht)],
    j: &J,
    len: usize,
    out: &mut Vec<(String, String)>,
) -> Result<(), String> {
    let c = format!("{PREFIX}{}", snake(ty));
    let upper = c.to_uppercase();
    let o = j.as_object().ok_or("an object expected")?;
    out.push((
        format!(
            "const uint8_t *t = p + {upper}_HEAD_LEN; {c}_tail_t tl; int walked = {c}_tail(p, &t, p + {len}u, &tl);"
        ),
        format!("walked == 0 && t == p + {len}u"),
    ));
    for (name, kind) in fields {
        let v = o
            .get(*name)
            .ok_or_else(|| format!("field {name} missing"))?;
        let acc = format!("{c}_{name}");
        match kind {
            Ht::U8 => out.push((
                String::new(),
                format!("{acc}(p) == {}", literal(v, &B::U8)?),
            )),
            Ht::Bit => out.push((
                String::new(),
                format!(
                    "{acc}(p) == {}u",
                    v.as_bool().ok_or("a bool expected")? as u8
                ),
            )),
            Ht::OptionU16 => {
                out.push((
                    String::new(),
                    format!("{acc}_flag(p) == {}u", !v.is_null() as u8),
                ));
                if v.is_null() {
                    out.push((String::new(), format!("tl.{name} == NULL")));
                } else {
                    out.push((
                        String::new(),
                        format!(
                            "tl.{name} != NULL && sw_fixed_u16(tl.{name}) == {}",
                            literal(v, &B::U16)?
                        ),
                    ));
                }
            }
            Ht::ListU16 => {
                let items = v.as_array().ok_or("a list expected")?;
                out.push((String::new(), format!("{acc}_count(p) == {}u", items.len())));
                for (i, item) in items.iter().enumerate() {
                    out.push((
                        String::new(),
                        format!(
                            "sw_fixed_u16(tl.{name} + {}u) == {}",
                            2 * i,
                            literal(item, &B::U16)?
                        ),
                    ));
                }
            }
            Ht::Str => {
                let s = v.as_str().ok_or("a string expected")?;
                out.push((String::new(), format!("{acc}_count(p) == {}u", s.len())));
                out.push((
                    String::new(),
                    format!(
                        "memcmp(tl.{name}, \"{}\", {}u) == 0",
                        s.escape_default(),
                        s.len()
                    ),
                ));
            }
        }
    }
    Ok(())
}

/// A requantize case (STD-63) through the C requantize (`sw_requantize.c`, CLT-54): the factor's constants worked
/// out on the host from the `Magnitude` the model gives (`CFactor::of`) and emitted into a generated C program, as a
/// code generator would; one program runs every case.
pub fn requantize(case: &RequantizeCase) -> Result<(), String> {
    let got = requantize_results()
        .get(&case.name)
        .ok_or("not in the generated C program")?
        .clone()?;
    crate::requantize::compare(got, case)
}

fn requantize_results() -> &'static HashMap<String, Result<Option<Repr>, String>> {
    static RESULTS: OnceLock<HashMap<String, Result<Option<Repr>, String>>> = OnceLock::new();
    RESULTS.get_or_init(run_requantize)
}

fn run_requantize() -> HashMap<String, Result<Option<Repr>, String>> {
    const PRELUDE: &str = r#"#include <inttypes.h>
#include <stdio.h>
#include <string.h>
#include "sw_requantize.h"

static sw_num f64_of(uint64_t bits) {
    sw_num x;
    x.kind = SW_NUM_F64;
    memcpy(&x.v.f, &bits, sizeof bits);
    return x;
}

static void put(int idx, const sw_requantizer *q, sw_num x) {
    sw_num n;
    uint64_t bits;
    if (!sw_requantize(q, x, &n)) {
        printf("%d none\n", idx);
    } else if (n.kind == SW_NUM_INT) {
        printf("%d i %" PRId64 "\n", idx, n.v.i);
    } else if (n.kind == SW_NUM_UINT) {
        printf("%d u %" PRIu64 "\n", idx, n.v.u);
    } else {
        memcpy(&bits, &n.v.f, sizeof bits);
        printf("%d f %" PRIx64 "\n", idx, bits);
    }
}

int main(void) {
"#;
    let corpus = crate::corpus::load();
    let mut results = HashMap::new();
    let mut program = String::from(PRELUDE);
    let mut names = vec![];
    for case in &corpus.requantize {
        match requantize_case(case) {
            Ok((q, x)) => {
                let idx = names.len();
                names.push(case.name.clone());
                let _ = writeln!(
                    program,
                    "    {{\n        static const sw_requantizer q = {q};\n        put({idx}, &q, {x});\n    }}"
                );
            }
            Err(e) => {
                results.insert(case.name.clone(), Err(e));
            }
        }
    }
    program.push_str("    return 0;\n}\n");
    let output = compile_and_run("requantize", &program, &["sw_requantize.c"]);
    for line in output.lines() {
        let mut parts = line.split(' ');
        let (idx, kind, v) = (parts.next(), parts.next(), parts.next());
        let name = names[idx.and_then(|i| i.parse::<usize>().ok()).expect("an index")].clone();
        let int = |v: Result<i128, std::num::ParseIntError>| {
            v.map(|i| Some(Repr::Int(i))).map_err(|e| e.to_string())
        };
        let got = match (kind, v) {
            (Some("none"), None) => Ok(None),
            (Some("i"), Some(v)) => int(v.parse::<i64>().map(i128::from)),
            (Some("u"), Some(v)) => int(v.parse::<u64>().map(i128::from)),
            (Some("f"), Some(v)) => u64::from_str_radix(v, 16)
                .map(|b| Some(Repr::Float(f64::from_bits(b))))
                .map_err(|e| e.to_string()),
            _ => Err(format!("C printed '{line}'")),
        };
        results.insert(name, got);
    }
    assert_eq!(
        results.len(),
        corpus.requantize.len(),
        "every case printed: {output}"
    );
    results
}

/// A case in C: the requantizer's initializer, with the factor's constants as a code generator emits them, and the
/// input number's expression.
fn requantize_case(case: &RequantizeCase) -> Result<(String, String), String> {
    use crate::requantize as rq;
    let factor = CFactor::of(&rq::factor(case)?);
    let q = format!(
        "{{{}, {}, {}, {}}}",
        factor.c_initializer(),
        c_storage(rq::storage(&case.to)?),
        c_rounding(rq::rounding(&case.rounding)?),
        c_overflow(rq::overflow(&case.overflow)?),
    );
    let beyond = |i: i128| format!("{i} is beyond 64 bits, C takes up to 64");
    let x = match rq::repr(&case.value)? {
        Repr::Int(i) if i < 0 => {
            let i = i64::try_from(i).map_err(|_| beyond(i))?;
            // -2^63 has no literal of its own
            format!(
                "(sw_num){{SW_NUM_INT, {{.i = -INT64_C({}) - 1}}}}",
                (i + 1).unsigned_abs()
            )
        }
        Repr::Int(i) => {
            let u = u64::try_from(i).map_err(|_| beyond(i))?;
            format!("(sw_num){{SW_NUM_UINT, {{.u = UINT64_C({u})}}}}")
        }
        Repr::Float(f) => format!("f64_of(UINT64_C({:#x}))", f.to_bits()),
    };
    Ok((q, x))
}
