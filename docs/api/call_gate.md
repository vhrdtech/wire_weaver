---
level: advanced
---

# Call gate

A device sometimes must not serve all of its API. A board guarded against reworks its firmware doesn't know is the
case this was built for: it answers who it is, `ww info`, firmware updates and reading its board record, and
nothing that drives a pin or a bus. The call gate does this in one place. The generated server asks the server
struct before it runs a request's handler, and a refusal goes back to the host with the firmware's reason. No
handler has a check of its own, and the API stays the same in every state, so typed clients keep working and get a
precise error.

`tests/call_gate` has a worked example, end to end through the host event loop.

## Turning it on

```rust
mod api_impl {
    wire_weaver::ww_codegen!(
        board_api :: Board for super::Server,
        server = true, no_alloc = true, use_async = true,
        call_gate = true,
    );
}
```

The server struct then implements `wire_weaver::gate::CallGate`:

```rust
use wire_weaver::gate::{CallGate, Op, Target, TraitId};

const BOARD_INFO: TraitId = TraitId::new("ww_board_info", "BoardInfo");
const FIRMWARE_INFO: TraitId = TraitId::new("ww_firmware_info", "FirmwareInfo");
const UID: TraitId = TraitId::new("ww_uid", "Uid");

impl CallGate for Server {
    fn admit(&self, target: &Target, _op: Op) -> Result<(), &str> {
        let Some(guard) = &self.guard else {
            return Ok(());
        };
        if target.is_in(&FIRMWARE_INFO) || target.is_in(&UID) || target.trait_id.crate_id == "ww_dfu" {
            return Ok(());
        }
        // the record can be read and the allowance written, the reworks themselves can't be changed
        if target.is_in(&BOARD_INFO) && target.item() != Some("set_reworks") {
            return Ok(());
        }
        Err(guard.reason.as_str())
    }
}
```

`ww api scaffold --call-gate` prints the `ww_codegen!` argument and an `impl CallGate` that admits everything.
Without `call_gate` no code is generated for it.

## What the gate is asked

`admit` runs before the request's arguments are decoded, for every request that reaches an item:

| Request | `Target` | `Op` |
|---|---|---|
| a method call | the method | `Call` |
| a property or `const` read, a property write | the property | `Read`, `Write` |
| a stream or sink open, close and other sideband | the stream or sink | `StreamOpen`, `Stream` |
| a write into a sink | the sink | `SinkWrite` |
| the valid indices of a resource array | the array item | `Read` |
| one index of a multi call, read or write | the item at that index, asked per index | `Call`, `Read`, `Write` |
| an observe request | the observed trait level (`level` is true, `item()` is `None`) | `Observe` |

A `Target` names the item by the trait it is defined in, `trait_id`, a crate and a trait name. A global trait is the
same `TraitId` wherever it sits in an API: `ww_board_info::BoardInfo` under `board` or under `io.board` both match
`BOARD_INFO`. `item()` is the item's name in that trait. `path` is the whole resource path from the root, trait items
and the item joined by dots (`board.set_reworks`), without array indices: it tells apart two uses of a project's own
trait.

Introspection is never asked, so a client can always learn what the device is. Streams that are already open keep
sending: the gate decides at the open. A firmware that enters its guarded state while streams run closes them
itself.

`admit` takes `&self` and is asked on every request, so the state it reads is the state the device is in at that
moment. A flag set from another task or an interrupt takes effect with the next request.

## The refusal

A refused request is answered with `ww_client_server::ErrorKind::Refused(reason)` at error site 10 (`err_seq::GATE`,
[Where on the device an error came from](errors.md#where-on-the-device-an-error-came-from)), or 60
(`OBSERVE_GATE`) for an observe request. Each index of a multi request that is refused gets the error in its slot,
the others are served. The reason is the firmware's own words and is sent with every refusal, so keep it short
(`board guarded: unknown rework 9`).

On the host, `wire_weaver_client` turns it into `Error::Refused { reason, err_seq }` (`is_refused()`), and `ww`
ends the command's error with a pointer to `ww info`, which shows the device's state:

```text
... The device refused the request: board guarded: unknown rework 9
run 'ww info' to see the device's state
```

`Refused` was appended to `ErrorKind`, so the wire is compatible both ways: a host built before it reads the
refusal as `Unknown(17)` and still reports an error.

## Cost on the device

Measured on a Cortex-M3 under QEMU (instructions, not cycles) with a server of five `ww_dfu` traits, 21 items, at
`opt-level = "z"` with fat LTO, the profile firmware ships with (`examples_mcu/mcu_qemu/src/bin/call_gate.rs` has the
table and how to run it):

- An admitted request takes 33-41 instructions more, 61 when the gate compares a trait. A `TraitId` compares by
  pointer first: the linker merges the firmware's string literals with the generated ones, so no bytes are compared.
- A refusal replaces the handler with writing the error and its reason.
- Flash: 772 B for the 21 items. Each item has a 16-byte `Target` and its path, and every item arm calls one shared
  function.
- Without `call_gate`: nothing.
