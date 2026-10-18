/*
 * shrink_wrap C99 runtime (SW-43), a port of shrink_wrap/shrink_wrap/src/buf_reader.rs and buf_writer.rs.
 * Checked against the Rust crate by shrink_wrap/c/tests (golden vectors and a property-based cross-check).
 *
 * Every public call works on a copy of the cursor and only commits it when the call succeeds, so a failing call
 * moves nothing. The kept error makes later calls no-ops (SW-41).
 *
 * 16-bit char (TI C2000): every buffer element goes through octet_get() / octet_put(), values are held in
 * unsigned / uint32_t / uint64_t and masked to 8 bits before they are stored, shifts are done on unsigned types
 * of known width (int may be 16 bits). Not portable there: float must be 32-bit IEEE 754 and, unless SW_NO_F64
 * is defined, double 64-bit IEEE 754 (checked at compile time).
 */
#include "shrink_wrap.h"

#include <limits.h>
#include <string.h>

/* compile-time checks, C99 has no _Static_assert */
typedef char sw_float_is_32_bits[(sizeof(float) * CHAR_BIT == 32) ? 1 : -1];
#ifndef SW_NO_F64
typedef char sw_double_is_64_bits[(sizeof(double) * CHAR_BIT == 64) ? 1 : -1];
#endif

#define UNIB_MORE 0x8u
#define VLQ_MORE 0x80u
#define VLQ32_MAX_LEN 5u
#define VLQ64_MAX_LEN 10u

static unsigned octet_get(const sw_octet *buf, size_t idx) {
    return (unsigned)buf[idx] & 0xFFu;
}

static void octet_put(sw_octet *buf, size_t idx, unsigned val) {
    buf[idx] = (sw_octet)(val & 0xFFu);
}

/* ===============================================================================================================
 * Reader
 * ============================================================================================================= */

void sw_reader_init(sw_reader *rd, const sw_octet *buf, size_t len) {
    rd->buf = buf;
    rd->len_bytes = len;
    rd->byte_idx = 0;
    rd->bit_idx = 7;
    rd->is_at_bit7_rev = false;
    rd->err = SW_OK;
}

sw_error sw_reader_error(const sw_reader *rd) {
    return rd->err;
}

sw_error sw_reader_take_error(sw_reader *rd) {
    sw_error e = rd->err;
    rd->err = SW_OK;
    return e;
}

static void rd_align_byte(sw_reader *rd) {
    if (rd->bit_idx != 7) {
        rd->bit_idx = 7;
        rd->byte_idx++;
    }
}

static void rd_align_nibble(sw_reader *rd) {
    if (rd->bit_idx == 7 || rd->bit_idx == 3) {
        return;
    }
    if (rd->bit_idx > 3) {
        rd->bit_idx = 3;
    } else {
        rd->bit_idx = 7;
        rd->byte_idx++;
    }
}

size_t sw_reader_bytes_left(const sw_reader *rd) {
    size_t left;
    if (rd->byte_idx >= rd->len_bytes) {
        return 0;
    }
    left = rd->len_bytes - rd->byte_idx;
    if (rd->bit_idx != 7) {
        /* already read a bit or a nibble of this byte */
        left--;
    }
    if (left == 0) {
        return 0;
    }
    /* already read one nibble of the last byte from the back */
    return rd->is_at_bit7_rev ? left - 1 : left;
}

static unsigned rd_nibbles_in_byte_left(const sw_reader *rd) {
    if (rd->byte_idx >= rd->len_bytes) {
        return 0;
    }
    if (rd->byte_idx + 1 == rd->len_bytes) {
        unsigned n = 2;
        if (rd->bit_idx != 7) {
            n--;
        }
        if (rd->is_at_bit7_rev) {
            n--;
        }
        return n;
    }
    if (rd->bit_idx == 7) {
        return 2;
    }
    return rd->bit_idx == 3 ? 1 : 0;
}

static unsigned rd_bits_in_byte_left(const sw_reader *rd) {
    if (rd->byte_idx >= rd->len_bytes) {
        return 0;
    }
    if (rd->byte_idx + 1 == rd->len_bytes && rd->is_at_bit7_rev) {
        /* the low nibble of the last byte is a length read from the back (SW-40) */
        return rd->bit_idx >= 3 ? rd->bit_idx - 3 : 0;
    }
    return rd->bit_idx + 1;
}

/* the bit where the forward side ends: the lengths read from the back begin there */
static size_t rd_front_end(const sw_reader *rd) {
    return rd->len_bytes * 8 - (rd->is_at_bit7_rev ? 4 : 0);
}

static size_t rd_bit_pos(const sw_reader *rd) {
    return rd->byte_idx * 8 + 7 - rd->bit_idx;
}

size_t sw_reader_nibbles_left(const sw_reader *rd) {
    /* nibble reads align to a nibble first */
    size_t at = (rd_bit_pos(rd) + 3) / 4 * 4;
    size_t end = rd_front_end(rd);
    return at < end ? (end - at) / 4 : 0;
}

size_t sw_reader_bits_left(const sw_reader *rd) {
    size_t at = rd_bit_pos(rd);
    size_t end = rd_front_end(rd);
    return at < end ? end - at : 0;
}

void sw_reader_pos(const sw_reader *rd, size_t *byte_idx, unsigned *bit_idx) {
    *byte_idx = rd->byte_idx;
    *bit_idx = rd->bit_idx;
}

