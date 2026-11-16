/* Sizes of the runtime's structs, so the Rust tests can check their #[repr(C)] mirrors. */
#include "shrink_wrap.h"
#include "sw_requantize.h"

size_t sw_test_sizeof_reader(void) {
    return sizeof(sw_reader);
}

size_t sw_test_sizeof_writer(void) {
    return sizeof(sw_writer);
}

size_t sw_test_sizeof_unsized(void) {
    return sizeof(sw_unsized);
}

size_t sw_test_sizeof_factor(void) {
    return sizeof(sw_factor);
}

size_t sw_test_sizeof_requantizer(void) {
    return sizeof(sw_requantizer);
}

size_t sw_test_sizeof_num(void) {
    return sizeof(sw_num);
}
