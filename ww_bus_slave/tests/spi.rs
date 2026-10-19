//! Every policy of the SPI slave engine, driven byte by byte as a firmware's interrupt handler would.

use ww_bus_slave::{ConfigError, MapError, SpiSlave};
use ww_si::{Prefix, Second};
use ww_spi::{BitOrder, SpiMode, SpiRegisters, SpiRwBit, SpiSlaveAnswers, SpiSlaveConfig};

type Slave = SpiSlave<64, 32, 3, 8, 8, 16>;

#[derive(Debug, PartialEq)]
struct Frame {
    start: u32,
    end: u32,
    truncated: bool,
    overrun: bool,
    underrun: bool,
    addr: Option<u32>,
    mosi: Vec<u8>,
    miso: Vec<u8>,
}

fn us(s: Second) -> u32 {
    s.to_u32_in(Prefix::Micro).unwrap()
}

fn frames(s: &mut Slave) -> Vec<Frame> {
    let mut out = vec![];
    while let Some((t, f)) = s.peek_event() {
        out.push(Frame {
            start: us(t),
            end: us(f.end),
            truncated: f.truncated,
            overrun: f.overrun,
            underrun: f.underrun,
            addr: f.addr,
            mosi: f.mosi.as_slice().to_vec(),
            miso: f.miso.as_slice().to_vec(),
        });
        s.pop_event();
    }
    out
}

fn config(answers: SpiSlaveAnswers) -> SpiSlaveConfig {
    SpiSlaveConfig {
        mode: SpiMode::Mode0,
        bit_order: BitOrder::MsbFirst,
        word_bits: 8,
        io_level: None,
        answers,
    }
}

fn slave(answers: SpiSlaveAnswers) -> Slave {
    let mut s = Slave::new();
    s.configure(&config(answers)).unwrap();
    s
}

fn regs(addr_bytes: u8, rw: SpiRwBit, dummy_bytes: u8, len: u32) -> Slave {
    slave(SpiSlaveAnswers::Registers(SpiRegisters {
        addr_bytes,
        rw,
        dummy_bytes,
        idle: 0xEE,
        len,
    }))
}

/// A frame where each MISO byte is asked for just before its MOSI byte comes in: a slave without a FIFO, or one
/// that keeps a single byte ahead.
fn frame(s: &mut Slave, mosi: &[u8]) -> Vec<u8> {
    s.cs_asserted(1);
    let miso = mosi
        .iter()
        .map(|&b| {
            let out = s.miso();
            s.mosi(b);
            out
        })
        .collect();
    s.cs_released(2);
    miso
}

/// A frame with `depth` bytes loaded into a TX FIFO ahead, before CS goes active.
fn fifo_frame(s: &mut Slave, mosi: &[u8], depth: usize) -> Vec<u8> {
    let mut fifo: std::collections::VecDeque<u8> = (0..depth).map(|_| s.miso()).collect();
    s.cs_asserted(1);
    let mut miso = vec![];
    for &b in mosi {
        miso.push(fifo.pop_front().unwrap());
        s.mosi(b);
        fifo.push_back(s.miso());
    }
    s.cs_released(2);
    miso
}

#[test]
fn queue_one_answer_per_frame_idle_for_rest() {
    let mut s = slave(SpiSlaveAnswers::Queue {
        idle: 0x5A,
        repeat_last: false,
    });
    s.push_answer(&[1, 2]).unwrap();
    s.push_answer(&[3, 4, 5]).unwrap();
    assert_eq!(frame(&mut s, &[0xA0, 0xA1, 0xA2]), vec![1, 2, 0x5A]);
    // shorter frame: the rest is dropped
    assert_eq!(frame(&mut s, &[0; 2]), vec![3, 4]);
    // none queued: idle, underrun
    assert_eq!(frame(&mut s, &[0; 2]), vec![0x5A, 0x5A]);
    let f = frames(&mut s);
    assert_eq!(
        f[0],
        Frame {
            start: 1,
            end: 2,
            truncated: false,
            overrun: false,
            underrun: false,
            addr: None,
            mosi: vec![0xA0, 0xA1, 0xA2],
            miso: vec![1, 2, 0x5A],
        }
    );
    assert!(!f[1].underrun);
    assert!(f[2].underrun);
    assert_eq!(s.free_answer_slots(), 3);
}

