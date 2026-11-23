---
level: advanced
---

# Metadata: effects, claims, pins, labels, tracks

Some things about an API matter to people and tools but not to the bytes on the wire: what calling a method does,
which resources one program may have to itself for a while, which pins a resource uses, what a person calls it,
how a recorder lays a stream out on a tape. They are written as typed
attributes next to the items, checked when the API is loaded (a typo is a compile error of the API crate, like a
wrong `#[errors(..)]` list), and carried in the device's introspection data as data, not as doc text, so the hub,
`ww` and GUIs read them without parsing prose.

```rust
#[ww_trait]
#[claim(exclusive)]
pub trait Uart {
    // ...
}

#[ww_api_root]
pub trait Board {
    /// Both CAN controllers: cooperating tools may claim them together.
    #[claim(shared)]
    #[shares(index = 0, pins = "PA11, PA12", peripherals = "FDCAN1")]
    #[shares(index = 1, pins = "PB12-PB13", peripherals = "FDCAN2")]
    #[label("CAN1, DB-9 on the front", "CAN2, terminal block")]
    ww_impl!(can[]: ww_can_bus::Can);

    /// Claimable through its trait (`Uart` above), with the pins it takes from port A.
    #[shares(pins = "PA9, PA10", peripherals = "USART1")]
    #[label("Debug UART, J4")]
    ww_impl!(uart: ww_uart::Uart);

    #[shares(pins = "PA0-PA15")]
    ww_impl!(gpio_a: ww_gpio::Bank);

    #[label("Main power")]
    #[effect(actuate)]
    property!(rw power: bool);
}
```

Nothing changes on the device: a server is generated the same way. Adding, changing or removing any of these
attributes is wire compatible (only the introspection data and so the API hash change, as for a doc comment);
`ww api check` reports them as compatible changes.

