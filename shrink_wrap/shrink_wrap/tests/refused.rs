//! A `validate` hook that returns `Refused` (SW-76): the read fails with `SubtypeOutOfRange` as with a plain
//! `Error`, and the reader keeps the value and the rule, also for a value nested in another one, read latched, in a
//! list or a `Series`.

use shrink_wrap::prelude::*;
use shrink_wrap::refused::{ReadError, Refused, RefusedNum, Rule};

/// 0..=100, transparent: read latched as a field.
#[derive_shrink_wrap(owned, transparent, validate = Percent::validate)]
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct Percent(u8);

impl Percent {
    fn validate(&self) -> Result<(), Refused> {
        if self.0 <= 100 {
            Ok(())
        } else {
            Err(Refused::out_of(
                RefusedNum::Int(self.0.into()),
                RefusedNum::Int(0),
                RefusedNum::Int(100),
            ))
        }
    }
}

/// A float that is never NaN, not transparent: read in a split reader when it is a field.
#[derive_shrink_wrap(owned, validate = NotNan::validate)]
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct NotNan {
    pub value: f32,
}

impl NotNan {
    fn validate(&self) -> Result<(), Refused> {
        if self.value.is_nan() {
            Err(Refused::new(
                RefusedNum::Float(self.value.into()),
                Rule::NotNan,
            ))
        } else {
            Ok(())
        }
    }
}

/// A hook that keeps returning a plain `Error`.
#[derive_shrink_wrap(owned, transparent, validate = Even::validate)]
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct Even(u8);

impl Even {
    fn validate(&self) -> Result<(), shrink_wrap::Error> {
        if self.0.is_multiple_of(2) {
            Ok(())
        } else {
            Err(shrink_wrap::Error::SubtypeOutOfRange)
        }
    }
}

#[derive_shrink_wrap(owned)]
#[derive(Debug, PartialEq, Clone)]
pub struct OuterOwned {
    pub flag: bool,
    pub percent: Percent,
    pub reading: NotNan,
    pub even: Even,
    pub list: Vec<Percent>,
}

fn outer() -> OuterOwned {
    OuterOwned {
        flag: true,
        percent: Percent(50),
        reading: NotNan { value: 1.5 },
        even: Even(2),
        list: vec![Percent(1), Percent(2)],
    }
}

fn bytes(v: &OuterOwned) -> Vec<u8> {
    v.to_ww_bytes_owned().unwrap()
}

fn range(value: i128) -> Option<Refused> {
    Some(Refused::out_of(
        RefusedNum::Int(value),
        RefusedNum::Int(0),
        RefusedNum::Int(100),
    ))
}

#[test]
fn a_latched_field() {
    let mut v = outer();
    v.percent = Percent(101);
    let err = OuterOwned::from_ww_bytes_explained(&bytes(&v)).unwrap_err();
    assert_eq!(
        err,
        ReadError {
            error: shrink_wrap::Error::SubtypeOutOfRange,
            refused: range(101)
        }
    );
    assert_eq!(err.to_string(), "101 is out of 0..=100");
    assert_eq!(
        OuterOwned::from_ww_bytes_owned(&bytes(&v)),
        Err(shrink_wrap::Error::SubtypeOutOfRange)
    );
}

#[test]
fn a_field_in_its_own_reader() {
    let mut v = outer();
    v.reading.value = f32::NAN;
    let err = OuterOwned::from_ww_bytes_explained(&bytes(&v)).unwrap_err();
    assert_eq!(err.error, shrink_wrap::Error::SubtypeOutOfRange);
    let refused = err.refused.unwrap();
    assert_eq!(refused.rule, Rule::NotNan);
    assert!(matches!(refused.value, RefusedNum::Float(x) if x.is_nan()));
}

#[test]
fn an_item_of_a_list() {
    let mut v = outer();
    v.list[1] = Percent(200);
    let err = OuterOwned::from_ww_bytes_explained(&bytes(&v)).unwrap_err();
    assert_eq!(err.refused, range(200));
}

#[test]
fn a_plain_error_has_no_numbers() {
    let mut v = outer();
    v.even = Even(3);
    let err = OuterOwned::from_ww_bytes_explained(&bytes(&v)).unwrap_err();
    assert_eq!(
        err,
        ReadError {
            error: shrink_wrap::Error::SubtypeOutOfRange,
            refused: None
        }
    );
}

#[test]
fn another_error_has_no_numbers() {
    let mut v = outer();
    v.percent = Percent(101);
    let b = bytes(&v);
    // cut short: the read fails on a missing byte, whatever was refused before
    let err = OuterOwned::from_ww_bytes_explained(&b[..1]).unwrap_err();
    assert_ne!(err.error, shrink_wrap::Error::SubtypeOutOfRange);
    assert_eq!(err.refused, None);
}

#[test]
fn a_good_value_reads() {
    let v = outer();
    assert_eq!(OuterOwned::from_ww_bytes_explained(&bytes(&v)), Ok(v));
}

