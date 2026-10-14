//! Builds the shrink_wrap C99 runtime (`shrink_wrap.h`, `shrink_wrap.c` next to this crate) and checks it against
//! the Rust crate (SW-43): golden vectors in `tests/golden.rs`, a property-based cross-check in both directions in
//! `tests/cross.rs`. Not published, the C files are what C users take.
//!
//! [`CReader`] and [`CWriter`] are thin safe wrappers over the C API, [`model`] runs the same sequence of
//! [`Op`](model::Op)s through the Rust and the C side.

use core::ffi::{c_char, c_uint};
use core::marker::PhantomData;

pub mod model;

/// `sw_reader`
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct SwReader {
    buf: *const u8,
    len_bytes: usize,
    byte_idx: usize,
    bit_idx: c_uint,
    is_at_bit7_rev: bool,
    err: u32,
}

/// `sw_writer`
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct SwWriter {
    buf: *mut u8,
    cap: usize,
    len_bytes: usize,
    byte_idx: usize,
    bit_idx: c_uint,
    err: u32,
}

/// `sw_bytes`
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct SwBytes {
    ptr: *const u8,
    len: usize,
}

/// `sw_unsized`
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct SwUnsized {
    size_slot: usize,
    start: usize,
}

unsafe extern "C" {
    fn sw_test_sizeof_reader() -> usize;
    fn sw_test_sizeof_writer() -> usize;
    fn sw_test_sizeof_unsized() -> usize;

    fn sw_reader_init(rd: *mut SwReader, buf: *const u8, len: usize);
    fn sw_reader_error(rd: *const SwReader) -> u32;
    fn sw_reader_take_error(rd: *mut SwReader) -> u32;
    fn sw_read_bool(rd: *mut SwReader) -> bool;
    fn sw_read_nib(rd: *mut SwReader) -> u8;
    fn sw_read_un8(rd: *mut SwReader, bit_count: c_uint) -> u8;
    fn sw_read_un16(rd: *mut SwReader, bit_count: c_uint) -> u16;
    fn sw_read_un32(rd: *mut SwReader, bit_count: c_uint) -> u32;
    fn sw_read_un64(rd: *mut SwReader, bit_count: c_uint) -> u64;
    fn sw_read_u8(rd: *mut SwReader) -> u8;
    fn sw_read_u16(rd: *mut SwReader) -> u16;
    fn sw_read_u32(rd: *mut SwReader) -> u32;
    fn sw_read_u64(rd: *mut SwReader) -> u64;
    fn sw_read_i8(rd: *mut SwReader) -> i8;
    fn sw_read_i16(rd: *mut SwReader) -> i16;
    fn sw_read_i32(rd: *mut SwReader) -> i32;
    fn sw_read_i64(rd: *mut SwReader) -> i64;
    fn sw_read_f32(rd: *mut SwReader) -> f32;
    fn sw_read_f64(rd: *mut SwReader) -> f64;
    fn sw_read_unib32(rd: *mut SwReader) -> u32;
    fn sw_read_uvlq32(rd: *mut SwReader) -> u32;
    fn sw_read_uvlq64(rd: *mut SwReader) -> u64;
    fn sw_read_rev_len(rd: *mut SwReader) -> u32;
    fn sw_read_raw(rd: *mut SwReader, len: usize) -> SwBytes;
    fn sw_read_bytes(rd: *mut SwReader) -> SwBytes;
    fn sw_read_str(rd: *mut SwReader) -> SwBytes;
    fn sw_reader_split(rd: *mut SwReader, len: usize) -> SwReader;
    fn sw_reader_split_unsized(rd: *mut SwReader) -> SwReader;
    fn sw_reader_join(rd: *mut SwReader, child: *const SwReader);
    fn sw_reader_align_byte(rd: *mut SwReader);
    fn sw_reader_bytes_left(rd: *const SwReader) -> usize;
    fn sw_reader_nibbles_left(rd: *const SwReader) -> usize;
    fn sw_reader_bits_left(rd: *const SwReader) -> usize;
    fn sw_reader_pos(rd: *const SwReader, byte_idx: *mut usize, bit_idx: *mut c_uint);

    fn sw_writer_init(wr: *mut SwWriter, buf: *mut u8, cap: usize);
    fn sw_writer_reset(wr: *mut SwWriter);
    fn sw_writer_error(wr: *const SwWriter) -> u32;
    fn sw_writer_take_error(wr: *mut SwWriter) -> u32;
    fn sw_write_bool(wr: *mut SwWriter, val: bool) -> u32;
    fn sw_write_nib(wr: *mut SwWriter, val: c_uint) -> u32;
    fn sw_write_un8(wr: *mut SwWriter, bit_count: c_uint, val: u8) -> u32;
    fn sw_write_un16(wr: *mut SwWriter, bit_count: c_uint, val: u16) -> u32;
    fn sw_write_un32(wr: *mut SwWriter, bit_count: c_uint, val: u32) -> u32;
    fn sw_write_un64(wr: *mut SwWriter, bit_count: c_uint, val: u64) -> u32;
    fn sw_write_u8(wr: *mut SwWriter, val: u8) -> u32;
    fn sw_write_u16(wr: *mut SwWriter, val: u16) -> u32;
    fn sw_write_u32(wr: *mut SwWriter, val: u32) -> u32;
    fn sw_write_u64(wr: *mut SwWriter, val: u64) -> u32;
    fn sw_write_i8(wr: *mut SwWriter, val: i8) -> u32;
    fn sw_write_i16(wr: *mut SwWriter, val: i16) -> u32;
    fn sw_write_i32(wr: *mut SwWriter, val: i32) -> u32;
    fn sw_write_i64(wr: *mut SwWriter, val: i64) -> u32;
    fn sw_write_f32(wr: *mut SwWriter, val: f32) -> u32;
    fn sw_write_f64(wr: *mut SwWriter, val: f64) -> u32;
    fn sw_write_unib32(wr: *mut SwWriter, val: u32) -> u32;
    fn sw_write_uvlq32(wr: *mut SwWriter, val: u32) -> u32;
    fn sw_write_uvlq64(wr: *mut SwWriter, val: u64) -> u32;
    fn sw_write_raw(wr: *mut SwWriter, val: *const u8, len: usize) -> u32;
    fn sw_write_bytes(wr: *mut SwWriter, val: *const u8, len: usize) -> u32;
    fn sw_write_str(wr: *mut SwWriter, val: *const c_char, len: usize) -> u32;
    fn sw_write_rev_len(wr: *mut SwWriter, len: usize, pos: *mut usize) -> u32;
    fn sw_rev_len_pos(wr: *const SwWriter) -> usize;
    fn sw_update_rev_len(wr: *mut SwWriter, pos: usize, len: usize) -> u32;
    fn sw_encode_len_fifo(wr: *mut SwWriter, from: usize, to: usize) -> u32;
    fn sw_unsized_begin(wr: *mut SwWriter, u: *mut SwUnsized) -> u32;
    fn sw_unsized_end(wr: *mut SwWriter, u: *const SwUnsized) -> u32;
    fn sw_writer_align_byte(wr: *mut SwWriter);
    fn sw_writer_align_nibble(wr: *mut SwWriter);
    fn sw_writer_finish(wr: *mut SwWriter, len: *mut usize) -> u32;
    fn sw_writer_bytes_left(wr: *const SwWriter) -> usize;
    fn sw_writer_nibbles_left(wr: *const SwWriter) -> usize;
    fn sw_writer_pos(wr: *const SwWriter, byte_idx: *mut usize, bit_idx: *mut c_uint);
}

