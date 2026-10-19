//! Every policy of the I2C slave engine, driven byte by byte as a firmware's interrupt handler would.

use ww_bus_slave::{ConfigError, I2cSlave, MapError, QueueError, ReadByte};
use ww_i2c::{
    I2cAddress, I2cRecordKind, I2cSlaveAnswers, I2cSlaveConfig, I2cSlaveEvent, I2cSlaveUnderrun,
};
use ww_si::{Prefix, Second};

type Slave = I2cSlave<64, 32, 3, 8, 8, 16>;

#[derive(Debug, PartialEq)]
enum Ev {
    Waiting(u32),
    T {
        start: u32,
        end: u32,
        read: bool,
        kind: I2cRecordKind,
        pointer: Option<u32>,
        bytes: Vec<u8>,
        filled: u16,
        stretched: Option<u32>,
    },
}

fn us(s: Second) -> u32 {
    s.to_u32_in(Prefix::Micro).unwrap()
}

fn events(s: &mut Slave) -> Vec<Ev> {
    let mut out = vec![];
    while let Some((t, e)) = s.peek_event() {
        out.push(match e {
            I2cSlaveEvent::ReadWaiting => Ev::Waiting(us(t)),
            I2cSlaveEvent::Transfer(tr) => Ev::T {
                start: us(t),
                end: us(tr.end),
                read: tr.read,
                kind: tr.kind,
                pointer: tr.pointer,
                bytes: tr.bytes.as_slice().to_vec(),
                filled: tr.filled,
                stretched: tr.stretched.map(us),
            },
            I2cSlaveEvent::Unknown(_) => panic!(),
        });
        s.pop_event();
    }
    out
}

/// Only the bytes of the transfers.
fn bytes(s: &mut Slave) -> Vec<(bool, Vec<u8>)> {
    events(s)
        .into_iter()
        .filter_map(|e| match e {
            Ev::T { read, bytes, .. } => Some((read, bytes)),
            Ev::Waiting(_) => None,
        })
        .collect()
}

fn config(answers: I2cSlaveAnswers) -> I2cSlaveConfig {
    I2cSlaveConfig {
        addr: I2cAddress::seven(0x50).unwrap(),
        io_level: None,
        answers,
    }
}

fn registers(pointer_bytes: u8, len: u32, page: Option<u16>, read_only: bool) -> Slave {
    let mut s = Slave::new();
    s.configure(&config(I2cSlaveAnswers::Registers {
        pointer_bytes,
        len,
        page,
        read_only,
    }))
    .unwrap();
    s
}

fn queue(empty: I2cSlaveUnderrun) -> Slave {
    let mut s = Slave::new();
    s.configure(&config(I2cSlaveAnswers::Queue { empty }))
        .unwrap();
    s
}

fn write(s: &mut Slave, data: &[u8]) {
    assert!(s.address(false, 0));
    for &b in data {
        s.write(b, 0);
    }
    s.stop(0);
}

fn read(s: &mut Slave, n: usize) -> Vec<u8> {
    assert!(s.address(true, 0));
    let out = (0..n)
        .map(|_| match s.read(0) {
            ReadByte::Byte(b) => b,
            ReadByte::Stretch => panic!("stretch"),
        })
        .collect();
    s.stop(0);
    out
}

/// Pointer write, repeated start, read: the usual register read.
fn write_read(s: &mut Slave, pointer: &[u8], n: usize) -> Vec<u8> {
    assert!(s.address(false, 0));
    for &b in pointer {
        s.write(b, 0);
    }
    assert!(s.address(true, 0));
    let out = (0..n)
        .map(|_| match s.read(0) {
            ReadByte::Byte(b) => b,
            ReadByte::Stretch => panic!("stretch"),
        })
        .collect();
    s.stop(0);
    out
}

#[test]
fn registers_pointer_write_and_read_back() {
    let mut s = registers(1, 64, None, false);
    write(&mut s, &[0x10, 1, 2, 3]);
    assert_eq!(&s.map()[0x10..0x13], &[1, 2, 3]);
    assert_eq!(write_read(&mut s, &[0x10], 3), vec![1, 2, 3]);
    // current-address read continues from where the last one ended
    assert_eq!(read(&mut s, 1), vec![0]);
    let ev = events(&mut s);
    assert_eq!(ev.len(), 4);
    assert!(
        matches!(&ev[0], Ev::T { read: false, pointer: Some(0x10), bytes, kind: I2cRecordKind::Complete, .. } if bytes == &[0x10, 1, 2, 3])
    );
    assert!(matches!(
        &ev[1],
        Ev::T {
            read: false,
            pointer: Some(0x10),
            kind: I2cRecordKind::RepeatedStart,
            ..
        }
    ));
    assert!(
        matches!(&ev[2], Ev::T { read: true, pointer: Some(0x10), bytes, kind: I2cRecordKind::Complete, .. } if bytes == &[1, 2, 3])
    );
    assert!(matches!(
        &ev[3],
        Ev::T {
            read: true,
            pointer: Some(0x13),
            ..
        }
    ));
}

