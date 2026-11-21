//! [TotalsSender]: running totals over a plain sink (API-70), for a lossy link where a later write must not wait for
//! an earlier one (a pointer, a scroll wheel, a jog dial over UDP). See `docs/api/totals.md`.

use std::hash::{BuildHasher, RandomState};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::{Notify, watch};
use wire_weaver::shrink_wrap::SerializeShrinkWrap;
use wire_weaver::ww_api::{Accumulate, Totals, TotalsSession};
use ww_stream::totals::{TotalsTx, TotalsTxStats};

use crate::locked::Locked;
use crate::{ConnectionState, Sink};

/// Sends running totals to a [Totals] sink: [Self::add] never waits, and a lost write loses no change.
///
/// [Self::run] sends the latest totals whenever they changed, one write at a time: changes made while a write is on
/// its way go together into the next one. Every link the client connects starts a session, whose first write only
/// sets where the device's totals start; changes made while offline, or before that first write went, are not
/// applied ([TotalsTxStats::unsent]).
///
/// A plain sink acknowledges nothing, so a lost last write would leave its change behind until the next one: after
/// [REPEAT_AFTER] without a change the last write goes again, at most [REPEATS_MAX] times
/// ([Self::with_repeats]).
///
/// Cheap to clone, `Send` and `Sync`: every clone adds to the same totals.
pub struct TotalsSender<T> {
    shared: Arc<Shared<T>>,
    repeat_after: Duration,
    repeats_max: u32,
}

/// How long [TotalsSender] waits without a change before it repeats the last write.
pub const REPEAT_AFTER: Duration = Duration::from_millis(50);

/// How many times [TotalsSender] repeats the last write after the totals stopped changing.
pub const REPEATS_MAX: u32 = 2;

struct Shared<T> {
    tx: Mutex<TotalsTx<T>>,
    changed: Notify,
}

impl<T> Clone for TotalsSender<T> {
    fn clone(&self) -> Self {
        Self {
            shared: self.shared.clone(),
            repeat_after: self.repeat_after,
            repeats_max: self.repeats_max,
        }
    }
}

impl<T: Accumulate + SerializeShrinkWrap> TotalsSender<T> {
    /// A sender whose totals start at zero; nothing is sent before [Self::run].
    pub fn new() -> Self {
        Self {
            shared: Arc::new(Shared {
                tx: Mutex::new(TotalsTx::new()),
                changed: Notify::new(),
            }),
            repeat_after: REPEAT_AFTER,
            repeats_max: REPEATS_MAX,
        }
    }

    /// Repeats the last write `max` times, each after `after` without a change; 0 never repeats.
    #[must_use]
    pub fn with_repeats(mut self, after: Duration, max: u32) -> Self {
        self.repeat_after = after;
        self.repeats_max = max;
        self
    }

    /// Adds `change` to the totals; [Self::run] sends them. Never waits for the link.
    pub fn add(&self, change: T) {
        self.shared.tx.locked().add(change);
        self.shared.changed.notify_one();
    }

    /// What the sender did so far: changes added, writes, changes merged into one write, changes never applied.
    pub fn stats(&self) -> TotalsTxStats {
        self.shared.tx.locked().stats()
    }

    /// Sends the totals through `sink` until `state`'s sender goes away: a session for every time `state` says
    /// connected, the first one at a random [TotalsSession], so a restarted program is a new sender to the device.
    ///
    /// `state` is [Commander::connection_state](crate::Commander::connection_state) of a client that keeps trying.
    /// A write that fails ends the session until the next connect.
    pub async fn run(
        &self,
        mut sink: Sink<Totals<T>>,
        mut state: watch::Receiver<ConnectionState>,
    ) {
        let random = RandomState::new()
            .hash_one(std::process::id())
            .to_le_bytes();
        let mut next = TotalsSession(u16::from_le_bytes([random[0], random[1]]));
        let mut in_session = false;
        let mut repeats_left = 0;
        loop {
            let connected = matches!(
                *state.borrow_and_update(),
                ConnectionState::Connected { .. }
            );
            if connected && !in_session {
                self.shared.tx.locked().new_session(next);
                next = next.next();
                in_session = true;
            } else if !connected && in_session {
                self.shared.tx.locked().end_session();
                in_session = false;
            }
            let write = self.shared.tx.locked().take();
            if let Some(write) = write {
                if sink.send(write).await.is_ok() {
                    repeats_left = self.repeats_max;
                    // more may have come while it was sent
                    continue;
                }
                // the link went: the next connect starts a session
                self.shared.tx.locked().end_session();
                in_session = false;
                if state.changed().await.is_err() {
                    return;
                }
                continue;
            }
            let quiet = async {
                if in_session && repeats_left > 0 {
                    tokio::time::sleep(self.repeat_after).await;
                } else {
                    std::future::pending::<()>().await;
                }
            };
            tokio::select! {
                changed = state.changed() => if changed.is_err() { return },
                () = self.shared.changed.notified() => {}
                () = quiet => {
                    repeats_left -= 1;
                    let write = self.shared.tx.locked().repeat();
                    if let Some(write) = write {
                        // a repeat that fails is as lost as one that is dropped: the state tells of the link
                        _ = sink.send(write).await;
                    }
                }
            }
        }
    }
}

impl<T: Accumulate + SerializeShrinkWrap> Default for TotalsSender<T> {
    fn default() -> Self {
        Self::new()
    }
}
