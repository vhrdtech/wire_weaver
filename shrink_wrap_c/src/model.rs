//! A sequence of writes / reads run through the Rust crate and through the C runtime, to compare the two.
//!
//! The Rust side is the reference. It stops at the first error, as code using `?` does; the C side keeps going and
//! must then give 0 for every read and write nothing (the SW-41 latch), with the first error kept.

use crate::{CReader, CWriter, code};
use shrink_wrap::buf_writer::UnsizedBuilder;
use shrink_wrap::{BufReader, BufWriter, Error, Nibble};

/// One write, read back with the matching read.
#[derive(Clone, Debug)]
pub enum Op {
    Bool(bool),
    Nib(u8),
    Un8(u8, u8),
    Un16(u8, u16),
    Un32(u8, u32),
    Un64(u8, u64),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    /// f32 by its bits, so NaNs compare
    F32(u32),
    F64(u64),
    UNib32(u32),
    UVlq32(u32),
    UVlq64(u64),
    /// bytes without a length, read back with their known length
    Raw(Vec<u8>),
    Bytes(Vec<u8>),
    Str(String),
    /// a bare length at the back
    RevLen(u16),
    AlignByte,
    /// an `Unsized` value: size at the back, read back through a split reader
    Unsized(Vec<Op>),
}

/// What one read gives.
#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    N(u64),
    B(Vec<u8>),
}

impl Val {
    fn is_zero(&self) -> bool {
        match self {
            Val::N(n) => *n == 0,
            Val::B(b) => b.is_empty(),
        }
    }
}

/// Number of values the reads of `ops` give.
pub fn read_count(ops: &[Op]) -> usize {
    ops.iter()
        .map(|op| match op {
            Op::AlignByte => 0,
            Op::Unsized(inner) => read_count(inner),
            _ => 1,
        })
        .sum()
}

/// The values the reads give back after the writes of `ops` succeeded.
pub fn expected(ops: &[Op], out: &mut Vec<Val>) {
    let mask = |bits: u8, v: u64| {
        if bits >= 64 {
            v
        } else {
            v & ((1u64 << bits) - 1)
        }
    };
    for op in ops {
        let v = match op {
            Op::Bool(b) => Val::N(*b as u64),
            Op::Nib(n) => Val::N((*n & 0xF) as u64),
            Op::Un8(b, v) => Val::N(mask(*b, *v as u64)),
            Op::Un16(b, v) => Val::N(mask(*b, *v as u64)),
            Op::Un32(b, v) => Val::N(mask(*b, *v as u64)),
            Op::Un64(b, v) => Val::N(mask(*b, *v)),
            Op::U8(v) => Val::N(*v as u64),
            Op::U16(v) => Val::N(*v as u64),
            Op::U32(v) => Val::N(*v as u64),
            Op::U64(v) => Val::N(*v),
            Op::I8(v) => Val::N(*v as i64 as u64),
            Op::I16(v) => Val::N(*v as i64 as u64),
            Op::I32(v) => Val::N(*v as i64 as u64),
            Op::I64(v) => Val::N(*v as u64),
            Op::F32(v) => Val::N(*v as u64),
            Op::F64(v) => Val::N(*v),
            Op::UNib32(v) => Val::N(*v as u64),
            Op::UVlq32(v) => Val::N(*v as u64),
            Op::UVlq64(v) => Val::N(*v),
            Op::Raw(b) | Op::Bytes(b) => Val::B(b.clone()),
            Op::Str(s) => Val::B(s.as_bytes().to_vec()),
            Op::RevLen(l) => Val::N(*l as u64),
            Op::AlignByte => continue,
            Op::Unsized(inner) => {
                expected(inner, out);
                continue;
            }
        };
        out.push(v);
    }
}

// ----------------------------------------------------------------------------------------------------------------
// Rust side
// ----------------------------------------------------------------------------------------------------------------