#[test]
fn registers_two_pointer_bytes_big_endian_and_wrap_at_len() {
    let mut s = registers(2, 40, None, false);
    // 0x0126 = 294 wraps to 294 % 40 = 14
    write(&mut s, &[0x01, 0x26, 7]);
    assert_eq!(s.map()[14], 7);
    // writes and reads wrap at len, not at the map size
    write(&mut s, &[0x00, 39, 0xA, 0xB]);
    assert_eq!((s.map()[39], s.map()[0]), (0xA, 0xB));
    assert_eq!(write_read(&mut s, &[0, 39], 2), vec![0xA, 0xB]);
}

#[test]
fn registers_without_pointer_bytes_move_by_access() {
    let mut s = registers(0, 4, None, false);
    write(&mut s, &[1, 2, 3]);
    assert_eq!(read(&mut s, 3), vec![0, 1, 2]);
    assert_eq!(&s.map()[..4], &[1, 2, 3, 0]);
}

#[test]
fn registers_page_wrap_on_writes_only() {
    let mut s = registers(1, 64, Some(8), false);
    // starts at 6 in page 0..8: 6, 7, then back to 0, 1
    write(&mut s, &[6, 0xA, 0xB, 0xC, 0xD]);
    assert_eq!(&s.map()[..8], &[0xC, 0xD, 0, 0, 0, 0, 0xA, 0xB]);
    assert_eq!(s.map()[8], 0);
    // reads cross the page
    assert_eq!(write_read(&mut s, &[7], 2), vec![0xB, 0]);
    // a write without pointer bytes after a read starts its page where the pointer is now (9: page 8..16)
    let mut s = registers(0, 64, Some(8), false);
    read(&mut s, 15);
    write(&mut s, &[1, 2]);
    assert_eq!((s.map()[15], s.map()[8]), (1, 2));
}

#[test]
fn registers_read_only_acks_but_keeps_map() {
    let mut s = registers(1, 64, None, true);
    s.map_write(5, &[9]).unwrap();
    write(&mut s, &[5, 1, 2]);
    assert_eq!(s.map()[5], 9);
    // the pointer still moved by the data bytes
    assert_eq!(read(&mut s, 1), vec![0]);
    let ev = bytes(&mut s);
    assert_eq!(ev[0], (false, vec![5, 1, 2]));
}

#[test]
fn host_update_applied_between_transactions_not_inside() {
    let mut s = registers(1, 64, None, false);
    s.map_write(0, &[1, 1]).unwrap();
    // the master reads a 2-byte value; the host updates it between the two bytes
    assert!(s.address(false, 0));
    s.write(0, 0);
    assert!(s.address(true, 0));
    assert_eq!(s.read(0), ReadByte::Byte(1));
    assert!(!s.is_idle());
    s.map_write(0, &[2, 2]).unwrap();
    assert_eq!(s.map_read(0, &mut [0; 2]), Ok(()));
    assert_eq!(s.read(0), ReadByte::Byte(1));
    s.stop(0);
    assert!(s.is_idle());
    assert_eq!(&s.map()[..2], &[2, 2]);
    // idle: at once
    s.map_write(2, &[3]).unwrap();
    assert_eq!(s.map()[2], 3);
}

#[test]
fn staged_updates_in_order_and_busy_when_full() {
    let mut s = registers(1, 64, None, false);
    assert!(s.address(false, 0));
    // 32 bytes of staging: 6 + 10 twice, then no room
    s.map_write(0, &[1; 10]).unwrap();
    s.map_write(5, &[2; 10]).unwrap();
    assert_eq!(s.map_write(0, &[3; 1]), Err(MapError::Busy));
    s.stop(0);
    assert_eq!(
        &s.map()[..15],
        &[1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2]
    );
    assert_eq!(s.map_write(60, &[0; 5]), Err(MapError::OutOfRange));
    assert_eq!(s.map_write(0, &[0; 30]), Ok(()));
    assert!(s.address(false, 0));
    assert_eq!(s.map_write(0, &[0; 30]), Err(MapError::OutOfRange));
}

#[test]
fn fifo_preload_taken_back() {
    let mut s = registers(1, 64, None, false);
    s.map_write(0, &[1, 2, 3, 4, 5, 6]).unwrap();
    assert!(s.address(true, 0));
    for _ in 0..4 {
        s.read(0);
    }
    // the master took 2 of the 4 loaded
    s.unsent(2);
    s.stop(0);
    assert_eq!(read(&mut s, 1), vec![3]);
    assert_eq!(bytes(&mut s)[0], (true, vec![1, 2]));
}