/// A hook that decodes something of its own, with a reader of its own: what that inner read refuses stays out of
/// the outer read's slot, which gets only what the hook itself returns.
#[derive_shrink_wrap(owned, transparent, validate = Wrapped::validate)]
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct Wrapped(u8);

impl Wrapped {
    fn validate(&self) -> Result<(), shrink_wrap::Error> {
        // the byte read as a Percent, through a reader made inside the hook
        Percent::from_ww_bytes_owned(&[self.0]).map(|_| ())
    }
}

#[test]
fn a_nested_read_inside_a_hook_stays_its_own() {
    let err = Wrapped::from_ww_bytes_explained(&[150]).unwrap_err();
    assert_eq!(
        err,
        ReadError {
            error: shrink_wrap::Error::SubtypeOutOfRange,
            refused: None
        }
    );
}

/// Two reads that alternate on one thread, each with its own slot: neither sees the other's refusal.
#[test]
fn two_interleaved_readers() {
    use shrink_wrap::refused::Slot;
    let bad = [101u8];
    let good = [7u8];
    let (slot_a, slot_b) = (Slot::new(), Slot::new());
    let mut a = BufReader::new(&bad).with_refusal_slot(&slot_a);
    let mut b = BufReader::new(&good).with_refusal_slot(&slot_b);
    let mut c = BufReader::new(&bad);
    assert_eq!(
        Percent::des_shrink_wrap_owned(&mut a),
        Err(shrink_wrap::Error::SubtypeOutOfRange)
    );
    assert_eq!(Percent::des_shrink_wrap_owned(&mut b), Ok(Percent(7)));
    // a reader without a slot keeps nothing anywhere
    assert!(Percent::des_shrink_wrap_owned(&mut c).is_err());
    assert_eq!(slot_b.take(), None);
    assert_eq!(slot_a.take(), range(101));
}

/// The slot is a pointer more in a reader, so a `RefVec` (which holds a reader) grows by no more.
#[test]
fn a_reader_grows_by_a_pointer() {
    assert_eq!(
        size_of::<Option<&shrink_wrap::refused::Slot>>(),
        size_of::<usize>()
    );
    // 40 B on a 64-bit host without the slot (`Option<Error>` is 4 B: `Error` starts at 1), 48 B with it
    assert!(
        size_of::<BufReader>() <= 6 * size_of::<usize>(),
        "{}",
        size_of::<BufReader>()
    );
}

/// A reader with a slot stays `Send` and `Sync`, so a `RefVec` in a `static` still builds.
#[test]
fn a_reader_stays_send_and_sync() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<BufReader<'static>>();
    send_sync::<shrink_wrap::refused::Slot>();
}

/// 1..=5000 ms, transparent: a bounded `Fixed`.
#[derive_shrink_wrap(owned, transparent, validate = Timeout::validate)]
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct Timeout(u16);

impl Timeout {
    fn validate(&self) -> Result<(), Refused> {
        if (1..=5000).contains(&self.0) {
            Ok(())
        } else {
            Err(Refused::out_of(
                RefusedNum::Int(self.0.into()),
                RefusedNum::Int(1),
                RefusedNum::Int(5000),
            ))
        }
    }
}

/// Counts from 1, transparent: an `IndexFrom1`, which refuses the 0 a latched read gives after an error.
#[derive_shrink_wrap(owned, transparent, validate = Channel::validate)]
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct Channel(u8);

impl Channel {
    fn validate(&self) -> Result<(), Refused> {
        if self.0 == 0 {
            Err(Refused::new(RefusedNum::Int(0), Rule::NotZero))
        } else {
            Ok(())
        }
    }
}

#[derive_shrink_wrap(owned)]
#[derive(Debug, PartialEq, Clone)]
pub struct Config {
    pub timeout: Timeout,
    pub channel: Channel,
}

/// The first refusal of a latched read is the one explained: the 0 the next field reads after it, refused by that
/// field's hook too, doesn't replace it.
#[test]
fn the_first_refusal_wins() {
    let b = Config {
        timeout: Timeout(6000),
        channel: Channel(2),
    }
    .to_ww_bytes_owned()
    .unwrap();
    let err = Config::from_ww_bytes_explained(&b).unwrap_err();
    assert_eq!(err.error, shrink_wrap::Error::SubtypeOutOfRange);
    assert_eq!(err.to_string(), "6000 is out of 1..=5000");
}

#[derive_shrink_wrap(owned)]
#[derive(Debug, PartialEq, Clone)]
pub struct EvenThenChannel {
    pub even: Even,
    pub channel: Channel,
}

/// A first refusal without numbers stays without them: the next field's refusal of the latched 0 doesn't lend its
/// numbers to the error of the first.
#[test]
fn a_first_plain_refusal_keeps_no_numbers() {
    let b = EvenThenChannel {
        even: Even(3),
        channel: Channel(2),
    }
    .to_ww_bytes_owned()
    .unwrap();
    let err = EvenThenChannel::from_ww_bytes_explained(&b).unwrap_err();
    assert_eq!(
        err,
        ReadError {
            error: shrink_wrap::Error::SubtypeOutOfRange,
            refused: None
        }
    );
}