pub fn rust_write(wr: &mut BufWriter, ops: &[Op]) -> Result<(), Error> {
    for op in ops {
        match op {
            Op::Bool(b) => wr.write_bool(*b)?,
            Op::Nib(n) => wr.write_nib(Nibble::new_masked(*n))?,
            Op::Un8(b, v) => wr.write_un8(*b, *v)?,
            Op::Un16(b, v) => wr.write_un16(*b, *v)?,
            Op::Un32(b, v) => wr.write_un32(*b, *v)?,
            Op::Un64(b, v) => wr.write_un64(*b, *v)?,
            Op::U8(v) => wr.write_u8(*v)?,
            Op::U16(v) => wr.write_u16(*v)?,
            Op::U32(v) => wr.write_u32(*v)?,
            Op::U64(v) => wr.write_u64(*v)?,
            Op::I8(v) => wr.write_i8(*v)?,
            Op::I16(v) => wr.write_i16(*v)?,
            Op::I32(v) => wr.write_i32(*v)?,
            Op::I64(v) => wr.write_i64(*v)?,
            Op::F32(v) => wr.write_f32(f32::from_bits(*v))?,
            Op::F64(v) => wr.write_f64(f64::from_bits(*v))?,
            Op::UNib32(v) => wr.write_unib32(*v)?,
            Op::UVlq32(v) => wr.write_uvlq32(*v)?,
            Op::UVlq64(v) => wr.write_uvlq64(*v)?,
            Op::Raw(b) => wr.write_raw_slice(b)?,
            Op::Bytes(b) => wr.write_bytes(b)?,
            Op::Str(s) => wr.write_str(s)?,
            Op::RevLen(l) => {
                wr.write_rev_len(*l as usize)?;
            }
            Op::AlignByte => wr.align_byte(),
            Op::Unsized(inner) => {
                let builder = UnsizedBuilder::new(wr)?;
                rust_write(wr, inner)?;
                builder.finish(wr)?;
            }
        }
    }
    Ok(())
}

/// Rust bytes of `ops` in a buffer of `cap` bytes, or the first error.
pub fn rust_bytes(ops: &[Op], cap: usize) -> Result<Vec<u8>, Error> {
    let mut buf = vec![0xA5; cap];
    let mut wr = BufWriter::new(&mut buf);
    rust_write(&mut wr, ops)?;
    Ok(wr.finish()?.to_vec())
}

/// Where the Rust reader stopped.
#[derive(Debug)]
pub struct RustReadErr {
    pub err: Error,
    /// Position of the top level reader the C one must be at: before the failing read (a C read that fails moves
    /// nothing), or after the split when the error is inside an `Unsized` value.
    pub pos: (usize, u8),
}

pub fn rust_read(rd: &mut BufReader, ops: &[Op], out: &mut Vec<Val>) -> Result<(), RustReadErr> {
    for op in ops {
        let pos = rd.pos();
        let fail = |err| RustReadErr { err, pos };
        let v = match op {
            Op::Bool(_) => Val::N(rd.read_bool().map_err(fail)? as u64),
            Op::Nib(_) => Val::N(rd.read_nib_value().map_err(fail)? as u64),
            Op::Un8(b, _) => Val::N(rd.read_un8(*b).map_err(fail)? as u64),
            Op::Un16(b, _) => Val::N(rd.read_un16(*b).map_err(fail)? as u64),
            Op::Un32(b, _) => Val::N(rd.read_un32(*b).map_err(fail)? as u64),
            Op::Un64(b, _) => Val::N(rd.read_un64(*b).map_err(fail)?),
            Op::U8(_) => Val::N(rd.read_u8().map_err(fail)? as u64),
            Op::U16(_) => Val::N(rd.read_u16().map_err(fail)? as u64),
            Op::U32(_) => Val::N(rd.read_u32().map_err(fail)? as u64),
            Op::U64(_) => Val::N(rd.read_u64().map_err(fail)?),
            Op::I8(_) => Val::N(rd.read_i8().map_err(fail)? as i64 as u64),
            Op::I16(_) => Val::N(rd.read_i16().map_err(fail)? as i64 as u64),
            Op::I32(_) => Val::N(rd.read_i32().map_err(fail)? as i64 as u64),
            Op::I64(_) => Val::N(rd.read_i64().map_err(fail)? as u64),
            Op::F32(_) => Val::N(rd.read_f32().map_err(fail)?.to_bits() as u64),
            Op::F64(_) => Val::N(rd.read_f64().map_err(fail)?.to_bits()),
            Op::UNib32(_) => Val::N(rd.read_unib32().map_err(fail)? as u64),
            Op::UVlq32(_) => Val::N(rd.read_uvlq32().map_err(fail)? as u64),
            Op::UVlq64(_) => Val::N(rd.read_uvlq64().map_err(fail)?),
            Op::Raw(b) => Val::B(rd.read_raw_slice(b.len()).map_err(fail)?.to_vec()),
            Op::Bytes(_) => Val::B(rd.read_bytes().map_err(fail)?.to_vec()),
            Op::Str(_) => Val::B(rd.read_str().map_err(fail)?.as_bytes().to_vec()),
            Op::RevLen(_) => Val::N(rd.read_rev_len().map_err(fail)? as u64),
            Op::AlignByte => {
                rd.align_byte();
                continue;
            }
            Op::Unsized(inner) => {
                let size = rd.read_rev_len().map_err(fail)?;
                let mut sub = rd.split(size).map_err(fail)?;
                let after_split = rd.pos();
                rust_read(&mut sub, inner, out).map_err(|e| RustReadErr {
                    err: e.err,
                    pos: after_split,
                })?;
                continue;
            }
        };
        out.push(v);
    }
    Ok(())
}

