/*
 * Requantize in C99 (CLT-54): a stored number converted to another scale and storage with a named rounding and
 * overflow policy, the same number as Rust's exact_units::Requantizer and the typed Fixed::requantize, bit for bit
 * (STD-63, see docs/std_library/rounding.md). No malloc, nothing from libc beyond <stdint.h>, <stdbool.h> and
 * <string.h> (memcpy); next to the shrink_wrap runtime, but it doesn't need it.
 *
 * The factor (the quotient of the source and the target scale, a Magnitude) is worked out on the host, as typed
 * Rust does at compile time, and given as constants: an exact ratio num/den (num up to 128 bits, den up to
 * 2^127 - 1) or, for a factor that isn't a fraction of integers (pi, a half exponent), the fixed point m * 2^shift
 * of Magnitude::fixed_point(). Integers are converted exactly in 128 bits, whatever the storages.
 */
#ifndef SW_REQUANTIZE_H
#define SW_REQUANTIZE_H

#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/** How a result that isn't a whole number of the target's steps is rounded, exact_units::Rounding. */
typedef enum {
    /** Toward zero: 7/2 is 3, -7/2 is -3. */
    SW_ROUND_TRUNCATE = 0,
    /** Toward -infinity. */
    SW_ROUND_FLOOR = 1,
    /** Toward +infinity. */
    SW_ROUND_CEIL = 2,
    /** To the nearest, a tie to the even neighbour: 2.5 is 2, 3.5 is 4. */
    SW_ROUND_NEAREST_EVEN = 3,
    /** To the nearest, a tie away from zero: 2.5 is 3, -2.5 is -3. */
    SW_ROUND_NEAREST_AWAY = 4
} sw_rounding;

/** What happens to a result the target storage can't hold, exact_units::Overflow. */
typedef enum {
    /** No value: sw_requantize() returns false. */
    SW_OVERFLOW_CHECKED = 0,
    /** The nearest end of the storage. */
    SW_OVERFLOW_SATURATE = 1,
    /** Modulo 2^bits, two's complement for a signed storage. */
    SW_OVERFLOW_WRAP = 2
} sw_overflow;

/** The kind of a number: an integer (signed or not) of up to 64 bits, or a float. */
typedef enum {
    SW_NUM_INT = 0,
    SW_NUM_UINT = 1,
#ifndef SW_NO_F64
    SW_NUM_F32 = 2,
    SW_NUM_F64 = 3
#endif
} sw_num_kind;

/** A target storage: `{SW_NUM_INT, 16}` is i16, `{SW_NUM_UINT, 4}` a nibble; `bits` (1 to 64) only for integers. */
typedef struct {
    sw_num_kind kind;
    unsigned bits;
} sw_storage;

/** A number: `i` for SW_NUM_INT, `u` for SW_NUM_UINT, `f` for the floats (an f32 widened to double). */
typedef struct {
    sw_num_kind kind;
    union {
        int64_t i;
        uint64_t u;
#ifndef SW_NO_F64
        double f;
#endif
    } v;
} sw_num;

/**
 * The factor of a conversion, from Magnitude on the host (Requantizer::new): with `ratio`, num / den
 * (Magnitude::to_ratio(), num as two 64-bit halves, den as well and at most 2^127 - 1: a bigger den rounds the
 * same); without, m * 2^shift (Magnitude::fixed_point()). `f_num / f_den` is what a float is multiplied with
 * (Magnitude::apply_f64()): num and den as doubles when both are at most 2^53, else Magnitude::to_f64() and 1.
 */
typedef struct {
    bool ratio;
    uint64_t num_hi, num_lo;
    uint64_t den_hi, den_lo;
    uint64_t m;
    int32_t shift;
#ifndef SW_NO_F64
    double f_num, f_den;
#endif
} sw_factor;

/** A conversion: the factor, the target storage and the policies. */
typedef struct {
    sw_factor factor;
    sw_storage to;
    sw_rounding rounding;
    sw_overflow overflow;
} sw_requantizer;

/**
 * `x * factor` in the storage `q->to`, rounded and settled as `q` says, into `out` (of kind `q->to.kind`). False
 * for no value: a checked overflow, or a ratio whose `x * num` doesn't fit 128 bits (where the typed conversion
 * doesn't compile). A float `x` into an integer storage is rounded by the same policy; a float result is
 * `x * f_num / f_den`, rounded to f32 for an f32 storage.
 */
bool sw_requantize(const sw_requantizer *q, sw_num x, sw_num *out);

#ifdef __cplusplus
}
#endif

#endif /* SW_REQUANTIZE_H */