#[test]
fn queue_answers_one_per_read_fill_for_longer_rest_dropped() {
    let mut s = queue(I2cSlaveUnderrun::Fill(0xEE));
    s.push_answer(&[1, 2, 3]).unwrap();
    s.push_answer(&[4, 5]).unwrap();
    assert_eq!(s.free_answer_slots(), 1);
    // shorter read: the rest of the answer is dropped
    assert_eq!(read(&mut s, 2), vec![1, 2]);
    // longer read: fill
    assert_eq!(read(&mut s, 4), vec![4, 5, 0xEE, 0xEE]);
    // empty: fill only
    assert_eq!(read(&mut s, 2), vec![0xEE, 0xEE]);
    assert_eq!(s.free_answer_slots(), 3);
    let ev = events(&mut s);
    assert!(matches!(
        &ev[1],
        Ev::T {
            filled: 2,
            pointer: None,
            ..
        }
    ));
    assert!(matches!(&ev[2], Ev::T { filled: 2, .. }));
    // writes don't take answers
    s.push_answer(&[7]).unwrap();
    write(&mut s, &[0x42]);
    assert_eq!(read(&mut s, 1), vec![7]);
}

#[test]
fn queue_full_and_too_long_and_clear() {
    let mut s = queue(I2cSlaveUnderrun::Fill(0xEE));
    assert_eq!(s.push_answer(&[0; 9]), Err(QueueError::TooLong));
    for i in 0..3 {
        s.push_answer(&[i]).unwrap();
    }
    assert_eq!(s.push_answer(&[3]), Err(QueueError::Full));
    // the answer being sent stays when the queue is cleared
    assert!(s.address(true, 0));
    assert_eq!(s.read(0), ReadByte::Byte(0));
    assert_eq!(s.clear_answers(), 2);
    assert_eq!(s.free_answer_slots(), 2);
    s.stop(0);
    assert_eq!(s.free_answer_slots(), 3);
    assert_eq!(read(&mut s, 1), vec![0xEE]);
}

#[test]
fn queue_stretch_until_answer() {
    let mut s = queue(I2cSlaveUnderrun::Stretch {
        max: Second::milli(5u16),
        fill: 0xFF,
    });
    // a command, then a read with nothing queued
    assert!(s.address(false, 100));
    s.write(0x30, 101);
    assert!(s.address(true, 110));
    assert_eq!(s.read(120), ReadByte::Stretch);
    assert!(s.is_stretching());
    assert_eq!(s.stretch_deadline(), Some(5120));
    assert_eq!(s.read(121), ReadByte::Stretch);
    // the host sees the command, then ReadWaiting, before the read ends
    let ev = events(&mut s);
    assert!(matches!(
        &ev[0],
        Ev::T {
            start: 100,
            end: 110,
            read: false,
            kind: I2cRecordKind::RepeatedStart,
            ..
        }
    ));
    assert_eq!(ev[1], Ev::Waiting(120));
    assert_eq!(ev.len(), 2);
    assert_eq!(s.resume(2000), None);
    s.push_answer(&[0xA1, 0xA2]).unwrap();
    assert_eq!(s.resume(3120), Some(0xA1));
    assert!(!s.is_stretching());
    assert_eq!(s.read(3200), ReadByte::Byte(0xA2));
    assert_eq!(s.read(3300), ReadByte::Byte(0xFF));
    s.stop(3400);
    assert_eq!(
        events(&mut s),
        vec![Ev::T {
            start: 110,
            end: 3400,
            read: true,
            kind: I2cRecordKind::Complete,
            pointer: None,
            bytes: vec![0xA1, 0xA2, 0xFF],
            filled: 1,
            stretched: Some(3000),
        }]
    );
}

#[test]
fn queue_stretch_deadline_then_late_answer_goes_to_next_read() {
    let mut s = queue(I2cSlaveUnderrun::Stretch {
        max: Second::micro(1000u32),
        fill: 0xEE,
    });
    assert!(s.address(true, 0));
    assert_eq!(s.read(0), ReadByte::Stretch);
    assert_eq!(s.resume(999), None);
    assert_eq!(s.resume(1000), Some(0xEE));
    // the rest of this read is fill even when the answer comes now
    s.push_answer(&[5]).unwrap();
    assert_eq!(s.read(1100), ReadByte::Byte(0xEE));
    s.stop(1200);
    assert_eq!(read(&mut s, 1), vec![5]);
    let ev = events(&mut s);
    assert_eq!(ev[0], Ev::Waiting(0));
    assert!(matches!(
        &ev[1],
        Ev::T {
            filled: 2,
            stretched: Some(1000),
            ..
        }
    ));
    assert!(matches!(
        &ev[2],
        Ev::T {
            filled: 0,
            stretched: None,
            ..
        }
    ));
}

