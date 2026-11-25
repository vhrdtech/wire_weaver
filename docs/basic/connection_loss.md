---
level: basic
---

# When the device goes away

A board resets, a cable gets pulled, a Wi-Fi network drops for a minute. A host program that connected once with
`connect()` doesn't survive any of these: its calls fail, and it stays disconnected. Some programs should work through them, such as a dashboard next to a board that reboots. Such a program needs a
client that exists at once, connects when the device answers, and connects again whenever the link goes. `connect_keep_trying()` makes one.

This chapter connects the thermometer that way and follows what the client does while the device is away. It also
shows what a program can ask of the client.

## A client that keeps trying

The only change from a plain connect is the last call, and a `keep_trying()` setting in the config:

```rust
--8<-- "examples/thermometer/examples/keep_trying.rs:connect"
```

`connect_keep_trying()` returns the client at once, before any connection exists. The first attempt runs in a task of
its own, so the program needs a tokio runtime. The device doesn't have to be up yet: the client keeps trying until it
is.

Try it. Start the simulated thermometer in one terminal and the program in another:

```sh
cargo run -p thermometer --example thermometer_sim
cargo run -p thermometer --example thermometer_keep_trying
```

Stop the simulated device with Ctrl-C, wait a few seconds, and start it again. The program reports each step and
goes on without being restarted.

## What happens while the device is away

The test `book_output_keep_trying` in [`examples/thermometer/tests/it/connected.rs`] runs the same client against a
simulated thermometer. The test controls the clock, so the numbers are the same on every run. The board is not
plugged in when the host starts. It is plugged in at 350 ms and reset at 2 000 ms, and it is back 250 ms later. At
5 000 ms it is unplugged for 3 seconds.

The figure shows the board, the link and every attempt on one time axis. A red mark is a failed attempt, and a green
one is an attempt that connected.

<figure>
--8<-- "docs/generated/thermometer/keep_trying.svg"
</figure>

The same run as text:

```text
--8<-- "docs/generated/thermometer/keep_trying.txt"
```

Four things in it are worth knowing.

- **The waits grow.** After a failed attempt the client waits 100 ms, then 200 ms, then 400 ms, doubling up to
  5 seconds. A short outage is noticed quickly, and a long one doesn't flood the network. After each connection the
  next wait starts at 100 ms again. The waits are the [[backoff]], a setting of `KeepTrying`.
- **The host connected at 700 ms, not at 350 ms.** The board was plugged in during a wait. The client had no way to know. Section [Retry now](#retry-now) shows how a program shortens that.
- **A call fails at once while there is no link.** The call at 5 500 ms returned the error `Disconnected` immediately.
  The client doesn't wait, and it doesn't send the call again later. A call may have reached the device before the link
  went, and calls aren't safe to repeat in general. The program decides.
- **Observers go on.** After each reconnect the client subscribes again to observed properties, so a
  [watched value](../api/observing.md) delivers updates again without a new subscription. Counted and reliable
  [streams](../api/stream_reliability.md) resume where they stopped. A plain stream ends with the link, and the program
  opens it again after the reconnect.

## Following the connection

A program that shows the state of the connection reads its events. `connection_events()` is a broadcast channel of what
happens from the moment it is called:

```rust
--8<-- "examples/thermometer/examples/keep_trying.rs:events"
```

| Event | When it comes |
|---|---|
| `Connecting` | An attempt starts. The number is 1 for the first attempt after the link went. |
| `AttemptFailed` | An attempt failed, and the next one comes after `retry_in`. |
| `Connected` | The first connection, with the address that won. |
| `Disconnected` | The link went, with the reason. |
| `Reconnected` | Connected again, with the number of streams and observers that went on. |
| `GaveUp` | No more attempts. |

A program that only draws a status light doesn't need every event. `connection_state()` is a value that holds where
the connection is now: connecting, connected or disconnected. It never misses the first events, as a receiver that
starts late might.

The calls in the example loop are ordinary. While there is no link they fail with the error that the log shows,
`Submitted a command requiring active connection, when there was none`, and the loop goes on:

```rust
--8<-- "examples/thermometer/examples/keep_trying.rs:calls"
```

## Retry now

The backoff is a guess, and the program often knows better. The network just came back, the screen woke up, or the
user pressed a retry button. `retry_now()` hurries a connection that reconnects by itself, and says what it did:

| Where the connection is | What happens |
|---|---|
| Waiting between attempts | The next attempt starts now, and the wait after it is the first one again. |
| An attempt is under way | It is stopped and a new one starts at once. |
| Connected | A ping goes to the device, which finds a link whose other end is gone. |
| Not trying | Nothing: the client was made with `connect()`, or stopped. |

## Giving up and stopping

A keep-trying client tries for ever by default. `KeepTrying::give_up` stops it after that long without a link, and
the `GaveUp` event says why. `disconnect()` stops trying and closes the connection, and dropping the last copy of the
client ends everything.

## Where to go next

[Keep-trying connections in depth](../transport/keep_trying.md) covers several addresses per attempt, a device that
comes back with another revision of its API, and connections through the hub.
