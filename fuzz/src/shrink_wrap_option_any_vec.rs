#![no_main]

//! `OptionAnyVec` (SW-55): random bytes into the reader never panic and the present ids increase; any present set
//! written in any mode reads back the same, and the picked mode is never longer than another one.

use libfuzzer_sys::fuzz_target;
use shrink_wrap::option_any_vec::{OptionAnyVec, OptionAnyVecWriter, PresenceMode, SetBits};

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
    if op & 1 == 0 {
        read_any(rest);
    } else {
        let words: Vec<u32> = rest
            .chunks(4)
            .take(32)
            .map(|c| c.iter().rev().fold(0, |w, b| w << 8 | *b as u32))
            .collect();
        round_trip(&words);
    }
});