void sw_reader_align_byte(sw_reader *rd) {
    if (rd->err == SW_OK) {
        rd_align_byte(rd);
    }
}

/* The rd_* operations below report an error without keeping it and may leave the cursor half moved, the public
 * functions run them on a copy. */

static sw_error rd_bit(sw_reader *rd, unsigned *out) {
    if (rd_bits_in_byte_left(rd) == 0) {
        return SW_ERR_OUT_OF_BOUNDS_READ_BOOL;
    }
    *out = (octet_get(rd->buf, rd->byte_idx) >> rd->bit_idx) & 1u;
    if (rd->bit_idx == 0) {
        rd->bit_idx = 7;
        rd->byte_idx++;
    } else {
        rd->bit_idx--;
    }
    return SW_OK;
}

static sw_error rd_nib(sw_reader *rd, unsigned *out) {
    rd_align_nibble(rd);
    if (rd_nibbles_in_byte_left(rd) == 0) {
        return SW_ERR_OUT_OF_BOUNDS_READ_U4;
    }
    if (rd->bit_idx == 7) {
        rd->bit_idx = 3;
        *out = octet_get(rd->buf, rd->byte_idx) >> 4;
    } else {
        *out = octet_get(rd->buf, rd->byte_idx) & 0xFu;
        rd->bit_idx = 7;
        rd->byte_idx++;
    }
    return SW_OK;
}

static sw_error rd_un(sw_reader *rd, unsigned bit_count, unsigned max_bits, uint64_t *out) {
    uint64_t result = 0;
    unsigned bits_left = bit_count;
    if (bit_count > max_bits) {
        return SW_ERR_INVALID_BIT_COUNT;
    }
    while (bits_left > 0) {
        unsigned in_byte = rd_bits_in_byte_left(rd);
        unsigned n, shift, bits;
        if (in_byte == 0) {
            return SW_ERR_OUT_OF_BOUNDS_READ_UN;
        }
        n = bits_left < in_byte ? bits_left : in_byte;
        shift = rd->bit_idx + 1 - n;
        bits = (octet_get(rd->buf, rd->byte_idx) >> shift) & ((1u << n) - 1u);
        result = (result << n) | bits;
        if (n == rd->bit_idx + 1) {
            rd->bit_idx = 7;
            rd->byte_idx++;
        } else {
            rd->bit_idx -= n;
        }
        bits_left -= n;
    }
    *out = result;
    return SW_OK;
}

/* align to byte and take `len` bytes, the start index goes to `start` */
static bool rd_take(sw_reader *rd, size_t len, size_t *start) {
    rd_align_byte(rd);
    if (sw_reader_bytes_left(rd) < len) {
        return false;
    }
    *start = rd->byte_idx;
    rd->byte_idx += len;
    return true;
}

/* align to byte and read `len` (1 to 8) bytes Little Endian */
static sw_error rd_le(sw_reader *rd, unsigned len, uint64_t *out) {
    size_t start;
    uint64_t val = 0;
    unsigned i;
    if (!rd_take(rd, len, &start)) {
        return len == 1 ? SW_ERR_OUT_OF_BOUNDS_READ_U8 : SW_ERR_OUT_OF_BOUNDS_READ_RAW_SLICE;
    }
    for (i = len; i > 0; i--) {
        val = (val << 8) | octet_get(rd->buf, start + i - 1);
    }
    *out = val;
    return SW_OK;
}

static sw_error rd_u4_rev(sw_reader *rd, unsigned *out) {
    if (rd->byte_idx >= rd->len_bytes) {
        return SW_ERR_OUT_OF_BOUNDS_REV;
    }
    /* the forward side is in the last byte: the nibble must be one it has not read into (SW-40) */
    if (rd->byte_idx + 1 == rd->len_bytes) {
        bool free_nib = rd->is_at_bit7_rev ? rd->bit_idx == 7 : rd->bit_idx >= 3;
        if (!free_nib) {
            return SW_ERR_OUT_OF_BOUNDS_REV;
        }
    }
    if (rd->is_at_bit7_rev) {
        rd->is_at_bit7_rev = false;
        rd->len_bytes--;
        *out = octet_get(rd->buf, rd->len_bytes) >> 4;
    } else {
        rd->is_at_bit7_rev = true;
        *out = octet_get(rd->buf, rd->len_bytes - 1) & 0xFu;
    }
    return SW_OK;
}

static sw_error rd_unib32(sw_reader *rd, bool reversed, uint64_t *out) {
    uint32_t num = 0;
    unsigned offset = 0;
    unsigned i;
    for (i = 0; i <= 10; i++) {
        unsigned nib;
        sw_error e = reversed ? rd_u4_rev(rd, &nib) : rd_nib(rd, &nib);
        if (e != SW_OK) {
            return e;
        }
        /* the 11th nibble is the last one of a u32 */
        if (i == 10 && (nib & UNIB_MORE) != 0) {
            return SW_ERR_MALFORMED_UNIB32;
        }
        if (reversed) {
            num |= (uint32_t)(nib & 7u);
        } else {
            num |= (uint32_t)(nib & 7u) << offset;
        }
        if ((nib & UNIB_MORE) == 0) {
            break;
        }
        if (reversed) {
            num <<= 3;
        } else {
            offset += 3;
        }
    }
    *out = num;
    return SW_OK;
}

