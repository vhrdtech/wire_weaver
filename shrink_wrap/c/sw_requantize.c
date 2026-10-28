/*
 * Requantize in C99 (CLT-54), see sw_requantize.h: a port of exact_units::Requantizer::apply
 * (src/fixed/rounding.rs in https://github.com/romixlab/exact_units), the same number bit for bit. Integers go
 * through sign and magnitude in 128 bits (two 64-bit halves, products by 32-bit halves), so nothing depends on
 * __int128 or implementation-defined casts of negative numbers.
 */
#include "sw_requantize.h"

#include <string.h>

typedef struct {
    uint64_t hi, lo;
} sw_u128;

static sw_u128 u128_of(uint64_t hi, uint64_t lo) {
    sw_u128 r;
    r.hi = hi;
    r.lo = lo;
    return r;
}

static bool u128_is_zero(sw_u128 a) { return (a.hi | a.lo) == 0; }

static int u128_cmp(sw_u128 a, sw_u128 b) {
    if (a.hi != b.hi) {
        return a.hi < b.hi ? -1 : 1;
    }
    if (a.lo != b.lo) {
        return a.lo < b.lo ? -1 : 1;
    }
    return 0;
}

static sw_u128 u128_sub(sw_u128 a, sw_u128 b) {
    sw_u128 r;
    r.lo = a.lo - b.lo;
    r.hi = a.hi - b.hi - (a.lo < b.lo ? 1u : 0u);
    return r;
}

static sw_u128 u128_add_small(sw_u128 a, uint64_t b) {
    sw_u128 r;
    r.lo = a.lo + b;
    r.hi = a.hi + (r.lo < a.lo ? 1u : 0u);
    return r;
}

/* a << s for s < 128 */
static sw_u128 u128_shl(sw_u128 a, unsigned s) {
    if (s == 0) {
        return a;
    }
    if (s >= 64) {
        return u128_of(a.lo << (s - 64), 0);
    }
    return u128_of((a.hi << s) | (a.lo >> (64 - s)), a.lo << s);
}

/* a >> s for s < 128 */
static sw_u128 u128_shr(sw_u128 a, unsigned s) {
    if (s == 0) {
        return a;
    }
    if (s >= 64) {
        return u128_of(0, a.hi >> (s - 64));
    }
    return u128_of(a.hi >> s, (a.lo >> s) | (a.hi << (64 - s)));
}

/* the low s bits of a, for s < 128 */
static sw_u128 u128_low_bits(sw_u128 a, unsigned s) {
    if (s >= 64) {
        return u128_of(s == 64 ? 0 : a.hi & ((UINT64_C(1) << (s - 64)) - 1), a.lo);
    }
    return u128_of(0, s == 0 ? 0 : a.lo & ((UINT64_C(1) << s) - 1));
}

static unsigned clz64(uint64_t x) {
    unsigned n = 0;
    if (x == 0) {
        return 64;
    }
    while ((x >> 63) == 0) {
        x <<= 1;
        n++;
    }
    return n;
}

static unsigned u128_clz(sw_u128 a) { return a.hi != 0 ? clz64(a.hi) : 64 + clz64(a.lo); }

/* the full product a * b */
static sw_u128 mul64(uint64_t a, uint64_t b) {
    uint64_t a0 = a & 0xffffffffu, a1 = a >> 32;
    uint64_t b0 = b & 0xffffffffu, b1 = b >> 32;
    uint64_t p00 = a0 * b0, p01 = a0 * b1, p10 = a1 * b0, p11 = a1 * b1;
    /* below 3 * 2^32, no carry out */
    uint64_t mid = (p00 >> 32) + (p01 & 0xffffffffu) + (p10 & 0xffffffffu);
    sw_u128 r;
    r.lo = (mid << 32) | (p00 & 0xffffffffu);
    r.hi = p11 + (p01 >> 32) + (p10 >> 32) + (mid >> 32);
    return r;
}

/* a * n in 192 bits: w2 is the top 64 */
static sw_u128 mul64_128(uint64_t a, sw_u128 n, uint64_t *w2) {
    sw_u128 p0 = mul64(a, n.lo), p1 = mul64(a, n.hi);
    sw_u128 r;
    r.lo = p0.lo;
    r.hi = p0.hi + p1.lo;
    *w2 = p1.hi + (r.hi < p0.hi ? 1u : 0u);
    return r;
}

