//! `ww_bus_slave` interrupt hooks, Cortex-M3 (SRV-26): what a slave peripheral's interrupt handler spends per byte.
//! Run under `qemu_icount`: the first `mark_start`/`mark_end` pair is calibration, then one pair per case, each
//! `BYTES` bytes of one transfer (the address and stop hooks are their own case); the bin prints the cases, so the
//! counts divide by `BYTES`. A fault exits QEMU with an error instead of looping.
//!
//! Cases: I2C register map write, read; I2C queue read; I2C address + stop of a one-byte write; SPI register map
//! read frame (`miso` + `mosi` per byte); SPI queue frame; SPI CS assert + release.
#![no_main]
#![no_std]

extern crate panic_semihosting;

use core::fmt::Write;
use core::hint::black_box;
use cortex_m_rt::{ExceptionFrame, entry, exception};
use cortex_m_semihosting::{debug, hio};
use ww_bus_slave::{I2cSlave, ReadByte, SpiSlave};
use ww_i2c::{I2cAddress, I2cSlaveAnswers, I2cSlaveConfig, I2cSlaveUnderrun};
use ww_spi::{BitOrder, SpiMode, SpiRegisters, SpiRwBit, SpiSlaveAnswers, SpiSlaveConfig};

const BYTES: usize = 64;

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

type I2c = I2cSlave<256, 64, 4, 64, 8, 64>;
type Spi = SpiSlave<256, 64, 4, 64, 8, 64>;

static mut I2C_REGS: I2c = I2c::new();
static mut I2C_QUEUE: I2c = I2c::new();
static mut SPI_REGS: Spi = Spi::new();
static mut SPI_QUEUE: Spi = Spi::new();

/// The I2C hooks behind `#[inline(never)]`, as an interrupt handler calls them.
#[inline(never)]
fn i2c_write(s: &mut I2c, byte: u8, now: u32) {
    s.write(byte, now)
}

#[inline(never)]
fn i2c_read(s: &mut I2c, now: u32) -> ReadByte {
    s.read(now)
}

#[inline(never)]
fn spi_byte(s: &mut Spi, mosi: u8) -> u8 {
    let out = s.miso();
    s.mosi(mosi);
    out
}

#[entry]
fn main() -> ! {
    let mut stdout = hio::hstdout().unwrap();
    let i2c_regs = unsafe { &mut *core::ptr::addr_of_mut!(I2C_REGS) };
    let i2c_queue = unsafe { &mut *core::ptr::addr_of_mut!(I2C_QUEUE) };
    let spi_regs = unsafe { &mut *core::ptr::addr_of_mut!(SPI_REGS) };
    let spi_queue = unsafe { &mut *core::ptr::addr_of_mut!(SPI_QUEUE) };
    let addr = I2cAddress::seven(0x50).unwrap();
    i2c_regs
        .configure(&I2cSlaveConfig {
            addr,
            io_level: None,
            answers: I2cSlaveAnswers::Registers {
                pointer_bytes: 1,
                len: 256,
                page: Some(16),
                read_only: false,
            },
        })
        .unwrap();
    i2c_queue
        .configure(&I2cSlaveConfig {
            addr,
            io_level: None,
            answers: I2cSlaveAnswers::Queue {
                empty: I2cSlaveUnderrun::Fill(0xFF),
            },
        })
        .unwrap();
    let spi_config = |answers| SpiSlaveConfig {
        mode: SpiMode::Mode0,
        bit_order: BitOrder::MsbFirst,
        word_bits: 8,
        io_level: None,
        answers,
    };
    spi_regs
        .configure(&spi_config(SpiSlaveAnswers::Registers(SpiRegisters {
            addr_bytes: 1,
            rw: SpiRwBit::SetIsRead { bit: 7 },
            dummy_bytes: 0,
            idle: 0,
            len: 256,
        })))
        .unwrap();
    spi_queue
        .configure(&spi_config(SpiSlaveAnswers::Queue {
            idle: 0,
            repeat_last: false,
        }))
        .unwrap();
    let mut answer = [0u8; BYTES];
    for (i, b) in answer.iter_mut().enumerate() {
        *b = i as u8;
    }
    i2c_queue.push_answer(&answer).unwrap();
    spi_queue.push_answer(&answer).unwrap();
    let mut sum = 0u32;

    mark_start();
    mark_end();

    // I2C register map write: pointer byte, then data with page wrap
    assert!(i2c_regs.address(false, 0));
    i2c_write(i2c_regs, 0, 1);
    mark_start();
    for i in 0..BYTES {
        i2c_write(i2c_regs, black_box(i as u8), black_box(2));
    }
    mark_end();
    i2c_regs.stop(3);

    // I2C register map read
    assert!(i2c_regs.address(true, 4));
    mark_start();
    for _ in 0..BYTES {
        if let ReadByte::Byte(b) = i2c_read(i2c_regs, black_box(5)) {
            sum += b as u32;
        }
    }
    mark_end();
    i2c_regs.stop(6);

    // I2C queue read
    assert!(i2c_queue.address(true, 7));
    mark_start();
    for _ in 0..BYTES {
        if let ReadByte::Byte(b) = i2c_read(i2c_queue, black_box(8)) {
            sum += b as u32;
        }
    }
    mark_end();
    i2c_queue.stop(9);

    // I2C address + stop around a one-byte write
    mark_start();
    let acked = i2c_regs.address(black_box(false), black_box(10));
    i2c_write(i2c_regs, 0, 11);
    i2c_regs.stop(black_box(12));
    mark_end();

    // SPI register map read frame: address byte with the read bit, then data
    spi_regs.cs_asserted(13);
    spi_byte(spi_regs, 0x80);
    mark_start();
    for _ in 0..BYTES {
        sum += spi_byte(spi_regs, black_box(0)) as u32;
    }
    mark_end();
    spi_regs.cs_released(14);

    // SPI queue frame
    spi_queue.cs_asserted(15);
    mark_start();
    for _ in 0..BYTES {
        sum += spi_byte(spi_queue, black_box(0)) as u32;
    }
    mark_end();
    spi_queue.cs_released(16);

    // SPI CS edges of a one-byte frame
    mark_start();
    spi_regs.cs_asserted(black_box(17));
    spi_byte(spi_regs, 0x80);
    spi_regs.cs_released(black_box(18));
    mark_end();

    let mut events = 0;
    while i2c_regs.peek_event().is_some() {
        i2c_regs.pop_event();
        events += 1;
    }
    writeln!(
        stdout,
        "bytes={BYTES} cases=i2c-map-write,i2c-map-read,i2c-queue-read,i2c-address-stop,spi-map-read,spi-queue,spi-cs \
         sum={sum} acked={acked} i2c_events={events}"
    )
    .unwrap();
    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}