static sw_error rd_uvlq(sw_reader *rd, unsigned max_len, uint64_t max, sw_error malformed, uint64_t *out) {
    uint64_t num = 0;
    unsigned i;
    for (i = 0; i < max_len; i++) {
        uint64_t byte;
        sw_error e = rd_le(rd, 1, &byte);
        if (e != SW_OK) {
            return e;
        }
        if (num > (max >> 7)) {
            /* shifting would lose the most significant bits */
            return malformed;
        }
        num = (num << 7) | (byte & 0x7Fu);
        if ((byte & VLQ_MORE) == 0) {
            *out = num;
            return SW_OK;
        }
    }
    return malformed;
}

typedef enum { RD_BOOL, RD_NIB, RD_UN, RD_LE, RD_UNIB32, RD_REV_LEN, RD_UVLQ32, RD_UVLQ64 } rd_kind;

/* Run one read on a copy of the cursor, commit it on success, keep the error otherwise. 0 after an error. */
static uint64_t rd_run(sw_reader *rd, rd_kind kind, unsigned arg, unsigned max_bits) {
    sw_reader tmp;
    sw_error e = SW_OK;
    uint64_t val = 0;
    unsigned small = 0;
    if (rd->err != SW_OK) {
        return 0;
    }
    tmp = *rd;
    switch (kind) {
    case RD_BOOL:
        e = rd_bit(&tmp, &small);
        val = small;
        break;
    case RD_NIB:
        e = rd_nib(&tmp, &small);
        val = small;
        break;
    case RD_UN:
        e = rd_un(&tmp, arg, max_bits, &val);
        break;
    case RD_LE:
        e = rd_le(&tmp, arg, &val);
        break;
    case RD_UNIB32:
        e = rd_unib32(&tmp, false, &val);
        break;
    case RD_REV_LEN:
        e = rd_unib32(&tmp, true, &val);
        break;
    case RD_UVLQ32:
        e = rd_uvlq(&tmp, VLQ32_MAX_LEN, UINT32_MAX, SW_ERR_MALFORMED_UVLQ32, &val);
        break;
    case RD_UVLQ64:
        e = rd_uvlq(&tmp, VLQ64_MAX_LEN, UINT64_MAX, SW_ERR_MALFORMED_UVLQ64, &val);
        break;
    }
    if (e != SW_OK) {
        rd->err = e;
        return 0;
    }
    *rd = tmp;
    return val;
}

bool sw_read_bool(sw_reader *rd) {
    return rd_run(rd, RD_BOOL, 0, 0) != 0;
}

uint_least8_t sw_read_nib(sw_reader *rd) {
    return (uint_least8_t)rd_run(rd, RD_NIB, 0, 0);
}

uint_least8_t sw_read_un8(sw_reader *rd, unsigned bit_count) {
    return (uint_least8_t)rd_run(rd, RD_UN, bit_count, 8);
}

uint_least16_t sw_read_un16(sw_reader *rd, unsigned bit_count) {
    return (uint_least16_t)rd_run(rd, RD_UN, bit_count, 16);
}

uint32_t sw_read_un32(sw_reader *rd, unsigned bit_count) {
    return (uint32_t)rd_run(rd, RD_UN, bit_count, 32);
}

uint64_t sw_read_un64(sw_reader *rd, unsigned bit_count) {
    return rd_run(rd, RD_UN, bit_count, 64);
}

uint_least8_t sw_read_u8(sw_reader *rd) {
    return (uint_least8_t)rd_run(rd, RD_LE, 1, 0);
}

uint_least16_t sw_read_u16(sw_reader *rd) {
    return (uint_least16_t)rd_run(rd, RD_LE, 2, 0);
}

uint32_t sw_read_u32(sw_reader *rd) {
    return (uint32_t)rd_run(rd, RD_LE, 4, 0);
}

uint64_t sw_read_u64(sw_reader *rd) {
    return rd_run(rd, RD_LE, 8, 0);
}

/* Two's complement from the unsigned bits without relying on an implementation-defined conversion. */
static int64_t to_signed(uint64_t val, unsigned bits) {
    uint64_t sign = (uint64_t)1 << (bits - 1);
    if ((val & sign) == 0) {
        return (int64_t)val;
    }
    /* val - 2^bits, as -(the inverted bits below the sign) - 1, which cannot overflow */
    return -(int64_t)(~val & (sign - 1)) - 1;
}

int_least8_t sw_read_i8(sw_reader *rd) {
    return (int_least8_t)to_signed(rd_run(rd, RD_LE, 1, 0), 8);
}

int_least16_t sw_read_i16(sw_reader *rd) {
    return (int_least16_t)to_signed(rd_run(rd, RD_LE, 2, 0), 16);
}

int32_t sw_read_i32(sw_reader *rd) {
    return (int32_t)to_signed(rd_run(rd, RD_LE, 4, 0), 32);
}

int64_t sw_read_i64(sw_reader *rd) {
    return to_signed(rd_run(rd, RD_LE, 8, 0), 64);
}

float sw_read_f32(sw_reader *rd) {
    uint32_t bits = (uint32_t)rd_run(rd, RD_LE, 4, 0);
    float val;
    memcpy(&val, &bits, sizeof val);
    return val;
}