// ----------------------------------------------------------------------------------------------------------------
// C side
// ----------------------------------------------------------------------------------------------------------------

/// Run the writes on the C writer, without stopping at an error. Returns the kept error after each write, in
/// order, for the first error to be found.
pub fn c_write(wr: &mut CWriter, ops: &[Op], errs: &mut Vec<u32>) {
    for op in ops {
        let e = match op {
            Op::Bool(b) => wr.write_bool(*b),
            Op::Nib(n) => wr.write_nib(*n),
            Op::Un8(b, v) => wr.write_un8(*b as u32, *v),
            Op::Un16(b, v) => wr.write_un16(*b as u32, *v),
            Op::Un32(b, v) => wr.write_un32(*b as u32, *v),
            Op::Un64(b, v) => wr.write_un64(*b as u32, *v),
            Op::U8(v) => wr.write_u8(*v),
            Op::U16(v) => wr.write_u16(*v),
            Op::U32(v) => wr.write_u32(*v),
            Op::U64(v) => wr.write_u64(*v),
            Op::I8(v) => wr.write_i8(*v),
            Op::I16(v) => wr.write_i16(*v),
            Op::I32(v) => wr.write_i32(*v),
            Op::I64(v) => wr.write_i64(*v),
            Op::F32(v) => wr.write_f32(f32::from_bits(*v)),
            Op::F64(v) => wr.write_f64(f64::from_bits(*v)),
            Op::UNib32(v) => wr.write_unib32(*v),
            Op::UVlq32(v) => wr.write_uvlq32(*v),
            Op::UVlq64(v) => wr.write_uvlq64(*v),
            Op::Raw(b) => wr.write_raw(b),
            Op::Bytes(b) => wr.write_bytes(b),
            Op::Str(s) => wr.write_str(s),
            Op::RevLen(l) => wr.write_rev_len(*l as usize).0,
            Op::AlignByte => {
                wr.align_byte();
                wr.error()
            }
            Op::Unsized(inner) => {
                let (e, u) = wr.unsized_begin();
                errs.push(e);
                c_write(wr, inner, errs);
                wr.unsized_end(&u)
            }
        };
        errs.push(e);
    }
}

/// C bytes of `ops` in a buffer of `cap` bytes, or the kept error.
pub fn c_bytes(ops: &[Op], cap: usize) -> Result<Vec<u8>, u32> {
    let mut buf = vec![0x5A; cap];
    let mut wr = CWriter::new(&mut buf);
    let mut errs = Vec::new();
    c_write(&mut wr, ops, &mut errs);
    let first = errs.iter().copied().find(|e| *e != 0).unwrap_or(0);
    assert_eq!(first, wr.error(), "the first error is the one kept");
    wr.finish().map(|b| b.to_vec())
}

