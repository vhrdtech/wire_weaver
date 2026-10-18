//! A power cut at every step of every write: each word written and each byte erased, on STM32G0-like flash (8-byte
//! ECC words, 2 KiB pages), STM32H7-like words (32 bytes, ECC) and STM32F4-like flash (4-byte words, no ECC, big
//! sectors, sampled). After the cut and a reboot, every group must load as its value from before the write or the
//! one being written, never anything else, never an error, and the store must keep working. No word is ever
//! written twice between erases (the fake flash fails such a write).
//!
//! A debug build samples the steps (every 11th, and the first and last 16 of each write) to stay within the test
//! timeout; `just test-power-cut` runs every step in release.

use embassy_futures::block_on;
use ww_settings::fake_flash::{EraseOrder, FakeFlash};
use ww_settings::{Debounce, Key, Settings, Store};

#[derive(ww_settings::Settings, Clone, Debug, PartialEq)]
pub struct User {
    #[setting(slot = 0, default = 0)]
    pub n: u32,
    #[setting(slot = 1, default = [0; 24])]
    pub pad: [u8; 24],
}

#[derive(ww_settings::Settings, Clone, Debug, PartialEq)]
pub struct Factory {
    #[setting(slot = 0, default = 0)]
    pub cal: i32,
}

/// Every step in release builds, a sample in debug ones.
const EVERY_STEP: bool = !cfg!(debug_assertions);

/// Whether a cut at step `c` of `steps` is run: every one in release, the first and last 16 and every 11th in debug.
fn sampled(c: u64, steps: u64) -> bool {
    EVERY_STEP || c <= 16 || c + 16 > steps || c % 11 == 0
}

#[derive(Copy, Clone, Debug)]
enum Op {
    User(u32, bool),
    Factory(i32),
}

fn user_value(n: u32, big: bool) -> User {
    User {
        n,
        pad: if big { [n as u8 | 1; 24] } else { [0; 24] },
    }
}

/// What both groups hold.
#[derive(Clone, Debug, PartialEq)]
struct State {
    user: User,
    factory: Factory,
}

struct Rig<const W: usize, const P: usize> {
    base: u32,
    pages: usize,
}

type St<const W: usize, const P: usize> = Store<FakeFlash<W, P>, 256>;

impl<const W: usize, const P: usize> Rig<W, P> {
    fn store(&self, flash: FakeFlash<W, P>) -> St<W, P> {
        Store::new(flash, self.base..self.base + (self.pages * P) as u32).unwrap()
    }

    /// Boot: load both groups. Errors are returned (a cut while loading repairs something).
    async fn boot(
        &self,
        st: &mut St<W, P>,
    ) -> Result<(Settings<User, 64>, Settings<Factory, 16>), String> {
        let mut user = Settings::new(Key::USER, Debounce::default());
        let mut factory = Settings::new(Key::FACTORY, Debounce::default());
        let r = user
            .load(st)
            .await
            .map_err(|e| format!("load user: {e:?}"))?;
        assert!(r.unreadable.is_none(), "{r:?}");
        let r = factory
            .load(st)
            .await
            .map_err(|e| format!("load factory: {e:?}"))?;
        assert!(r.unreadable.is_none(), "{r:?}");
        Ok((user, factory))
    }

    async fn apply(
        &self,
        st: &mut St<W, P>,
        user: &mut Settings<User, 64>,
        factory: &mut Settings<Factory, 16>,
        op: Op,
    ) -> Result<(), String> {
        match op {
            Op::User(n, big) => {
                user.set(0, |u| *u = user_value(n, big));
                user.persist(st).await.map_err(|e| format!("{e:?}"))?;
            }
            Op::Factory(c) => {
                factory.set(0, |f| f.cal = c);
                factory.persist(st).await.map_err(|e| format!("{e:?}"))?;
            }
        }
        Ok(())
    }

    /// Boot and apply `op`.
    async fn boot_and_apply(&self, st: &mut St<W, P>, op: Op) -> Result<(), String> {
        let (mut user, mut factory) = self.boot(st).await?;
        self.apply(st, &mut user, &mut factory, op).await
    }

    async fn state(&self, st: &mut St<W, P>, ctx: &str) -> State {
        let (user, factory) = self.boot(st).await.unwrap_or_else(|e| panic!("{ctx}: {e}"));
        State {
            user: user.get().clone(),
            factory: factory.get().clone(),
        }
    }

