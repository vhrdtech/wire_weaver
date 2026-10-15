/*
 * shrink_wrap C99 runtime (SW-43): BufReader and BufWriter of the shrink_wrap wire format, the same bytes as the
 * Rust crate (shrink_wrap/shrink_wrap/src/buf_reader.rs, buf_writer.rs). No malloc, no libc beyond <stdint.h>,
 * <stddef.h>, <stdbool.h>, <string.h> and the freestanding <limits.h>. See docs/c.md.
 *
 * Buffers are arrays of sw_octet, one octet (8 bits) per element. With CHAR_BIT == 8 that is a plain byte array.
 * With CHAR_BIT == 16 (TI C2000) every element still holds one octet in its low 8 bits: the runtime reads an
 * element masked with 0xFF and writes it with the high bits cleared, so unpack packed words before reading and
 * pack them after writing.
 *
 * Errors: the first error is kept in the reader or writer (the SW-41 latch). A call that fails moves nothing, and
 * while an error is kept no call moves, reads give 0 (an empty view for bytes) and writes write nothing. Check it
 * once after a run of calls with sw_reader_error() / sw_writer_error(); sw_writer_finish() returns it too.
 */
#ifndef SHRINK_WRAP_H
#define SHRINK_WRAP_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/** One octet of a buffer, in the low 8 bits of the element. */
typedef uint_least8_t sw_octet;

/** Same numbers as the Rust `shrink_wrap::Error` (`#[repr(u32)]`), 0 is no error. */
typedef enum {
    SW_OK = 0,
    SW_ERR_OUT_OF_BOUNDS_WRITE_BOOL = 1,
    SW_ERR_OUT_OF_BOUNDS_READ_BOOL = 2,
    SW_ERR_OUT_OF_BOUNDS_WRITE_U4 = 3,
    SW_ERR_OUT_OF_BOUNDS_READ_U4 = 4,
    SW_ERR_OUT_OF_BOUNDS_WRITE_U8 = 5,
    SW_ERR_OUT_OF_BOUNDS_READ_U8 = 6,
    SW_ERR_OUT_OF_BOUNDS_WRITE_RAW_SLICE = 7,
    SW_ERR_OUT_OF_BOUNDS_READ_RAW_SLICE = 8,
    SW_ERR_OUT_OF_BOUNDS_WRITE_UN = 9,
    SW_ERR_OUT_OF_BOUNDS_READ_UN = 10,
    SW_ERR_OUT_OF_BOUNDS_SPLIT = 11,
    SW_ERR_OUT_OF_BOUNDS_REV = 12,
    SW_ERR_OUT_OF_BOUNDS_REV_COMPACT = 13,
    SW_ERR_INTERNAL_SLICE_TO_ARRAY_CAST = 14,
    SW_ERR_MALFORMED_UNIB32 = 15,
    SW_ERR_MALFORMED_UVLQ32 = 16,
    SW_ERR_MALFORMED_UVLQ64 = 17,
    SW_ERR_MALFORMED_LEB = 18,
    SW_ERR_MALFORMED_UTF8 = 19,
    SW_ERR_LEN_TOO_LONG = 20,
    SW_ERR_ENUM_FUTURE_VERSION_OR_MALFORMED_DATA = 21,
    SW_ERR_INVALID_BIT_COUNT = 22,
    SW_ERR_SUBTYPE_OUT_OF_RANGE = 23,
    SW_ERR_MALFORMED_SERIES = 24,
    SW_ERR_LEN_FIFO_NOT_EMPTY = 25,
    SW_ERR_ENUM_VARIANT_DISABLED = 26,
    SW_ERR_MAX_LEN_EXCEEDED = 27,
    SW_ERR_MALFORMED_OPTION_ANY_VEC = 28,
    SW_ERR_OPTION_ANY_VEC_WRONG_SLOT = 29,
    SW_ERR_OPTION_ANY_VEC_UNKNOWN_SLOT = 30
} sw_error;

/** A borrowed run of octets inside a reader's buffer (bytes or a UTF-8 string, not NUL terminated). */
typedef struct {
    const sw_octet *ptr;
    size_t len;
} sw_bytes;

/* ---------------------------------------------------------------------------------------------------------------
 * Reader
 * ------------------------------------------------------------------------------------------------------------- */

/** Reads bits, nibbles and bytes from the front and lengths (reversed UNib32) from the back. Fields are private. */
typedef struct {
    const sw_octet *buf;
    /** Buffer length from the front, shrinks as lengths are read from the back */
    size_t len_bytes;
    /** Next byte to read from */
    size_t byte_idx;
    /** Next bit to read from, starts from 7 */
    unsigned bit_idx;
    /** false: next reversed nibble is bits 3:0 of the last byte, true: bits 7:4 */
    bool is_at_bit7_rev;
    sw_error err;
} sw_reader;

