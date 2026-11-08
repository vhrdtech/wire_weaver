//! The device's RTT receive path with and without the hello scan (EVO-23), Cortex-M3: a stream of messages of 3, 20
//! and 150 bytes as the host's RTT framer writes them, read in chunks into `FramedRx` in stream mode, as
//! `ww_device::StreamRx` does, and through `ww_link::hello::rtt::Scanner` first, as it does since the hello goes
//! outside the framer. Run under `qemu_icount`: the first `mark_start`/`mark_end` pair is calibration, then one
//! bracket each for
//! 1. the read alone (into the staging area), 64 byte chunks: the baseline of 3 and 7,
//! 2. the framer, 64 byte chunks (the path before the scan),
//! 3. the scan and the framer, 64 byte chunks (the path now): 3 - 2 is what the scan adds,
//! 4. the scan alone, 64 byte chunks: 4 - 1 is the scan's own cost,
//! 5. the read alone, 8 byte chunks: the baseline of 6,
//! 6. the scan alone, 8 byte chunks: against 4, the cost of a read,
//! 7. the scan alone over bytes that are all `0x30` (ASCII zeros, the head's first byte), 64 byte chunks.
//!
//! The bin prints the bytes and messages of each. A fault exits QEMU with an error instead of looping.
#![no_main]
#![no_std]

extern crate panic_semihosting;

use core::fmt::Write;
use cortex_m_rt::{ExceptionFrame, entry, exception};
use cortex_m_semihosting::{debug, hio};
use ww_link::hello::rtt::Scanner;
use ww_link::{Role, RttChecksum, RttHead, RttTail};

const MESSAGES: usize = 30;
const STREAM_MAX: usize = 4096;

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

static mut STREAM: [u8; STREAM_MAX] = [0; STREAM_MAX];
static mut ASSEMBLY: [u8; 512] = [0; 512];
static mut SCANNER: Scanner = Scanner::new(Role::Host);

#[entry]
fn main() -> ! {
    let stream = unsafe { &mut *core::ptr::addr_of_mut!(STREAM) };
    let mut message = [0u8; 150];
    for (i, b) in message.iter_mut().enumerate() {
        *b = i as u8;
    }
    let mut len = 0;
    {
        let mut chunk = [0u8; 256];
        let mut tx = ww_framer::Tx::<RttHead, RttChecksum, RttTail>::new(&mut chunk);
        for i in 0..MESSAGES {
            let m = &message[..[3, 20, 150][i % 3]];
            if !tx.write_full(0, m).unwrap() {
                let n = tx.flush();
                stream[len..len + n].copy_from_slice(&tx.buf()[..n]);
                len += n;
                assert!(tx.write_full(0, m).unwrap());
            }
        }
        let n = tx.flush();
        stream[len..len + n].copy_from_slice(&tx.buf()[..n]);
        len += n;
    }

    mark_start();
    mark_end();
    receive("read, 64 B chunks", &stream[..len], 64, false, false);
    receive("framer, 64 B chunks", &stream[..len], 64, false, true);
    receive("scan + framer, 64 B chunks", &stream[..len], 64, true, true);
    receive("scan, 64 B chunks", &stream[..len], 64, true, false);
    receive("read, 8 B chunks", &stream[..len], 8, false, false);
    receive("scan, 8 B chunks", &stream[..len], 8, true, false);
    let worst = &mut stream[..len];
    worst.fill(0x30);
    receive("scan, all 0x30, 64 B chunks", worst, 64, true, false);
    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}

#[inline(never)]
fn receive(name: &str, stream: &[u8], chunk: usize, scan: bool, framer: bool) {
    let mut stdout = hio::hstdout().unwrap();
    let assembly = unsafe { &mut *core::ptr::addr_of_mut!(ASSEMBLY) };
    let scanner = unsafe { &mut *core::ptr::addr_of_mut!(SCANNER) };
    scanner.reset();
    let mut rx = ww_framer::FramedRx::<RttHead, RttChecksum, RttTail>::new(assembly);
    let (mut messages, mut hellos) = (0usize, 0usize);
    mark_start();
    for c in core::hint::black_box(stream).chunks(chunk) {
        // as StreamRx: the read lands in the staging area, the scan looks at it before it is committed
        let buf = rx.staging_buf();
        buf[..c.len()].copy_from_slice(c);
        if scan && scanner.scan(&rx.staging_buf()[..c.len()]).is_some() {
            hellos += 1;
            rx.reset();
            continue;
        }
        if framer {
            _ = rx.commit_staged(c.len());
            loop {
                rx.reassemble();
                if rx.message().is_none() {
                    break;
                }
                messages += 1;
            }
        }
    }
    mark_end();
    writeln!(
        stdout,
        "{name}: bytes={} messages={messages} hellos={hellos}",
        stream.len()
    )
    .unwrap();
}
