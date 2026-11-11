/* The C views generated from tests/head_tail.rs (head_tail.h), SW-46 and SW-79. Built and run by `just lint-c` and
   by the `c_views_check_bounds` test, with every warning an error.

   With no arguments: its own checks (the bytes of the `exact_bytes` test as the Utx variant of a Request, every
   truncation of them, a NULL buffer). With `-`: one case per line on stdin, `<type> <hex or ->`; for each it prints
   `<type> <length>` of the view, or `<type> -1` when it is refused. The bytes of a case are in a buffer of exactly
   their length and every reader of the view is called (indexes past the end too), so AddressSanitizer catches any
   read outside them. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "head_tail.h"

/* every value read goes here, so that no read is optimized away */
static volatile uint64_t sink;
/* a reader broke its promise (a value past the end, a view that should be empty) */
static int bad;

static void use_u(uint64_t x) { sink += x; }

static void poke_wave(t_wave_view_t v) {
    use_u(t_wave_line(v));
    use_u(t_wave_idle(v));
    use_u(t_wave_hold(v));
    use_u(t_wave_delay(v));
}

static void poke_pin(t_pin_view_t v) {
    t_pin_low_view_t lo;
    t_pin_high_view_t hi;
    use_u(t_pin_tag(v));
    if (t_pin_as_low(v, &lo) == 0) {
        use_u(t_pin_low_0(lo));
    }
    if (t_pin_as_high(v, &hi) == 0) {
        use_u(t_pin_high_0(hi));
    }
}

static void poke_bytes(const uint8_t *p, size_t n) {
    size_t i;
    for (i = 0; i < n; i++) {
        use_u(p[i]);
    }
}

static void poke_slave(const t_slave_view_t *v) {
    uint16_t a10;
    uint8_t ptr;
    use_u(t_slave_on(v));
    use_u(t_slave_scl(v));
    use_u(t_slave_a10_flag(v));
    if (t_slave_a10(v, &a10) == 0) {
        use_u(a10);
    } else if (t_slave_a10_flag(v) != 0u) {
        bad = 1;
    }
    use_u(t_slave_ptr_flag(v));
    if (t_slave_ptr(v, &ptr) == 0) {
        use_u(ptr);
    }
    use_u(t_slave_filt(v));
}

static void poke_utx(const t_utx_view_t *v) {
    const t_wave_view_t *glitch;
    size_t i;
    use_u(t_utx_ch(v));
    if (t_utx_glitch(v, &glitch) == 0) {
        poke_wave(*glitch);
    }
    for (i = 0; i < (size_t)t_utx_perr_count(v) + 2u; i++) {
        use_u(t_utx_perr(v, i));
    }
    if (t_utx_perr(v, t_utx_perr_count(v)) != 0u) {
        bad = 1; /* past the end reads 0 */
    }
    poke_slave(t_utx_s(v));
    poke_bytes(t_utx_name(v), t_utx_name_count(v));
    use_u(t_utx_defer(v));
}

static void poke_request(const t_request_view_t *v) {
    const t_request_stop_view_t *stop;
    const t_request_waves_view_t *waves;
    const t_request_utx_view_t *utx;
    const t_request_res_view_t *res;
    t_wave_view_t w;
    uint32_t ok;
    uint8_t err;
    size_t i;
    use_u(t_request_tag(v));
    if (t_request_as_stop(v, &stop) == 0) {
        use_u(t_request_stop_mask(stop));
    }
    if (t_request_as_waves(v, &waves) == 0) {
        for (i = 0; i < (size_t)t_request_waves_waves_count(waves) + 2u; i++) {
            if (t_request_waves_waves(waves, i, &w) == 0) {
                poke_wave(w);
            } else if (i < t_request_waves_waves_count(waves)) {
                bad = 1; /* a Wave has no tag: every item reads */
            }
        }
        use_u(t_request_waves_defer(waves));
    }
    if (t_request_as_utx(v, &utx) == 0) {
        poke_utx(t_request_utx_0(utx));
    }
    if (t_request_as_res(v, &res) == 0) {
        if (t_request_res_r_ok(res, &ok) == 0) {
            use_u(ok);
        }
        if (t_request_res_r_err(res, &err) == 0) {
            use_u(err);
        }
        poke_bytes(t_request_res_tag(res), T_REQUEST_RES_TAG_COUNT);
    }
}

static void poke_mixed(const t_mixed_view_t *v) {
    const t_slave_view_t *slave;
    const t_wave_view_t *wave;
    t_pin_view_t pin;
    uint16_t x;
    size_t i;
    poke_pin(*t_mixed_pin(v));
    if (t_mixed_slave(v, &slave) == 0) {
        poke_slave(slave);
    }
    if (t_mixed_r_ok(v, &wave) == 0) {
        poke_wave(*wave);
    }
    if (t_mixed_r_err(v, &x) == 0) {
        use_u(x);
    }
    for (i = 0; i < (size_t)t_mixed_pins_count(v) + 2u; i++) {
        if (t_mixed_pins(v, i, &pin) == 0) {
            poke_pin(pin);
        } else if (pin.p != NULL) {
            bad = 1;
        }
    }
    poke_bytes(t_mixed_raw(v), t_mixed_raw_count(v));
    for (i = 0; i < 4u; i++) {
        if (t_mixed_words(v, i, &x) == 0) {
            use_u(x);
        }
    }
}