void sw_reader_init(sw_reader *rd, const sw_octet *buf, size_t len);
/** The kept error, SW_OK if none. */
sw_error sw_reader_error(const sw_reader *rd);
/** Return the kept error and clear it (Rust `latched()`): reads move again afterwards. */
sw_error sw_reader_take_error(sw_reader *rd);

bool sw_read_bool(sw_reader *rd);
/** Align to nibble and read one nibble. */
uint_least8_t sw_read_nib(sw_reader *rd);
/** Read `bit_count` bits (up to 8 / 16 / 32 / 64) without alignment, most significant first. */
uint_least8_t sw_read_un8(sw_reader *rd, unsigned bit_count);
uint_least16_t sw_read_un16(sw_reader *rd, unsigned bit_count);
uint32_t sw_read_un32(sw_reader *rd, unsigned bit_count);
uint64_t sw_read_un64(sw_reader *rd, unsigned bit_count);
/** Align to byte and read a Little Endian number. */
uint_least8_t sw_read_u8(sw_reader *rd);
uint_least16_t sw_read_u16(sw_reader *rd);
uint32_t sw_read_u32(sw_reader *rd);
uint64_t sw_read_u64(sw_reader *rd);
int_least8_t sw_read_i8(sw_reader *rd);
int_least16_t sw_read_i16(sw_reader *rd);
int32_t sw_read_i32(sw_reader *rd);
int64_t sw_read_i64(sw_reader *rd);
float sw_read_f32(sw_reader *rd);
#ifndef SW_NO_F64
/** Needs a 64-bit double; define SW_NO_F64 where double is 32 bits and read the bits with sw_read_u64(). */
double sw_read_f64(sw_reader *rd);
#endif
/** Align to nibble and read a forward UNib32. */
uint32_t sw_read_unib32(sw_reader *rd);
/** Align to byte and read a UVlq32 / UVlq64 (big endian VLQ). */
uint32_t sw_read_uvlq32(sw_reader *rd);
uint64_t sw_read_uvlq64(sw_reader *rd);
/** Read a length (reversed UNib32) from the back of the buffer. */
uint32_t sw_read_rev_len(sw_reader *rd);
/** Align to byte and take `len` bytes as they are. */
sw_bytes sw_read_raw(sw_reader *rd, size_t len);
/** A length from the back, then that many bytes. */
sw_bytes sw_read_bytes(sw_reader *rd);
/** sw_read_bytes() that also checks the bytes are UTF-8 (SW_ERR_MALFORMED_UTF8). */
sw_bytes sw_read_str(sw_reader *rd);
/**
 * Align to byte and split off a reader of the next `len` bytes, the parent skips them. How `Unsized` values and
 * everything after them stay readable by older and newer code. The child has the parent's error if there is one,
 * or SW_ERR_OUT_OF_BOUNDS_SPLIT is kept in both when `len` bytes are not there.
 */
sw_reader sw_reader_split(sw_reader *rd, size_t len);
/** sw_reader_split() of a length read from the back: how an `Unsized` value is read with Rust `read::<T>()`. */
sw_reader sw_reader_split_unsized(sw_reader *rd);
/** Keep the child's error in the parent, if the parent has none yet. */
void sw_reader_join(sw_reader *rd, const sw_reader *child);
void sw_reader_align_byte(sw_reader *rd);
/** Bytes, nibbles and bits left, the lengths read from the back taken into account. */
size_t sw_reader_bytes_left(const sw_reader *rd);
size_t sw_reader_nibbles_left(const sw_reader *rd);
size_t sw_reader_bits_left(const sw_reader *rd);
/** Byte and bit (7..0) the next read starts at. */
void sw_reader_pos(const sw_reader *rd, size_t *byte_idx, unsigned *bit_idx);

/* ---------------------------------------------------------------------------------------------------------------
 * Writer
 * ------------------------------------------------------------------------------------------------------------- */

/** Writes from the front, lengths are pushed to the back as u16 and encoded as reversed UNib32 later. */
typedef struct {
    sw_octet *buf;
    size_t cap;
    /** Buffer length from the front, shrinks when a length is pushed to the back */
    size_t len_bytes;
    /** Next byte to write to */
    size_t byte_idx;
    /** Next bit to write to, starts from 7 */
    unsigned bit_idx;
    sw_error err;
} sw_writer;

/** Position of a length slot at the back of the buffer, see sw_write_rev_len(). */
typedef size_t sw_rev_pos;

/** Saved write position, see sw_writer_save() */
typedef struct {
    size_t byte_idx;
    unsigned bit_idx;
    size_t len_bytes;
} sw_writer_state;

/** Started `Unsized` value, see sw_unsized_begin() */
typedef struct {
    sw_rev_pos size_slot;
    size_t start;
} sw_unsized;

/** `buf` does not need to be zeroed. */
void sw_writer_init(sw_writer *wr, sw_octet *buf, size_t cap);
/** Back to the start, forgetting what was written and the kept error. */
void sw_writer_reset(sw_writer *wr);
sw_error sw_writer_error(const sw_writer *wr);
sw_error sw_writer_take_error(sw_writer *wr);

