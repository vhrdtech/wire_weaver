/* The C views generated from tests/fixed_layout.rs (fixed_layout.h), SW-45 and SW-79. Built and run by `just lint-c`
   and by the `c_views_check_bounds` test, with every warning an error.

   With no arguments: its own checks (the bytes of the `known_bits` test, every truncation of them, a NULL buffer).
   With `-`: one case per line on stdin, `<type> <hex or ->`; for each it prints `<type> <length>` of the view, or
   `<type> -1` when it is refused. The bytes of a case are in a buffer of exactly their length and every reader of
   the view is called (indexes past the end too), so AddressSanitizer catches any read outside them. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "fixed_layout.h"

/* every value read goes here, so that no read is optimized away */
static volatile uint64_t sink;
/* a reader broke its promise (a value past the end, a view that should be empty) */
static int bad;

static void use_u(uint64_t x) { sink += x; }
static void use_i(int64_t x) { sink += (uint64_t)(x < 0); }
static void use_f(double x) { sink += (uint64_t)(x != x); }

static void poke_inner(t_inner_view_t v) {
    size_t i;
    use_u(t_inner_id(v));
    for (i = 0; i < T_INNER_HASH_COUNT; i++) {
        use_u(t_inner_hash(v)[i]);
    }
}

static void poke_numbers(t_numbers_view_t v) {
    size_t i;
    use_u(t_numbers_a(v));
    use_u(t_numbers_b(v));
    use_u(t_numbers_c(v));
    use_u(t_numbers_d(v));
    use_i(t_numbers_e(v));
    use_i(t_numbers_f(v));
    use_i(t_numbers_g(v));
    use_i(t_numbers_h(v));
    use_f(t_numbers_x(v));
    use_f(t_numbers_y(v));
    for (i = 0; i < T_NUMBERS_BYTES_COUNT; i++) {
        use_u(t_numbers_bytes(v)[i]);
    }
    for (i = 0; i < T_NUMBERS_WORDS_COUNT + 2u; i++) {
        use_u(t_numbers_words(v, i));
    }
    if (t_numbers_words(v, T_NUMBERS_WORDS_COUNT) != 0u) {
        bad = 1; /* past the end reads 0 */
    }
}

static void poke_outer(t_outer_view_t v) {
    size_t i;
    use_u(t_outer_tag(v));
    poke_inner(t_outer_inner(v));
    for (i = 0; i < T_OUTER_SIGNED_COUNT + 2u; i++) {
        use_i(t_outer_signed(v, i));
    }
    use_u(t_outer_tail(v));
}

static void poke_final(t_final_view_t v) {
    use_u(t_final_a(v));
    poke_inner(t_final_b(v));
}

static void poke_op(t_op_view_t v) {
    t_op_erase_view_t e;
    t_op_copy_view_t c;
    t_op_inner_view_t in;
    size_t i;
    use_u(t_op_tag(v));
    if (t_op_as_erase(v, &e) == 0) {
        use_u(t_op_erase_block(e));
    } else if (e.p != NULL) {
        bad = 1;
    }
    if (t_op_as_copy(v, &c) == 0) {
        use_u(t_op_copy_dst(c));
        use_u(t_op_copy_src(c));
        for (i = 0; i < T_OP_COPY_HASH_COUNT; i++) {
            use_u(t_op_copy_hash(c)[i]);
        }
    }
    if (t_op_as_inner(v, &in) == 0) {
        poke_inner(t_op_inner_0(in));
    }
}

static void poke_wide(t_wide_view_t v) {
    t_wide_a_view_t a;
    t_wide_b_view_t b;
    use_u(t_wide_tag(v));
    if (t_wide_as_a(v, &a) == 0) {
        use_u(t_wide_a_0(a));
    }
    if (t_wide_as_b(v, &b) == 0) {
        use_i(t_wide_b_0(b));
    }
}

static void poke_bits(t_bits_view_t v) {
    use_u(t_bits_a(v));
    use_u(t_bits_n(v));
    use_u(t_bits_b(v));
    use_i(t_bits_s(v));
    use_u(t_bits_x(v));
    use_u(t_bits_wide(v));
    use_i(t_bits_i(v));
    use_u(t_bits_last(v));
    use_u(t_bits_end(v));
}

static void poke_ends_with_bits(t_ends_with_bits_view_t v) {
    use_u(t_ends_with_bits_id(v));
    use_u(t_ends_with_bits_on(v));
}

static void poke_wrap(t_wrap_view_t v) {
    use_u(t_wrap_lead(v));
    poke_inner(t_wrap_inner(v));
    poke_ends_with_bits(t_wrap_flags(v));
    use_u(t_wrap_after(v));
}

static void poke_maybe(t_maybe_view_t v) {
    t_inner_view_t in;
    use_u(t_maybe_id(v));
    use_u(t_maybe_on(v));
    use_u(t_maybe_extra_flag(v));
    if (t_maybe_extra(v, &in) == 0) {
        poke_inner(in);
    } else if (in.p != NULL || t_maybe_extra_flag(v) != 0u) {
        bad = 1;
    }
}

static void poke_relocated(t_relocated_view_t v) {
    uint16_t x;
    size_t i;
    use_u(t_relocated_value_flag(v));
    use_u(t_relocated_kind(v));
    use_u(t_relocated_id(v));
    for (i = 0; i < T_RELOCATED_VALUE_COUNT + 2u; i++) {
        if (t_relocated_value(v, i, &x) == 0) {
            use_u(x);
        }
    }
}

