//! `Settings` on a `Store`: load, set, write-on-change, debounce, reset, and records written by an older or a
//! newer version of the struct read and written back without losing anything.

use embassy_futures::block_on;
use shrink_wrap::prelude::*;
use ww_settings::fake_flash::FakeFlash;
use ww_settings::record::Record;
use ww_settings::{Debounce, Key, Persisted, Settings, SettingsGroup, Store};

type G0 = FakeFlash<8, 2048>;
const BASE: u32 = 0x0807_8000;
const RANGE: core::ops::Range<u32> = BASE..BASE + 4 * 2048;

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone, Copy), ww_repr = u4)]
pub enum ButtonAction {
    Toggle,
    Cycle,
    Nothing,
}

#[derive(ww_settings::Settings, Clone, Debug, PartialEq)]
pub struct User {
    #[setting(slot = 0, default = 100)]
    pub window_ms: u16,
    #[setting(slot = 1, default = false)]
    pub vbus_at_boot: bool,
    #[setting(slot = 2, default = ButtonAction::Toggle)]
    pub button: ButtonAction,
}

fn store(flash: G0) -> Store<G0, 256> {
    Store::new(flash, RANGE).unwrap()
}

#[test]
fn defaults_when_nothing_stored() {
    block_on(async {
        let mut st = store(G0::new(BASE, 4));
        let mut s: Settings<User, 64> = Settings::new(Key::USER, Debounce::default());
        let report = s.load(&mut st).await.unwrap();
        assert!(!report.found);
        assert_eq!(*s.get(), User::default());
        assert_eq!(s.get().window_ms, 100);
        // nothing changed: nothing to write
        assert!(!s.is_dirty());
        assert_eq!(s.persist(&mut st).await.unwrap(), Persisted::Unchanged);
        assert_eq!(st.flash().stats().word_writes, 0);
    });
}

#[test]
fn set_persist_reload() {
    block_on(async {
        let mut st = store(G0::new(BASE, 4));
        let mut s: Settings<User, 64> = Settings::new(Key::USER, Debounce::default());
        s.load(&mut st).await.unwrap();
        assert!(s.set(0, |u| u.button = ButtonAction::Cycle));
        assert!(
            !s.set(1, |u| u.button = ButtonAction::Cycle),
            "same value is no change"
        );
        assert!(
            matches!(s.persist(&mut st).await.unwrap(), Persisted::Written(st) if st.fields == 1)
        );
        // only the field that isn't at its default is stored
        let rec = Record::parse(s.stored_record()).unwrap();
        let slots: Vec<u32> = rec.blobs().map(|b| b.unwrap().0).collect();
        assert_eq!(slots, [2]);
        // a reboot
        let flash = st.into_flash();
        let mut st = store(G0::from_image(BASE, flash.image()));
        let mut s: Settings<User, 64> = Settings::new(Key::USER, Debounce::default());
        let report = s.load(&mut st).await.unwrap();
        assert!(report.found);
        assert_eq!(report.stats.fields, 1);
        assert_eq!(s.get().button, ButtonAction::Cycle);
        assert_eq!(s.get().window_ms, 100);
    });
}

#[test]
fn write_on_change_only() {
    block_on(async {
        let mut st = store(G0::new(BASE, 4));
        let mut s: Settings<User, 64> = Settings::new(Key::USER, Debounce::default());
        s.load(&mut st).await.unwrap();
        s.set(0, |u| u.window_ms = 250);
        s.persist(&mut st).await.unwrap();
        let writes = st.flash().stats().word_writes;
        // changed and changed back before it was written: the record is the same, nothing written
        s.set(1, |u| u.window_ms = 300);
        s.set(2, |u| u.window_ms = 250);
        assert!(s.is_dirty());
        assert_eq!(s.persist(&mut st).await.unwrap(), Persisted::Unchanged);
        assert!(!s.is_dirty());
        assert_eq!(st.flash().stats().word_writes, writes);
        // back to the default: an empty record is written
        s.set(3, |u| u.window_ms = 100);
        assert!(
            matches!(s.persist(&mut st).await.unwrap(), Persisted::Written(st) if st.fields == 0)
        );
        assert_eq!(s.stored_record().len(), 5);
    });
}

#[test]
fn debounce() {
    block_on(async {
        let mut st = store(G0::new(BASE, 4));
        let deb = Debounce {
            quiet_ms: 1000,
            max_ms: 5000,
        };
        let mut s: Settings<User, 64> = Settings::new(Key::USER, deb);
        s.load(&mut st).await.unwrap();
        assert_eq!(s.due_at(), None);
        s.set(10_000, |u| u.window_ms = 1);
        assert_eq!(s.due_at(), Some(11_000));
        assert_eq!(s.persist_if_due(&mut st, 10_999).await.unwrap(), None);
        // a slider moved every 500 ms: written once max_ms passed since the first change
        for i in 1..20u16 {
            s.set(10_000 + i as u64 * 500, |u| u.window_ms = 1 + i);
        }
        assert_eq!(s.due_at(), Some(15_000));
        assert!(s.persist_if_due(&mut st, 15_000).await.unwrap().is_some());
        assert_eq!(s.due_at(), None);
    });
}

