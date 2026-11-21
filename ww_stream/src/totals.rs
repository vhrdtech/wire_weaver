//! The two ends of a [Totals] sink (API-70): [TotalsTx] adds changes up and says what to write, [TotalsRx] turns
//! the writes back into changes. See `docs/api/totals.md`.
//!
//! A plain sink loses writes on a lossy link and acknowledges nothing. Each write of running totals carries the sum
//! of every change so far, so the next write that arrives brings what a lost one had, and changes made while a write
//! was on its way go together into the next one. Nothing is resent and nothing waits.
//!
//! The link keeps writes in order: `ww_link`'s UDP framing drops late and duplicated datagrams, the other links
//! are ordered. A write carries no sequence number for that reason.

use wire_weaver::ww_api::{Accumulate, Totals, TotalsSession};

/// The sending end of a [Totals] sink: adds changes up and says what to write next.
///
/// [Self::new_session] starts a session on every new link; its first write only sets where the receiver's totals
/// start. [Self::take] gives the next write, `None` while the receiver has everything.
#[derive(Copy, Clone, Debug)]
pub struct TotalsTx<T> {
    session: TotalsSession,
    total: T,
    /// The totals when the session started, until its first write is taken.
    base: Option<T>,
    /// The totals of the last write taken.
    sent: Option<T>,
    /// Changes added since the last write taken that are in the next one.
    pending: u32,
    stats: TotalsTxStats,
}

/// What a [TotalsTx] did so far, every count wrapping.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct TotalsTxStats {
    /// Changes added.
    pub adds: u32,
    /// Writes taken, the first of every session included.
    pub writes: u32,
    /// Changes that went out in a write together with an earlier one: one write of three changes merged two.
    pub merged: u32,
    /// Changes added without a session, or before one started: the receiver never applies them.
    pub unsent: u32,
    /// Writes repeated ([TotalsTx::repeat]).
    pub repeats: u32,
    /// Sessions started.
    pub sessions: u32,
}

impl<T: Accumulate> TotalsTx<T> {
    /// A sender without a session yet: changes add up, nothing is written until [Self::new_session].
    pub fn new() -> Self {
        Self {
            session: TotalsSession(0),
            total: T::default(),
            base: None,
            sent: None,
            pending: 0,
            stats: TotalsTxStats::default(),
        }
    }

    /// Adds `change` to the totals.
    #[inline]
    pub fn add(&mut self, change: T) {
        self.total = self.total.accumulate(change);
        self.pending = self.pending.wrapping_add(1);
        self.stats.adds = self.stats.adds.wrapping_add(1);
    }

    /// Starts `session` on a new link: its first write carries the totals as they are now, which the receiver
    /// takes as where they start. Changes not yet written are not applied.
    pub fn new_session(&mut self, session: TotalsSession) {
        self.stats.unsent = self.stats.unsent.wrapping_add(self.pending);
        self.stats.sessions = self.stats.sessions.wrapping_add(1);
        self.session = session;
        self.base = Some(self.total);
        self.sent = None;
        self.pending = 0;
    }

    /// Ends the session when its link goes: changes from now on are not written until the next
    /// [Self::new_session].
    pub fn end_session(&mut self) {
        self.base = None;
        self.sent = None;
    }

    /// The session writes belong to: the one [Self::new_session] started last.
    pub fn session(&self) -> TotalsSession {
        self.session
    }

    /// The next write, taken as sent: the session's first write, then the totals whenever they changed since the
    /// last write taken. `None` without a session or while the last write has everything.
    pub fn take(&mut self) -> Option<Totals<T>> {
        let total = if let Some(base) = self.base.take() {
            base
        } else {
            if self.sent? == self.total {
                return None;
            }
            self.stats.merged = self
                .stats
                .merged
                .wrapping_add(self.pending.saturating_sub(1));
            self.pending = 0;
            self.total
        };
        self.sent = Some(total);
        self.stats.writes = self.stats.writes.wrapping_add(1);
        Some(Totals {
            session: self.session,
            total,
        })
    }