/// The C struct sizes, to check the `#[repr(C)]` mirrors against: (reader, writer, unsized).
pub fn c_struct_sizes() -> (usize, usize, usize) {
    unsafe {
        (
            sw_test_sizeof_reader(),
            sw_test_sizeof_writer(),
            sw_test_sizeof_unsized(),
        )
    }
}

/// The number of a Rust error, as `sw_error` has it.
pub fn code(e: shrink_wrap::Error) -> u32 {
    e as u32
}

fn bytes<'a>(b: SwBytes) -> &'a [u8] {
    if b.len == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(b.ptr, b.len) }
    }
}

/// `sw_reader` over a borrowed buffer.
#[derive(Copy, Clone, Debug)]
pub struct CReader<'i> {
    raw: SwReader,
    _buf: PhantomData<&'i [u8]>,
}

macro_rules! c_reads {
    ($($name:ident => $f:ident -> $t:ty;)*) => {
        $(pub fn $name(&mut self) -> $t { unsafe { $f(&mut self.raw) } })*
    };
}

macro_rules! c_reads_un {
    ($($name:ident => $f:ident -> $t:ty;)*) => {
        $(pub fn $name(&mut self, bit_count: u32) -> $t { unsafe { $f(&mut self.raw, bit_count) } })*
    };
}