#[test]
fn keys_are_independent_and_reset_keeps_factory() {
    #[derive(ww_settings::Settings, Clone, Debug, PartialEq)]
    pub struct Factory {
        #[setting(slot = 0, default = 0)]
        pub offset_uv: i32,
    }
    block_on(async {
        let mut st = store(G0::new(BASE, 4));
        let mut user: Settings<User, 64> = Settings::new(Key::USER, Debounce::default());
        let mut fac: Settings<Factory, 64> = Settings::new(Key::FACTORY, Debounce::default());
        user.load(&mut st).await.unwrap();
        fac.load(&mut st).await.unwrap();
        user.set(0, |u| u.vbus_at_boot = true);
        fac.set(0, |f| f.offset_uv = -1234);
        user.persist(&mut st).await.unwrap();
        fac.persist(&mut st).await.unwrap();
        user.reset_to_defaults(1);
        assert!(matches!(
            user.persist(&mut st).await.unwrap(),
            Persisted::Written(_)
        ));
        let flash = st.into_flash();
        let mut st = store(G0::from_image(BASE, flash.image()));
        user.load(&mut st).await.unwrap();
        fac.load(&mut st).await.unwrap();
        assert_eq!(*user.get(), User::default());
        assert_eq!(fac.get().offset_uv, -1234);
    });
}

#[test]
fn many_writes_wrap_around_the_pages() {
    block_on(async {
        let mut st = store(G0::new(BASE, 4));
        let mut s: Settings<User, 64> = Settings::new(Key::USER, Debounce::default());
        s.load(&mut st).await.unwrap();
        for i in 0..2000u16 {
            s.set(i as u64, |u| u.window_ms = i);
            s.persist(&mut st).await.unwrap();
        }
        let stats = st.flash().stats();
        let flash = st.into_flash();
        let mut st = store(G0::from_image(BASE, flash.image()));
        s.load(&mut st).await.unwrap();
        assert_eq!(s.get().window_ms, 1999);
        // 2000 records of 3 words: 2032 / 24 = 84 per page, so about 24 page erases
        assert!(
            stats.page_erases >= 20 && stats.page_erases <= 30,
            "{stats:?}"
        );
    });
}

mod evolution {
    use super::*;

    #[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone))]
    pub struct LimitsV1 {
        pub max_ma: u16,
    }

    /// LimitsV1 with a field appended
    #[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone))]
    pub struct LimitsV2 {
        pub max_ma: u16,
        #[default = 0]
        pub max_mv: u16,
    }

    #[derive(ww_settings::Settings, Clone, Debug, PartialEq)]
    pub struct V1 {
        #[setting(slot = 0, default = 100)]
        pub window_ms: u16,
        #[setting(slot = 1, default = false)]
        pub old_flag: bool,
        #[setting(slot = 3, default = LimitsV1 { max_ma: 500 })]
        pub limits: LimitsV1,
    }

    /// V1 with `old_flag` removed, a field in the gap at slot 2, `limits` evolved, a field appended.
    #[derive(ww_settings::Settings, Clone, Debug, PartialEq)]
    #[settings(retired(1))]
    pub struct V2 {
        #[setting(slot = 0, default = 100)]
        pub window_ms: u16,
        #[setting(slot = 2, default = 7)]
        pub gap_field: u32,
        #[setting(slot = 3, default = LimitsV2 { max_ma: 500, max_mv: 0 })]
        pub limits: LimitsV2,
        #[setting(slot = 4, default = [0; 3])]
        pub levels: [u8; 3],
    }

    fn reboot(st: Store<G0, 256>) -> Store<G0, 256> {
        let flash = st.into_flash();
        store(G0::from_image(BASE, flash.image()))
    }

