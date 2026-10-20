//! Streaming LZ4 block encoder for a byte stream cut into chunks (a UART's RX, a log), `no_std` and no alloc.
//!
//! Each chunk becomes one standard [LZ4 block](https://github.com/lz4/lz4/blob/dev/doc/lz4_Block_format.md) whose
//! matches may reach back into earlier chunks, up to the history the [`Encoder`] keeps. A decoder that keeps the
//! bytes it decoded before as the dictionary decodes it: [`Decoder`] (feature `std`), lz4_flex's
//! `decompress_into_with_dict` or liblz4's `LZ4_decompress_safe_usingDict`. History is what makes small chunks
//! compress: 512 B chunks of shell output go 3.5-5x with a few KiB of history, about 2x without.
//!
//! ```
//! use ww_lz4::{Encoder, max_block_len};
//!
//! // 2^12 hash entries (8 KiB) and 12 KiB of history and input, 4 KiB of it kept when the buffer slides
//! static mut ENC: Encoder<4096, 12288> = Encoder::new(4096);
//! let enc = unsafe { &mut *(&raw mut ENC) }; // or a StaticCell, a Mutex, ..
//!
//! let mut block = [0u8; max_block_len(512)];
//! let input = enc.input(512)?; // the free tail of the history, at least 512 bytes
//! let line = b"[    0.000000] Linux version 6.17.2-arch1-1\r\n";
//! input[..line.len()].copy_from_slice(line); // DMA or serialize straight into it
//! let n = enc.compress(line.len(), &mut block)?;
//! // send block[..n]; on stream open or lost bytes, enc.reset() and the decoder's reset()
//! # assert!(n > 0);
//! # Ok::<(), ww_lz4::Error>(())
//! ```
//!
//! A chunk that doesn't shrink goes out as a literal-only block (still valid LZ4, the decoder needs no flag), and
//! after a few of those in a row the encoder stops trying for a while ([`Encoder::with_backoff`]), so binary or
//! already compressed data costs little more than a copy.
//!
//! The encoder lives in a `static` and is set up in place by the `const fn` [`Encoder::new`]: it is mostly buffer
//! (`BUF` bytes of history plus 2 bytes per hash entry), never build it on the stack. Feature `ram` puts the hot
//! functions in `.data.ww_lz4` on bare-metal targets, which cortex-m-rt copies to SRAM: no flash wait states, about
//! 30 % fewer cycles than from flash on an STM32G0 at 64 MHz (1.5 KiB of RAM). It does nothing on a host.
#![no_std]

#[cfg(feature = "std")]
extern crate std;

#[cfg(feature = "std")]
mod decoder;
#[cfg(feature = "std")]
pub use decoder::{DecodeError, Decoder};

const MIN_MATCH: usize = 4;
/// A match must start at least this far before the block end.
const MF_LIMIT: usize = 12;
/// The last bytes of a block are always literals.
const LAST_LITERALS: usize = 5;
/// The longest offset an LZ4 block can express, and so the dictionary a decoder must keep.
pub const MAX_DISTANCE: usize = 65535;
/// Misses before the search step grows (LZ4's "acceleration").
const SKIP_TRIGGER: u32 = 6;

/// Worst case LZ4 block length for a chunk of `n` bytes: the size of the `out` buffer [`Encoder::compress`] needs.
pub const fn max_block_len(n: usize) -> usize {
    n + n / 255 + 16
}

/// Streaming LZ4 block encoder with `HASH` hash table entries (a power of two, 2^10-2^12 is typical, 2 bytes each) and
/// a `BUF` byte buffer holding the history and the chunk being compressed (at most 64 KiB, the longest LZ4 offset).
///
/// The hash table holds the stream position of the last 4-byte sequence with that hash, truncated to 16 bits: a
/// position that aliased an older one is caught by comparing the bytes, so the table never needs rewriting when the
/// buffer slides.
pub struct Encoder<const HASH: usize, const BUF: usize> {
    table: [u16; HASH],
    buf: [u8; BUF],
    /// Stream position of `buf[0]`, wrapping.
    pos: u32,
    /// Bytes in `buf`: history, compressed already.
    len: usize,
    /// History that survives a slide.
    keep: usize,
    /// Bytes of chunks in a row that didn't shrink before backing off, 0 for never.
    miss_bytes: usize,
    /// Bytes sent as literals without trying once backed off.
    retry_bytes: usize,
    /// Bytes of chunks in a row that didn't shrink.
    missed: usize,
    /// Bytes still to go as literals before the next try.
    skip: usize,
}

impl<const HASH: usize, const BUF: usize> Encoder<HASH, BUF> {
    const CHECK: () = {
        assert!(
            HASH.is_power_of_two() && HASH >= 16 && HASH <= 1 << 16,
            "HASH must be a power of two, 16..=65536"
        );
        assert!(
            BUF > MF_LIMIT && BUF <= 1 << 16,
            "BUF must be 13..=65536 bytes"
        );
    };

