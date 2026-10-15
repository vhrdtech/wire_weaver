//! `sparse` structs (DER-13): every field an `Option`, written as an `OptionAnyVec` slot, only the present ones take
//! space. Round trips, nesting, explicit slots with gaps, readers and writers of different versions.

use shrink_wrap::prelude::*;

#[derive_shrink_wrap(sparse, owned, derive(Debug, PartialEq, Clone, Default))]
struct Motor<'i> {
    speed: Option<u16>,
    name: Option<&'i str>,
    #[slot = 5]
    samples: Option<RefVec<'i, i16>>,
    on: Option<bool>,
    limits: Option<Limits>,
}

#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone))]
struct Limits {
    min: u8,
    max: u8,
}

/// A trait level with a child level: the child is one slot.
#[derive_shrink_wrap(sparse, owned, derive(Debug, PartialEq, Clone, Default))]
struct Root<'i> {
    t: Option<u32>,
    #[slot = 3]
    motor: Option<Motor<'i>>,
    label: Option<&'i str>,
}

fn motor_all() -> Motor<'static> {
    Motor {
        speed: Some(1500),
        name: Some("left"),
        samples: Some(RefVec::Slice {
            slice: &[-1, 0, 1000],
        }),
        on: Some(true),
        limits: Some(Limits { min: 1, max: 200 }),
    }
}

fn round_trip(motor: &Motor<'_>) -> Vec<u8> {
    let mut buf = [0u8; 256];
    let bytes = motor.to_ww_bytes(&mut buf).unwrap();
    let back = Motor::from_ww_bytes(bytes).unwrap();
    assert_eq!(&back, motor);
    bytes.to_vec()
}

#[test]
fn round_trips_borrowed() {
    let all = motor_all();
    round_trip(&all);
    assert!(
        round_trip(&Motor::default()).is_empty(),
        "nothing present: no bytes"
    );
    // every subset of the five fields
    for mask in 0u8..32 {
        let m = Motor {
            speed: all.speed.filter(|_| mask & 1 != 0),
            name: all.name.filter(|_| mask & 2 != 0),
            samples: all.samples.filter(|_| mask & 4 != 0),
            on: all.on.filter(|_| mask & 8 != 0),
            limits: all.limits.clone().filter(|_| mask & 16 != 0),
        };
        round_trip(&m);
    }
}

#[test]
fn only_the_present_slots_take_space() {
    let speed = Motor {
        speed: Some(0x1234),
        ..Default::default()
    };
    // all mode, 1 slot (`0 1`), then the u16 little-endian
    assert_eq!(round_trip(&speed), [0x01, 0x34, 0x12]);
    let on = Motor {
        on: Some(true),
        ..Default::default()
    };
    // `on` is slot 6, after the explicit slot 5: index list (`3`), one entry (`1`), delta 6 (`6`), then the bool
    assert_eq!(round_trip(&on), [0x31, 0x68]);
}

#[test]
fn owned_writes_the_same_bytes() {
    let all = motor_all();
    let mut buf = [0u8; 256];
    let bytes = all.to_ww_bytes(&mut buf).unwrap();
    let owned = MotorOwned {
        speed: Some(1500),
        name: Some("left".into()),
        samples: Some(vec![-1, 0, 1000]),
        on: Some(true),
        limits: Some(Limits { min: 1, max: 200 }),
    };
    assert_eq!(owned.to_ww_bytes_owned().unwrap(), bytes);
    assert_eq!(MotorOwned::from_ww_bytes_owned(bytes).unwrap(), owned);
    let partial = MotorOwned {
        name: Some("r".into()),
        limits: Some(Limits { min: 0, max: 9 }),
        ..Default::default()
    };
    let bytes = partial.to_ww_bytes_owned().unwrap();
    assert_eq!(MotorOwned::from_ww_bytes_owned(&bytes).unwrap(), partial);
    let borrowed = Motor::from_ww_bytes(&bytes).unwrap();
    assert_eq!(borrowed.name, Some("r"));
    assert_eq!(borrowed.speed, None);
}