    /// The last write taken again, for a sender that repeats it after a quiet while so that a lost last write
    /// heals: the receiver finds no change in a repeat that arrives. `None` without a session or before its first
    /// write.
    pub fn repeat(&mut self) -> Option<Totals<T>> {
        let total = self.sent?;
        self.stats.repeats = self.stats.repeats.wrapping_add(1);
        Some(Totals {
            session: self.session,
            total,
        })
    }

    /// What the sender did so far.
    pub fn stats(&self) -> TotalsTxStats {
        self.stats
    }
}

impl<T: Accumulate> Default for TotalsTx<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// The receiving end of a [Totals] sink: turns each write into the change since the last one applied.
///
/// A write of another session than the last one, and the first write after [Self::new], only set where the totals
/// start. One receiver serves one sender: two senders writing the same sink by turns change the session every
/// write, and nothing is applied.
#[derive(Copy, Clone, Debug)]
pub struct TotalsRx<T> {
    last: Option<Totals<T>>,
    stats: TotalsRxStats,
}

/// What a [TotalsRx] received so far, every count wrapping.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct TotalsRxStats {
    /// Writes received.
    pub writes: u32,
    /// Writes that brought a change.
    pub applied: u32,
    /// Writes that brought no change: a duplicate, or totals that changed and changed back.
    pub unchanged: u32,
    /// Writes that started a session: only where the totals start.
    pub sessions: u32,
}

impl<T: Accumulate> TotalsRx<T> {
    /// A receiver whose next write starts a session.
    pub const fn new() -> Self {
        Self {
            last: None,
            stats: TotalsRxStats {
                writes: 0,
                applied: 0,
                unchanged: 0,
                sessions: 0,
            },
        }
    }

    /// The change `write` brings since the last write applied, `None` for a write that starts a session or brings
    /// no change.
    pub fn receive(&mut self, write: Totals<T>) -> Option<T> {
        self.stats.writes = self.stats.writes.wrapping_add(1);
        let last = self.last.replace(write);
        let Some(last) = last.filter(|last| last.session == write.session) else {
            self.stats.sessions = self.stats.sessions.wrapping_add(1);
            return None;
        };
        let change = write.total.since(last.total);
        if change == T::default() {
            self.stats.unchanged = self.stats.unchanged.wrapping_add(1);
            return None;
        }
        self.stats.applied = self.stats.applied.wrapping_add(1);
        Some(change)
    }

    /// What the receiver got so far.
    pub fn stats(&self) -> TotalsRxStats {
        self.stats
    }
}

