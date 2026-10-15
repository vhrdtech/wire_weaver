//! `ww_settings::dump`: the raw walk and the cross-check against a real `Store` over the same bytes (TOOL-12).

use embassy_futures::block_on;
use ww_settings::dump::{DumpError, ItemState, analyze};
use ww_settings::fake_flash::FakeFlash;
use ww_settings::{Key, Store};

type G0 = FakeFlash<8, 2048>;
const BASE: u32 = 0x0807_8000;

fn store<const K: usize>(flash: G0, pages: u32) -> Store<G0, 256, K> {
    Store::new(flash, BASE..BASE + pages * 2048).unwrap()
}

#[test]
fn unsupported_sizes_are_rejected() {
    let image = vec![0xffu8; 4096];
    assert!(matches!(
        analyze(&image, BASE, 3, 2048),
        Err(DumpError::UnsupportedWordSize(3))
    ));
    assert!(matches!(
        analyze(&image, BASE, 8, 3000),
        Err(DumpError::UnsupportedPageSize(3000))
    ));
    assert!(matches!(
        analyze(&image[..100], BASE, 8, 2048),
        Err(DumpError::BadImageLen { len: 100, .. })
    ));
    // one page only: Store always wants 2+
    assert!(matches!(
        analyze(&image[..2048], BASE, 8, 2048),
        Err(DumpError::BadImageLen { len: 2048, .. })
    ));
}

#[test]
fn matches_what_a_real_store_reads() {
    block_on(async {
        let mut st = store::<8>(G0::new(BASE, 2), 2);
        for i in 0..150u32 {
            st.store(Key::USER, &[i as u8; 40]).await.unwrap();
            st.store(Key::FACTORY, &[!(i as u8); 12]).await.unwrap();
        }
        // enough writes on a 2-page store to force several rollovers, leaving garbage behind
        assert!(st.flash().stats().page_erases >= 2);
        let image = st.flash().image().to_vec();

        let dump = analyze(&image, BASE, 8, 2048).unwrap();
        assert_eq!(dump.pages.len(), 2);
        assert_eq!(dump.pages.iter().filter(|p| p.is_current).count(), 1);
        assert!(dump.store_mismatch.is_empty());

        let user = dump.current.iter().find(|r| r.key == Key::USER.0).unwrap();
        assert_eq!(user.bytes, [149u8; 40]);
        let factory = dump
            .current
            .iter()
            .find(|r| r.key == Key::FACTORY.0)
            .unwrap();
        assert_eq!(factory.bytes, [!149u8; 12]);

        // some free space should show up as Erased, and the superseded records as stale Valid items
        // (Store never erases a page that still holds one, but old pages do get erased on rollover)
        assert!(dump.items.iter().any(|i| i.state == ItemState::Erased));
        assert!(
            dump.items
                .iter()
                .filter(|i| i.state == ItemState::Valid)
                .count()
                >= 2
        );
    });
}

#[test]
fn a_cut_item_write_is_reported_as_cut_not_valid() {
    block_on(async {
        let mut st = store::<8>(G0::new(BASE, 2), 2);
        st.store(Key::USER, &[0xAB; 16]).await.unwrap();
        let mut image = st.flash().image().to_vec();

        // flip a data byte: breaks the item's crc32 without touching its header. Page header rounds to 16
        // bytes (8-byte word), the item header is 12 bytes, so the data starts at byte 28.
        image[16 + 12 + 10] ^= 0xff;

        let dump = analyze(&image, BASE, 8, 2048).unwrap();
        assert!(dump.current.is_empty());
        let item = dump
            .items
            .iter()
            .find(|i| i.state != ItemState::Erased)
            .expect("the corrupted item slot");
        assert_eq!(item.state, ItemState::Cut);
        assert!(dump.store_mismatch.is_empty());
    });
}

#[test]
fn an_invalid_page_header_is_shown_but_not_current() {
    block_on(async {
        let mut st = store::<8>(G0::new(BASE, 2), 2);
        st.store(Key::USER, &[0xCD; 16]).await.unwrap();
        let mut image = st.flash().image().to_vec();
        // corrupt the page_seq byte: breaks the header's own crc32
        image[4] ^= 0xff;

        let dump = analyze(&image, BASE, 8, 2048).unwrap();
        assert!(!dump.pages[0].valid_header);
        assert!(dump.pages[0].page_seq.is_none());
        // the item itself still parses fine, it's just on a page Store would never look at
        let item = dump
            .items
            .iter()
            .find(|i| i.key == Some(Key::USER.0))
            .unwrap();
        assert_eq!(item.state, ItemState::Valid);
        assert!(!item.is_current);
        assert!(dump.current.is_empty());
    });
}
