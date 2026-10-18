//! LEB128 `uvar` and zigzag `ivar`, as in the logring format.

/// Bytes a `uvar` of `v` takes.
pub const fn uvar_len(mut v: u64) -> usize {
    let mut n = 1;
    while v >= 0x80 {
        v >>= 7;
        n += 1;
    }
    n
}

/// Bytes an `ivar` of `v` takes.
pub const fn ivar_len(v: i64) -> usize {
    uvar_len(zigzag(v))
}

const fn zigzag(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

/// Write `v` at the start of `buf` (which must be long enough), returns the bytes written.
pub(crate) fn put_uvar(buf: &mut [u8], mut v: u64) -> usize {
    let mut i = 0;
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            buf[i] = b;
            return i + 1;
        }
        buf[i] = b | 0x80;
        i += 1;
    }
}

pub(crate) fn put_ivar(buf: &mut [u8], v: i64) -> usize {
    put_uvar(buf, zigzag(v))
}

pub(crate) struct Reader<'b>(pub &'b [u8]);

impl<'b> Reader<'b> {
    pub fn uvar(&mut self) -> Option<u64> {
        let mut v = 0u64;
        for i in 0..10 {
            let (&b, rest) = self.0.split_first()?;
            self.0 = rest;
            v |= ((b & 0x7f) as u64).checked_shl(7 * i)?;
            if b & 0x80 == 0 {
                return Some(v);
            }
        }
        None
    }

    pub fn ivar(&mut self) -> Option<i64> {
        let u = self.uvar()?;
        Some(((u >> 1) as i64) ^ -((u & 1) as i64))
    }

    pub fn take(&mut self, n: usize) -> Option<&'b [u8]> {
        if self.0.len() < n {
            return None;
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Some(a)
    }
}
