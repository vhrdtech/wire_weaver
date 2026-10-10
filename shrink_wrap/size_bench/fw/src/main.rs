//! One firmware image per test case (a cargo feature each), built by the `size_bench` runner next to this crate
//! to see how much code shrink_wrap costs on a small CPU. The images are measured, not run: `_start` sets up
//! nothing.
//!
//! Every case is a `run(rx, tx) -> usize`: decode what is in `rx`, encode into `tx`, return the bytes written.
//! Inputs and outputs go through [core::hint::black_box], so nothing is folded away at compile time and every
//! decoded field counts as used.
#![no_std]
#![no_main]

mod cases;
#[cfg(feature = "busgen_hand")]
mod hand;
#[allow(dead_code)]
mod types;

use core::hint::black_box;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

static mut RX: [u8; 1100] = [0; 1100];
static mut TX: [u8; 300] = [0; 300];

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
extern "C" fn _start() -> ! {
    // SAFETY: one thread, no interrupts; this function owns the buffers
    let (rx, tx) = unsafe { (&*core::ptr::addr_of!(RX), &mut *core::ptr::addr_of_mut!(TX)) };
    loop {
        let rx = black_box(&rx[..]);
        let len = cases::run(rx, tx);
        black_box(&tx[..len]);
    }
}