Claiming through the hub, its `claims` API group, is described in
[Claims on resources](../transport/hub.md#claims-on-resources-srv-34).

## `#[effect(..)]`: what an item does

Every method, writable property and sink says what using it does, so a GUI knows which buttons it may press
freely, an agent or a script knows what to confirm first, and the hub knows which calls only read. One or more
effects, on the item or on its trait:

```rust
#[ww_trait]
#[effect(edit)] // the default of every item below without its own
pub trait Settings {
    #[effect(read)]
    fn summary() -> Summary<'i>;
    /// Writes the settings to flash.
    #[effect(persist)]
    fn save();
    /// Back to the factory settings, in flash.
    #[effect(persist, destructive)]
    fn reset_to_defaults();
    /// Takes the `edit` of the trait.
    property!(rw window: Millis<u16>);
}
```

| Effect | Meaning |
|---|---|
| `read` | changes no state; combines with no other effect |
| `edit` | changes state that can be undone or overwritten: a setpoint, a mode, a volatile buffer |
| `persist` | writes non-volatile storage: wears flash, survives a reboot |
| `destructive` | loses data: an erase, a cleared log, a factory reset |
| `permanent` | an irreversible hardware change: OTP, fuses, readout protection |
| `outbound` | reaches outside the system: sends a frame on a bus, publishes, calls out |
| `actuate` | switches power or outputs, moves things: a rail, a relay, a motor enable, an LED |
| `reset` | reboots the device or drops the link |
| `job` | starts work that outlives the call: a capture, a scan, an update |

- Required: a method, a writable (`rw`, `wo`) property or a sink without an effect of its own or of its trait is a
  compile error naming it.
- On a trait definition (after `#[ww_trait]`) the attribute is the default for that trait's items; one on an item
  wins. It doesn't reach the items of traits nested with `ww_impl!`, and it can't go on a `ww_impl!`.
- Reads by their kind need none: a property read, a `ro` or `const` property, a stream open (`stream!`, a stream
  function) are `read`. On a property, `#[effect]` describes its write. A stream whose opening does more (powers a
  sensor) may say so: `#[effect(actuate)] stream!(..)`. A `ro` property or a const can't have one.
- `read` is the API author's promise, nothing checks it. A FIFO that a read empties is no `read`: make it a method
  with `edit`, or claim it with `#[claim(observe)]`.

Who reads them:

- The hub treats a call of a `read` method as a read: it stays open under another program's exclusive or shared
  claim, like a property read. With per-peer rights (SRV-37), a peer with only the right to observe may call such
  methods.
- An `Authorize` hook gets each item's effects and metadata in `Resource::items`, a read by kind included
  (`resource.effects()` for the item itself): a server's for every request, `Authorized`'s for every call in the same
  process ([Authorization](host_server.md#authorization)). A hook can hold an `outbound` or `destructive` call until
  a person approves it ([Waiting for an answer](host_server.md#waiting-for-an-answer)).
- `ww info --effects` lists the device's items that change something, by effect; `ww api tree` shows the attribute
  in front of each item, rustdoc of a `#[ww_trait]` notes it.

### As MCP tool annotations

A tool that exposes API items to a language model through the Model Context Protocol maps the effects to its tool
annotations:

| MCP annotation | From the effects |
|---|---|
| `readOnlyHint` | `true` for `read` alone |
| `destructiveHint` | `true` with `destructive`, `permanent` or `reset`, with a bit the tool doesn't know, or with none declared; `false` otherwise (an `edit` can be written back) |
| `idempotentHint` | `true` for `read`; otherwise left out (`false`), the effects don't say |
| `openWorldHint` | `true` with `outbound`, `false` otherwise |

`actuate` and `permanent` have no MCP annotation of their own: a host asks before such a call, as `ww` and GUIs
should.

## `#[claim(..)]`: resources one program can have to itself

Several programs can share one device through the hub (one per user, it holds the devices and serves every client).
Most of the time that is what you want: a GUI shows a CAN bus while a script sends frames on it. Sometimes one
program needs a resource to itself for a while: a test sequence that reconfigures a bus, a firmware update. It then
*claims* the resource, and until it releases it, the hub refuses other programs' calls and property writes at or
under that path with `CommonError::InUse`, naming who holds it, before they reach the device.

What a claim takes depends on the mode the API gives the resource:

| Attribute | Claims | Others' calls and writes | Others' reads, streams, observing |
|---|---|---|---|
| `#[claim]`, `#[claim(exclusive)]` | one client at a time | refused | open |
| `#[claim(shared)]` | several clients together (tools that cooperate on a bus); an exclusive claim of it or an overlapping resource is refused until the last one lets go | refused for clients without a claim | open |
| `#[claim(observe)]` | one client at a time | refused | refused too: for a resource where reading has an effect (a FIFO that a read empties, a capture another client would restart) |

- On a trait definition (after `#[ww_trait]`, it is read from the source like the rest of the trait), every
  `ww_impl!` of that trait anywhere in an API is claimable in that mode. Stdlib IO traits are marked this way, all
  `exclusive`: `ww_can_bus` (`Can`, `CanGroup`, `CanTimeTriggered`, `CanXl`), `ww_uart::Uart`, `ww_i2c::I2c`,
  `ww_spi::Spi`, `ww_gpio::Bank`, `ww_dfu` (`BootControl`, `Stage`, `StageBulk`) and `ww_pin_diag::PinDiag`.
- On a `ww_impl!`, that one place is claimable in that mode, whatever its trait says. For an array
  (`ww_impl!(port[]: ..)`) each element is claimed on its own (`port[2]`).
- The root of a device is always claimable, exclusively: claiming it takes the whole device (a firmware update
  does).
- Claims nest along the path: a claim on `can` conflicts with one on `can.interface[0]` held by another program.
- Methods, properties, streams and consts can't be marked: a claim is on a resource, put the attribute on the
  `ww_impl!` (or the trait) they belong to.

A program connected to a device directly (no hub) holds the whole device anyway, so claims only matter through the
hub.

## `#[shares(..)]`: pins and peripherals a resource uses

Two resources can be different paths in the API and still use the same hardware: a UART's pins are also pins of a
GPIO bank, two buses share a DMA controller. `#[shares(..)]` on a `ww_impl!` names them, so the hub refuses a claim
that overlaps one held on another resource:

- `pins = "PA9, PA10"`: pin names as the board knows them; `PA0-PA15` is a range (same prefix, a number at the end
  of both, at most 256 pins). A name that only looks like a range (`VDD-IO`, `PA1-PB2`) is kept as it is.
- `peripherals = "USART1, DMA1"`: peripherals or any other named thing (`5V rail`), compared by name.
- `index = N` on a resource array: the line is about element `N`; without it, about every element. Write one
  `#[shares]` per element.

Pins and peripherals belong to a board, not to a reusable trait, so `#[shares]` goes on the `ww_impl!` in the
board's API, not on a trait definition.

## `#[label(..)]`: what a person calls it

`#[label("CAN1, DB-9 on the front")]` on any item (a `ww_impl!`, method, property, stream or const) is the name a
person recognises on the bench. `ww` shows it, an `InUse` error names the claimed resource by it, GUIs put it next
to the item. A resource array takes one label per element, by index: `#[label("CAN1", "CAN2")]`; any other item
takes one.

## `#[track(..)]`: how a stream is recorded

A recorder that writes a device's streams to a tape (`ww_tape`, through `ww_tape_bundle` in the ww_tools repository)
needs to know what an item of each stream is there: an event at an instant, an event with a start and an end, or a
sample of a series. `#[track(..)]` on a `stream!` or a stream function says so, or on a trait for every stream of it
without its own:

```rust
#[ww_api_root]
#[track(points, dims(input, producer))] // the default of every stream below without its own
pub trait Detector {
    /// Something seen, at an instant.
    stream!(events: Event);

    /// A detection with a start and an end, its severity in the flags word.
    #[track(spans, dims(input), flags)]
    stream!(detections: Detection);

    /// Left and right levels: two channels of a series.
    #[track(series, dims(input))]
    stream!(levels: Levels);
}
```

| Layout   | An item on the tape                                                                                  |
|----------|------------------------------------------------------------------------------------------------------|
| `points` | an event at an instant, the item its payload; what a stream without `#[track]` is                     |
| `spans`  | an event with a start and an end, which the tape holds; the item holds the rest                       |
| `series` | a sample at an exact period, no time per item; the writer gives the period, the API doesn't           |

- The layout comes first, then `dims(..)` and `flags` in any order, each at most once.
- `dims(input, producer, ..)` names the dimensions a track of the stream may carry, at most 16, each once. A writer
  gives their values when it records (`input=mic1`); a name not listed is refused. Without `dims` a track has none.
- `flags`: every item carries a `u32` flags word beside it (a severity in its low bits, which summaries OR together).
  Points and spans only; `#[track(series, flags)]` is refused.
- A `series` item is a number, a fixed-size array of numbers or a struct of numbers, nested (each number a channel,
  a `Fixed<..>` is its number). Anything else (a bool, a string, a vector, an enum, an option) is refused when the
  API is loaded. A dependency's type the device leaves out of its introspection data is checked by the recorder once
  the host has put its definition back (`ApiBundleOwned::series_shape` answers `Unresolved` until then).
- Only streams a device sends are recorded: `#[track]` on a method, property, const, sink or `ww_impl!` is refused.
  A trait's `#[track]` reaches the streams of that trait, not those of the traits it has through `ww_impl!`, which
  have their own.

The model names no tape type: a recorder maps `points` to `ww_tape`'s `EventsDef { ends: Instants }`, `spans` to
`EventsDef { ends: Spans }` and `series` to a `SeriesDef`. Changing a stream's layout or `flags`, or dropping a name
from `dims`, makes its tracks other tracks: `ww api check` reports a break of the API crate that keeps the wire,
adding a name is compatible ([recorded streams](../evolution/rules.md#recorded-streams)).

## In the API model

`ww_self::Meta` holds the attributes: `claim: Option<ClaimMode>`, `shares: Vec<Shares>`, `labels: Vec<String>`,
`effects: Effects`, `track: Option<Track>`. It sits on `ApiItem::meta` (the item's own attributes) and
`ApiLevel::meta` (a trait definition's `#[claim]`, `#[effect]` and `#[track]`), both `Option` and evolvable, so the attributes that come later (device-level metadata,
API-57) are new fields there. `Effects` is a set of bits, one per effect in the table's order (`read` is bit 0);
the codegen stores on each item its own effects or its trait's, and nothing for a read by kind, so introspection
grows only where an item changes something. A new effect takes the next bit, and a reader keeps a bit it doesn't
know (`Effects::unknown`), treating the item as the most restrictive case. `ApiItemOwned::claim_mode(&bundle)` answers how a
`ww_impl!` can be claimed (its own `#[claim]`, else its trait's), `ApiItemOwned::label(index)` its label,
`ApiItemOwned::effects()` its effects. `Track` is a `TrackLayout`, the `dims` names and `flags`; the codegen stores
on each stream its own or its trait's, nothing on a stream without either (model format 0.6.0). `ww api
tree` shows the attributes in front of the item, and rustdoc of a `#[ww_trait]` notes them.
