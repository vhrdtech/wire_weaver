//! `ww_framer` receive path, Cortex-M3: USB-framed 64-byte packets (the `ww_link` USB configuration) staged and
//! reassembled by `FramedRx`, as `ww_device::FramedRx` does on a device. Messages of 3, 20 and 150 bytes in turn,
//! so frames carry several messages, and the long ones are split over three frames. Run under `qemu_icount`: the
//! first `mark_start`/`mark_end` pair is calibration, the second brackets receiving every frame; the bin prints
//! the frames, messages and bytes it got. A fault exits QEMU with an error instead of looping.
#![no_main]
#![no_std]

extern crate panic_semihosting;

use core::fmt::Write;
use cortex_m_rt::{ExceptionFrame, entry, exception};
use cortex_m_semihosting::{debug, hio};
use ww_link::{UsbChecksum, UsbHead, UsbTail};

const FRAME: usize = 64;
const MESSAGES: usize = 60;
const MAX_FRAMES: usize = 128;

#[inline(never)]
fn mark_start() {
    unsafe { core::arch::asm!("nop", options(nostack, preserves_flags)) }
}

#[inline(never)]
fn mark_end() {
    unsafe { core::arch::asm!("nop", "nop", options(nostack, preserves_flags)) }
}

#[exception]
unsafe fn HardFault(_frame: &ExceptionFrame) -> ! {
    debug::exit(debug::EXIT_FAILURE);
    loop {}
}

static mut FRAMES: [[u8; FRAME]; MAX_FRAMES] = [[0; FRAME]; MAX_FRAMES];
static mut LENS: [usize; MAX_FRAMES] = [0; MAX_FRAMES];
static mut ASSEMBLY: [u8; 512 + FRAME] = [0; 512 + FRAME];

#[entry]
fn main() -> ! {
    let mut stdout = hio::hstdout().unwrap();
    let frames = unsafe { &mut *core::ptr::addr_of_mut!(FRAMES) };
    let lens = unsafe { &mut *core::ptr::addr_of_mut!(LENS) };
    let assembly = unsafe { &mut *core::ptr::addr_of_mut!(ASSEMBLY) };

    let mut message = [0u8; 150];
    for (i, b) in message.iter_mut().enumerate() {
        *b = i as u8;
    }
    let mut buf = [0u8; FRAME];
    let mut tx = ww_framer::Tx::<UsbHead, UsbChecksum, UsbTail>::new(&mut buf);
    let mut n_frames = 0;
    for i in 0..MESSAGES {
        let len = [3, 20, 150][i % 3];
        while !tx.write(0, &message[..len]).unwrap() {
            let n = tx.flush();
            frames[n_frames][..n].copy_from_slice(&tx.buf()[..n]);
            lens[n_frames] = n;
            n_frames += 1;
        }
    }
    let n = tx.flush();
    frames[n_frames][..n].copy_from_slice(&tx.buf()[..n]);
    lens[n_frames] = n;
    n_frames += 1;

    mark_start();
    mark_end();
    let mut rx = ww_framer::FramedRx::<UsbHead, UsbChecksum, UsbTail>::new(assembly);
    let (mut got, mut bytes) = (0usize, 0usize);
    mark_start();
    for f in 0..n_frames {
        rx.stage(core::hint::black_box(&frames[f][..lens[f]])).unwrap();
        loop {
            rx.reassemble();
            match rx.message() {
                Some((_, m)) => {
                    got += 1;
                    bytes += m.len();
                }
                None => break,
            }
        }
    }
    mark_end();
    writeln!(stdout, "frames={n_frames} messages={got} bytes={bytes}").unwrap();
    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}