impl<'i> CReader<'i> {
    pub fn new(buf: &'i [u8]) -> Self {
        let mut raw = core::mem::MaybeUninit::<SwReader>::uninit();
        unsafe { sw_reader_init(raw.as_mut_ptr(), buf.as_ptr(), buf.len()) };
        CReader {
            raw: unsafe { raw.assume_init() },
            _buf: PhantomData,
        }
    }

    /// The kept error, 0 if none.
    pub fn error(&self) -> u32 {
        unsafe { sw_reader_error(&self.raw) }
    }

    pub fn take_error(&mut self) -> u32 {
        unsafe { sw_reader_take_error(&mut self.raw) }
    }

    c_reads! {
        read_bool => sw_read_bool -> bool;
        read_nib => sw_read_nib -> u8;
        read_u8 => sw_read_u8 -> u8;
        read_u16 => sw_read_u16 -> u16;
        read_u32 => sw_read_u32 -> u32;
        read_u64 => sw_read_u64 -> u64;
        read_i8 => sw_read_i8 -> i8;
        read_i16 => sw_read_i16 -> i16;
        read_i32 => sw_read_i32 -> i32;
        read_i64 => sw_read_i64 -> i64;
        read_f32 => sw_read_f32 -> f32;
        read_f64 => sw_read_f64 -> f64;
        read_unib32 => sw_read_unib32 -> u32;
        read_uvlq32 => sw_read_uvlq32 -> u32;
        read_uvlq64 => sw_read_uvlq64 -> u64;
        read_rev_len => sw_read_rev_len -> u32;
    }

    c_reads_un! {
        read_un8 => sw_read_un8 -> u8;
        read_un16 => sw_read_un16 -> u16;
        read_un32 => sw_read_un32 -> u32;
        read_un64 => sw_read_un64 -> u64;
    }

    pub fn read_raw(&mut self, len: usize) -> &'i [u8] {
        bytes(unsafe { sw_read_raw(&mut self.raw, len) })
    }

    pub fn read_bytes(&mut self) -> &'i [u8] {
        bytes(unsafe { sw_read_bytes(&mut self.raw) })
    }

    pub fn read_str(&mut self) -> &'i [u8] {
        bytes(unsafe { sw_read_str(&mut self.raw) })
    }

    pub fn split(&mut self, len: usize) -> CReader<'i> {
        CReader {
            raw: unsafe { sw_reader_split(&mut self.raw, len) },
            _buf: PhantomData,
        }
    }

    pub fn split_unsized(&mut self) -> CReader<'i> {
        CReader {
            raw: unsafe { sw_reader_split_unsized(&mut self.raw) },
            _buf: PhantomData,
        }
    }

    pub fn join(&mut self, child: &CReader<'i>) {
        unsafe { sw_reader_join(&mut self.raw, &child.raw) }
    }

    pub fn align_byte(&mut self) {
        unsafe { sw_reader_align_byte(&mut self.raw) }
    }

    pub fn bytes_left(&self) -> usize {
        unsafe { sw_reader_bytes_left(&self.raw) }
    }

    pub fn nibbles_left(&self) -> usize {
        unsafe { sw_reader_nibbles_left(&self.raw) }
    }

    pub fn bits_left(&self) -> usize {
        unsafe { sw_reader_bits_left(&self.raw) }
    }

    pub fn pos(&self) -> (usize, u8) {
        let mut byte_idx = 0;
        let mut bit_idx = 0;
        unsafe { sw_reader_pos(&self.raw, &mut byte_idx, &mut bit_idx) };
        (byte_idx, bit_idx as u8)
    }
}

/// `sw_writer` over a borrowed buffer.
pub struct CWriter<'i> {
    raw: SwWriter,
    _buf: PhantomData<&'i mut [u8]>,
}

macro_rules! c_writes {
    ($($name:ident => $f:ident($t:ty);)*) => {
        $(pub fn $name(&mut self, val: $t) -> u32 { unsafe { $f(&mut self.raw, val) } })*
    };
}

macro_rules! c_writes_un {
    ($($name:ident => $f:ident($t:ty);)*) => {
        $(pub fn $name(&mut self, bit_count: u32, val: $t) -> u32 {
            unsafe { $f(&mut self.raw, bit_count, val) }
        })*
    };
}

impl<'i> CWriter<'i> {
    pub fn new(buf: &'i mut [u8]) -> Self {
        let mut raw = core::mem::MaybeUninit::<SwWriter>::uninit();
        unsafe { sw_writer_init(raw.as_mut_ptr(), buf.as_mut_ptr(), buf.len()) };
        CWriter {
            raw: unsafe { raw.assume_init() },
            _buf: PhantomData,
        }
    }