#[test]
fn nested_sparse_is_one_slot() {
    let root = Root {
        t: Some(77),
        motor: Some(Motor {
            name: Some("m"),
            on: Some(false),
            ..Default::default()
        }),
        label: Some("root"),
    };
    let mut buf = [0u8; 256];
    let bytes = root.to_ww_bytes(&mut buf).unwrap();
    assert_eq!(Root::from_ww_bytes(bytes).unwrap(), root);
    let owned = RootOwned::from_ww_bytes_owned(bytes).unwrap();
    assert_eq!(owned.motor.as_ref().unwrap().name.as_deref(), Some("m"));
    assert_eq!(owned.to_ww_bytes_owned().unwrap(), bytes);

    // a present child with nothing present in it, and no child at all
    for motor in [Some(Motor::default()), None] {
        let root = Root {
            motor,
            label: Some("x"),
            ..Default::default()
        };
        let bytes = root.to_ww_bytes(&mut buf).unwrap();
        assert_eq!(Root::from_ww_bytes(bytes).unwrap(), root);
    }
}

/// An outer, ordinary struct with a sparse field between other fields.
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
struct Envelope<'i> {
    seq: u8,
    update: Motor<'i>,
    after: u16,
    tail: &'i str,
}

#[test]
fn inside_an_outer_struct_with_garbage_after() {
    let env = Envelope {
        seq: 9,
        update: Motor {
            speed: Some(3),
            samples: Some(RefVec::Slice { slice: &[5, 6] }),
            ..Default::default()
        },
        after: 0xBEEF,
        tail: "end",
    };
    let mut buf = [0u8; 128];
    let valid = env.to_ww_bytes(&mut buf).unwrap().to_vec();
    assert_eq!(Envelope::from_ww_bytes(&valid).unwrap(), env);

    // the envelope as a field of something larger, garbage after it
    let mut wr = BufWriterOwned::new();
    wr.write_rev_len(valid.len()).unwrap();
    wr.write_raw_slice(&valid).unwrap();
    wr.write_raw_slice(&[0xFF, 0x00, 0xA5, 0x77]).unwrap();
    let outer = wr.finish_and_take().unwrap();
    let mut rd = BufReader::new(&outer);
    let size = rd.read_rev_len().unwrap();
    let mut inner = rd.split(size).unwrap();
    assert_eq!(Envelope::des_shrink_wrap(&mut inner).unwrap(), env);
    assert_eq!(rd.read_raw_slice(4), Ok(&[0xFF, 0x00, 0xA5, 0x77][..]));
}

mod v1 {
    use shrink_wrap::prelude::*;

    #[derive_shrink_wrap(sparse, derive(Debug, PartialEq, Default))]
    pub struct Level<'i> {
        pub a: Option<u8>,
        pub b: Option<&'i str>,
    }
}

mod v2 {
    use shrink_wrap::prelude::*;

    /// `v1::Level` with fields appended, one at a later slot
    #[derive_shrink_wrap(sparse, derive(Debug, PartialEq, Default))]
    pub struct Level<'i> {
        pub a: Option<u8>,
        pub b: Option<&'i str>,
        pub c: Option<RefVec<'i, u8>>,
        #[slot = 40]
        pub d: Option<u32>,
    }
}

#[test]
fn old_reader_new_data() {
    let mut buf = [0u8; 64];
    let new = v2::Level {
        a: Some(1),
        b: Some("two"),
        c: Some(RefVec::Slice { slice: &[3, 3, 3] }),
        d: Some(4),
    };
    let bytes = new.to_ww_bytes(&mut buf).unwrap();
    let old = v1::Level::from_ww_bytes(bytes).unwrap();
    assert_eq!(
        old,
        v1::Level {
            a: Some(1),
            b: Some("two")
        }
    );
    // only the new fields present: nothing for the old reader
    let new = v2::Level {
        d: Some(4),
        ..Default::default()
    };
    let bytes = new.to_ww_bytes(&mut buf).unwrap();
    assert_eq!(
        v1::Level::from_ww_bytes(bytes).unwrap(),
        v1::Level::default()
    );
}

