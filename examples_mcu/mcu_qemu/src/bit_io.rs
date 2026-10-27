//! Shared body of the bit I/O benches: what `BufWriter` and `BufReader` cost per operation on an MCU, for the
//! writes and reads every message goes through (bits of 1 to 24 and 25 to 32 bits, bools, nibbles, `UNib32`,
//! reverse lengths), a host -> device request as a server decodes it, a device -> host reply as a server writes
//! it, and a `Series` (the `i_a` signal of [super::motor_signal], 32 items, `DeltaPacked`) encoded and decoded.
//!
//! Each case runs [RUNS] times (the first is the cold one); the inputs come through `black_box`. Every buffer a
//! case writes and every value a case reads goes into an FNV hash per case, so a change to the writer or the
//! reader that changes a byte or a value shows up as a different hash (the same on every core and opt level).
//!
//! Used by `qemu-bit-io` / `qemu-m4f-bit-io` (instructions, `qemu_icount`) and `stm32h725ig-bit-io-cycles` (DWT
//! cycles), through [Meter].

// each bin uses a part
#![allow(dead_code)]

use core::hint::black_box;
use wire_weaver::prelude::*;
use wire_weaver::shrink_wrap::series::DeltaPacked;
use wire_weaver::shrink_wrap::tail_bytes::TailBytes;
use wire_weaver::shrink_wrap::{RefVec, Series, UNib32, UVlq32, UVlq32Backfill};
use ww_client_server::{Event, EventKind, PathKind, Request, RequestKind};
use ww_si::{Ampere, Second, Timed};

/// Runs of each case: the first is cold, the others warm.
pub const RUNS: usize = 3;
/// Operations of the bit, bool and nibble cases.
const OPS: usize = 64;
/// Values of the `UNib32` cases.
const NIBS: usize = 32;
/// Byte slices (and so reverse lengths) of the rev len cases.
const LENS: usize = 16;
/// Items of the `Series` cases.
const ITEMS: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Case {
    WriteUn32Narrow,
    WriteUn32Wide,
    WriteBool,
    WriteNib,
    WriteUnib32,
    WriteRevLen,
    WriteEvent,
    SeriesEncode,
    ReadUn32Narrow,
    ReadUn32Wide,
    ReadUn64,
    ReadBool,
    ReadNib,
    ReadUnib32,
    ReadRevLen,
    ReadRequest,
    SeriesDecode,
}

impl Case {
    pub const ALL: [Case; 17] = [
        Case::WriteUn32Narrow,
        Case::WriteUn32Wide,
        Case::WriteBool,
        Case::WriteNib,
        Case::WriteUnib32,
        Case::WriteRevLen,
        Case::WriteEvent,
        Case::SeriesEncode,
        Case::ReadUn32Narrow,
        Case::ReadUn32Wide,
        Case::ReadUn64,
        Case::ReadBool,
        Case::ReadNib,
        Case::ReadUnib32,
        Case::ReadRevLen,
        Case::ReadRequest,
        Case::SeriesDecode,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Case::WriteUn32Narrow => "write_un32 1-24 bits",
            Case::WriteUn32Wide => "write_un32 25-32 bits",
            Case::WriteBool => "write_bool",
            Case::WriteNib => "write_nib",
            Case::WriteUnib32 => "write_unib32",
            Case::WriteRevLen => "write_bytes + finish (rev len)",
            Case::WriteEvent => "Event reply, u32 value",
            Case::SeriesEncode => "Series encode, DeltaPacked",
            Case::ReadUn32Narrow => "read_un32 1-24 bits",
            Case::ReadUn32Wide => "read_un32 25-32 bits",
            Case::ReadUn64 => "read_un64 1-24 bits",
            Case::ReadBool => "read_bool",
            Case::ReadNib => "read_nib",
            Case::ReadUnib32 => "read_unib32",
            Case::ReadRevLen => "read_bytes (rev len)",
            Case::ReadRequest => "Request decode, call with args",
            Case::SeriesDecode => "Series decode, DeltaPacked",
        }
    }

    /// Operations per run, to give a cost per operation (items for a `Series`, 1 for a message).
    pub fn ops(self) -> usize {
        match self {
            Case::WriteUnib32 | Case::ReadUnib32 => NIBS,
            Case::WriteRevLen | Case::ReadRevLen => LENS,
            Case::WriteEvent | Case::ReadRequest => 1,
            Case::SeriesEncode | Case::SeriesDecode => ITEMS,
            _ => OPS,
        }
    }
}