static void poke_res(t_res_view_t v) {
    uint32_t ok;
    t_inner_view_t err;
    use_u(t_res_a(v));
    use_u(t_res_r_flag(v));
    if (t_res_r_ok(v, &ok) == 0) {
        use_u(ok);
    }
    if (t_res_r_err(v, &err) == 0) {
        poke_inner(err);
    }
}

static void poke_tail_op(t_tail_op_view_t v) {
    use_u(t_tail_op_seq(v));
    poke_op(t_tail_op_op(v));
}

static void poke_msg(t_msg_view_t v) {
    t_msg_data_view_t d;
    const uint8_t *payload;
    size_t i;
    use_u(t_msg_tag(v));
    if (t_msg_as_data(v, &d) == 0) {
        use_u(t_msg_data_urgent(d));
        use_u(t_msg_data_prio(d));
        use_u(t_msg_data_seq(d));
        use_u(t_msg_data_payload_flag(d));
        if (t_msg_data_payload(d, &payload) == 0) {
            for (i = 0; i < T_MSG_DATA_PAYLOAD_COUNT; i++) {
                use_u(payload[i]);
            }
        }
    }
}

/* the view of the type named ty on the n bytes at b: its length, or -1 refused; -2 when a refused view isn't
   empty or a view doesn't point at b and in it, -3 when a reader broke its promise */
#define VIEW(name)                                                                                                    \
    if (strcmp(ty, #name) == 0) {                                                                                     \
        t_##name##_view_t v;                                                                                          \
        if (t_##name##_view(b, n, &v) != 0) {                                                                         \
            return v.p == NULL && v.len == 0u ? -1 : -2;                                                              \
        }                                                                                                             \
        if (v.p != b || v.len > n) {                                                                                  \
            return -2;                                                                                                \
        }                                                                                                             \
        poke_##name(v);                                                                                               \
        return bad ? -3 : (long)v.len;                                                                                           \
    }

static long view(const char *ty, const uint8_t *b, size_t n) {
    VIEW(numbers)
    VIEW(inner)
    VIEW(outer)
    VIEW(final)
    VIEW(op)
    VIEW(wide)
    VIEW(bits)
    VIEW(ends_with_bits)
    VIEW(wrap)
    VIEW(maybe)
    VIEW(relocated)
    VIEW(res)
    VIEW(tail_op)
    VIEW(msg)
    fprintf(stderr, "no type %s\n", ty);
    exit(2);
}

static const char *const TYPES[] = {"numbers", "inner", "outer", "final", "op", "wide", "bits",
                                    "ends_with_bits", "wrap", "maybe", "relocated", "res", "tail_op", "msg"};

static int hex(int c) {
    if (c >= '0' && c <= '9') {
        return c - '0';
    }
    if (c >= 'a' && c <= 'f') {
        return c - 'a' + 10;
    }
    return -1;
}

static int cases(void) {
    static char line[4096];
    while (fgets(line, sizeof line, stdin) != NULL) {
        char *sp = strchr(line, ' ');
        char *h;
        size_t digits = 0, n, i;
        uint8_t *b;
        if (sp == NULL) {
            return 2;
        }
        *sp = '\0';
        h = sp + 1;
        while (hex(h[digits]) >= 0) {
            digits++;
        }
        n = digits / 2u;
        /* exactly n bytes, so that a read past them is a heap overflow */
        b = malloc(n);
        if (b == NULL && n != 0u) {
            return 2;
        }
        for (i = 0; i < n; i++) {
            b[i] = (uint8_t)(hex(h[2u * i]) * 16 + hex(h[2u * i + 1u]));
        }
        printf("%s %ld\n", line, view(line, b, n));
        free(b);
    }
    return 0;
}

static int own_checks(void) {
    static const uint8_t bits[] = {0x8a, 0xbd, 0x34, 0x12, 0xfe, 0xdc, 0xbf, 0xfe, 0xc0};
    t_bits_view_t v;
    size_t cut, i;
    int ok = t_bits_view(bits, sizeof bits, &v) == 0 && v.p == bits && v.len == T_BITS_LEN;
    ok = ok && t_bits_a(v) == 1u && t_bits_n(v) == 0xau && t_bits_b(v) == 5u && t_bits_s(v) == -3 &&
         t_bits_x(v) == 0x1234u && t_bits_wide(v) == 0xfedcbu && t_bits_i(v) == -2 && t_bits_last(v) == 1u &&
         t_bits_end(v) == 2u && T_BITS_LEN == sizeof bits;
    /* every truncation is refused and leaves an empty view */
    for (cut = 0; cut < sizeof bits; cut++) {
        ok = ok && t_bits_view(bits, cut, &v) == -1 && v.p == NULL && v.len == 0u;
    }
    /* no type takes a NULL buffer or an empty one */
    for (i = 0; i < sizeof TYPES / sizeof TYPES[0]; i++) {
        ok = ok && view(TYPES[i], NULL, 0u) == -1 && view(TYPES[i], NULL, 64u) == -1 && view(TYPES[i], bits, 0u) == -1;
    }
    printf("fixed_layout C views: %s\n", ok ? "ok" : "WRONG");
    return ok ? 0 : 1;
}

int main(int argc, char **argv) {
    if (argc > 1 && strcmp(argv[1], "-") == 0) {
        return cases();
    }
    return own_checks();
}