#ifndef SW_NO_F64
double sw_read_f64(sw_reader *rd) {
    uint64_t bits = rd_run(rd, RD_LE, 8, 0);
    double val;
    memcpy(&val, &bits, sizeof val);
    return val;
}
#endif

uint32_t sw_read_unib32(sw_reader *rd) {
    return (uint32_t)rd_run(rd, RD_UNIB32, 0, 0);
}

uint32_t sw_read_uvlq32(sw_reader *rd) {
    return (uint32_t)rd_run(rd, RD_UVLQ32, 0, 0);
}

uint64_t sw_read_uvlq64(sw_reader *rd) {
    return rd_run(rd, RD_UVLQ64, 0, 0);
}

uint32_t sw_read_rev_len(sw_reader *rd) {
    return (uint32_t)rd_run(rd, RD_REV_LEN, 0, 0);
}

/* A length as size_t: with a 16-bit size_t (AVR, MSP430) a longer one is out of bounds anyway, as SIZE_MAX is. */
static size_t rev_len_size(uint32_t len) {
#if UINT32_MAX > SIZE_MAX
    if (len > SIZE_MAX) {
        return SIZE_MAX;
    }
#endif
    return (size_t)len;
}

static const sw_bytes empty_bytes = {NULL, 0};

sw_bytes sw_read_raw(sw_reader *rd, size_t len) {
    sw_reader tmp;
    size_t start;
    sw_bytes out;
    if (rd->err != SW_OK) {
        return empty_bytes;
    }
    tmp = *rd;
    if (!rd_take(&tmp, len, &start)) {
        rd->err = SW_ERR_OUT_OF_BOUNDS_READ_RAW_SLICE;
        return empty_bytes;
    }
    *rd = tmp;
    out.ptr = rd->buf + start;
    out.len = len;
    return out;
}

sw_bytes sw_read_bytes(sw_reader *rd) {
    sw_reader start = *rd;
    size_t len = rev_len_size(sw_read_rev_len(rd));
    sw_bytes out = sw_read_raw(rd, len);
    if (rd->err != SW_OK && start.err == SW_OK) {
        /* failing calls move nothing, also the length read before the bytes */
        start.err = rd->err;
        *rd = start;
    }
    return out;
}

/* The checks of Rust's core::str::from_utf8: no overlong forms, no surrogates, nothing above U+10FFFF. */
static bool is_utf8(const sw_octet *s, size_t len) {
    size_t i = 0;
    while (i < len) {
        unsigned b = octet_get(s, i);
        unsigned lo = 0x80, hi = 0xBF;
        size_t n, k;
        if (b < 0x80) {
            i++;
            continue;
        } else if (b >= 0xC2 && b <= 0xDF) {
            n = 1;
        } else if (b >= 0xE0 && b <= 0xEF) {
            n = 2;
            if (b == 0xE0) {
                lo = 0xA0;
            } else if (b == 0xED) {
                hi = 0x9F;
            }
        } else if (b >= 0xF0 && b <= 0xF4) {
            n = 3;
            if (b == 0xF0) {
                lo = 0x90;
            } else if (b == 0xF4) {
                hi = 0x8F;
            }
        } else {
            return false;
        }
        if (len - i - 1 < n) {
            return false;
        }
        for (k = 1; k <= n; k++) {
            unsigned c = octet_get(s, i + k);
            /* the first continuation byte has the narrower range */
            if (k == 1 ? (c < lo || c > hi) : (c < 0x80 || c > 0xBF)) {
                return false;
            }
        }
        i += n + 1;
    }
    return true;
}

sw_bytes sw_read_str(sw_reader *rd) {
    sw_reader start = *rd;
    sw_bytes out = sw_read_bytes(rd);
    if (rd->err == SW_OK && !is_utf8(out.ptr, out.len)) {
        start.err = SW_ERR_MALFORMED_UTF8;
        *rd = start;
        return empty_bytes;
    }
    return out;
}

sw_reader sw_reader_split(sw_reader *rd, size_t len) {
    sw_reader child;
    sw_reader tmp;
    size_t start;
    sw_reader_init(&child, rd->buf, 0);
    if (rd->err != SW_OK) {
        child.err = rd->err;
        return child;
    }
    tmp = *rd;
    if (!rd_take(&tmp, len, &start)) {
        rd->err = SW_ERR_OUT_OF_BOUNDS_SPLIT;
        child.err = rd->err;
        return child;
    }
    *rd = tmp;
    sw_reader_init(&child, rd->buf + start, len);
    return child;
}

sw_reader sw_reader_split_unsized(sw_reader *rd) {
    sw_reader start = *rd;
    size_t len = rev_len_size(sw_read_rev_len(rd));
    sw_reader child = sw_reader_split(rd, len);
    if (rd->err != SW_OK && start.err == SW_OK) {
        start.err = rd->err;
        *rd = start;
    }
    return child;
}

void sw_reader_join(sw_reader *rd, const sw_reader *child) {
    if (rd->err == SW_OK) {
        rd->err = child->err;
    }
}

/* ===============================================================================================================
 * Writer
 * ============================================================================================================= */

void sw_writer_init(sw_writer *wr, sw_octet *buf, size_t cap) {
    wr->buf = buf;
    wr->cap = cap;
    sw_writer_reset(wr);
}

void sw_writer_reset(sw_writer *wr) {
    wr->len_bytes = wr->cap;
    wr->byte_idx = 0;
    wr->bit_idx = 7;
    wr->err = SW_OK;
}