/* n / d and n % d, for d != 0 and d < 2^127 */
static sw_u128 u128_divmod(sw_u128 n, sw_u128 d, sw_u128 *rem) {
    sw_u128 q = u128_of(0, 0), r = u128_of(0, 0);
    int i;
    if (n.hi == 0 && d.hi == 0) {
        *rem = u128_of(0, n.lo % d.lo);
        return u128_of(0, n.lo / d.lo);
    }
    for (i = 127; i >= 0; i--) {
        uint64_t bit = i >= 64 ? (n.hi >> (i - 64)) & 1u : (n.lo >> i) & 1u;
        r = u128_shl(r, 1);
        r.lo |= bit;
        if (u128_cmp(r, d) >= 0) {
            r = u128_sub(r, d);
            if (i >= 64) {
                q.hi |= UINT64_C(1) << (i - 64);
            } else {
                q.lo |= UINT64_C(1) << i;
            }
        }
    }
    *rem = r;
    return q;
}

static const sw_u128 TWO_127 = {UINT64_C(1) << 63, 0};

/* A signed result as sign and magnitude: `fits` when the magnitude is below 2^128, `low` the magnitude modulo
 * 2^128 (Scaled of rounding.rs). */
typedef struct {
    bool neg;
    sw_u128 low;
    bool fits;
} sw_scaled;

/* the value is an i128 */
static bool scaled_exact(sw_scaled s) {
    if (!s.fits) {
        return false;
    }
    return s.neg ? u128_cmp(s.low, TWO_127) <= 0 : (s.low.hi >> 63) == 0;
}

/* the value modulo 2^64 */
static uint64_t scaled_wrapped64(sw_scaled s) { return s.neg ? 0u - s.low.lo : s.low.lo; }

/* the magnitude q of a quotient rounded up to q + 1 by `r`: `neg` its sign, `rem` whether there is a remainder,
 * `above` and `tie` the remainder against half a step */
static bool round_up(sw_rounding r, bool neg, bool rem, bool above, bool tie, bool q_odd) {
    switch (r) {
    case SW_ROUND_TRUNCATE:
        return false;
    case SW_ROUND_FLOOR:
        return neg && rem;
    case SW_ROUND_CEIL:
        return !neg && rem;
    case SW_ROUND_NEAREST_AWAY:
        return above || tie;
    case SW_ROUND_NEAREST_EVEN:
    default:
        return above || (tie && q_odd);
    }
}

/* |x| * m * 2^shift rounded by r (mul_fixed_point of rounding.rs) */
static sw_scaled mul_fixed_point(bool neg, uint64_t mag, uint64_t m, int32_t shift, sw_rounding r) {
    sw_scaled s;
    sw_u128 p = mul64(mag, m);
    s.neg = neg;
    if (shift >= 0) {
        unsigned sh = (unsigned)shift;
        s.low = sh < 128 ? u128_shl(p, sh) : u128_of(0, 0);
        s.fits = u128_is_zero(p) || u128_clz(p) >= sh;
        return s;
    }
    {
        unsigned k = (unsigned)(-(int64_t)shift);
        sw_u128 q = k >= 128 ? u128_of(0, 0) : u128_shr(p, k);
        sw_u128 rem = k >= 128 ? p : u128_low_bits(p, k);
        bool above = false, tie = false;
        if (k <= 128) {
            sw_u128 half = u128_shl(u128_of(0, 1), k - 1);
            int c = u128_cmp(rem, half);
            above = c > 0;
            tie = c == 0;
        }
        if (round_up(r, neg, !u128_is_zero(rem), above, tie, (q.lo & 1u) != 0)) {
            q = u128_add_small(q, 1);
        }
        s.low = q;
        s.fits = true;
        return s;
    }
}

/* |x| * num / den rounded by r; false where x * num doesn't fit an i128 */
static bool mul_ratio(bool neg, uint64_t mag, const sw_factor *f, sw_rounding r, sw_scaled *out) {
    sw_u128 num = u128_of(f->num_hi, f->num_lo), den = u128_of(f->den_hi, f->den_lo);
    uint64_t w2;
    sw_u128 p = mul64_128(mag, num, &w2);
    bool fits = (num.hi >> 63) == 0 && w2 == 0 && (neg ? u128_cmp(p, TWO_127) <= 0 : (p.hi >> 63) == 0);
    out->neg = neg;
    if (fits) {
        sw_u128 rem, q = u128_divmod(p, den, &rem);
        /* rem against den - rem, the other side of half a step */
        sw_u128 other = u128_sub(den, rem);
        int c = u128_cmp(rem, other);
        if (round_up(r, neg, !u128_is_zero(rem), c > 0, c == 0, (q.lo & 1u) != 0)) {
            q = u128_add_small(q, 1);
        }
        out->low = q;
        out->fits = true;
        return true;
    }
    if (den.hi == 0 && den.lo == 1) {
        /* a whole factor: the result is beyond 128 bits, and modulo 2^128 still right */
        out->low = p;
        out->fits = false;
        return true;
    }
    /* the product doesn't fit 128 bits, where the typed conversion doesn't compile */
    return false;
}