#[test]
fn queue_repeat_last() {
    let mut s = slave(SpiSlaveAnswers::Queue {
        idle: 0,
        repeat_last: true,
    });
    // nothing ever queued: idle
    assert_eq!(frame(&mut s, &[0; 2]), vec![0, 0]);
    s.push_answer(&[7, 8]).unwrap();
    assert_eq!(frame(&mut s, &[0; 2]), vec![7, 8]);
    // kept to repeat: takes its slot
    assert_eq!(s.free_answer_slots(), 2);
    assert_eq!(frame(&mut s, &[0; 3]), vec![7, 8, 0]);
    s.push_answer(&[9]).unwrap();
    assert_eq!(frame(&mut s, &[0; 1]), vec![9]);
    assert_eq!(s.free_answer_slots(), 2);
    let u: Vec<bool> = frames(&mut s).iter().map(|f| f.underrun).collect();
    assert_eq!(u, vec![true, false, true, false]);
}

#[test]
fn queue_answer_picked_at_preload_and_rewind() {
    let mut s = slave(SpiSlaveAnswers::Queue {
        idle: 0xFF,
        repeat_last: false,
    });
    // the FIFO was loaded with idle before the answer came: it goes to the next frame
    let first = s.miso();
    assert_eq!(first, 0xFF);
    s.push_answer(&[1, 2]).unwrap();
    s.cs_asserted(1);
    s.mosi(0);
    s.cs_released(2);
    assert_eq!(fifo_frame(&mut s, &[0, 0], 2), vec![1, 2]);
    // a firmware that can empty its FIFO rewinds and the new answer goes out in this frame
    assert_eq!(s.miso(), 0xFF);
    s.push_answer(&[3]).unwrap();
    s.rewind();
    assert_eq!(frame(&mut s, &[0]), vec![3]);
    // rewinding a frame with its answer picked puts the answer back
    s.push_answer(&[4]).unwrap();
    assert_eq!(s.miso(), 4);
    s.rewind();
    assert_eq!(s.answers_waiting(), 1);
    assert_eq!(frame(&mut s, &[0]), vec![4]);
    let f = frames(&mut s);
    assert_eq!(f.len(), 4);
    assert!(f[0].underrun);
    assert_eq!(f[1].miso, vec![1, 2]);
    assert_eq!(f[2].miso, vec![3]);
}

#[test]
fn registers_read_and_write_with_rw_bit() {
    // a typical sensor: bit 7 set reads, 7-bit address
    let mut s = regs(1, SpiRwBit::SetIsRead { bit: 7 }, 0, 64);
    s.map_write(0x10, &[1, 2, 3]).unwrap();
    assert_eq!(frame(&mut s, &[0x90, 0, 0, 0]), vec![0xEE, 1, 2, 3]);
    // write frame: data stored, idle sent
    assert_eq!(frame(&mut s, &[0x20, 9, 8]), vec![0xEE; 3]);
    assert_eq!(&s.map()[0x20..0x22], &[9, 8]);
    let f = frames(&mut s);
    assert_eq!(f[0].addr, Some(0x10));
    assert_eq!(f[1].addr, Some(0x20));
    assert_eq!(f[1].mosi, vec![0x20, 9, 8]);
    // clear is read
    let mut s = regs(1, SpiRwBit::ClearIsRead { bit: 0 }, 0, 64);
    s.map_write(2, &[5]).unwrap();
    // 0x02: bit 0 clear, read address 2
    assert_eq!(frame(&mut s, &[0x02, 0]), vec![0xEE, 5]);
    // 0x05: bit 0 set, write address 4
    frame(&mut s, &[0x05, 6]);
    assert_eq!(s.map()[4], 6);
}

#[test]
fn registers_two_address_bytes_dummy_and_wrap() {
    // SPI flash-like: 2 address bytes, read only, 1 dummy byte; address wraps at len
    let mut s = regs(2, SpiRwBit::ReadOnly, 1, 40);
    s.map_write(38, &[0xA, 0xB]).unwrap();
    s.map_write(0, &[0xC]).unwrap();
    // 0x0126 = 294 % 40 = 14; 0x0026 = 38
    assert_eq!(
        frame(&mut s, &[0x00, 0x26, 0, 0, 0, 0]),
        vec![0xEE, 0xEE, 0xEE, 0xA, 0xB, 0xC]
    );
    // the master's data bytes of a read-only map are ignored
    frame(&mut s, &[0x01, 0x26, 0, 0x77]);
    assert_eq!(s.map()[14], 0);
    assert_eq!(frames(&mut s)[1].addr, Some(14));
}

