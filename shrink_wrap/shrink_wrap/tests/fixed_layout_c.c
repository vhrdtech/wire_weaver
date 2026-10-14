/* The C reader generated from tests/fixed_layout.rs (fixed_layout.h) on the bytes of its `known_bits` test:
   built and run by `just lint-c` with every warning an error (SW-45). */
#include <stdio.h>

#include "fixed_layout.h"

int main(void) {
    static const uint8_t bits[] = {0x8a, 0xbd, 0x34, 0x12, 0xfe, 0xdc, 0xbf, 0xfe, 0xc0};
    int ok = t_bits_a(bits) == 1u && t_bits_n(bits) == 0xau && t_bits_b(bits) == 5u && t_bits_s(bits) == -3 &&
             t_bits_x(bits) == 0x1234u && t_bits_wide(bits) == 0xfedcbu && t_bits_i(bits) == -2 &&
             t_bits_last(bits) == 1u && t_bits_end(bits) == 2u && T_BITS_LEN == sizeof bits;
    printf("fixed_layout C reader: %s\n", ok ? "ok" : "WRONG");
    return ok ? 0 : 1;
}