    /// After a cut while going from `before` to `after`: each group is at one of the two.
    fn check(state: &State, before: &State, after: &State, ctx: &str) {
        assert!(
            state.user == before.user || state.user == after.user,
            "{ctx}: user {:?}, expected {:?} or {:?}",
            state.user,
            before.user,
            after.user
        );
        assert!(
            state.factory == before.factory || state.factory == after.factory,
            "{ctx}: factory {:?}, expected {:?} or {:?}",
            state.factory,
            before.factory,
            after.factory
        );
    }

    fn state_after(state: &State, op: Op) -> State {
        let mut s = state.clone();
        match op {
            Op::User(n, big) => s.user = user_value(n, big),
            Op::Factory(c) => s.factory.cal = c,
        }
        s
    }

    /// Run `ops`, cutting the power at every step `cut_at(step, steps_of_op)` picks of each op; at every
    /// `second_every`-th cut also cut the recovery at each of its steps.
    fn run(
        &self,
        ecc: bool,
        order: EraseOrder,
        ops: &[Op],
        cut_at: impl Fn(u64, u64) -> bool,
        second_every: usize,
    ) -> Report {
        block_on(async {
            let mut flash = FakeFlash::<W, P>::new(self.base, self.pages);
            flash.ecc = ecc;
            flash.erase_order = order;

            // the clean run: a snapshot and the steps of each op
            let mut snapshots = Vec::new();
            let mut states = Vec::new();
            let mut steps_of = Vec::new();
            let mut st = self.store(flash);
            let mut state = self.state(&mut st, "clean").await;
            for &op in ops {
                snapshots.push(st.flash().clone());
                states.push(state.clone());
                let s0 = st.flash().stats().steps;
                // a fresh store per op: what the cut runs do
                let mut s = self.store(st.into_flash());
                self.boot_and_apply(&mut s, op).await.unwrap();
                steps_of.push(s.flash().stats().steps - s0);
                st = s;
                state = Self::state_after(&state, op);
            }
            assert_eq!(self.state(&mut st, "clean").await, state);
            let erases = st.flash().stats().page_erases;
            assert_eq!(
                st.flash().stats().rewrites,
                0,
                "a word written twice without a cut"
            );

            let mut report = Report {
                ops: ops.len(),
                erases,
                ..Default::default()
            };
            let mut nth_cut = 0usize;
            for (j, &op) in ops.iter().enumerate() {
                let before = &states[j];
                let after = Self::state_after(before, op);
                let steps = steps_of[j];

                for c in 1..=steps {
                    if !cut_at(c, steps) || !sampled(c, steps) {
                        continue;
                    }
                    report.cuts += 1;
                    let mut f = snapshots[j].clone();
                    f.seed(c * 7919 + j as u64);
                    f.cut_after(c);
                    let mut s = self.store(f);
                    let r = self.boot_and_apply(&mut s, op).await;
                    assert!(
                        r.is_err() && s.flash().is_off(),
                        "op {j} cut {c}/{steps}: {r:?}"
                    );
                    let mut f = s.into_flash();
                    f.power_on();
                    let ctx = format!("op {j} {op:?} cut at {c}/{steps}");

                    nth_cut += 1;
                    let second_every = if EVERY_STEP {
                        second_every
                    } else {
                        second_every * 7
                    };
                    if second_every > 0 && nth_cut.is_multiple_of(second_every) {
                        // a second cut while booting and writing again after the first one
                        let next = Op::User(1_000_000 + j as u32, c % 2 == 0);
                        // the steps of booting (and repairing) and writing again, uncut: sampled as the first cut
                        let mut s2 = self.store(f.clone());
                        let s0 = s2.flash().stats().steps;
                        self.boot_and_apply(&mut s2, next)
                            .await
                            .unwrap_or_else(|e| panic!("{ctx}, then {next:?}: {e}"));
                        let steps2 = s2.flash().stats().steps - s0;
                        for c2 in (1..=steps2).filter(|&c2| sampled(c2, steps2)) {
                            let mut f2 = f.clone();
                            f2.seed(c2 * 104729 + c);
                            f2.cut_after(c2);
                            let mut s2 = self.store(f2);
                            let r = self.boot_and_apply(&mut s2, next).await;
                            assert!(
                                r.is_err() && s2.flash().is_off(),
                                "{ctx}, then {next:?} cut {c2}/{steps2}: {r:?}"
                            );
                            report.second_cuts += 1;
                            let mut f2 = s2.into_flash();
                            f2.power_on();
                            let mut s2 = self.store(f2);
                            let ctx2 = format!("{ctx}, second cut {c2}");
                            let got = self.state(&mut s2, &ctx2).await;
                            // the first write may or may not have landed, the second as well
                            Self::check(&got, before, &after, &ctx2);
                        }
                    }

                    let mut s = self.store(f);
                    let got = self.state(&mut s, &ctx).await;
                    Self::check(&got, before, &after, &ctx);
                    // the store keeps working: write the op again and something else, reboot, both there
                    self.boot_and_apply(&mut s, op)
                        .await
                        .unwrap_or_else(|e| panic!("{ctx}, redo: {e}"));
                    let next = Op::User(2_000_000 + c as u32, c % 3 == 0);
                    self.boot_and_apply(&mut s, next)
                        .await
                        .unwrap_or_else(|e| panic!("{ctx}, next: {e}"));
                    let mut f = s.into_flash();
                    f.power_on();
                    let mut s = self.store(f);
                    assert_eq!(
                        self.state(&mut s, &ctx).await,
                        Self::state_after(&after, next),
                        "{ctx}, after recovery"
                    );
                }
            }
            report
        })
    }
}