/* the bits low bits of v as a number of the storage `to` */
static void put_wrapped(sw_storage to, uint64_t v, sw_num *out) {
    uint64_t mask = to.bits >= 64 ? ~UINT64_C(0) : (UINT64_C(1) << to.bits) - 1;
    v &= mask;
    out->kind = to.kind;
    if (to.kind == SW_NUM_INT) {
        if ((v >> (to.bits - 1)) & 1u) {
            /* -(2^bits - v) without casting a big unsigned to signed */
            out->v.i = -(int64_t)(~v & mask) - 1;
        } else {
            out->v.i = (int64_t)v;
        }
    } else {
        out->v.u = v;
    }
}

static void put_min(sw_storage to, sw_num *out) {
    out->kind = to.kind;
    if (to.kind == SW_NUM_INT) {
        out->v.i = -(int64_t)((UINT64_C(1) << (to.bits - 1)) - 1) - 1;
    } else {
        out->v.u = 0;
    }
}

static void put_max(sw_storage to, sw_num *out) {
    out->kind = to.kind;
    if (to.kind == SW_NUM_INT) {
        out->v.i = (int64_t)((UINT64_C(1) << (to.bits - 1)) - 1);
    } else {
        out->v.u = to.bits >= 64 ? ~UINT64_C(0) : (UINT64_C(1) << to.bits) - 1;
    }
}

/* s into the integer storage `to` by o (settle_int of rounding.rs) */
static bool settle_int(sw_storage to, sw_scaled s, sw_overflow o, sw_num *out) {
    bool signed_ = to.kind == SW_NUM_INT;
    if (o == SW_OVERFLOW_WRAP) {
        put_wrapped(to, scaled_wrapped64(s), out);
        return true;
    }
    if (!scaled_exact(s)) {
        if (o == SW_OVERFLOW_CHECKED) {
            return false;
        }
        if (s.neg) {
            put_min(to, out);
        } else {
            put_max(to, out);
        }
        return true;
    }
    if (s.neg && !u128_is_zero(s.low)) {
        /* at least -2^(bits - 1) for signed, nothing below 0 for unsigned */
        uint64_t lim = UINT64_C(1) << (to.bits - 1);
        if (!signed_ || s.low.hi != 0 || s.low.lo > lim) {
            if (o == SW_OVERFLOW_CHECKED) {
                return false;
            }
            put_min(to, out);
            return true;
        }
        out->kind = to.kind;
        out->v.i = -(int64_t)(s.low.lo - 1) - 1;
        return true;
    }
    {
        uint64_t max = signed_ ? (UINT64_C(1) << (to.bits - 1)) - 1
                               : (to.bits >= 64 ? ~UINT64_C(0) : (UINT64_C(1) << to.bits) - 1);
        if (s.low.hi != 0 || s.low.lo > max) {
            if (o == SW_OVERFLOW_CHECKED) {
                return false;
            }
            put_max(to, out);
            return true;
        }
        out->kind = to.kind;
        if (signed_) {
            out->v.i = (int64_t)s.low.lo;
        } else {
            out->v.u = s.low.lo;
        }
        return true;
    }
}

#ifndef SW_NO_F64
/* x rounded to a whole number by r; NaN and infinities as they are (round_f64 of rounding.rs) */
static double round_f64(double x, sw_rounding r) {
    const double whole = 4503599627370496.0; /* 2^52, from where on every double is whole */
    double t, frac, away, a;
    bool neg;
    if (!(x < whole && x > -whole)) {
        return x;
    }
    t = (double)(int64_t)x;
    frac = x - t;
    if (frac == 0.0) {
        return t;
    }
    neg = x < 0.0;
    away = neg ? t - 1.0 : t + 1.0;
    switch (r) {
    case SW_ROUND_TRUNCATE:
        return t;
    case SW_ROUND_FLOOR:
        return neg ? away : t;
    case SW_ROUND_CEIL:
        return neg ? t : away;
    case SW_ROUND_NEAREST_EVEN:
    case SW_ROUND_NEAREST_AWAY:
    default:
        a = neg ? -frac : frac;
        if (a > 0.5) {
            return away;
        }
        if (a < 0.5) {
            return t;
        }
        return r == SW_ROUND_NEAREST_AWAY || (int64_t)t % 2 != 0 ? away : t;
    }
}