/* Every write returns the kept error after it, SW_OK if there is none. */
sw_error sw_write_bool(sw_writer *wr, bool val);
/** Align to nibble and write the low 4 bits of `val`. */
sw_error sw_write_nib(sw_writer *wr, unsigned val);
/** Write the low `bit_count` bits (up to 8 / 16 / 32 / 64) without alignment, most significant first. */
sw_error sw_write_un8(sw_writer *wr, unsigned bit_count, uint_least8_t val);
sw_error sw_write_un16(sw_writer *wr, unsigned bit_count, uint_least16_t val);
sw_error sw_write_un32(sw_writer *wr, unsigned bit_count, uint32_t val);
sw_error sw_write_un64(sw_writer *wr, unsigned bit_count, uint64_t val);
/** Align to byte and write a Little Endian number. */
sw_error sw_write_u8(sw_writer *wr, uint_least8_t val);
sw_error sw_write_u16(sw_writer *wr, uint_least16_t val);
sw_error sw_write_u32(sw_writer *wr, uint32_t val);
sw_error sw_write_u64(sw_writer *wr, uint64_t val);
sw_error sw_write_i8(sw_writer *wr, int_least8_t val);
sw_error sw_write_i16(sw_writer *wr, int_least16_t val);
sw_error sw_write_i32(sw_writer *wr, int32_t val);
sw_error sw_write_i64(sw_writer *wr, int64_t val);
sw_error sw_write_f32(sw_writer *wr, float val);
#ifndef SW_NO_F64
sw_error sw_write_f64(sw_writer *wr, double val);
#endif
/** Align to nibble and write a forward UNib32 (1 to 11 nibbles). */
sw_error sw_write_unib32(sw_writer *wr, uint32_t val);
/** Align to byte and write a UVlq32 / UVlq64 (big endian VLQ, 1 to 5 / 10 bytes). */
sw_error sw_write_uvlq32(sw_writer *wr, uint32_t val);
sw_error sw_write_uvlq64(sw_writer *wr, uint64_t val);
/** Align to byte and write `len` octets as they are, no length. */
sw_error sw_write_raw(sw_writer *wr, const sw_octet *val, size_t len);
/** A length to the back, then the bytes. */
sw_error sw_write_bytes(sw_writer *wr, const sw_octet *val, size_t len);
/** sw_write_bytes() from chars, the low 8 bits of each (UTF-8 is not checked). */
sw_error sw_write_str(sw_writer *wr, const char *val, size_t len);
/**
 * Push a length (at most 65535) to the back of the buffer as u16, encoded to reversed UNib32 by
 * sw_encode_len_fifo() or sw_writer_finish(). `pos` (may be NULL) gets the slot for sw_update_rev_len().
 */
sw_error sw_write_rev_len(sw_writer *wr, size_t len, sw_rev_pos *pos);
/** Top of the length FIFO now. */
sw_rev_pos sw_rev_len_pos(const sw_writer *wr);
/** Overwrite a length pushed earlier. */
sw_error sw_update_rev_len(sw_writer *wr, sw_rev_pos pos, size_t len);
/** Encode the lengths pushed between `from` (top, pushed last) and `to` as reversed UNib32 at the front. */
sw_error sw_encode_len_fifo(sw_writer *wr, sw_rev_pos from, sw_rev_pos to);
/** Start an `Unsized` value as Rust `write::<T>()` does: align, reserve its size slot. */
sw_error sw_unsized_begin(sw_writer *wr, sw_unsized *u);
/** End it: encode the lengths its fields pushed, align and fill in its size. */
sw_error sw_unsized_end(sw_writer *wr, const sw_unsized *u);
void sw_writer_align_byte(sw_writer *wr);
/** Align to nibble, zeroing the bits skipped. */
void sw_writer_align_nibble(sw_writer *wr);
/**
 * Encode the lengths left in the FIFO, align to byte and put the number of bytes written into `len`: the message
 * is buf[0..len]. Returns the kept error instead if there is one (`len` is then 0). On success the writer starts
 * over, as after sw_writer_reset().
 */
sw_error sw_writer_finish(sw_writer *wr, size_t *len);
/** Whole bytes left (a partly written byte is not counted). */
size_t sw_writer_bytes_left(const sw_writer *wr);
size_t sw_writer_nibbles_left(const sw_writer *wr);
void sw_writer_pos(const sw_writer *wr, size_t *byte_idx, unsigned *bit_idx);
/** Save the write position, to go back and patch e.g. a flag or a discriminant. */
sw_writer_state sw_writer_save(const sw_writer *wr);
/** Go back to a saved position. Writing lengths or `Unsized` values after that corrupts the ones written before. */
void sw_writer_restore(sw_writer *wr, sw_writer_state state);

#ifdef __cplusplus
}
#endif

#endif /* SHRINK_WRAP_H */