#[test]
fn registers_address_too_late_for_a_fifo() {
    // 1 address byte, no dummy: a FIFO loaded 2 ahead had to send idle for the first data byte, which counts as
    // gone by, so the second data byte is the map's second
    let mut s = regs(1, SpiRwBit::ReadOnly, 0, 64);
    s.map_write(0, &[1, 2, 3, 4]).unwrap();
    assert_eq!(fifo_frame(&mut s, &[0, 0, 0, 0], 2), vec![0xEE, 0xEE, 2, 3]);
    // with dummy bytes covering the FIFO depth, every byte comes out right
    let mut s = regs(1, SpiRwBit::ReadOnly, 2, 64);
    s.map_write(0, &[1, 2, 3, 4]).unwrap();
    assert_eq!(
        fifo_frame(&mut s, &[0, 0, 0, 0, 0], 2),
        vec![0xEE, 0xEE, 0xEE, 1, 2]
    );
    // what the device sent in the record: only bytes clocked out, not those left in the FIFO
    assert_eq!(
        frames(&mut s).last().unwrap().miso,
        vec![0xEE, 0xEE, 0xEE, 1, 2]
    );
}

#[test]
fn registers_no_address_bytes() {
    let mut s = regs(0, SpiRwBit::ReadOnly, 0, 3);
    s.map_write(0, &[1, 2, 3]).unwrap();
    assert_eq!(frame(&mut s, &[0; 4]), vec![1, 2, 3, 1]);
    assert_eq!(frames(&mut s)[0].addr, Some(0));
}

#[test]
fn host_update_between_frames() {
    let mut s = regs(1, SpiRwBit::ReadOnly, 0, 64);
    s.map_write(0, &[1, 1]).unwrap();
    s.cs_asserted(0);
    s.miso();
    s.mosi(0);
    assert!(!s.is_idle());
    s.map_write(0, &[2, 2]).unwrap();
    assert_eq!(s.miso(), 1);
    s.mosi(0);
    assert_eq!(s.miso(), 1);
    s.mosi(0);
    s.cs_released(0);
    assert!(s.is_idle());
    assert_eq!(&s.map()[..2], &[2, 2]);
    // a frame loaded ahead counts as going on
    s.miso();
    assert!(!s.is_idle());
    assert_eq!(s.map_write(0, &[0; 27]), Err(MapError::OutOfRange));
}

#[test]
fn truncated_overrun_dropped() {
    let mut s = slave(SpiSlaveAnswers::Queue {
        idle: 0,
        repeat_last: false,
    });
    s.cs_asserted(0);
    for _ in 0..20 {
        s.miso();
        s.mosi(1);
    }
    s.overrun();
    s.cs_released(0);
    let f = frames(&mut s);
    assert!(f[0].truncated && f[0].overrun);
    assert_eq!((f[0].mosi.len(), f[0].miso.len()), (16, 16));
    for _ in 0..10 {
        frame(&mut s, &[0]);
    }
    assert_eq!(frames(&mut s).len(), 8);
    assert_eq!(s.take_dropped(), 2);
}

#[test]
fn config_errors_and_off() {
    let mut s = Slave::new();
    assert_eq!(s.miso(), 0xFF);
    s.cs_asserted(0);
    s.mosi(0);
    s.cs_released(0);
    assert!(s.peek_event().is_none());
    let bad = |answers, word_bits| {
        let mut c = config(answers);
        c.word_bits = word_bits;
        Slave::new().configure(&c)
    };
    let r = |addr_bytes, rw, len| {
        SpiSlaveAnswers::Registers(SpiRegisters {
            addr_bytes,
            rw,
            dummy_bytes: 0,
            idle: 0,
            len,
        })
    };
    let q = SpiSlaveAnswers::Queue {
        idle: 0,
        repeat_last: false,
    };
    assert_eq!(bad(q, 16), Err(ConfigError::Unsupported));
    assert_eq!(
        bad(r(5, SpiRwBit::ReadOnly, 8), 8),
        Err(ConfigError::Unsupported)
    );
    assert_eq!(
        bad(r(1, SpiRwBit::ReadOnly, 65), 8),
        Err(ConfigError::BufferTooBig)
    );
    assert!(matches!(
        bad(r(1, SpiRwBit::SetIsRead { bit: 8 }, 8), 8),
        Err(ConfigError::InvalidArgument(_))
    ));
    assert!(matches!(
        bad(r(0, SpiRwBit::SetIsRead { bit: 7 }, 8), 8),
        Err(ConfigError::InvalidArgument(_))
    ));
    let no_queue = SpiSlave::<8, 0, 0, 0, 1, 1>::new().configure(&config(q));
    assert_eq!(no_queue, Err(ConfigError::Unsupported));
    assert_eq!(
        ww_spi::SpiError::from(ConfigError::BufferTooBig),
        ww_spi::SpiError::BufferTooBig
    );
}