    /// An empty encoder; `keep` bytes of history survive when the buffer slides, so chunks are at most `BUF - keep`
    /// bytes. Backs off after 1 KiB that didn't shrink and retries every 4 KiB, see
    /// [`with_backoff`](Self::with_backoff).
    pub const fn new(keep: usize) -> Self {
        #[allow(clippy::let_unit_value)]
        let () = Self::CHECK;
        assert!(keep < BUF, "keep must leave room for a chunk");
        Encoder {
            table: [0; HASH],
            buf: [0; BUF],
            pos: 0,
            len: 0,
            keep,
            miss_bytes: 1024,
            retry_bytes: 4096,
            missed: 0,
            skip: 0,
        }
    }

    /// After chunks of `miss_bytes` in a row that didn't shrink, send the next `retry_bytes` bytes as literals without
    /// trying, then try again. Counted in bytes, so it is the same for any chunk size; `miss_bytes = 0` always tries.
    pub const fn with_backoff(mut self, miss_bytes: usize, retry_bytes: usize) -> Self {
        self.miss_bytes = miss_bytes;
        self.retry_bytes = retry_bytes;
        self
    }

    /// Starts the history over (stream open, bytes lost): the next block references nothing before it. The decoder
    /// resets at the same point.
    pub fn reset(&mut self) {
        // positions continue, nothing in the buffer is reachable any more; clearing the table only makes the output
        // depend on the bytes since the reset alone
        self.pos = self.pos.wrapping_add(self.len as u32);
        self.len = 0;
        self.table = [0; HASH];
        self.missed = 0;
        self.skip = 0;
    }

    /// How far back a block may reach: the history a decoder must keep, `min(BUF - 1, 65535)`.
    pub const HISTORY: usize = if BUF - 1 < MAX_DISTANCE {
        BUF - 1
    } else {
        MAX_DISTANCE
    };

    /// Largest chunk [`input`](Self::input) can give: `BUF - keep`.
    pub const fn max_chunk(&self) -> usize {
        BUF - self.keep
    }

    /// The free tail of the buffer, at least `want` bytes: put the next chunk at its start, then
    /// [`compress`](Self::compress) it. Slides the history first when the tail is shorter, keeping the last `keep`
    /// bytes; [`Error::ChunkTooLong`] when `want` is over [`max_chunk`](Self::max_chunk).
    pub fn input(&mut self, want: usize) -> Result<&mut [u8], Error> {
        if want > BUF - self.keep {
            return Err(Error::ChunkTooLong);
        }
        if BUF - self.len < want {
            let drop = self.len - self.keep;
            self.buf.copy_within(drop..self.len, 0);
            self.pos = self.pos.wrapping_add(drop as u32);
            self.len = self.keep;
        }
        Ok(&mut self.buf[self.len..])
    }

    /// Compresses the first `len` bytes of [`input`](Self::input)'s tail into `out` (at least
    /// [`max_block_len(len)`](max_block_len) bytes) as one LZ4 block and returns its length; the bytes become
    /// history. A chunk that doesn't shrink goes out as a literal-only block, at most `max_block_len(len)` bytes.
    pub fn compress(&mut self, len: usize, out: &mut [u8]) -> Result<usize, Error> {
        if len > BUF - self.len {
            return Err(Error::ChunkTooLong);
        }
        if out.len() < max_block_len(len) {
            return Err(Error::OutTooShort);
        }
        let start = self.len;
        self.len += len;
        if self.skip >= len {
            self.skip -= len;
            return Ok(literals(out, &self.buf[start..self.len]));
        }
        self.skip = 0;
        match encode_block(&mut self.table, &self.buf[..self.len], self.pos, start, out) {
            Some(n) => {
                self.missed = 0;
                Ok(n)
            }
            None => {
                // stays above miss_bytes until a chunk shrinks, so a retry that misses backs off at once
                self.missed = self.missed.saturating_add(len);
                if self.miss_bytes != 0 && self.missed >= self.miss_bytes {
                    self.skip = self.retry_bytes;
                }
                Ok(literals(out, &self.buf[start..self.len]))
            }
        }
    }

    /// Copies `chunk` in through [`input`](Self::input) and [`compress`](Self::compress)es it.
    pub fn compress_slice(&mut self, chunk: &[u8], out: &mut [u8]) -> Result<usize, Error> {
        self.input(chunk.len())?[..chunk.len()].copy_from_slice(chunk);
        self.compress(chunk.len(), out)
    }
}

/// A call the encoder can't do; nothing changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The chunk is longer than [`Encoder::max_chunk`], or than the tail [`Encoder::input`] gave.
    ChunkTooLong,
    /// `out` is shorter than [`max_block_len`] of the chunk.
    OutTooShort,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::ChunkTooLong => f.write_str("chunk longer than the encoder's free buffer"),
            Error::OutTooShort => f.write_str("output buffer shorter than max_block_len"),
        }
    }
}

