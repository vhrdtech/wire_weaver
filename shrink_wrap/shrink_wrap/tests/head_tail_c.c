/* The C reader generated from tests/head_tail.rs (head_tail.h) on the bytes of its `exact_bytes` test, as the
   Utx variant of a Request: built and run by `just lint-c` with every warning an error (SW-46). */
#include <stdio.h>

#include "head_tail.h"

int main(void) {
    static const uint8_t msg[] = {
        3,                                                   /* Request::Utx */
        0x07, 0x80, 0x01, 0x00, 0x80, 0x02, 0x80, 0x09, 0x02, 0x00, 0x80, /* Utx's head */
        0x01, 0x80, 0x04, 0x03, 0x02, 0x01,                  /* glitch */
        0x0b, 0x0a,                                          /* perr */
        0x04, 0x03,                                          /* s.a10 */
        0x68, 0x69,                                          /* name */
    };
    const uint8_t *end = msg + sizeof msg;
    const uint8_t *t = msg + T_REQUEST_TAG_LEN;
    t_request_tail_t tl;
    const uint8_t *u;
    const uint8_t *s;
    int ok = t_request_tail(msg, &t, end, &tl) == 0 && t == end && t_request_tag(msg) == T_REQUEST_UTX;
    u = t_request_utx_0(tl.head);
    s = t_utx_s(u);
    ok = ok && t_utx_ch(u) == 7u && t_utx_glitch_flag(u) == 1u && t_utx_defer(u) == 1u;
    ok = ok && tl.utx_0.glitch != NULL && t_wave_line(tl.utx_0.glitch) == 1u && t_wave_idle(tl.utx_0.glitch) == 1u &&
         t_wave_hold(tl.utx_0.glitch) == 0u && t_wave_delay(tl.utx_0.glitch) == 0x01020304u;
    ok = ok && t_utx_perr_count(u) == 1u && sw_fixed_u16(tl.utx_0.perr) == 0x0a0bu;
    ok = ok && t_slave_on(s) == 1u && t_slave_scl(s) == 2u && t_slave_filt(s) == 9u;
    ok = ok && tl.utx_0.s.a10 != NULL && sw_fixed_u16(tl.utx_0.s.a10) == 0x0304u && tl.utx_0.s.ptr == NULL;
    ok = ok && t_utx_name_count(u) == 2u && memcmp(tl.utx_0.name, "hi", 2) == 0;
    /* one byte short: an error */
    t = msg + T_REQUEST_TAG_LEN;
    ok = ok && t_request_tail(msg, &t, end - 1, &tl) == -1;
    printf("head_tail C reader: %s\n", ok ? "ok" : "WRONG");
    return ok ? 0 : 1;
}
