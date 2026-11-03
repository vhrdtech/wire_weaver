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
        // the gate decides BoardInfo item by item, see below
        call_gate_items = "ww_board_info::BoardInfo",
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

The gate is coarse by default, so its flash cost follows the number of traits, not of items:

- **A trait level, before a request goes into it.** A request for `board.set_reworks` first asks for the `board`
  level: a `Target` whose `trait_id` is the trait attached there (`ww_board_info::BoardInfo`), `path` is `board`,
  `level` is true and `item()` is `None`. A refusal ends the request there. Nested levels are asked on the way, one
  by one (`io`, then `io.board`). Each index of a resource array of traits asks for its level.
- **The items of the root level**, which belong to no attached trait.
- **The items of the levels named in `call_gate_items`**, after their level ([Asking item by item](#asking-item-by-item)).

Every other item is not asked on its own: admitting its level admits all of it. The `Op` is what the request does,
whether the gate is asked for the level or for the item:

| Request | `Op` |
|---|---|
| a method call | `Call` |
| a property or `const` read, the valid indices of a resource array | `Read` |
| a property write | `Write` |
| a stream or sink open | `StreamOpen` |
| a stream's close and other sideband | `Stream` |
| a write into a sink | `SinkWrite` |
| one index of a multi call, read or write, asked per index | `Call`, `Read`, `Write` |
| an observe request | `Observe` |

An observe request asks for the levels on the way to the observed level, and the observed level itself; where that
level's items are asked one by one, each observed property is asked too (`level` false). Observing the root asks for
the root level, with an empty `path`.

A `Target` names a level or an item by its trait, `trait_id`, a crate and a trait name: for a level the trait
attached there, for an item the trait it is defined in. A global trait is the same `TraitId` wherever it sits in an
API: `ww_board_info::BoardInfo` under `board` or under `io.board` both match `BOARD_INFO`. `item()` is the item's
name in that trait, `None` for a level. `path` is the whole resource path from the root, trait items and the item
joined by dots (`board`, `board.set_reworks`), without array indices: it tells apart two uses of a project's own
trait. The valid indices of a resource array are an item of the level that holds the array.

Introspection is never asked, so a client can always learn what the device is. Streams that are already open keep
sending: the gate decides at the open. A firmware that enters its guarded state while streams run closes them
itself.

`admit` takes `&self` and is asked on every request, so the state it reads is the state the device is in at that
moment. A flag set from another task or an interrupt takes effect with the next request.

## Asking item by item

A trait the firmware admits in part, like `BoardInfo` above that may be read but not have its reworks set, is named
in `call_gate_items`, by trait or by resource path, as in the `ww_codegen!` call above.

A trait name covers every place the trait is attached; a path (`board`, `io.board`) covers that one. The gate is then
asked for the level, and after it for each item: `admit` lets the level through (`target.level &&
target.is_in(&BOARD_INFO)`) and decides by item (`target.is(&BOARD_INFO, "set_reworks")`). The `impl CallGate` above
admits the `BoardInfo` level and all of its items but one; without `BoardInfo` in `call_gate_items` it would admit
`set_reworks` with the level. A name that is no trait level of the API, or `call_gate_items` without `call_gate`,
fails to compile with the list of levels. `ww api scaffold --call-gate --call-gate-items NAMES` prints both
arguments.

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
table, `opt-level = 3` and how to run it):

- An admitted request takes 32-37 instructions more, 60 when the gate compares a trait. A `TraitId` compares by
  pointer first: the linker merges the firmware's string literals with the generated ones, so no bytes are compared.
  An item asked as well adds about 30.
- A refusal replaces the handler with writing the error and its reason.
- Flash: 636 B with the five levels asked, mostly fixed (one shared function every check calls, the op helper, a
  16-byte `Target` per level). Asking all 21 items one by one adds 276 B at `z` and 1420 B at `opt-level = 3`: a
  `Target`, its path and the check per item. So name in `call_gate_items` only the traits the firmware splits.
- Without `call_gate`: nothing.