#[derive(Debug, Default)]
#[allow(dead_code)] // printed
struct Report {
    ops: usize,
    erases: u64,
    cuts: u64,
    second_cuts: u64,
}

/// Enough writes to go around the pages twice, records of two sizes, the factory group written now and then.
fn ops(user_writes: u32) -> Vec<Op> {
    let mut ops = vec![Op::Factory(-5)];
    for n in 1..=user_writes {
        ops.push(Op::User(n, n % 3 == 0));
        if n % 37 == 0 {
            ops.push(Op::Factory(n as i32));
        }
    }
    ops
}

const G0_BASE: u32 = 0x0807_8000;

#[test]
fn g0_every_step_4_pages() {
    // 2 KiB pages hold about 50 of these records: 220 writes go around 4 pages more than once
    let rig = Rig::<8, 2048> {
        base: G0_BASE,
        pages: 4,
    };
    let r = rig.run(true, EraseOrder::Forward, &ops(600), |_, _| true, 97);
    println!("{r:?}");
    assert!(r.erases >= 4, "{r:?}");
}

#[test]
fn g0_every_step_2_pages_backward_erase() {
    let rig = Rig::<8, 2048> {
        base: G0_BASE,
        pages: 2,
    };
    let r = rig.run(true, EraseOrder::Backward, &ops(300), |_, _| true, 89);
    println!("{r:?}");
    assert!(r.erases >= if EVERY_STEP { 2 } else { 1 }, "{r:?}");
}

#[test]
fn g0_without_ecc_half_written_bits_read_as_they_are() {
    let rig = Rig::<8, 2048> {
        base: G0_BASE,
        pages: 3,
    };
    let r = rig.run(false, EraseOrder::Forward, &ops(400), |_, _| true, 0);
    println!("{r:?}");
}

#[test]
fn h7_words_every_step() {
    // 32-byte ECC words on small pages: every item is 2 words or more
    let rig = Rig::<32, 4096> {
        base: 0x081e_0000,
        pages: 2,
    };
    let r = rig.run(
        true,
        EraseOrder::Backward,
        &ops(if EVERY_STEP { 400 } else { 260 }),
        |_, _| true,
        71,
    );
    println!("{r:?}");
    assert!(r.erases >= if EVERY_STEP { 2 } else { 1 }, "{r:?}");
}

#[test]
fn f4_16k_sectors_sampled() {
    // F4 sectors 1 and 2 (16 KiB each), 4-byte words, no ECC, random erase order; the erases are sampled: every
    // 97th byte and the ends of each write (a debug build only cuts the writes that start a page)
    let rig = Rig::<4, 16384> {
        base: 0x0800_4000,
        pages: 2,
    };
    let r = rig.run(
        false,
        EraseOrder::Random,
        &ops(if EVERY_STEP { 1600 } else { 1300 }),
        |c, steps| {
            (EVERY_STEP || steps > 20) && (c <= 64 || c + 64 >= steps || c.is_multiple_of(97))
        },
        0,
    );
    println!("{r:?}");
    assert!(r.erases >= if EVERY_STEP { 2 } else { 1 }, "{r:?}");
}

#[test]
fn g0_random_erase_2_pages() {
    let rig = Rig::<8, 2048> {
        base: G0_BASE,
        pages: 2,
    };
    let r = rig.run(true, EraseOrder::Random, &ops(300), |_, _| true, 61);
    println!("{r:?}");
}

#[test]
fn g0_random_erase_4_pages() {
    let rig = Rig::<8, 2048> {
        base: G0_BASE,
        pages: 4,
    };
    let r = rig.run(true, EraseOrder::Random, &ops(600), |_, _| true, 0);
    println!("{r:?}");
}
