/*
 * Requantize in C99 (CLT-54), see sw_requantize.h: a port of exact_units::Requantizer::apply
 * (src/fixed/rounding.rs in https://github.com/romixlab/exact_units), the same number bit for bit. Integers go
 * through sign and magnitude in 128 bits (two 64-bit halves, products by 32-bit halves), so nothing depends on
 * __int128 or implementation-defined casts of negative numbers; a float on either side is worked exactly from its
 * bits in 256 (exact_float.rs, NUM-30), never by a float multiply.
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

/* r doubled plus bit, against d: the next remainder of a binary long division, r < d; true where d is taken off
 * (2r >= d is asked as r >= d - r, so nothing overflows for a d of 128 bits) */
static bool long_div_step(sw_u128 *r, sw_u128 d, uint64_t bit) {
    sw_u128 other = u128_sub(d, *r);
    if (u128_cmp(*r, other) >= 0) {
        *r = u128_add_small(u128_sub(*r, other), bit);
        return true;
    }
    /* 2r < d: doubling doesn't overflow */
    *r = u128_add_small(u128_shl(*r, 1), bit);
    if (u128_cmp(*r, d) == 0) {
        *r = u128_of(0, 0);
        return true;
    }
    return false;
}

/* n / d and n % d, for d != 0 */
static sw_u128 u128_divmod(sw_u128 n, sw_u128 d, sw_u128 *rem) {
    sw_u128 q = u128_of(0, 0), r = u128_of(0, 0);
    int i;
    if (n.hi == 0 && d.hi == 0) {
        *rem = u128_of(0, n.lo % d.lo);
        return u128_of(0, n.lo / d.lo);
    }
    for (i = 127; i >= 0; i--) {
        uint64_t bit = i >= 64 ? (n.hi >> (i - 64)) & 1u : (n.lo >> i) & 1u;
        if (long_div_step(&r, d, bit)) {
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
    case SW_ROUND_NEAREST_CEIL:
        return above || (tie && !neg);
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
/* A conversion with a float on either side, worked exactly (exact_float.rs of exact_units, NUM-30): x * num / den *
 * 2^shift for an integer or a finite float x as its top 128 bits and a sticky bit for the rest, rounded once into a
 * float or an integer. A float is a binary fraction, so the product is a fraction of integers like any other: no
 * float multiply in between. */

/* 256 bits, w[0] the least significant 64 */
typedef struct {
    uint64_t w[4];
} sw_u256;

/* the full product a * b (mul_wide) */
static sw_u256 mul_wide(sw_u128 a, sw_u128 b) {
    sw_u128 p00 = mul64(a.lo, b.lo), p01 = mul64(a.lo, b.hi), p10 = mul64(a.hi, b.lo), p11 = mul64(a.hi, b.hi);
    sw_u256 r;
    uint64_t t, c;
    r.w[0] = p00.lo;
    /* w1 = p00.hi + p01.lo + p10.lo, its carries (at most 2) into w2 */
    t = p00.hi + p01.lo;
    c = t < p00.hi ? 1u : 0u;
    r.w[1] = t + p10.lo;
    c += r.w[1] < t ? 1u : 0u;
    /* w2 = p01.hi + p10.hi + p11.lo + c, its carries into w3 */
    t = p01.hi + p10.hi;
    r.w[3] = p11.hi + (t < p01.hi ? 1u : 0u);
    r.w[2] = t + p11.lo;
    r.w[3] += r.w[2] < t ? 1u : 0u;
    t = r.w[2];
    r.w[2] = t + c;
    r.w[3] += r.w[2] < t ? 1u : 0u;
    return r;
}

static bool u256_is_zero(const sw_u256 *n) { return (n->w[0] | n->w[1] | n->w[2] | n->w[3]) == 0; }

/* the number of bits of n, 0 for 0 */
static unsigned u256_bits(const sw_u256 *n) {
    int i;
    for (i = 3; i >= 0; i--) {
        if (n->w[i] != 0) {
            return 64u * (unsigned)i + 64u - clz64(n->w[i]);
        }
    }
    return 0;
}

/* n * 2^s for s < 256, the result below 2^256 */
static sw_u256 u256_shl(const sw_u256 *n, unsigned s) {
    sw_u256 r;
    unsigned k = s / 64, b = s % 64;
    int i;
    for (i = 3; i >= 0; i--) {
        int j = i - (int)k;
        uint64_t v = j >= 0 ? n->w[j] << b : 0;
        if (b != 0 && j >= 1) {
            v |= n->w[j - 1] >> (64 - b);
        }
        r.w[i] = v;
    }
    return r;
}

/* n / 2^s rounded down for s < 256, and whether a bit shifted out was set */
static sw_u256 u256_shr(const sw_u256 *n, unsigned s, bool *out) {
    sw_u256 r;
    unsigned k = s / 64, b = s % 64;
    int i;
    *out = false;
    for (i = 0; i < (int)k; i++) {
        *out = *out || n->w[i] != 0;
    }
    if (b != 0 && (n->w[k] & ((UINT64_C(1) << b) - 1)) != 0) {
        *out = true;
    }
    for (i = 0; i < 4; i++) {
        unsigned j = (unsigned)i + k;
        uint64_t v = j < 4 ? n->w[j] >> b : 0;
        if (b != 0 && j + 1 < 4) {
            v |= n->w[j + 1] << (64 - b);
        }
        r.w[i] = v;
    }
    return r;
}

/* n / d rounded down and whether the remainder is not 0, d != 0: binary long division */
static sw_u256 u256_div(const sw_u256 *n, sw_u128 d, bool *rem) {
    sw_u256 q;
    sw_u128 r = u128_of(0, 0);
    unsigned i = u256_bits(n);
    memset(&q, 0, sizeof q);
    if (d.hi == 0 && d.lo == 1) {
        *rem = false;
        return *n;
    }
    while (i > 0) {
        uint64_t bit;
        i--;
        bit = (n->w[i / 64] >> (i % 64)) & 1u;
        if (long_div_step(&r, d, bit)) {
            q.w[i / 64] |= UINT64_C(1) << (i % 64);
        }
    }
    *rem = !u128_is_zero(r);
    return q;
}

/* |v| = m * 2^e + rest with 0 <= rest < 2^e, sticky whether rest != 0; m has its top bit set, or is 0 for v = 0
 * (neg then the sign of a float zero) */
typedef struct {
    bool neg;
    sw_u128 m;
    int32_t e;
    bool sticky;
} sw_quotient;

/* +-a * 2^exp * num / den * 2^shift (Quotient::of) */
static sw_quotient quotient_of(bool neg, sw_u128 a, int32_t exp, sw_u128 num, sw_u128 den, int32_t shift) {
    sw_quotient q;
    sw_u256 n = mul_wide(a, num), qn;
    bool out = false, rem;
    int32_t s;
    q.neg = neg;
    q.m = u128_of(0, 0);
    q.e = 0;
    q.sticky = false;
    if (u256_is_zero(&n) || u128_is_zero(den)) {
        return q;
    }
    /* n * 2^s / den is in [2^127, 2^129): bits(n) + s = 128 + bits(den) <= 256 */
    s = 128 + (int32_t)(128u - u128_clz(den)) - (int32_t)u256_bits(&n);
    n = s >= 0 ? u256_shl(&n, (unsigned)s) : u256_shr(&n, (unsigned)-s, &out);
    qn = u256_div(&n, den, &rem);
    q.m = u128_of(qn.w[1], qn.w[0]);
    q.e = exp + shift - s;
    q.sticky = out || rem;
    if ((qn.w[2] | qn.w[3]) != 0) {
        q.sticky = q.sticky || (q.m.lo & 1u) != 0;
        q.m = u128_of((q.m.hi >> 1) | (UINT64_C(1) << 63), (q.m.lo >> 1) | (q.m.hi << 63));
        q.e += 1;
    }
    return q;
}

/* x * factor for an integer or a finite float x (Quotient::of_repr); false for NaN and infinities */
static bool quotient_of_num(sw_num x, const sw_factor *f, sw_quotient *q) {
    sw_u128 num, den;
    int32_t shift;
    bool neg;
    sw_u128 a;
    int32_t exp = 0;
    if (f->ratio) {
        num = u128_of(f->num_hi, f->num_lo);
        den = u128_of(f->den_hi, f->den_lo);
        shift = 0;
    } else {
        num = u128_of(0, f->m);
        den = u128_of(0, 1);
        shift = f->shift;
    }
    if (x.kind == SW_NUM_INT) {
        neg = x.v.i < 0;
        a = u128_of(0, neg ? 0u - (uint64_t)x.v.i : (uint64_t)x.v.i);
    } else if (x.kind == SW_NUM_UINT) {
        neg = false;
        a = u128_of(0, x.v.u);
    } else {
        uint64_t b, frac;
        int32_t field;
        memcpy(&b, &x.v.f, sizeof b);
        field = (int32_t)((b >> 52) & 0x7ffu);
        if (field == 0x7ff) {
            return false;
        }
        frac = b & ((UINT64_C(1) << 52) - 1);
        neg = (b >> 63) != 0;
        if (field == 0) {
            a = u128_of(0, frac);
            exp = -1074;
        } else {
            a = u128_of(0, frac | (UINT64_C(1) << 52));
            exp = field - 1075;
        }
    }
    *q = quotient_of(neg, a, exp, num, den, shift);
    return true;
}

/* m cut at bit `shift` (at least 1): the bits kept, where the rest lies against half of 2^shift (-1, 0, 1; the
 * sticky bit breaking a tie upward), and whether anything is cut off (Quotient::cut) */
static sw_u128 quotient_cut(const sw_quotient *q, int32_t shift, int *ord, bool *inexact) {
    sw_u128 rest, half;
    int c;
    *inexact = true;
    if (shift > 128) {
        *ord = -1;
        return u128_of(0, 0);
    }
    if (shift == 128) {
        bool tie = q->m.hi == (UINT64_C(1) << 63) && q->m.lo == 0 && !q->sticky;
        *ord = tie ? 0 : 1;
        return u128_of(0, 0);
    }
    rest = u128_low_bits(q->m, (unsigned)shift);
    half = u128_shl(u128_of(0, 1), (unsigned)shift - 1);
    c = u128_cmp(rest, half);
    *ord = c == 0 && q->sticky ? 1 : c;
    *inexact = !u128_is_zero(rest) || q->sticky;
    return u128_shr(q->m, (unsigned)shift);
}

/* the value of q cut at bit `shift`, rounded by r */
static sw_u128 quotient_round(const sw_quotient *q, int32_t shift, sw_rounding r) {
    int ord;
    bool inexact;
    sw_u128 kept = quotient_cut(q, shift, &ord, &inexact);
    if (round_up(r, q->neg, inexact, ord > 0, ord == 0, (kept.lo & 1u) != 0)) {
        kept = u128_add_small(kept, 1);
    }
    return kept;
}

/* The bits of the float with `bits` of mantissa (the hidden one included) and normal exponents
 * 1 - max_exp ..= max_exp that r rounds q to, the sign in bit `sign`; false past the largest finite float
 * (Quotient::to_float). Subnormals included. */
static bool quotient_to_float(const sw_quotient *q, unsigned bits, int32_t max_exp, unsigned sign, sw_rounding r,
                              uint64_t *out) {
    uint64_t sign_bit = q->neg ? UINT64_C(1) << sign : 0;
    int32_t lead, lsb;
    uint64_t kept, field, v;
    if (u128_is_zero(q->m)) {
        *out = sign_bit;
        return true;
    }
    lead = q->e + 127;
    if (lead > max_exp) {
        return false;
    }
    if (lead < 1 - max_exp) {
        lead = 1 - max_exp;
    }
    lsb = lead - (int32_t)(bits - 1);
    /* at least 2^127 cut by at least 128 - bits: below 2^bits, plus 1 */
    kept = quotient_round(q, lsb - q->e, r).lo;
    field = (uint64_t)(lead + max_exp - 1);
    v = (field << (bits - 1)) + kept;
    if ((v >> (bits - 1)) > (uint64_t)(2 * max_exp)) {
        return false;
    }
    *out = sign_bit | v;
    return true;
}

/* the integer r rounds q to as sign and magnitude in `s`, fits false past 2^128 (Quotient::to_int; past an i128,
 * settle_int takes it as its range) */
static sw_scaled quotient_to_int(const sw_quotient *q, sw_rounding r) {
    sw_scaled s;
    s.neg = q->neg;
    s.low = u128_of(0, 0);
    s.fits = true;
    if (u128_is_zero(q->m)) {
        s.neg = false;
        return s;
    }
    /* m >= 2^127: from e = 0 on the value is past i128 */
    if (q->e >= 0) {
        s.fits = false;
        return s;
    }
    s.low = quotient_round(q, -q->e, r);
    return s;
}

/* a whole double y into the integer storage `to` by o: NaN and infinities, which have no quotient */
static bool settle_f64(sw_storage to, double y, sw_overflow o, sw_num *out) {
    bool pos;
    out->kind = to.kind;
    if (o == SW_OVERFLOW_CHECKED) {
        return false;
    }
    pos = y > 0.0;
    if (o == SW_OVERFLOW_SATURATE && y == y) {
        if (pos) {
            put_max(to, out);
        } else {
            put_min(to, out);
        }
        return true;
    }
    /* saturated NaN, and wrapped NaN and infinities, are 0 */
    put_wrapped(to, 0, out);
    return true;
}

static double f64_of_bits(uint64_t b) {
    double y;
    memcpy(&y, &b, sizeof y);
    return y;
}

static double f64_of_f32_bits(uint64_t b) {
    uint32_t b32 = (uint32_t)b;
    float y;
    memcpy(&y, &b32, sizeof y);
    return (double)y;
}

/* x * factor with a float on either side (Requantizer::apply through exact_float::convert) */
static bool requantize_float(const sw_requantizer *q, sw_num x, sw_num *out) {
    sw_quotient qt;
    bool single = q->to.kind == SW_NUM_F32;
    if (q->to.kind == SW_NUM_F32 || q->to.kind == SW_NUM_F64) {
        uint64_t bits;
        out->kind = q->to.kind;
        if (!quotient_of_num(x, &q->factor, &qt)) {
            /* NaN and infinities stay as they are (the factor is positive) */
            out->v.f = single ? (double)(float)x.v.f : x.v.f;
            return true;
        }
        if (single ? quotient_to_float(&qt, 24, 127, 31, q->rounding, &bits)
                   : quotient_to_float(&qt, 53, 1023, 63, q->rounding, &bits)) {
            out->v.f = single ? f64_of_f32_bits(bits) : f64_of_bits(bits);
            return true;
        }
        /* past the largest finite float: saturating is it, wrapping an infinity (a float has no modulo) */
        switch (q->overflow) {
        case SW_OVERFLOW_CHECKED:
            return false;
        case SW_OVERFLOW_SATURATE:
            bits = single ? UINT64_C(0x7f7fffff) : UINT64_C(0x7fefffffffffffff);
            break;
        case SW_OVERFLOW_WRAP:
        default:
            bits = single ? UINT64_C(0x7f800000) : UINT64_C(0x7ff0000000000000);
            break;
        }
        out->v.f = single ? f64_of_f32_bits(bits) : f64_of_bits(bits);
        if (qt.neg) {
            out->v.f = -out->v.f;
        }
        return true;
    }
    if (!quotient_of_num(x, &q->factor, &qt)) {
        return settle_f64(q->to, x.v.f, q->overflow, out);
    }
    return settle_int(q->to, quotient_to_int(&qt, q->rounding), q->overflow, out);
}
#endif

bool sw_requantize(const sw_requantizer *q, sw_num x, sw_num *out) {
    bool neg;
    uint64_t mag;
    sw_scaled s;
#ifndef SW_NO_F64
    if (q->to.kind == SW_NUM_F64 || q->to.kind == SW_NUM_F32 || x.kind == SW_NUM_F64 || x.kind == SW_NUM_F32) {
        return requantize_float(q, x, out);
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
