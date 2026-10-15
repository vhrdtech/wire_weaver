//! `Store` on its own: erasing ahead with `maintain`, a full store, a range holding something else, the key limit.

use embassy_futures::block_on;
use ww_settings::fake_flash::FakeFlash;
use ww_settings::{Key, Store, StoreError};

type G0 = FakeFlash<8, 2048>;
const BASE: u32 = 0x0807_8000;

fn store<const K: usize>(flash: G0, pages: u32) -> Store<G0, 256, K> {
    Store::new(flash, BASE..BASE + pages * 2048).unwrap()
}

async fn get<const K: usize>(st: &mut Store<G0, 256, K>, key: Key) -> Option<Vec<u8>> {
    let mut buf = [0u8; 1024];
    st.fetch(key, &mut buf)
        .await
        .unwrap()
        .map(|n| buf[..n].to_vec())
}

#[test]
fn config_is_checked() {
    use ww_settings::ConfigError;
    let r = Store::<G0, 256>::new(G0::new(BASE, 1), BASE..BASE + 2048);
    assert_eq!(r.err(), Some(ConfigError::TooFewPages));
    let r = Store::<G0, 256>::new(G0::new(BASE, 2), BASE + 8..BASE + 4096);
    assert_eq!(r.err(), Some(ConfigError::NotAligned));
    let r = Store::<G0, 20>::new(G0::new(BASE, 2), BASE..BASE + 4096);
    assert_eq!(r.err(), Some(ConfigError::BufferTooSmall));
}

#[test]
fn maintain_erases_ahead_so_a_write_never_does() {
    block_on(async {
        let mut st = store::<8>(G0::new(BASE, 2), 2);
        // 40-byte records: 52-byte items, 56 with padding, 36 per page
        let mut erases_in_writes = 0;
        for i in 0..200u32 {
            if st.erase_pending().await.unwrap() {
                assert!(st.maintain().await.unwrap());
                assert!(!st.erase_pending().await.unwrap());
            }
            let before = st.flash().stats().page_erases;
            st.store(Key::USER, &[i as u8; 40]).await.unwrap();
            erases_in_writes += st.flash().stats().page_erases - before;
        }
        assert_eq!(erases_in_writes, 0);
        assert!(st.flash().stats().page_erases >= 4);
        assert_eq!(get(&mut st, Key::USER).await, Some(vec![199; 40]));
    });
}

#[test]
fn without_maintain_the_write_erases() {
    block_on(async {
        let mut st = store::<8>(G0::new(BASE, 2), 2);
        for i in 0..200u32 {
            st.store(Key::USER, &[i as u8; 40]).await.unwrap();
        }
        assert!(st.flash().stats().page_erases >= 4);
        let flash = st.into_flash();
        let mut st = store::<8>(G0::from_image(BASE, flash.image()), 2);
        assert_eq!(get(&mut st, Key::USER).await, Some(vec![199; 40]));
    });
}

#[test]
fn full_store_keeps_what_it_has() {
    block_on(async {
        let mut st = store::<8>(G0::new(BASE, 2), 2);
        // a page holds 2032 bytes of items: 3 records of 600 take 1836, a 4th doesn't fit with them
        for k in 0..3 {
            st.store(Key(k), &[k; 600]).await.unwrap();
        }
        let r = st.store(Key(3), &[3; 600]).await;
        assert!(matches!(r, Err(StoreError::Full)), "{r:?}");
        // writes that replace a record still work, by compacting
        for i in 0..10 {
            st.store(Key(1), &[i; 600]).await.unwrap();
        }
        let flash = st.into_flash();
        let mut st = store::<8>(G0::from_image(BASE, flash.image()), 2);
        assert_eq!(get(&mut st, Key(0)).await, Some(vec![0; 600]));
        assert_eq!(get(&mut st, Key(1)).await, Some(vec![9; 600]));
        assert_eq!(get(&mut st, Key(2)).await, Some(vec![2; 600]));
        assert_eq!(get(&mut st, Key(3)).await, None);
    });
}

#[test]
fn a_range_of_something_else_reads_empty_and_is_taken_over() {
    block_on(async {
        // what an old firmware image looks like to the store
        let image: Vec<u8> = (0..4 * 2048u32)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
            .collect();
        let mut st = store::<8>(G0::from_image(BASE, &image), 4);
        assert_eq!(get(&mut st, Key::USER).await, None);
        st.store(Key::USER, b"hello").await.unwrap();
        let flash = st.into_flash();
        let mut st = store::<8>(G0::from_image(BASE, flash.image()), 4);
        assert_eq!(get(&mut st, Key::USER).await, Some(b"hello".to_vec()));
    });
}

#[test]
fn key_limit() {
    block_on(async {
        let mut st = store::<2>(G0::new(BASE, 2), 2);
        st.store(Key(0), b"a").await.unwrap();
        st.store(Key(1), b"b").await.unwrap();
        assert!(matches!(
            st.store(Key(2), b"c").await,
            Err(StoreError::TooManyKeys)
        ));
        st.store(Key(1), b"bb").await.unwrap();
        assert_eq!(get(&mut st, Key(1)).await, Some(b"bb".to_vec()));
    });
}

#[test]
fn loading_never_writes() {
    block_on(async {
        let mut st = store::<8>(G0::new(BASE, 2), 2);
        for i in 0..100u32 {
            st.store(Key::USER, &i.to_le_bytes()).await.unwrap();
        }
        let flash = st.into_flash();
        let mut f = G0::from_image(BASE, flash.image());
        // any write or erase would be cut at once and fail
        f.cut_after(1);
        let mut st = store::<8>(f, 2);
        assert_eq!(
            get(&mut st, Key::USER).await,
            Some(99u32.to_le_bytes().to_vec())
        );
        assert!(!st.flash().is_off());
    });
}
