---
level: basic
---

# Keep-trying connections

A phone app on Wi-Fi, a dashboard next to a board that reboots, a tool started before the device is up: they want a
client that exists at once, connects when the device answers, and connects again whenever the link goes.
`connect_keep_trying()` makes one (CLT-75).

```rust
let tv = Tv::config(|c| {
    c.websocket_urls(["ws://192.168.1.95:7590/ww", "ws://tv.tailnet:7590/ww"])
        .keep_trying(KeepTrying::default())
})
.connect_keep_trying()?; // returns at once, connects in the background

let mut events = tv.cmd.connection_events();
while let Ok(event) = events.recv().await {
    match event {
        ConnectionEvent::Connecting { attempt } => show_connecting(attempt),
        ConnectionEvent::Connected { address, .. } => {
            show_connected(&address);
            // observers opened now go on through every later reconnect
            volume = Some(tv.observe_volume(on_change(Duration::from_millis(50))).subscribe().await?);
        }
        ConnectionEvent::Disconnected { .. } => show_offline(),
        ConnectionEvent::Reconnected { .. } => show_back(),
        ConnectionEvent::GaveUp { error } => break,
        _ => {}
    }
}
```

It runs on the caller's tokio runtime and needs one (an error says so otherwise).

## What it does

- **The handle at once.** `connect_keep_trying()` returns the client before any connection; the first attempt starts
  right away in a task of its own.
- **Addresses at once (CLT-80).** Each attempt opens all of the config's addresses at once:
  `ClientConfig::websocket_urls([..])` for WebSocket (a LAN address and a VPN one, a discovered one and a typed
  one), `in_process_paths([..])` for in-process devices (the registered ones). The first to open (TCP and the
  WebSocket handshake) goes on to link setup, the others still opening are dropped; on a tie the earlier address
  wins. An address that doesn't open within the link setup timeout (1 s for WebSocket,
  `ClientConfig::link_setup_timeout` when set) counts as failed, so a host gone from the LAN costs nothing while
  another answers. When link setup fails on the winner, the other addresses race again within the same attempt.
  `connect()` and `reconnect()` race the same way; `connect_blocking()` races too, without the second race. The
  winner is the `address` of `Connected` and `ConnectionState::Connected`; when all fail, the error names each.
- **Backoff.** Between attempts the client waits `KeepTrying::backoff`: `first` after the first failed attempt,
  doubled after each further one up to `max` (`Backoff::NETWORK`: 100 ms up to 5 s). Each new gap starts from
  `first` again.
- **Calls.** Until the link is up, and while it is down again, every call fails at once with `Error::Disconnected`,
  as do calls in flight when the link goes. None is sent again by itself: a call may have reached the device before
  the link went, and calls aren't idempotent in general.
- **Streams and observers.** After each reconnect, counted and reliable streams resume from their cursor, observed
  trait levels are subscribed again (`ObserveEvent::Resumed`), sinks are registered again: the same path as
  [`disconnect().keep_streams()` and `Commander::resume_streams()`](../api/stream_reliability.md). Plain streams end
  with the link; open them again after `Reconnected`. Streams and observers open only while connected: open them after
  `Connected`.
- **Stopping.** `disconnect()` stops trying; dropping the last handle ends everything. `KeepTrying::give_up`
  (`None` by default: never) stops after that long without a link, the first connect included: the waiting streams
  end and `GaveUp` says why.

## Retry now

The backoff is a guess; the app often knows better: the network just came back, the screen woke up, the user
pressed "retry". `Commander::retry_now().await` hurries a connection that reconnects by itself (a keep-trying one,
or one [through the hub](#through-the-hub)) and says what it did:

| Where the connection is | What happens | Answer |
|---|---|---|
| waiting between attempts | the next attempt starts now; the wait after it is `backoff.first` again | `RetryNow::Attempting` |
| an attempt under way | it is stopped (an address still opening too) and a new one starts at once | `Restarted` |
| connected | a ping goes to the device | `Pinged` |
| not trying: a plain `connect()`, `connect_blocking()`, after `disconnect()` or `GaveUp` | nothing | `NotTrying` |

The ping finds a link whose other end is gone when the medium fails on it (a TCP connection the peer reset); a
silent one (a phone that left the Wi-Fi without a word) goes at the peer timeout, 10 s without a word from the
device, as before. Either way the lost link is then reconnected as usual. A stopped attempt is not an
`AttemptFailed`; the new one comes as `Connecting { attempt }` with the next number.
`TotalsSender::retry_now()` reaches the connection its `run` sends on ([running totals](../api/totals.md)).

## Events and state

`Commander::connection_events()` is a broadcast receiver of what happens, from the moment it is called:

| Event | When |
|---|---|
| `Connecting { attempt }` | an attempt starts, 1 for the first since the link went (or since the client was made) |
| `AttemptFailed { attempt, error, retry_in }` | it failed, the next comes after `retry_in` |
| `Connected { address, attempts }` | the first connect, at `address` (as `DeviceInfo::location` names it) |
| `Disconnected { reason, reconnecting }` | the link went, `GapReason` says why |
| `Reconnected { gap, attempts, resumed }` | connected again, `resumed` streams and levels went on |
| `GaveUp { error }` | no more attempts |

`Commander::connection_state()` is a `watch` of where the connection is now, for an app that shows it rather than
following every event (and can't miss the first ones): `ConnectionState::Connecting { attempt }`,
`Connected { address }` or `Disconnected` (not trying).

## The device's API

Until the first connect the client assumes the device runs the client's own API; a client that speaks several wire
lines ([EVO-28](../evolution/multi_line_host.md)) assumes its first one. The first connect learns the device's API as
`connect()` does (link setup, introspection data when the device's API differs from the client's, the resource
check), and every clone of the handle sees it. A device on another wire line than the assumed one, or one with
another API on a later reconnect (other firmware), ends the trying with `GaveUp`: make a new client for it.

## Through the hub

A connection through the ww hub reconnects by itself already ([CLT-40](hub.md#reconnect-after-a-hub-restart-clt-40)),
with its own backoff (10 ms up to 1 s) and give-up time (`ClientConfig::hub_reconnect`). A keep-trying connection
follows its own settings on every transport.

## Tests

`tests/keep_trying` runs a generated client against a generated server on the
[simulated WebSocket](websocket.md#simulated-websocket-in-tests) (`tests_common::start_device_async_ws`), on tokio's
paused clock: the server starts after the client, goes away and comes back while an observer keeps delivering; the
first address with a server wins; the waits between attempts and the give-up time are the configured ones;
`retry_now()` hurries a waiting connection, restarts an attempt whose WebSocket handshake never ends, pings a running
link and leaves a plain `connect()` alone; an address whose handshake never ends doesn't hold the next one, a winner
that fails link setup leaves the others to race, and addresses that all fail do so after one open timeout.