    #[test]
    fn old_record_read_by_newer_struct_and_back() {
        block_on(async {
            let mut st = store(G0::new(BASE, 4));

            // V1 firmware
            let mut v1: Settings<V1, 64> = Settings::new(Key::USER, Debounce::default());
            v1.load(&mut st).await.unwrap();
            v1.set(0, |s| {
                s.window_ms = 5;
                s.old_flag = true;
                s.limits.max_ma = 1500;
            });
            v1.persist(&mut st).await.unwrap();

            // upgrade to V2: the retired slot is skipped, `limits` reads with its new field at its default
            let mut st = reboot(st);
            let mut v2: Settings<V2, 64> = Settings::new(Key::USER, Debounce::default());
            let report = v2.load(&mut st).await.unwrap();
            assert_eq!(report.stats.fields, 2);
            assert_eq!(report.stats.retired, 1);
            assert_eq!(v2.get().window_ms, 5);
            assert_eq!(
                v2.get().limits,
                LimitsV2 {
                    max_ma: 1500,
                    max_mv: 0
                }
            );
            assert_eq!(v2.get().gap_field, 7);
            v2.set(0, |s| {
                s.gap_field = 42;
                s.limits.max_mv = 5000;
                s.levels = [1, 2, 3];
            });
            assert!(matches!(
                v2.persist(&mut st).await.unwrap(),
                Persisted::Written(w) if w.retired_dropped == 1 && w.fields == 4
            ));

            // downgrade to V1: slots 2 and 4 are unknown, `limits` reads its first field
            let mut st = reboot(st);
            let mut v1: Settings<V1, 64> = Settings::new(Key::USER, Debounce::default());
            let report = v1.load(&mut st).await.unwrap();
            assert_eq!(report.stats.unknown, 2);
            assert_eq!(v1.get().window_ms, 5);
            assert!(!v1.get().old_flag, "dropped by V2");
            assert_eq!(v1.get().limits.max_ma, 1500);
            // V1 changes something else and writes: the unknown slots and V2's `limits` bytes are kept
            v1.set(0, |s| s.window_ms = 6);
            assert!(matches!(
                v1.persist(&mut st).await.unwrap(),
                Persisted::Written(w) if w.unknown_kept == 2
            ));

            // and up to V2 again: nothing lost
            let mut st = reboot(st);
            let mut v2: Settings<V2, 64> = Settings::new(Key::USER, Debounce::default());
            v2.load(&mut st).await.unwrap();
            assert_eq!(
                *v2.get(),
                V2 {
                    window_ms: 6,
                    gap_field: 42,
                    limits: LimitsV2 {
                        max_ma: 1500,
                        max_mv: 5000
                    },
                    levels: [1, 2, 3],
                }
            );
        });
    }

    #[test]
    fn changed_field_of_newer_type_drops_its_newer_part() {
        block_on(async {
            let mut st = store(G0::new(BASE, 4));
            let mut v2: Settings<V2, 64> = Settings::new(Key::USER, Debounce::default());
            v2.load(&mut st).await.unwrap();
            v2.set(0, |s| {
                s.limits = LimitsV2 {
                    max_ma: 900,
                    max_mv: 3300,
                }
            });
            v2.persist(&mut st).await.unwrap();
            let mut st = reboot(st);
            let mut v1: Settings<V1, 64> = Settings::new(Key::USER, Debounce::default());
            v1.load(&mut st).await.unwrap();
            // V1 changes `limits` itself: it can only write what it knows, max_mv goes back to its default
            v1.set(0, |s| s.limits.max_ma = 1000);
            v1.persist(&mut st).await.unwrap();
            let mut st = reboot(st);
            v2.load(&mut st).await.unwrap();
            assert_eq!(
                v2.get().limits,
                LimitsV2 {
                    max_ma: 1000,
                    max_mv: 0
                }
            );
        });
    }

    #[test]
    fn unknown_format_is_left_alone() {
        block_on(async {
            let mut st = store(G0::new(BASE, 4));
            st.store(Key::USER, &[2, 0xaa, 0xbb]).await.unwrap();
            let mut v2: Settings<V2, 64> = Settings::new(Key::USER, Debounce::default());
            let report = v2.load(&mut st).await.unwrap();
            assert!(report.unreadable.is_some());
            assert!(v2.is_read_only());
            v2.set(0, |s| s.window_ms = 1);
            assert!(matches!(
                v2.persist(&mut st).await,
                Err(ww_settings::Error::ReadOnly)
            ));
            // a factory reset overwrites it
            v2.reset_to_defaults(1);
            v2.persist(&mut st).await.unwrap();
            let mut st = reboot(st);
            let report = v2.load(&mut st).await.unwrap();
            assert!(report.unreadable.is_none());
        });
    }

    #[test]
    fn slot_table() {
        assert_eq!(
            V2::SLOTS.iter().map(|s| s.id).collect::<Vec<_>>(),
            [0, 2, 3, 4]
        );
        assert_eq!(V2::SLOTS[1].name, "gap_field");
        assert_eq!(V2::RETIRED, &[1]);
        assert_eq!(
            V2::VERSION,
            ww_settings::Version::parse(env!("CARGO_PKG_VERSION"))
        );
    }
}