sw_error sw_writer_error(const sw_writer *wr) {
    return wr->err;
}

sw_error sw_writer_take_error(sw_writer *wr) {
    sw_error e = wr->err;
    wr->err = SW_OK;
    return e;
}

size_t sw_writer_bytes_left(const sw_writer *wr) {
    if (wr->byte_idx >= wr->len_bytes) {
        return 0;
    }
    return wr->len_bytes - wr->byte_idx - (wr->bit_idx == 7 ? 0 : 1);
}

size_t sw_writer_nibbles_left(const sw_writer *wr) {
    return sw_writer_bytes_left(wr) * 2 + (wr->bit_idx == 3 ? 1 : 0);
}

void sw_writer_pos(const sw_writer *wr, size_t *byte_idx, unsigned *bit_idx) {
    *byte_idx = wr->byte_idx;
    *bit_idx = wr->bit_idx;
}

sw_writer_state sw_writer_save(const sw_writer *wr) {
    sw_writer_state s;
    s.byte_idx = wr->byte_idx;
    s.bit_idx = wr->bit_idx;
    s.len_bytes = wr->len_bytes;
    return s;
}

void sw_writer_restore(sw_writer *wr, sw_writer_state state) {
    wr->byte_idx = state.byte_idx;
    wr->bit_idx = state.bit_idx;
    wr->len_bytes = state.len_bytes;
}

sw_rev_pos sw_rev_len_pos(const sw_writer *wr) {
    return wr->len_bytes;
}

/* bits of a byte that are written already, when bit_idx is the next one to write */
static unsigned written_mask(unsigned bit_idx) {
    return (0xFF00u >> (7 - bit_idx)) & 0xFFu;
}

static void wr_and(sw_writer *wr, unsigned mask) {
    octet_put(wr->buf, wr->byte_idx, octet_get(wr->buf, wr->byte_idx) & mask);
}

static void wr_align_byte(sw_writer *wr) {
    if (wr->bit_idx == 7) {
        return;
    }
    wr_and(wr, written_mask(wr->bit_idx));
    wr->bit_idx = 7;
    wr->byte_idx++;
}

static void wr_align_nibble(sw_writer *wr) {
    if (wr->bit_idx < 3) {
        wr_align_byte(wr);
    } else if ((wr->bit_idx & 3u) != 3) {
        /* in the middle of a byte, so the byte is there */
        wr_and(wr, written_mask(wr->bit_idx));
        wr->bit_idx = 3;
    }
}

void sw_writer_align_byte(sw_writer *wr) {
    if (wr->err == SW_OK) {
        wr_align_byte(wr);
    }
}

void sw_writer_align_nibble(sw_writer *wr) {
    if (wr->err == SW_OK) {
        wr_align_nibble(wr);
    }
}

/* The wr_* operations below report an error without keeping it and may leave the cursor half moved, the public
 * functions run them on a copy. A failed write may have changed bytes past the cursor, nothing before it. */

static sw_error wr_bool(sw_writer *wr, bool val) {
    unsigned byte;
    if (sw_writer_bytes_left(wr) == 0 && wr->bit_idx == 7) {
        return SW_ERR_OUT_OF_BOUNDS_WRITE_BOOL;
    }
    byte = octet_get(wr->buf, wr->byte_idx) & ~(1u << wr->bit_idx);
    octet_put(wr->buf, wr->byte_idx, byte | ((val ? 1u : 0u) << wr->bit_idx));
    if (wr->bit_idx == 0) {
        wr->bit_idx = 7;
        wr->byte_idx++;
    } else {
        wr->bit_idx--;
    }
    return SW_OK;
}

static sw_error wr_nib(sw_writer *wr, unsigned val) {
    unsigned byte;
    val &= 0xFu;
    /* align to nibble and write in one: the bits left over in a half written nibble are cleared with it */
    if (wr->bit_idx < 3) {
        wr_align_byte(wr);
    }
    if (wr->byte_idx >= wr->len_bytes) {
        return SW_ERR_OUT_OF_BOUNDS_WRITE_U4;
    }
    byte = octet_get(wr->buf, wr->byte_idx);
    if (wr->bit_idx == 7) {
        /* the low nibble is kept: it may hold data when writing after sw_writer_restore() */
        octet_put(wr->buf, wr->byte_idx, (byte & 0x0Fu) | (val << 4));
        wr->bit_idx = 3;
    } else {
        octet_put(wr->buf, wr->byte_idx, (byte & written_mask(wr->bit_idx)) | val);
        wr->bit_idx = 7;
        wr->byte_idx++;
    }
    return SW_OK;
}

static sw_error wr_un(sw_writer *wr, unsigned bit_count, unsigned max_bits, uint64_t val) {
    unsigned bits_left = bit_count;
    if (bit_count > max_bits) {
        return SW_ERR_INVALID_BIT_COUNT;
    }
    while (bits_left > 0) {
        unsigned n, mask, shift, bits, byte;
        if (sw_writer_bytes_left(wr) == 0 && wr->bit_idx == 7) {
            return SW_ERR_OUT_OF_BOUNDS_WRITE_UN;
        }
        n = bits_left < wr->bit_idx + 1 ? bits_left : wr->bit_idx + 1;
        mask = (1u << n) - 1u;
        bits = (unsigned)((val >> (bits_left - n)) & mask);
        shift = wr->bit_idx + 1 - n;
        /* same as Rust's `!(mask as u8) << shift`: clears the bits written now and the ones below them */
        byte = octet_get(wr->buf, wr->byte_idx) & ((~mask & 0xFFu) << shift);
        octet_put(wr->buf, wr->byte_idx, byte | (bits << shift));
        if (n == wr->bit_idx + 1) {
            wr->bit_idx = 7;
            wr->byte_idx++;
        } else {
            wr->bit_idx -= n;
        }
        bits_left -= n;
    }
    return SW_OK;
}