impl<T: Accumulate> Default for TotalsRx<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{TotalsRx, TotalsRxStats, TotalsTx, TotalsTxStats};
    use wire_weaver::ww_api::{Totals, TotalsSession};

    fn write(session: u16, total: i32) -> Totals<i32> {
        Totals {
            session: TotalsSession(session),
            total,
        }
    }

    #[test]
    fn a_session_starts_with_the_totals_as_they_are() {
        let mut tx = TotalsTx::<i32>::new();
        tx.add(5);
        assert_eq!(tx.take(), None, "no session yet");
        tx.new_session(TotalsSession(7));
        tx.add(3);
        assert_eq!(tx.take(), Some(write(7, 5)), "the base first");
        assert_eq!(tx.take(), Some(write(7, 8)));
        assert_eq!(tx.take(), None, "nothing new");
        tx.add(1);
        tx.add(-4);
        tx.add(2);
        assert_eq!(tx.take(), Some(write(7, 7)));
        tx.end_session();
        tx.add(10);
        assert_eq!(tx.take(), None, "no session");
        tx.new_session(TotalsSession(8));
        assert_eq!(tx.take(), Some(write(8, 17)));
        assert_eq!(
            tx.stats(),
            TotalsTxStats {
                adds: 6,
                writes: 4,
                merged: 2,
                unsent: 2,
                repeats: 0,
                sessions: 2,
            }
        );
    }

    #[test]
    fn a_repeat_is_the_last_write_again() {
        let mut tx = TotalsTx::<i32>::new();
        tx.new_session(TotalsSession(2));
        assert_eq!(tx.repeat(), None, "nothing written yet");
        assert_eq!(tx.take(), Some(write(2, 0)));
        tx.add(4);
        assert_eq!(tx.take(), Some(write(2, 4)));
        assert_eq!(tx.repeat(), Some(write(2, 4)));
        let mut rx = TotalsRx::<i32>::new();
        assert_eq!(rx.receive(write(2, 0)), None);
        assert_eq!(
            rx.receive(tx.repeat().expect("a write")),
            Some(4),
            "the last write was lost"
        );
        assert_eq!(rx.receive(tx.repeat().expect("a write")), None);
        tx.end_session();
        assert_eq!(tx.repeat(), None);
        assert_eq!(tx.stats().repeats, 3);
    }

    #[test]
    fn changes_back_and_forth_need_no_write() {
        let mut tx = TotalsTx::<i32>::new();
        tx.new_session(TotalsSession(0));
        assert_eq!(tx.take(), Some(write(0, 0)));
        tx.add(4);
        tx.add(-4);
        assert_eq!(tx.take(), None);
    }

    #[test]
    fn the_receiver_applies_the_change_since_the_last_write() {
        let mut rx = TotalsRx::<i32>::new();
        assert_eq!(rx.receive(write(1, 500)), None, "where the totals start");
        assert_eq!(rx.receive(write(1, 510)), Some(10));
        assert_eq!(rx.receive(write(1, 510)), None, "a duplicate");
        // a lost write's change comes with the next one
        assert_eq!(rx.receive(write(1, 530)), Some(20));
        assert_eq!(
            rx.receive(write(1, i32::MIN + 1)),
            Some((i32::MIN + 1).wrapping_sub(530))
        );
        assert_eq!(rx.receive(write(1, i32::MAX)), Some(-2), "wrapping");
        assert_eq!(rx.receive(write(2, 3)), None, "another session");
        assert_eq!(rx.receive(write(2, 4)), Some(1));
        assert_eq!(
            rx.stats(),
            TotalsRxStats {
                writes: 8,
                applied: 5,
                unchanged: 1,
                sessions: 2,
            }
        );
    }

    /// Every change the sender adds after a session started reaches the receiver, whichever writes are lost.
    #[test]
    fn lost_writes_lose_no_change() {
        let mut tx = TotalsTx::<i32>::new();
        let mut rx = TotalsRx::<i32>::new();
        tx.add(100);
        tx.new_session(TotalsSession(u16::MAX));
        let mut applied = 0i32;
        let mut added = 0i32;
        for i in 0..1000i32 {
            let change = i % 7 - 3;
            tx.add(change);
            added += change;
            // every third write lost, the first one gets through
            if let Some(w) = tx.take()
                && i % 3 != 1
            {
                applied += rx.receive(w).unwrap_or(0);
            }
        }
        tx.add(1);
        added += 1;
        applied += rx.receive(tx.take().expect("a change")).unwrap_or(0);
        assert_eq!(applied, added);
        assert_eq!(tx.stats().unsent, 1);
    }

    /// The first write of a session is where the receiver's totals start: when it is lost, the next one is, and
    /// the changes in between are not applied.
    #[test]
    fn a_lost_first_write_loses_the_changes_up_to_the_next() {
        let mut tx = TotalsTx::<i32>::new();
        let mut rx = TotalsRx::<i32>::new();
        tx.new_session(TotalsSession(3));
        let _lost = tx.take();
        tx.add(5);
        assert_eq!(rx.receive(tx.take().expect("a change")), None);
        tx.add(2);
        assert_eq!(rx.receive(tx.take().expect("a change")), Some(2));
    }
}