impl core::error::Error for Error {}

#[inline(always)]
fn read32(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

/// Greedy LZ4 of `buf[start..]` with `buf[..start]` as history into `out`; `None` when the block isn't shorter than
/// the input. Not generic, so a firmware carries it once whatever the encoder's sizes.
#[cfg_attr(
    all(feature = "ram", target_os = "none"),
    unsafe(link_section = ".data.ww_lz4"),
    inline(never)
)]
fn encode_block(
    table: &mut [u16],
    buf: &[u8],
    pos: u32,
    start: usize,
    out: &mut [u8],
) -> Option<usize> {
    let end = buf.len();
    let limit = end - start;
    let shift = 32 - table.len().trailing_zeros();
    let hash = |seq: u32| (seq.wrapping_mul(2654435761) >> shift) as usize;
    let mut o = 0;
    let mut anchor = start;
    if end - start > MF_LIMIT {
        let mf_limit = end - MF_LIMIT;
        let match_limit = end - LAST_LITERALS;
        let mut ip = start;
        'outer: loop {
            // find a match
            let mut search = 1u32 << SKIP_TRIGGER;
            let cand;
            loop {
                if ip > mf_limit {
                    break 'outer;
                }
                let seq = read32(buf, ip);
                let h = hash(seq);
                let here = pos.wrapping_add(ip as u32) as u16;
                let dist = here.wrapping_sub(table[h]) as usize;
                table[h] = here;
                if dist != 0 && dist <= ip && read32(buf, ip - dist) == seq {
                    cand = ip - dist;
                    break;
                }
                ip += (search >> SKIP_TRIGGER) as usize;
                search += 1;
            }
            // extend backwards over literals, forwards up to the last literals
            let (mut ip_m, mut c) = (ip, cand);
            while ip_m > anchor && c > 0 && buf[ip_m - 1] == buf[c - 1] {
                ip_m -= 1;
                c -= 1;
            }
            let mut m = ip + MIN_MATCH;
            let mut cm = cand + MIN_MATCH;
            while m < match_limit && buf[m] == buf[cm] {
                m += 1;
                cm += 1;
            }
            o = emit(out, o, &buf[anchor..ip_m], ip_m - c, m - ip_m);
            if o >= limit {
                return None;
            }
            ip = m;
            anchor = ip;
            if ip > mf_limit {
                break;
            }
            let h = hash(read32(buf, ip - 2));
            table[h] = pos.wrapping_add((ip - 2) as u32) as u16;
        }
    }
    // last literals
    let lit = end - anchor;
    o = put_token_len(out, o, lit, 0);
    if o + lit >= limit {
        return None;
    }
    out[o..o + lit].copy_from_slice(&buf[anchor..end]);
    Some(o + lit)
}

/// A literal-only block of `lit`.
#[cfg_attr(
    all(feature = "ram", target_os = "none"),
    unsafe(link_section = ".data.ww_lz4"),
    inline(never)
)]
fn literals(out: &mut [u8], lit: &[u8]) -> usize {
    let o = put_token_len(out, 0, lit.len(), 0);
    out[o..o + lit.len()].copy_from_slice(lit);
    o + lit.len()
}

#[cfg_attr(
    all(feature = "ram", target_os = "none"),
    unsafe(link_section = ".data.ww_lz4"),
    inline(never)
)]
fn put_token_len(out: &mut [u8], o: usize, lit: usize, low: u8) -> usize {
    out[o] = ((lit.min(15) as u8) << 4) | low;
    if lit >= 15 {
        put_len(out, o + 1, lit - 15)
    } else {
        o + 1
    }
}

/// LZ4's length continuation bytes: 255 while more follows, then the rest.
#[inline(always)]
fn put_len(out: &mut [u8], mut o: usize, mut r: usize) -> usize {
    while r >= 255 {
        out[o] = 255;
        o += 1;
        r -= 255;
    }
    out[o] = r as u8;
    o + 1
}

#[cfg_attr(
    all(feature = "ram", target_os = "none"),
    unsafe(link_section = ".data.ww_lz4"),
    inline(never)
)]
fn emit(out: &mut [u8], o: usize, lit: &[u8], offset: usize, mlen: usize) -> usize {
    let ml = mlen - MIN_MATCH;
    let mut o = put_token_len(out, o, lit.len(), ml.min(15) as u8);
    out[o..o + lit.len()].copy_from_slice(lit);
    o += lit.len();
    out[o] = offset as u8;
    out[o + 1] = (offset >> 8) as u8;
    o += 2;
    if ml >= 15 {
        put_len(out, o, ml - 15)
    } else {
        o
    }
}
