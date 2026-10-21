use crate::sim::{self, BASE, SimFlash};
use crate::{EccSafeFlash, EccStrictFlash, Error, ecc_errors};
use embedded_storage::nor_flash::{NorFlash, ReadNorFlash};
use std::sync::Mutex;

/// `ecc_errors()` is one counter for all threads: tests that break words run one at a time.
static COUNTER: Mutex<()> = Mutex::new(());

const SIZE: usize = 1024;

fn pattern() -> [u8; SIZE] {
    core::array::from_fn(|i| i as u8)
}

/// A flash of `SIZE` bytes holding `pattern()`, with the words at `broken` (offsets) failing their ECC check.
fn flash<const WORD: usize>(broken: &[u32]) -> SimFlash<WORD> {
    sim::reset(SIZE);
    let mut f = SimFlash::<WORD>;
    f.write(0, &pattern()).unwrap();
    for &o in broken {
        sim::break_word(BASE + o);
    }
    f
}

#[test]
fn reads_like_the_flash_at_any_offset_and_length() {
    let expected = pattern();
    let mut zeros = unsafe { EccSafeFlash::new(flash::<32>(&[]), BASE) };
    let mut strict = unsafe { EccStrictFlash::new_strict(SimFlash::<32>, BASE) };
    for (offset, len) in [
        (0, SIZE),
        (1, 0),
        (3, 1),
        (5, 70),
        (31, 2),
        (32, 32),
        (100, 500),
        (SIZE - 1, 1),
    ] {
        let mut buf = std::vec![0xAA; len];
        zeros.read(offset as u32, &mut buf).unwrap();
        assert_eq!(buf, expected[offset..offset + len]);
        buf.fill(0xAA);
        strict.read(offset as u32, &mut buf).unwrap();
        assert_eq!(buf, expected[offset..offset + len]);
    }
}

#[test]
fn out_of_bounds() {
    let mut f = unsafe { EccStrictFlash::new_strict(flash::<8>(&[]), BASE) };
    let mut buf = [0; 2];
    assert_eq!(f.read(SIZE as u32 - 1, &mut buf), Err(Error::OutOfBounds));
}

#[test]
fn zeros_mode_reads_a_broken_word_as_zeros() {
    let _c = COUNTER.lock().unwrap();
    let mut f = unsafe { EccSafeFlash::new(flash::<32>(&[64, 128]), BASE) };
    let before = ecc_errors();
    let mut buf = [0xAA; 200];
    f.read(40, &mut buf).unwrap();
    let mut expected = pattern();
    expected[64..96].fill(0);
    expected[128..160].fill(0);
    assert_eq!(buf, expected[40..240]);
    assert_eq!(ecc_errors() - before, 2);
}

#[test]
fn strict_mode_fails_at_the_first_broken_word() {
    let _c = COUNTER.lock().unwrap();
    let mut f = unsafe { EccStrictFlash::new_strict(flash::<32>(&[64, 128]), BASE) };
    let before = ecc_errors();
    let mut buf = [0xAA; 200];
    let e = f.read(40, &mut buf).unwrap_err();
    assert_eq!(e, Error::Ecc { offset: 64 });
    assert!(e.is_ecc());
    // the bytes before the broken word are read
    assert_eq!(buf[..24], pattern()[40..64]);
    assert_eq!(ecc_errors() - before, 1);
    // one byte of the broken word is enough, a read that ends before it is fine
    assert_eq!(f.read(95, &mut buf[..1]), Err(Error::Ecc { offset: 64 }));
    f.read(0, &mut buf[..64]).unwrap();
    f.read(96, &mut buf[..32]).unwrap();
    // an erase heals the word
    f.erase(0, 256).unwrap();
    f.read(0, &mut buf).unwrap();
    assert_eq!(buf, [0xFF; 200]);
}

#[test]
fn strict_mode_with_8_byte_words() {
    let _c = COUNTER.lock().unwrap();
    let mut f = unsafe { EccStrictFlash::new_strict(flash::<8>(&[16]), BASE) };
    let mut buf = [0; 8];
    assert_eq!(f.read(20, &mut buf), Err(Error::Ecc { offset: 16 }));
    f.read(24, &mut buf).unwrap();
    assert_eq!(buf, pattern()[24..32]);
}

#[test]
fn async_read_follows_the_mode() {
    use embedded_storage_async::nor_flash::ReadNorFlash as AsyncRead;
    let _c = COUNTER.lock().unwrap();
    let mut strict = unsafe { EccStrictFlash::new_strict(flash::<16>(&[32]), BASE) };
    let mut buf = [0xAA; 16];
    let r = block_on(AsyncRead::read(&mut strict, 32, &mut buf));
    assert_eq!(r, Err(Error::Ecc { offset: 32 }));
    let mut zeros = unsafe { EccSafeFlash::new(SimFlash::<16>, BASE) };
    block_on(AsyncRead::read(&mut zeros, 32, &mut buf)).unwrap();
    assert_eq!(buf, [0; 16]);
}

/// The reads never wait: one poll finishes them.
fn block_on<T>(f: impl Future<Output = T>) -> T {
    let mut f = core::pin::pin!(f);
    match f.as_mut().poll(&mut core::task::Context::from_waker(
        core::task::Waker::noop(),
    )) {
        core::task::Poll::Ready(v) => v,
        core::task::Poll::Pending => unreachable!(),
    }
}