pub trait Meter {
    /// Runs `work` once and measures it.
    fn measure(&mut self, case: Case, run: usize, work: &mut dyn FnMut());
}

/// What a case wrote and read, the same on every core when the writer and the reader are right.
#[derive(Clone, Copy)]
pub struct Report {
    pub bytes: usize,
    pub hash: u32,
}

fn fnv(hash: &mut u32, bytes: &[u8]) {
    for &b in bytes {
        *hash = (*hash ^ b as u32).wrapping_mul(0x0100_0193);
    }
}

/// A fixed pseudo-random sequence (xorshift32).
fn sequence<const N: usize>(seed: u32) -> [u32; N] {
    let mut x = seed;
    core::array::from_fn(|_| {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        x
    })
}

/// Widths and values of the bit cases: `lo..=hi` bits.
fn bit_ops(seed: u32, lo: u32, hi: u32) -> [(u8, u32); OPS] {
    sequence::<OPS>(seed).map(|x| {
        let width = lo + x % (hi - lo + 1);
        (width as u8, (x >> 3) & (u32::MAX >> (32 - width)))
    })
}

/// `UNib32` values from 1 to 11 nibbles, most of them small (lengths, ids, indices).
fn unib_values() -> [u32; NIBS] {
    sequence::<NIBS>(0x1234_5678).map(|x| x >> (x % 32))
}

#[derive_shrink_wrap(borrowed, derive(Clone, Copy))]
struct SetArgs {
    pwm: u16,
    enable: bool,
    mode: U4,
    gain: f32,
    channel: UNib32,
}

fn items() -> [Timed<Ampere>; ITEMS] {
    core::array::from_fn(|i| {
        Timed::new(
            Second::micro(super::motor_signal::T0_US + i as u32 * super::motor_signal::DT_US),
            Ampere::milli(super::motor_signal::I_A_MILLIAMPS[i]),
        )
    })
}