/* the view of the type named ty on the n bytes at b: its length, or -1 refused; -2 when a refused view isn't
   empty or a view doesn't point at b and in it, -3 when a reader broke its promise */
#define VIEW_FIXED(name)                                                                                              \
    if (strcmp(ty, #name) == 0) {                                                                                     \
        t_##name##_view_t v;                                                                                          \
        if (t_##name##_view(b, n, &v) != 0) {                                                                         \
            return v.p == NULL && v.len == 0u ? -1 : -2;                                                              \
        }                                                                                                             \
        if (v.p != b || v.len > n) {                                                                                  \
            return -2;                                                                                                \
        }                                                                                                             \
        poke_##name(v);                                                                                               \
        return bad ? -3 : (long)v.len;                                                                                \
    }
#define VIEW(name)                                                                                                    \
    if (strcmp(ty, #name) == 0) {                                                                                     \
        t_##name##_view_t v;                                                                                          \
        if (t_##name##_view(b, n, &v) != 0) {                                                                         \
            return v.p == NULL && v.len == 0u ? -1 : -2;                                                              \
        }                                                                                                             \
        if (v.p != b || v.len > n) {                                                                                  \
            return -2;                                                                                                \
        }                                                                                                             \
        poke_##name(&v);                                                                                              \
        return bad ? -3 : (long)v.len;                                                                                \
    }

static long view(const char *ty, const uint8_t *b, size_t n) {
    VIEW_FIXED(wave)
    VIEW_FIXED(pin)
    VIEW(slave)
    VIEW(utx)
    VIEW(request)
    VIEW(mixed)
    fprintf(stderr, "no type %s\n", ty);
    exit(2);
}

static const char *const TYPES[] = {"wave", "pin", "slave", "utx", "request", "mixed"};

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
    static const uint8_t msg[] = {
        3,                                                                /* Request::Utx */
        0x07, 0x80, 0x01, 0x00, 0x80, 0x02, 0x80, 0x09, 0x02, 0x00, 0x80, /* Utx's head */
        0x01, 0x80, 0x04, 0x03, 0x02, 0x01,                               /* glitch */
        0x0b, 0x0a,                                                       /* perr */
        0x04, 0x03,                                                       /* s.a10 */
        0x68, 0x69,                                                       /* name */
    };
    t_request_view_t req;
    const t_request_utx_view_t *variant;
    const t_request_stop_view_t *stop;
    const t_utx_view_t *u;
    const t_slave_view_t *s;
    const t_wave_view_t *glitch;
    uint16_t a10;
    uint8_t ptr;
    size_t cut, i;
    int ok = t_request_view(msg, sizeof msg, &req) == 0 && req.len == sizeof msg &&
             t_request_tag(&req) == T_REQUEST_UTX && t_request_as_utx(&req, &variant) == 0 &&
             t_request_as_stop(&req, &stop) == -1 && stop == NULL;
    u = ok ? t_request_utx_0(variant) : NULL;
    ok = ok && t_utx_ch(u) == 7u && t_utx_glitch_flag(u) == 1u && t_utx_defer(u) == 1u;
    ok = ok && t_utx_glitch(u, &glitch) == 0 && t_wave_line(*glitch) == 1u && t_wave_idle(*glitch) == 1u &&
         t_wave_hold(*glitch) == 0u && t_wave_delay(*glitch) == 0x01020304u;
    ok = ok && t_utx_perr_count(u) == 1u && t_utx_perr(u, 0) == 0x0a0bu && t_utx_perr(u, 1) == 0u;
    s = ok ? t_utx_s(u) : NULL;
    ok = ok && t_slave_on(s) == 1u && t_slave_scl(s) == 2u && t_slave_filt(s) == 9u;
    ok = ok && t_slave_a10(s, &a10) == 0 && a10 == 0x0304u && t_slave_ptr(s, &ptr) == -1;
    ok = ok && t_utx_name_count(u) == 2u && memcmp(t_utx_name(u), "hi", 2) == 0;
    /* every truncation is refused and leaves a zeroed view */
    for (cut = 0; cut < sizeof msg; cut++) {
        ok = ok && t_request_view(msg, cut, &req) == -1 && req.p == NULL && req.len == 0u;
    }
    /* no type takes a NULL buffer or an empty one */
    for (i = 0; i < sizeof TYPES / sizeof TYPES[0]; i++) {
        ok = ok && view(TYPES[i], NULL, 0u) == -1 && view(TYPES[i], NULL, 64u) == -1 && view(TYPES[i], msg, 0u) == -1;
    }
    printf("head_tail C views: %s\n", ok ? "ok" : "WRONG");
    return ok ? 0 : 1;
}

int main(int argc, char **argv) {
    if (argc > 1 && strcmp(argv[1], "-") == 0) {
        return cases();
    }
    return own_checks();
}