/* the low 64 bits of a whole double, NaN and infinities as 0 (wrap_f64 of rounding.rs: from 2^127 on a whole
 * double is a multiple of 2^75) */
static uint64_t wrap_f64(double y) {
    uint64_t bits, mant, mag;
    int exp, sh;
    memcpy(&bits, &y, sizeof bits);
    exp = (int)((bits >> 52) & 0x7ffu);
    if (exp == 0x7ff || exp == 0) {
        /* NaN, infinities, zeros (a whole double isn't subnormal) */
        return 0;
    }
    mant = (bits & ((UINT64_C(1) << 52) - 1)) | (UINT64_C(1) << 52);
    sh = exp - 1075;
    if (sh >= 64 || sh <= -64) {
        mag = 0;
    } else if (sh >= 0) {
        mag = mant << sh;
    } else {
        mag = mant >> -sh;
    }
    return (bits >> 63) != 0 ? 0u - mag : mag;
}

/* a whole double y into the integer storage `to` by o */
static bool settle_f64(sw_storage to, double y, sw_overflow o, sw_num *out) {
    bool signed_ = to.kind == SW_NUM_INT;
    /* the range as doubles: [min, end), exact powers of 2 */
    double end = signed_ ? (double)(UINT64_C(1) << (to.bits - 1))
                         : (to.bits >= 64 ? 18446744073709551616.0 : (double)(UINT64_C(1) << to.bits));
    double min = signed_ ? -end : 0.0;
    out->kind = to.kind;
    switch (o) {
    case SW_OVERFLOW_CHECKED:
        if (!(y >= min && y < end)) {
            return false;
        }
        if (signed_) {
            out->v.i = (int64_t)y;
        } else {
            out->v.u = (uint64_t)y;
        }
        return true;
    case SW_OVERFLOW_SATURATE: {
        /* against max as a double, which may round up (2^64 - 1 is 2^64) like Rust's `as f64` */
        sw_num max;
        double max_d;
        put_max(to, &max);
        max_d = signed_ ? (double)max.v.i : (double)max.v.u;
        if (y != y) {
            if (signed_) {
                out->v.i = 0;
            } else {
                out->v.u = 0;
            }
        } else if (y >= max_d) {
            *out = max;
        } else if (y <= min) {
            put_min(to, out);
        } else if (signed_) {
            out->v.i = (int64_t)y;
        } else {
            out->v.u = (uint64_t)y;
        }
        return true;
    }
    case SW_OVERFLOW_WRAP:
    default:
        put_wrapped(to, wrap_f64(y), out);
        return true;
    }
}

/* x * factor in doubles (Magnitude::apply_f64) */
static double apply_f64(const sw_factor *f, double x) { return x * f->f_num / f->f_den; }
#endif

bool sw_requantize(const sw_requantizer *q, sw_num x, sw_num *out) {
    bool neg;
    uint64_t mag;
    sw_scaled s;
#ifndef SW_NO_F64
    if (q->to.kind == SW_NUM_F64 || q->to.kind == SW_NUM_F32) {
        double xd = x.kind == SW_NUM_INT ? (double)x.v.i : x.kind == SW_NUM_UINT ? (double)x.v.u : x.v.f;
        double y = apply_f64(&q->factor, xd);
        out->kind = q->to.kind;
        /* an f32 rounded once, from the double; out of range is an infinity (IEEE 754, C99 Annex F) */
        out->v.f = q->to.kind == SW_NUM_F32 ? (double)(float)y : y;
        return true;
    }
    if (x.kind == SW_NUM_F64 || x.kind == SW_NUM_F32) {
        return settle_f64(q->to, round_f64(apply_f64(&q->factor, x.v.f), q->rounding), q->overflow, out);
    }
#endif
    if (x.kind == SW_NUM_INT) {
        neg = x.v.i < 0;
        /* |x| without overflow at INT64_MIN */
        mag = neg ? 0u - (uint64_t)x.v.i : (uint64_t)x.v.i;
    } else {
        neg = false;
        mag = x.v.u;
    }
    if (mag == 0) {
        s.neg = false;
        s.low = u128_of(0, 0);
        s.fits = true;
    } else if (q->factor.ratio) {
        if (!mul_ratio(neg, mag, &q->factor, q->rounding, &s)) {
            return false;
        }
    } else {
        s = mul_fixed_point(neg, mag, q->factor.m, q->factor.shift, q->rounding);
    }
    return settle_int(q->to, s, q->overflow, out);
}