#[test]
fn new_reader_old_data() {
    let mut buf = [0u8; 64];
    let old = v1::Level {
        a: None,
        b: Some("bee"),
    };
    let bytes = old.to_ww_bytes(&mut buf).unwrap();
    let new = v2::Level::from_ww_bytes(bytes).unwrap();
    assert_eq!(
        new,
        v2::Level {
            b: Some("bee"),
            ..Default::default()
        }
    );
}

/// A reader that has a gap where the writer has a field: the writer's value can't be skipped.
#[derive_shrink_wrap(sparse, borrowed, derive(Debug, PartialEq, Default))]
struct Gapped {
    a: Option<u8>,
    #[slot = 2]
    c: Option<u8>,
}

#[derive_shrink_wrap(sparse, borrowed, derive(Debug, PartialEq, Default))]
struct Filled {
    a: Option<u8>,
    b: Option<u8>,
    c: Option<u8>,
}

#[test]
fn present_slot_in_a_gap_is_an_error() {
    let mut buf = [0u8; 16];
    // absent in the gap: fine
    let bytes = Filled {
        a: Some(1),
        b: None,
        c: Some(3),
    }
    .to_ww_bytes(&mut buf)
    .unwrap();
    assert_eq!(
        Gapped::from_ww_bytes(bytes).unwrap(),
        Gapped {
            a: Some(1),
            c: Some(3)
        }
    );
    let bytes = Filled {
        a: Some(1),
        b: Some(2),
        c: Some(3),
    }
    .to_ww_bytes(&mut buf)
    .unwrap();
    assert_eq!(
        Gapped::from_ww_bytes(bytes),
        Err(ShrinkWrapError::OptionAnyVecUnknownSlot)
    );
    // present after the reader's last slot only: ignored
    let bytes = Gapped {
        a: Some(1),
        c: Some(3),
    }
    .to_ww_bytes(&mut buf)
    .unwrap();
    assert_eq!(
        Filled::from_ww_bytes(bytes).unwrap(),
        Filled {
            a: Some(1),
            b: None,
            c: Some(3)
        }
    );
}

#[test]
fn max_size_bounds_every_value() {
    #[derive_shrink_wrap(sparse, borrowed, derive(Debug, PartialEq, Default))]
    struct Bounded {
        a: Option<u8>,
        #[slot = 20]
        b: Option<u32>,
        c: Option<Limits>,
    }
    const MAX: usize = MaxSize::buffer_len_of(<Bounded as SerializeShrinkWrap>::MAX_SIZE).unwrap();
    let all = Bounded {
        a: Some(u8::MAX),
        b: Some(u32::MAX),
        c: Some(Limits { min: 1, max: 2 }),
    };
    let mut buf = [0u8; MAX];
    let bytes = all.to_ww_bytes(&mut buf).unwrap();
    assert_eq!(Bounded::from_ww_bytes(bytes).unwrap(), all);
}

#[test]
fn malformed_data_is_an_error_not_a_panic() {
    let all = motor_all();
    let mut buf = [0u8; 256];
    let valid = all.to_ww_bytes(&mut buf).unwrap().to_vec();
    for i in 0..valid.len() {
        for xor in [0x01, 0x10, 0x80, 0xFF] {
            let mut bytes = valid.clone();
            bytes[i] ^= xor;
            let _ = Motor::from_ww_bytes(&bytes);
            let _ = MotorOwned::from_ww_bytes_owned(&bytes);
        }
        let _ = Motor::from_ww_bytes(&valid[..i]);
        let _ = Motor::from_ww_bytes(&valid[i..]);
    }
}
