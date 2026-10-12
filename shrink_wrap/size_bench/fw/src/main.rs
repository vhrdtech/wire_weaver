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

/// A byte at a time `memcpy`, instead of the word-wise one of `compiler_builtins` (338 bytes on RV32IC, 316 on
/// Cortex-M0): about 20 bytes. The firmware's own symbols win over the weak ones of `compiler_builtins`. Off by
/// default: a measurement, and advice for very small targets only, it is slower and replaces `memcpy` for the
/// whole firmware (docs/serdes/code_size.md, "Build settings").
#[cfg(feature = "byte_memcpy")]
mod byte_memcpy {
    use core::ffi::c_void;

    /// # Safety
    /// `dest` and `src` are valid for `n` bytes and do not overlap.
    #[inline(always)]
    unsafe fn copy(dest: *mut c_void, src: *const c_void, n: usize) {
        let (dest, src) = (dest.cast::<u8>(), src.cast::<u8>());
        let mut i = 0;
        while i < n {
            // SAFETY: as the caller promised. The volatile store keeps LLVM from turning the loop back into a
            // call of `memcpy`, i.e. of itself.
            unsafe { dest.add(i).write_volatile(*src.add(i)) };
            i += 1;
        }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn memcpy(
        dest: *mut c_void,
        src: *const c_void,
        n: usize,
    ) -> *mut c_void {
        // SAFETY: the same contract as ours
        unsafe { copy(dest, src, n) };
        dest
    }

    /// What Arm code calls; the one of `compiler_builtins` would call its own `memcpy`, not the one above.
    #[cfg(target_arch = "arm")]
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn __aeabi_memcpy(dest: *mut c_void, src: *const c_void, n: usize) {
        // SAFETY: the same contract as ours
        unsafe { copy(dest, src, n) };
    }
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
