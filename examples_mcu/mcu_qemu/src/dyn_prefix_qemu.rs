//! The QEMU meter of the dynamic-prefix bins ([super::dyn_prefix]): brackets each piece of work for `qemu_icount`
//! and prints the bytes over semihosting.

use super::dyn_prefix::{self, BATCHES, Group};

#[inline(never)]
pub fn mark_start() {
    unsafe { core::arch::asm!("nop", options(nostack, preserves_flags)) }
}

#[inline(never)]
pub fn mark_end() {
    unsafe { core::arch::asm!("nop", "nop", options(nostack, preserves_flags)) }
}

/// The QEMU bins' meter: brackets each piece of work with [mark_start] / [mark_end] for `qemu_icount`, and sums
/// the bytes per batch and group.
pub struct Marks {
    pub bytes: [[usize; Group::COUNT]; BATCHES],
    pub changes: [(usize, usize); BATCHES],
}

impl Marks {
    pub const fn new() -> Self {
        Marks {
            bytes: [[0; Group::COUNT]; BATCHES],
            changes: [(0, 0); BATCHES],
        }
    }

    /// Calibration first, then `run` with the marks, then the bytes over semihosting, then exits QEMU.
    pub fn main(groups: &[Group], run: impl FnOnce(&mut Marks)) -> ! {
        use core::fmt::Write;
        use cortex_m_semihosting::{debug, hio};
        let mut stdout = hio::hstdout().unwrap();
        mark_start();
        mark_end();
        let mut marks = Marks::new();
        run(&mut marks);
        for batch in 0..BATCHES {
            for &group in groups {
                writeln!(
                    stdout,
                    "batch={batch} {} bytes={}",
                    group.label(),
                    marks.bytes[batch][group as usize]
                )
                .unwrap();
            }
            if groups.contains(&Group::Auto) {
                let (columns, changes) = marks.changes[batch];
                writeln!(
                    stdout,
                    "batch={batch} prefix changes: {changes} in {columns} columns"
                )
                .unwrap();
            }
        }
        debug::exit(debug::EXIT_SUCCESS);
        loop {}
    }
}

impl dyn_prefix::Meter for Marks {
    fn measure(&mut self, group: Group, batch: usize, work: &mut dyn FnMut() -> usize) {
        mark_start();
        let bytes = work();
        mark_end();
        self.bytes[batch][group as usize] += bytes;
    }

    fn changes(&mut self, batch: usize, columns: usize, changes: usize) {
        self.changes[batch] = (columns, changes);
    }
}