/// Run the reads on the C reader, without stopping at an error: every read gives a value.
pub fn c_read(rd: &mut CReader, ops: &[Op], out: &mut Vec<Val>) {
    for op in ops {
        let v = match op {
            Op::Bool(_) => Val::N(rd.read_bool() as u64),
            Op::Nib(_) => Val::N(rd.read_nib() as u64),
            Op::Un8(b, _) => Val::N(rd.read_un8(*b as u32) as u64),
            Op::Un16(b, _) => Val::N(rd.read_un16(*b as u32) as u64),
            Op::Un32(b, _) => Val::N(rd.read_un32(*b as u32) as u64),
            Op::Un64(b, _) => Val::N(rd.read_un64(*b as u32)),
            Op::U8(_) => Val::N(rd.read_u8() as u64),
            Op::U16(_) => Val::N(rd.read_u16() as u64),
            Op::U32(_) => Val::N(rd.read_u32() as u64),
            Op::U64(_) => Val::N(rd.read_u64()),
            Op::I8(_) => Val::N(rd.read_i8() as i64 as u64),
            Op::I16(_) => Val::N(rd.read_i16() as i64 as u64),
            Op::I32(_) => Val::N(rd.read_i32() as i64 as u64),
            Op::I64(_) => Val::N(rd.read_i64() as u64),
            Op::F32(_) => Val::N(rd.read_f32().to_bits() as u64),
            Op::F64(_) => Val::N(rd.read_f64().to_bits()),
            Op::UNib32(_) => Val::N(rd.read_unib32() as u64),
            Op::UVlq32(_) => Val::N(rd.read_uvlq32() as u64),
            Op::UVlq64(_) => Val::N(rd.read_uvlq64()),
            Op::Raw(b) => Val::B(rd.read_raw(b.len()).to_vec()),
            Op::Bytes(_) => Val::B(rd.read_bytes().to_vec()),
            Op::Str(_) => Val::B(rd.read_str().to_vec()),
            Op::RevLen(_) => Val::N(rd.read_rev_len() as u64),
            Op::AlignByte => {
                rd.align_byte();
                continue;
            }
            Op::Unsized(inner) => {
                let mut sub = rd.split_unsized();
                c_read(&mut sub, inner, out);
                rd.join(&sub);
                continue;
            }
        };
        out.push(v);
    }
}

/// Read `bytes` with `ops` on both sides and check the C reader against the Rust one: the same values up to the
/// first error, then zeros, the same error, the same position. Returns the Rust result.
pub fn check_reads(bytes: &[u8], ops: &[Op]) -> Result<Vec<Val>, Error> {
    let mut rd = BufReader::new(bytes);
    let mut rust_vals = Vec::new();
    let rust = rust_read(&mut rd, ops, &mut rust_vals);

    let mut crd = CReader::new(bytes);
    let mut c_vals = Vec::new();
    c_read(&mut crd, ops, &mut c_vals);

    assert_eq!(c_vals.len(), read_count(ops), "one value per read");
    assert_eq!(
        &c_vals[..rust_vals.len()],
        &rust_vals[..],
        "values up to the error"
    );
    match rust {
        Ok(()) => {
            assert_eq!(crd.error(), 0, "no error");
            assert_eq!(crd.pos(), rd.pos(), "position");
            assert_eq!(crd.bytes_left(), rd.bytes_left(), "bytes left");
            assert_eq!(crd.nibbles_left(), rd.nibbles_left(), "nibbles left");
            assert_eq!(crd.bits_left(), rd.bits_left(), "bits left");
            Ok(rust_vals)
        }
        Err(e) => {
            assert!(
                c_vals[rust_vals.len()..].iter().all(Val::is_zero),
                "zeros after the error: {:?}",
                &c_vals[rust_vals.len()..]
            );
            assert_eq!(crd.error(), code(e.err), "kept error, Rust: {:?}", e.err);
            assert_eq!(crd.pos(), e.pos, "a failing read moves nothing");
            Err(e.err)
        }
    }
}

/// Write `ops` on both sides into a buffer of `cap` bytes and check they give the same bytes or the same first
/// error. Returns the Rust result.
pub fn check_writes(ops: &[Op], cap: usize) -> Result<Vec<u8>, Error> {
    let rust = rust_bytes(ops, cap);
    let c = c_bytes(ops, cap);
    match &rust {
        Ok(bytes) => assert_eq!(c.as_deref(), Ok(bytes.as_slice()), "bytes"),
        Err(e) => assert_eq!(c, Err(code(*e)), "first error, Rust: {e:?}"),
    }
    rust
}
