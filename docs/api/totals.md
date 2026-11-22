---
level: advanced
---

# Running totals over a plain sink

A pointer, a scroll wheel or a jog dial sends small changes, many per second. Over TCP a lost segment holds every
later change until it is resent, and the pointer stutters. Over UDP a [plain sink](streams.md#sinks) skips a lost
write, and its change is gone. **Running totals** fix both: each write carries the sum of every change so far, the
device applies the difference to the last write it applied, so the next write that arrives brings what a lost one had.
Nothing is resent, nothing waits, and a plain sink carries it.

```rust
#[ww_trait]
trait Pointer {
    /// Moves the pointer by the change of the totals since the last write applied.
    #[effect(edit)]
    sink!(motion: Totals<Delta>);
    /// Scrolls by the change since the last write applied.
    #[effect(edit)]
    sink!(scroll: Totals<Fixed<Detent, One, i32>>);
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Copy, Clone, Debug, Default, PartialEq, Eq), final_structure)]
pub struct Delta {
    pub dx: Px, // Fixed<Pixel, One, i32>
    pub dy: Px,
}

impl Accumulate for Delta {
    fn accumulate(self, change: Self) -> Self {
        Self { dx: self.dx.accumulate(change.dx), dy: self.dy.accumulate(change.dy) }
    }
    fn since(self, earlier: Self) -> Self {
        Self { dx: self.dx.since(earlier.dx), dy: self.dy.since(earlier.dy) }
    }
}
```

`Totals<T>`, `TotalsSession` and `Accumulate` are in `ww_api` and its prelude. `Accumulate` is wrapping addition and
the difference of two totals: implemented for the integers, for `Fixed` over an integer without bounds (a `Range`
would not survive the wrap) and for arrays of them; a struct of several totals implements it field by field, as
above. Floats have no `Accumulate`: their sum loses the small changes once it is large.

## On the wire

`Totals<T>` is `final_structure`: the session as a `u16`, then `T`, no length. `Totals<Fixed<Item, One, i32>>` of
session `0xBEEF` and total -5 is `ef be fb ff ff ff` (conformance case `api.totals.item`).

| Field     | Type                  | What                                                                     |
|-----------|-----------------------|--------------------------------------------------------------------------|
| `session` | `TotalsSession(u16)`  | the sender's session: a new one for every link, a random one per process |
| `total`   | `T`                   | every change of the session added up, wrapping at the ends of `T`        |

## Sessions

The first write of a session only sets where the device's totals start; from the next one on, the device applies
the change since the last write. A write of another session starts the totals anew, so the device needs no signal
of its own when a host connects: a host program that restarts, a second program through `ww_hub` (whose link to
the device outlives its clients), and a reconnect all write a new session. A device that keeps its receiver across
links needs nothing more; one that drops it starts over at the next write.

Changes made while no link is up, or before a session's first write went out, are not applied: a pointer does not
jump by what a hand did while the device was away. When the first write of a session is lost, the next one is where the
totals start, and the changes between them are not applied either.

One receiver serves one sender at a time. Two senders writing the same sink by turns change the session with every
write, and nothing is applied.

## Host: `TotalsSender`

`wire_weaver_client::TotalsSender<T>` adds changes up from any thread and sends them through the sink:

```rust
let sender = TotalsSender::<Delta>::new();
let sink = client.motion().await?;
tokio::spawn(sender.run(sink, &client.cmd)); // holds neither the sender nor the client

sender.add(Delta { dx: Px::new(3), dy: Px::new(-1) }); // never waits
```

`run` takes the client's `Commander`, best one that [keeps trying](../transport/keep_trying.md), and starts a
session every time its connection state says connected, then sends the latest totals whenever they
changed, one write at a time: changes made while a write is on its way go together into the next one. A plain sink
acknowledges nothing, so a lost last write would leave its change behind until the next one: after `REPEAT_AFTER`
(50 ms) without a change the last write goes again, at most `REPEATS_MAX` (2) times (`with_repeats(after, max)`;
the device finds no change in a repeat that arrives).

`sender.retry_now()` is the running connection's [`retry_now()`](../transport/keep_trying.md#retry-now): input that
waits for the link (a hand on the pointer while it is down) hurries its reconnect without the client at hand.

## Device: `TotalsRx`

`ww_stream::totals::TotalsRx<T>` (`no_std`, no allocation) turns each write back into the change to apply:

```rust
async fn write_motion(&mut self, _cx: &mut Context<'_, impl EventOut>, motion: Totals<Delta>) {
    if let Some(change) = self.motion_rx.receive(motion) {
        self.pointer.move_by(change.dx, change.dy);
    }
}
```

`receive` returns `None` for the first write of a session and for a write that brings no change (a duplicate, a
repeat). `TotalsTx<T>` is the sending end's state for code without `std`: `add`, `new_session`, `end_session`, `take`
for the next write and `repeat` for the last one again.

## Statistics

| Sender (`TotalsTxStats`) | What                                                                        |
|--------------------------|-----------------------------------------------------------------------------|
| `adds`                   | changes added                                                               |
| `writes`                 | writes taken, the first of every session included                           |
| `merged`                 | changes that went out in a write with an earlier one: one write of 3 merged 2 |
| `unsent`                 | changes made without a session or before its first write: never applied     |
| `repeats`                | writes repeated after a quiet while                                         |
| `sessions`               | sessions started                                                            |

| Receiver (`TotalsRxStats`) | What                                                       |
|----------------------------|------------------------------------------------------------|
| `writes`                   | writes received                                            |
| `applied`                  | writes that brought a change                               |
| `unchanged`                | writes that brought none: a duplicate or a repeat          |
| `sessions`                 | writes that started a session                              |

Writes lost on the way are the sender's `writes + repeats` less the receiver's `writes`. The device alone can't tell
them apart from merged changes, as a write carries no sequence number; the link counts its lost datagrams
(`ww stats`, `ww monitor`).

## Why no sequence number

A write would need one to drop a duplicate or a late write. The links already do it:

| Link            | Duplicates and late writes                                                                                                              |
|-----------------|-----------------------------------------------------------------------------------------------------------------------------------------|
| UDP             | `ww_link`'s frame counter drops duplicated, late and previous-session datagrams ([Reliability](../transport/udp.md#reliability))        |
| WebSocket, TCP  | ordered and without duplicates; a reconnect is a new connection and a new session                                                       |
| USB, RTT, local | ordered and without duplicates                                                                                                          |

A duplicate would do no harm anyway: it brings no change. The one case that gets past the link is a datagram held
back by more than 8 others on UDP: the 4-bit frame counter takes it for one ahead (TR-33). The device then steps back
by what the later writes brought and forward again with the next one, so nothing is lost, and the link drops up to 8
datagrams in order after it, which the next change or a repeat makes good. `tests/totals` shows both, with a simulated
UDP link that drops and reorders datagrams.