/* align to byte and write the `len` (1 to 8) low bytes of `val` Little Endian */
static sw_error wr_le(sw_writer *wr, uint64_t val, unsigned len) {
    unsigned i;
    wr_align_byte(wr);
    if (sw_writer_bytes_left(wr) < len) {
        return len == 1 ? SW_ERR_OUT_OF_BOUNDS_WRITE_U8 : SW_ERR_OUT_OF_BOUNDS_WRITE_RAW_SLICE;
    }
    for (i = 0; i < len; i++) {
        octet_put(wr->buf, wr->byte_idx + i, (unsigned)(val & 0xFFu));
        val >>= 8;
    }
    wr->byte_idx += len;
    return SW_OK;
}

static sw_error wr_raw(sw_writer *wr, const sw_octet *val, size_t len) {
    size_t i;
    wr_align_byte(wr);
    if (sw_writer_bytes_left(wr) < len) {
        return SW_ERR_OUT_OF_BOUNDS_WRITE_RAW_SLICE;
    }
    for (i = 0; i < len; i++) {
        octet_put(wr->buf, wr->byte_idx + i, octet_get(val, i));
    }
    wr->byte_idx += len;
    return SW_OK;
}

static unsigned unib32_len_nibbles(uint32_t val) {
    unsigned n = 1;
    val >>= 3;
    while (val != 0) {
        val >>= 3;
        n++;
    }
    return n;
}

static sw_error wr_unib32(sw_writer *wr, uint32_t val) {
    unsigned left = unib32_len_nibbles(val);
    while (left > 0) {
        unsigned nib = (unsigned)(val & 7u) | (left > 1 ? UNIB_MORE : 0u);
        sw_error e = wr_nib(wr, nib);
        if (e != SW_OK) {
            return e;
        }
        val >>= 3;
        left--;
    }
    return SW_OK;
}

/* reversed UNib32 is written left to right, but read from right to left: the nibble written first is the last one
 * read, so it is the one without the "one more nibble" flag */
static sw_error wr_unib32_reversed(sw_writer *wr, uint32_t val) {
    unsigned more = 0;
    for (;;) {
        if (wr_nib(wr, (unsigned)(val & 7u) | more) != SW_OK) {
            return SW_ERR_OUT_OF_BOUNDS_REV_COMPACT;
        }
        val >>= 3;
        if (val == 0) {
            return SW_OK;
        }
        more = UNIB_MORE;
    }
}

static sw_error wr_uvlq(sw_writer *wr, uint64_t val) {
    sw_octet bytes[VLQ64_MAX_LEN];
    unsigned len = 1;
    unsigned i;
    uint64_t v = val >> 7;
    while (v != 0) {
        v >>= 7;
        len++;
    }
    /* most significant group first */
    for (i = 0; i < len; i++) {
        unsigned group = (unsigned)((val >> (7 * (len - 1 - i))) & 0x7Fu);
        octet_put(bytes, i, group | (i + 1 < len ? VLQ_MORE : 0u));
    }
    return wr_raw(wr, bytes, len);
}

static sw_error wr_rev_len(sw_writer *wr, size_t len, sw_rev_pos *pos) {
    if (len > 0xFFFFu) {
        return SW_ERR_LEN_TOO_LONG;
    }
    if (sw_writer_bytes_left(wr) < 2) {
        return SW_ERR_OUT_OF_BOUNDS_REV;
    }
    octet_put(wr->buf, wr->len_bytes - 2, (unsigned)(len & 0xFFu));
    octet_put(wr->buf, wr->len_bytes - 1, (unsigned)(len >> 8));
    wr->len_bytes -= 2;
    if (pos != NULL) {
        *pos = wr->len_bytes;
    }
    return SW_OK;
}

static sw_error wr_update_rev_len(sw_writer *wr, sw_rev_pos pos, size_t len) {
    if (pos + 1 >= wr->cap) {
        return SW_ERR_OUT_OF_BOUNDS_REV;
    }
    if (len > 0xFFFFu) {
        return SW_ERR_LEN_TOO_LONG;
    }
    octet_put(wr->buf, pos, (unsigned)(len & 0xFFu));
    octet_put(wr->buf, pos + 1, (unsigned)(len >> 8));
    return SW_OK;
}

static uint32_t slot_get(const sw_writer *wr, size_t idx) {
    return (uint32_t)octet_get(wr->buf, idx) | ((uint32_t)octet_get(wr->buf, idx + 1) << 8);
}