pub fn run(meter: &mut dyn Meter) -> [Report; 17] {
    let mut reports = [Report {
        bytes: 0,
        hash: 0x811c_9dc5,
    }; 17];
    let narrow = black_box(bit_ops(0xACE1, 1, 24));
    let wide = black_box(bit_ops(0xBEEF, 25, 32));
    let bools = black_box(sequence::<OPS>(0x5EED).map(|x| x & 1 != 0));
    let nibs = black_box(sequence::<OPS>(0xF00D).map(|x| Nibble::new_masked(x as u8)));
    let unibs = black_box(unib_values());
    let slices = black_box(sequence::<LENS>(0xCAFE).map(|x| (x % 24) as usize));
    let payload = black_box([0x5Au8; 24]);
    let items = black_box(items());

    // the bytes the read cases read, written once
    let mut narrow_buf = [0u8; 256];
    let mut wide_buf = [0u8; 300];
    let mut bool_buf = [0u8; 16];
    let mut nib_buf = [0u8; 40];
    let mut unib_buf = [0u8; 200];
    let mut rev_buf = [0u8; 512];
    let mut request_buf = [0u8; 64];
    let mut series_buf = [0u8; 256];
    let narrow_len = write_bits(&mut narrow_buf, &narrow);
    let wide_len = write_bits(&mut wide_buf, &wide);
    let bool_len = write_bools(&mut bool_buf, &bools);
    let nib_len = write_nibs(&mut nib_buf, &nibs);
    let unib_len = write_unibs(&mut unib_buf, &unibs);
    let rev_len = write_slices(&mut rev_buf, &slices, &payload);
    let request_len = write_request(&mut request_buf);
    let series_len = Series::with_policy(&items[..], DeltaPacked)
        .to_ww_bytes(&mut series_buf)
        .unwrap()
        .len();

    for case in Case::ALL {
        let report = &mut reports[case as usize];
        for run in 0..RUNS {
            let mut buf = [0u8; 512];
            let mut len = 0;
            let mut sum = 0u32;
            let mut work = || match case {
                Case::WriteUn32Narrow => len = write_bits(&mut buf, &narrow),
                Case::WriteUn32Wide => len = write_bits(&mut buf, &wide),
                Case::WriteBool => len = write_bools(&mut buf, &bools),
                Case::WriteNib => len = write_nibs(&mut buf, &nibs),
                Case::WriteUnib32 => len = write_unibs(&mut buf, &unibs),
                Case::WriteRevLen => len = write_slices(&mut buf, &slices, &payload),
                Case::WriteEvent => len = write_event(&mut buf, black_box(0x1234_5678)),
                Case::SeriesEncode => {
                    len = Series::with_policy(&items[..], DeltaPacked)
                        .to_ww_bytes(&mut buf)
                        .unwrap()
                        .len()
                }
                Case::ReadUn32Narrow => sum = read_bits(&narrow_buf[..narrow_len], &narrow, false),
                Case::ReadUn32Wide => sum = read_bits(&wide_buf[..wide_len], &wide, false),
                Case::ReadUn64 => sum = read_bits(&narrow_buf[..narrow_len], &narrow, true),
                Case::ReadBool => sum = read_bools(&bool_buf[..bool_len]),
                Case::ReadNib => sum = read_nibs(&nib_buf[..nib_len]),
                Case::ReadUnib32 => sum = read_unibs(&unib_buf[..unib_len]),
                Case::ReadRevLen => sum = read_slices(&rev_buf[..rev_len]),
                Case::ReadRequest => sum = read_request(&request_buf[..request_len]),
                Case::SeriesDecode => sum = read_series(&series_buf[..series_len]),
            };
            meter.measure(case, run, &mut work);
            if run == 0 {
                report.bytes = len;
                fnv(&mut report.hash, &buf[..len]);
                fnv(&mut report.hash, &sum.to_le_bytes());
            }
        }
    }
    reports
}

#[inline(never)]
fn write_bits(buf: &mut [u8], ops: &[(u8, u32)]) -> usize {
    let mut wr = BufWriter::new(buf);
    for &(width, value) in ops {
        wr.write_un32(width, value).unwrap();
    }
    wr.finish().unwrap().len()
}

#[inline(never)]
fn write_bools(buf: &mut [u8], bools: &[bool]) -> usize {
    let mut wr = BufWriter::new(buf);
    for &b in bools {
        wr.write_bool(b).unwrap();
    }
    wr.finish().unwrap().len()
}

#[inline(never)]
fn write_nibs(buf: &mut [u8], nibs: &[Nibble]) -> usize {
    let mut wr = BufWriter::new(buf);
    for &n in nibs {
        wr.write_nib(n).unwrap();
    }
    wr.finish().unwrap().len()
}

#[inline(never)]
fn write_unibs(buf: &mut [u8], values: &[u32]) -> usize {
    let mut wr = BufWriter::new(buf);
    for &v in values {
        wr.write_unib32(v).unwrap();
    }
    wr.finish().unwrap().len()
}

#[inline(never)]
fn write_slices(buf: &mut [u8], lens: &[usize], payload: &[u8]) -> usize {
    let mut wr = BufWriter::new(buf);
    for &len in lens {
        wr.write_bytes(&payload[..len]).unwrap();
    }
    wr.finish().unwrap().len()
}

/// A reply as a server sends it: seq, `Ok(Value)`, the return value's bytes.
#[inline(never)]
fn write_event(buf: &mut [u8], value: u32) -> usize {
    let bytes = value.to_le_bytes();
    let event = Event {
        seq: UVlq32(5),
        result: Ok(EventKind::Value {
            data: TailBytes(&bytes),
        }),
    };
    event.to_ww_bytes(buf).unwrap().len()
}