    pub fn reset(&mut self) {
        unsafe { sw_writer_reset(&mut self.raw) }
    }

    /// The kept error, 0 if none.
    pub fn error(&self) -> u32 {
        unsafe { sw_writer_error(&self.raw) }
    }

    pub fn take_error(&mut self) -> u32 {
        unsafe { sw_writer_take_error(&mut self.raw) }
    }

    c_writes! {
        write_bool => sw_write_bool(bool);
        write_u8 => sw_write_u8(u8);
        write_u16 => sw_write_u16(u16);
        write_u32 => sw_write_u32(u32);
        write_u64 => sw_write_u64(u64);
        write_i8 => sw_write_i8(i8);
        write_i16 => sw_write_i16(i16);
        write_i32 => sw_write_i32(i32);
        write_i64 => sw_write_i64(i64);
        write_f32 => sw_write_f32(f32);
        write_f64 => sw_write_f64(f64);
        write_unib32 => sw_write_unib32(u32);
        write_uvlq32 => sw_write_uvlq32(u32);
        write_uvlq64 => sw_write_uvlq64(u64);
    }

    c_writes_un! {
        write_un8 => sw_write_un8(u8);
        write_un16 => sw_write_un16(u16);
        write_un32 => sw_write_un32(u32);
        write_un64 => sw_write_un64(u64);
    }

    pub fn write_nib(&mut self, val: u8) -> u32 {
        unsafe { sw_write_nib(&mut self.raw, val as c_uint) }
    }

    pub fn write_raw(&mut self, val: &[u8]) -> u32 {
        unsafe { sw_write_raw(&mut self.raw, val.as_ptr(), val.len()) }
    }

    pub fn write_bytes(&mut self, val: &[u8]) -> u32 {
        unsafe { sw_write_bytes(&mut self.raw, val.as_ptr(), val.len()) }
    }

    pub fn write_str(&mut self, val: &str) -> u32 {
        unsafe { sw_write_str(&mut self.raw, val.as_ptr().cast(), val.len()) }
    }

    /// Returns (error, slot position).
    pub fn write_rev_len(&mut self, len: usize) -> (u32, usize) {
        let mut pos = 0;
        let e = unsafe { sw_write_rev_len(&mut self.raw, len, &mut pos) };
        (e, pos)
    }

    pub fn rev_len_pos(&self) -> usize {
        unsafe { sw_rev_len_pos(&self.raw) }
    }

    pub fn update_rev_len(&mut self, pos: usize, len: usize) -> u32 {
        unsafe { sw_update_rev_len(&mut self.raw, pos, len) }
    }

    pub fn encode_len_fifo(&mut self, from: usize, to: usize) -> u32 {
        unsafe { sw_encode_len_fifo(&mut self.raw, from, to) }
    }

    pub fn unsized_begin(&mut self) -> (u32, SwUnsized) {
        let mut u = SwUnsized::default();
        let e = unsafe { sw_unsized_begin(&mut self.raw, &mut u) };
        (e, u)
    }

    pub fn unsized_end(&mut self, u: &SwUnsized) -> u32 {
        unsafe { sw_unsized_end(&mut self.raw, u) }
    }

    pub fn align_byte(&mut self) {
        unsafe { sw_writer_align_byte(&mut self.raw) }
    }

    pub fn align_nibble(&mut self) {
        unsafe { sw_writer_align_nibble(&mut self.raw) }
    }

    /// The written bytes, or the kept error.
    pub fn finish(&mut self) -> Result<&[u8], u32> {
        let mut len = 0;
        let e = unsafe { sw_writer_finish(&mut self.raw, &mut len) };
        if e != 0 {
            return Err(e);
        }
        Ok(unsafe { core::slice::from_raw_parts(self.raw.buf, len) })
    }

    pub fn bytes_left(&self) -> usize {
        unsafe { sw_writer_bytes_left(&self.raw) }
    }

    pub fn nibbles_left(&self) -> usize {
        unsafe { sw_writer_nibbles_left(&self.raw) }
    }

    pub fn pos(&self) -> (usize, u8) {
        let mut byte_idx = 0;
        let mut bit_idx = 0;
        unsafe { sw_writer_pos(&self.raw, &mut byte_idx, &mut bit_idx) };
        (byte_idx, bit_idx as u8)
    }
}