static sw_error wr_encode_len_fifo(sw_writer *wr, sw_rev_pos from, sw_rev_pos to) {
    unsigned odd = 0;
    size_t idx;
    if (from > to || to > wr->cap || to - from < 2) {
        return SW_OK;
    }
    /* one u16 per length, the one pushed last first */
    for (idx = from; idx + 1 < to; idx += 2) {
        odd ^= unib32_len_nibbles(slot_get(wr, idx));
    }
    /* wr_nib() aligns to a nibble first: from bits 6 to 3 that is the middle of a byte */
    if (wr->bit_idx >= 3 && wr->bit_idx < 7) {
        odd ^= 1;
    }
    if ((odd & 1u) != 0) {
        /* ensure that reading from the back always starts from a valid UNib32 */
        if (wr_nib(wr, 0) != SW_OK) {
            return SW_ERR_OUT_OF_BOUNDS_REV_COMPACT;
        }
    }
    idx = from;
    while (idx + 1 < to) {
        uint32_t val = slot_get(wr, idx);
        sw_error e;
        idx += 2;
        /* the slot is free from here on, the nibbles may take its place */
        wr->len_bytes = idx;
        e = wr_unib32_reversed(wr, val);
        if (e != SW_OK) {
            return e;
        }
    }
    return SW_OK;
}

typedef enum {
    WR_BOOL,
    WR_NIB,
    WR_UN,
    WR_LE,
    WR_UNIB32,
    WR_UVLQ,
    WR_RAW,
    WR_REV_LEN,
    WR_UPDATE_REV_LEN,
    WR_ENCODE_LEN_FIFO
} wr_kind;

typedef struct {
    uint64_t val;
    unsigned bits;
    unsigned max_bits;
    const sw_octet *ptr;
    size_t len;
    sw_rev_pos pos;
    sw_rev_pos *pos_out;
} wr_args;

/* Run one write on a copy of the cursor, commit it on success, keep the error otherwise. */
static sw_error wr_run(sw_writer *wr, wr_kind kind, const wr_args *a) {
    sw_writer tmp;
    sw_error e = SW_OK;
    if (wr->err != SW_OK) {
        return wr->err;
    }
    tmp = *wr;
    switch (kind) {
    case WR_BOOL:
        e = wr_bool(&tmp, a->val != 0);
        break;
    case WR_NIB:
        e = wr_nib(&tmp, (unsigned)(a->val & 0xFu));
        break;
    case WR_UN:
        e = wr_un(&tmp, a->bits, a->max_bits, a->val);
        break;
    case WR_LE:
        e = wr_le(&tmp, a->val, a->bits);
        break;
    case WR_UNIB32:
        e = wr_unib32(&tmp, (uint32_t)a->val);
        break;
    case WR_UVLQ:
        e = wr_uvlq(&tmp, a->val);
        break;
    case WR_RAW:
        e = wr_raw(&tmp, a->ptr, a->len);
        break;
    case WR_REV_LEN:
        e = wr_rev_len(&tmp, a->len, a->pos_out);
        break;
    case WR_UPDATE_REV_LEN:
        e = wr_update_rev_len(&tmp, a->pos, a->len);
        break;
    case WR_ENCODE_LEN_FIFO:
        e = wr_encode_len_fifo(&tmp, a->pos, (sw_rev_pos)a->val);
        break;
    }
    if (e != SW_OK) {
        wr->err = e;
        return e;
    }
    *wr = tmp;
    return SW_OK;
}

static sw_error wr_value(sw_writer *wr, wr_kind kind, uint64_t val, unsigned bits, unsigned max_bits) {
    wr_args a;
    memset(&a, 0, sizeof a);
    a.val = val;
    a.bits = bits;
    a.max_bits = max_bits;
    return wr_run(wr, kind, &a);
}

sw_error sw_write_bool(sw_writer *wr, bool val) {
    return wr_value(wr, WR_BOOL, val ? 1u : 0u, 0, 0);
}

sw_error sw_write_nib(sw_writer *wr, unsigned val) {
    return wr_value(wr, WR_NIB, val, 0, 0);
}

sw_error sw_write_un8(sw_writer *wr, unsigned bit_count, uint_least8_t val) {
    return wr_value(wr, WR_UN, val, bit_count, 8);
}

sw_error sw_write_un16(sw_writer *wr, unsigned bit_count, uint_least16_t val) {
    return wr_value(wr, WR_UN, val, bit_count, 16);
}

sw_error sw_write_un32(sw_writer *wr, unsigned bit_count, uint32_t val) {
    return wr_value(wr, WR_UN, val, bit_count, 32);
}

sw_error sw_write_un64(sw_writer *wr, unsigned bit_count, uint64_t val) {
    return wr_value(wr, WR_UN, val, bit_count, 64);
}

sw_error sw_write_u8(sw_writer *wr, uint_least8_t val) {
    return wr_value(wr, WR_LE, val, 1, 0);
}

sw_error sw_write_u16(sw_writer *wr, uint_least16_t val) {
    return wr_value(wr, WR_LE, val, 2, 0);
}

sw_error sw_write_u32(sw_writer *wr, uint32_t val) {
    return wr_value(wr, WR_LE, val, 4, 0);
}

sw_error sw_write_u64(sw_writer *wr, uint64_t val) {
    return wr_value(wr, WR_LE, val, 8, 0);
}

/* conversion of a negative number to unsigned is defined (modulo 2^N), only the bytes written are kept */
sw_error sw_write_i8(sw_writer *wr, int_least8_t val) {
    return wr_value(wr, WR_LE, (uint64_t)(int64_t)val, 1, 0);
}