fn write_request(buf: &mut [u8]) -> usize {
    let args = SetArgs {
        pwm: 1200,
        enable: true,
        mode: U4::new(5).unwrap(),
        gain: 0.75,
        channel: UNib32(3),
    };
    let mut args_buf = [0u8; 32];
    let args = args.to_ww_bytes(&mut args_buf).unwrap();
    let path = [UNib32(3), UNib32(1), UNib32(12)];
    let request = Request {
        seq: UVlq32Backfill(5),
        priority: ww_client_server::Priority::NORMAL.nibble(),
        path_kind: PathKind::Absolute {
            path: RefVec::Slice { slice: &path },
        },
        kind: RequestKind::Call {
            args: TailBytes(args),
        },
    };
    request.to_ww_bytes(buf).unwrap().len()
}

#[inline(never)]
fn read_bits(buf: &[u8], ops: &[(u8, u32)], wide: bool) -> u32 {
    let mut rd = BufReader::new(buf);
    let mut sum = 0u32;
    for &(width, _) in ops {
        let v = if wide {
            rd.read_un64(width).unwrap() as u32
        } else {
            rd.read_un32(width).unwrap()
        };
        sum = sum.rotate_left(5) ^ v;
    }
    sum
}

#[inline(never)]
fn read_bools(buf: &[u8]) -> u32 {
    let mut rd = BufReader::new(buf);
    let mut sum = 0u32;
    for _ in 0..OPS {
        sum = sum << 1 | rd.read_bool().unwrap() as u32 ^ sum >> 31;
    }
    sum
}

#[inline(never)]
fn read_nibs(buf: &[u8]) -> u32 {
    let mut rd = BufReader::new(buf);
    let mut sum = 0u32;
    for _ in 0..OPS {
        sum = sum.rotate_left(4) ^ rd.read_nib().unwrap().value() as u32;
    }
    sum
}

#[inline(never)]
fn read_unibs(buf: &[u8]) -> u32 {
    let mut rd = BufReader::new(buf);
    let mut sum = 0u32;
    for _ in 0..NIBS {
        sum = sum.rotate_left(7) ^ rd.read_unib32().unwrap();
    }
    sum
}

#[inline(never)]
fn read_slices(buf: &[u8]) -> u32 {
    let mut rd = BufReader::new(buf);
    let mut sum = 0u32;
    for _ in 0..LENS {
        let bytes = rd.read_bytes().unwrap();
        sum = sum.rotate_left(5) ^ bytes.len() as u32 ^ bytes.first().copied().unwrap_or(0) as u32;
    }
    sum
}

/// What a generated server does with a request: decode it, walk the path, decode the call's arguments.
#[inline(never)]
fn read_request(buf: &[u8]) -> u32 {
    let mut rd = BufReader::new(buf);
    let request = Request::des_shrink_wrap(&mut rd).unwrap();
    let mut sum = request.seq.0;
    let PathKind::Absolute { path } = &request.path_kind else {
        return 0;
    };
    for id in path.iter() {
        sum = sum.rotate_left(5) ^ id.unwrap().0;
    }
    let RequestKind::Call { args } = request.kind else {
        return 0;
    };
    let mut rd = BufReader::new(args.0);
    let args = SetArgs::des_shrink_wrap(&mut rd).unwrap();
    sum ^ args.pwm as u32
        ^ args.enable as u32
        ^ args.mode.value() as u32
        ^ args.gain.to_bits()
        ^ args.channel.0
}

#[inline(never)]
fn read_series(buf: &[u8]) -> u32 {
    let series = Series::<Timed<Ampere>>::from_ww_bytes(buf).unwrap();
    let mut sum = 0u32;
    for item in series.iter() {
        sum = sum.rotate_left(3)
            ^ item.value.value.to_i128().unwrap_or(0) as u32
            ^ item.t.value.to_i128().unwrap_or(0) as u32;
    }
    sum
}
