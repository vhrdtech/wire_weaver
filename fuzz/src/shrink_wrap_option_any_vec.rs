#![no_main]

//! `OptionAnyVec` (SW-55): random bytes into the reader never panic and the present ids increase; any present set
//! written in any mode reads back the same, and the picked mode is never longer than another one.
//!
//! `sparse` structs on top of it (DER-13): any value round-trips through the borrowed and the owned side with the
//! same bytes, also nested and as a field of an ordinary struct; an older reader (fewer fields) reads the fields it
//! knows, a reader with a gap where the writer has a field reads it or fails with `OptionAnyVecUnknownSlot`; random
//! bytes into every reader never panic, and when borrowed and owned both read them they agree.

use arbitrary::{Arbitrary, Unstructured};
use libfuzzer_sys::fuzz_target;
use shrink_wrap::option_any_vec::{OptionAnyVec, OptionAnyVecWriter, PresenceMode, SetBits};
use shrink_wrap::prelude::*;

#[derive_shrink_wrap(
    sparse,
    owned,
    derive(Debug, PartialEq, Clone, Default),
    derive_owned(Arbitrary)
)]
struct Child<'i> {
    a: Option<u8>,
    #[slot = 9]
    s: Option<&'i str>,
}

#[derive_shrink_wrap(
    sparse,
    owned,
    derive(Debug, PartialEq, Clone, Default),
    derive_owned(Arbitrary)
)]
struct Level<'i> {
    speed: Option<u16>,
    name: Option<&'i str>,
    #[slot = 5]
    samples: Option<RefVec<'i, i16>>,
    on: Option<bool>,
    child: Option<Child<'i>>,
    #[slot = 40]
    far: Option<u64>,
}

/// `Level` as an older version knew it: the first fields only
#[derive_shrink_wrap(sparse, borrowed, derive(Debug, PartialEq))]
struct LevelV1<'i> {
    speed: Option<u16>,
    name: Option<&'i str>,
}

/// `Level` with a gap where it has `on` (slot 6)
#[derive_shrink_wrap(sparse, borrowed, derive(Debug, PartialEq))]
struct LevelGap<'i> {
    speed: Option<u16>,
    #[slot = 7]
    child: Option<Child<'i>>,
}

#[derive_shrink_wrap(owned, derive(Debug, PartialEq))]
struct EnvelopeOwned {
    seq: u8,
    level: LevelOwned,
    after: String,
}

fn sparse_round_trip(level: LevelOwned, seq: u8, after: String) {
    let bytes = level.to_ww_bytes_owned().unwrap();
    assert_eq!(LevelOwned::from_ww_bytes_owned(&bytes).unwrap(), level);
    let borrowed = Level::from_ww_bytes(&bytes).unwrap();
    let mut buf = vec![0u8; bytes.len() + 64];
    assert_eq!(borrowed.to_ww_bytes(&mut buf).unwrap(), bytes);

    let v1 = LevelV1::from_ww_bytes(&bytes).unwrap();
    assert_eq!((v1.speed, v1.name), (borrowed.speed, borrowed.name));
    // the slots in the gap reader's gap: 1, 5 and 6
    let in_gap = level.name.is_some() || level.samples.is_some() || level.on.is_some();
    match LevelGap::from_ww_bytes(&bytes) {
        Ok(gap) => {
            assert!(!in_gap);
            assert_eq!((gap.speed, gap.child), (borrowed.speed, borrowed.child));
        }
        Err(e) => {
            assert!(in_gap);
            assert_eq!(e, ShrinkWrapError::OptionAnyVecUnknownSlot);
        }
    }

    let env = EnvelopeOwned { seq, level, after };
    let bytes = env.to_ww_bytes_owned().unwrap();
    assert_eq!(EnvelopeOwned::from_ww_bytes_owned(&bytes).unwrap(), env);
}

fn sparse_read_any(data: &[u8]) {
    let borrowed = Level::from_ww_bytes(data);
    let owned = LevelOwned::from_ww_bytes_owned(data);
    if let (Ok(borrowed), Ok(owned)) = (&borrowed, &owned) {
        let mut buf = vec![0u8; data.len() * 2 + 64];
        assert_eq!(
            borrowed.to_ww_bytes(&mut buf).unwrap(),
            owned.to_ww_bytes_owned().unwrap()
        );
    }
    let _ = LevelV1::from_ww_bytes(data);
    let _ = LevelGap::from_ww_bytes(data);
    let _ = EnvelopeOwned::from_ww_bytes_owned(data);
}

const MODES: [PresenceMode; 4] = [
    PresenceMode::All,
    PresenceMode::Dense,
    PresenceMode::TwoLevel,
    PresenceMode::IndexList,
];

fn read_any(data: &[u8]) {
    let Ok(vec) = OptionAnyVec::new(data) else {
        return;
    };
    let mut prev = None;
    for id in vec.present().take(1 << 16) {
        assert!(prev.is_none_or(|p| id > p));
        prev = Some(id);
    }
    let mut iter = vec.iter();
    for i in 0..256u32 {
        let _ = match i % 4 {
            0 => iter.next::<u8>().map(|_| ()),
            1 => iter.next::<bool>().map(|_| ()),
            2 => iter.next::<&str>().map(|_| ()),
            _ => iter.next::<OptionAnyVec>().map(|_| ()),
        };
    }
}

fn round_trip(words: &[u32]) {
    let set: Vec<u32> = SetBits::new(words).collect();
    let mut sizes = vec![];
    for mode in [None].into_iter().chain(MODES.map(Some)) {
        let mut buf = [0u8; 2048];
        let wr = match mode {
            Some(mode) => OptionAnyVecWriter::new_with_mode(&mut buf, set.iter().copied(), mode),
            None => OptionAnyVecWriter::new(&mut buf, set.iter().copied()),
        };
        let Ok(mut wr) = wr else {
            // only All refuses a set, the one that isn't 0..n
            assert_eq!(mode, Some(PresenceMode::All));
            assert!(set.iter().enumerate().any(|(i, s)| i as u32 != *s));
            continue;
        };
        for &slot in &set {
            wr.write(slot, &(slot as u8)).unwrap();
        }
        let bytes = wr.finish_and_take().unwrap();
        sizes.push(bytes.len());
        let vec = OptionAnyVec::new(bytes).unwrap();
        assert_eq!(vec.present().collect::<Vec<_>>(), set);
        let mut iter = vec.iter();
        for slot in 0..(words.len() as u32 * 32 + 8) {
            let expected = set.contains(&slot).then_some(slot as u8);
            assert_eq!(iter.next::<u8>(), Ok(expected));
        }
    }
    // the header of the picked mode has the fewest bits, so its bytes are the fewest too
    assert!(sizes.iter().all(|s| *s >= sizes[0]));
}

fuzz_target!(|data: &[u8]| {
    let Some((&op, rest)) = data.split_first() else {
        return;
    };
    match op & 3 {
        0 => read_any(rest),
        1 => round_trip_words(rest),
        2 => sparse_read_any(rest),
        _ => {
            let mut u = Unstructured::new(rest);
            let (Ok(level), Ok(seq), Ok(after)) = (
                LevelOwned::arbitrary(&mut u),
                u8::arbitrary(&mut u),
                String::arbitrary(&mut u),
            ) else {
                return;
            };
            sparse_round_trip(level, seq, after);
        }
    }
});

fn round_trip_words(rest: &[u8]) {
    let words: Vec<u32> = rest
        .chunks(4)
        .take(32)
        .map(|c| c.iter().rev().fold(0, |w, b| w << 8 | *b as u32))
        .collect();
    round_trip(&words);
}