#[test]
fn stretch_deadline_across_clock_wrap() {
    let mut s = queue(I2cSlaveUnderrun::Stretch {
        max: Second::micro(100u32),
        fill: 0,
    });
    let t0 = u32::MAX - 10;
    assert!(s.address(true, t0));
    assert_eq!(s.read(t0), ReadByte::Stretch);
    assert_eq!(s.stretch_deadline(), Some(89));
    assert_eq!(s.resume(50), None);
    assert_eq!(s.resume(89), Some(0));
}

#[test]
fn queue_nack_while_empty() {
    let mut s = queue(I2cSlaveUnderrun::Nack);
    assert!(!s.acks_address());
    // writes NACKed too, nothing reported
    assert!(!s.address(false, 0));
    s.stop(0);
    assert!(events(&mut s).is_empty());
    s.push_answer(&[1, 2]).unwrap();
    assert!(s.acks_address());
    write(&mut s, &[9]);
    assert_eq!(read(&mut s, 3), vec![1, 2, 0xFF]);
    assert!(!s.acks_address());
    assert_eq!(
        bytes(&mut s),
        vec![(false, vec![9]), (true, vec![1, 2, 0xFF])]
    );
}

#[test]
fn direction_seen_from_bytes_without_address_calls() {
    // DW_apb_i2c reports bytes and a stop, no address match for writes
    let mut s = registers(1, 64, None, false);
    s.map_write(3, &[0x33]).unwrap();
    s.write(3, 0);
    assert_eq!(s.read(0), ReadByte::Byte(0x33));
    s.stop(0);
    let ev = events(&mut s);
    assert!(matches!(
        &ev[0],
        Ev::T {
            read: false,
            kind: I2cRecordKind::RepeatedStart,
            ..
        }
    ));
    assert!(matches!(
        &ev[1],
        Ev::T {
            read: true,
            kind: I2cRecordKind::Complete,
            ..
        }
    ));
}

#[test]
fn truncated_bus_error_and_dropped() {
    let mut s = registers(1, 64, None, false);
    write(&mut s, &[0; 20]);
    assert!(s.address(false, 0));
    s.write(0, 0);
    s.bus_error(5);
    let ev = events(&mut s);
    assert!(
        matches!(&ev[0], Ev::T { kind: I2cRecordKind::Truncated, bytes, .. } if bytes.len() == 16)
    );
    assert!(matches!(
        &ev[1],
        Ev::T {
            kind: I2cRecordKind::BusError,
            end: 5,
            ..
        }
    ));
    for _ in 0..10 {
        write(&mut s, &[0]);
    }
    assert_eq!(events(&mut s).len(), 8);
    assert_eq!(s.take_dropped(), 2);
    assert_eq!(s.take_dropped(), 0);
}

#[test]
fn off_and_config_errors() {
    let mut s = Slave::new();
    assert!(!s.address(false, 0));
    assert!(!s.acks_address());
    let bad = |answers| Slave::new().configure(&config(answers));
    let regs = |pointer_bytes, len, page| I2cSlaveAnswers::Registers {
        pointer_bytes,
        len,
        page,
        read_only: false,
    };
    assert_eq!(bad(regs(5, 8, None)), Err(ConfigError::Unsupported));
    assert_eq!(bad(regs(1, 65, None)), Err(ConfigError::BufferTooBig));
    assert!(matches!(
        bad(regs(1, 0, None)),
        Err(ConfigError::InvalidArgument(_))
    ));
    assert!(matches!(
        bad(regs(1, 8, Some(0))),
        Err(ConfigError::InvalidArgument(_))
    ));
    let no_map = I2cSlave::<0, 0, 1, 1, 1, 1>::new().configure(&config(regs(1, 8, None)));
    assert_eq!(no_map, Err(ConfigError::Unsupported));
    let no_queue = I2cSlave::<8, 0, 0, 0, 1, 1>::new().configure(&config(I2cSlaveAnswers::Queue {
        empty: I2cSlaveUnderrun::Nack,
    }));
    assert_eq!(no_queue, Err(ConfigError::Unsupported));
    assert_eq!(
        ww_i2c::I2cError::from(ConfigError::BufferTooBig),
        ww_i2c::I2cError::BufferTooBig
    );
    // disabled: the address is not acknowledged, answers dropped, the map kept
    let mut s = registers(1, 64, None, false);
    s.map_write(0, &[7]).unwrap();
    s.disable();
    assert!(!s.address(true, 0));
    assert_eq!(s.map()[0], 7);
}
