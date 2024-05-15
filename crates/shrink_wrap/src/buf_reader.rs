/// Buffer reader that treats input as a stream of nibbles.
pub struct BufReader<'i> {
    buf: &'i mut [u8],
    // Maximum number of bytes available (not whole slice might be available)
    len_bytes: usize,
    // Next byte to read from
    idx: usize,
    // Next bit to read from
    bit_idx: u8,
}