sw_error sw_write_i16(sw_writer *wr, int_least16_t val) {
    return wr_value(wr, WR_LE, (uint64_t)(int64_t)val, 2, 0);
}

sw_error sw_write_i32(sw_writer *wr, int32_t val) {
    return wr_value(wr, WR_LE, (uint64_t)(int64_t)val, 4, 0);
}

sw_error sw_write_i64(sw_writer *wr, int64_t val) {
    return wr_value(wr, WR_LE, (uint64_t)val, 8, 0);
}

sw_error sw_write_f32(sw_writer *wr, float val) {
    uint32_t bits;
    memcpy(&bits, &val, sizeof bits);
    return wr_value(wr, WR_LE, bits, 4, 0);
}

#ifndef SW_NO_F64
sw_error sw_write_f64(sw_writer *wr, double val) {
    uint64_t bits;
    memcpy(&bits, &val, sizeof bits);
    return wr_value(wr, WR_LE, bits, 8, 0);
}
#endif

sw_error sw_write_unib32(sw_writer *wr, uint32_t val) {
    return wr_value(wr, WR_UNIB32, val, 0, 0);
}

sw_error sw_write_uvlq32(sw_writer *wr, uint32_t val) {
    return wr_value(wr, WR_UVLQ, val, 0, 0);
}

sw_error sw_write_uvlq64(sw_writer *wr, uint64_t val) {
    return wr_value(wr, WR_UVLQ, val, 0, 0);
}

sw_error sw_write_raw(sw_writer *wr, const sw_octet *val, size_t len) {
    wr_args a;
    memset(&a, 0, sizeof a);
    a.ptr = val;
    a.len = len;
    return wr_run(wr, WR_RAW, &a);
}

sw_error sw_write_rev_len(sw_writer *wr, size_t len, sw_rev_pos *pos) {
    wr_args a;
    memset(&a, 0, sizeof a);
    a.len = len;
    a.pos_out = pos;
    return wr_run(wr, WR_REV_LEN, &a);
}

sw_error sw_write_bytes(sw_writer *wr, const sw_octet *val, size_t len) {
    sw_writer start = *wr;
    if (sw_write_rev_len(wr, len, NULL) == SW_OK && sw_write_raw(wr, val, len) != SW_OK) {
        /* failing calls move nothing, also the length pushed before the bytes */
        start.err = wr->err;
        *wr = start;
    }
    return wr->err;
}

sw_error sw_write_str(sw_writer *wr, const char *val, size_t len) {
    sw_writer start = *wr;
    size_t i;
    if (sw_write_rev_len(wr, len, NULL) != SW_OK) {
        return wr->err;
    }
    sw_writer_align_byte(wr);
    if (sw_writer_bytes_left(wr) < len) {
        start.err = SW_ERR_OUT_OF_BOUNDS_WRITE_RAW_SLICE;
        *wr = start;
        return wr->err;
    }
    for (i = 0; i < len; i++) {
        octet_put(wr->buf, wr->byte_idx + i, (unsigned)(unsigned char)val[i]);
    }
    wr->byte_idx += len;
    return SW_OK;
}

sw_error sw_update_rev_len(sw_writer *wr, sw_rev_pos pos, size_t len) {
    wr_args a;
    memset(&a, 0, sizeof a);
    a.pos = pos;
    a.len = len;
    return wr_run(wr, WR_UPDATE_REV_LEN, &a);
}

sw_error sw_encode_len_fifo(sw_writer *wr, sw_rev_pos from, sw_rev_pos to) {
    wr_args a;
    memset(&a, 0, sizeof a);
    a.pos = from;
    a.val = to;
    return wr_run(wr, WR_ENCODE_LEN_FIFO, &a);
}

sw_error sw_unsized_begin(sw_writer *wr, sw_unsized *u) {
    sw_writer start = *wr;
    /* the start below is on a byte boundary */
    sw_writer_align_byte(wr);
    if (sw_write_rev_len(wr, 0, &u->size_slot) != SW_OK) {
        start.err = wr->err;
        *wr = start;
        return wr->err;
    }
    u->start = wr->byte_idx;
    return SW_OK;
}

sw_error sw_unsized_end(sw_writer *wr, const sw_unsized *u) {
    sw_writer start = *wr;
    /* the value may have pushed lengths of its own, they go after its data */
    if (sw_encode_len_fifo(wr, sw_rev_len_pos(wr), u->size_slot) == SW_OK) {
        /* e.g. an enum that only wrote a nibble */
        sw_writer_align_byte(wr);
        /* the size is encoded later, by the enclosing value or sw_writer_finish() */
        if (sw_update_rev_len(wr, u->size_slot, wr->byte_idx - u->start) == SW_OK) {
            return SW_OK;
        }
    }
    start.err = wr->err;
    *wr = start;
    return wr->err;
}

sw_error sw_writer_finish(sw_writer *wr, size_t *len) {
    *len = 0;
    if (wr->err != SW_OK) {
        return wr->err;
    }
    if (wr->cap - wr->len_bytes >= 2) {
        if (sw_encode_len_fifo(wr, wr->len_bytes, wr->cap) != SW_OK) {
            return wr->err;
        }
    } else {
        wr_align_byte(wr);
    }
    *len = wr->byte_idx;
    sw_writer_reset(wr);
    return SW_OK;
}
