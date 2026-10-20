---
level: basic
---

# ww_pin_diag

`ww_pin_diag` gives a test raw access to a board's digital points, for bring-up, verification and production tests:
shorts to GND, to the supply and between pins, external pull resistors, floating pins, and which pins follow an
expander output. It is one generic trait, so one host tool tests any board that implements it.

```rust
#[ww_trait]
pub trait PinDiag {
    const POINTS: RefVec<'i, Point<'i>>;
    fn set(point: u16, state: PointState) -> Result<(), PinDiagError<'i>>;
    fn read(point: u16) -> Result<Level, PinDiagError<'i>>;
    fn release_all();
}

pub enum PointState {
    Input { pull: Pull },
    Output { level: Level, drive: Drive }, // Drive::Normal or Drive::Weak
}
```

## Points

A *point* is anything digital the firmware lets a test touch: an MCU pin, an output of a shift register, a pin of an
I/O expander. `POINTS` lists them, each with a `name` ("GPIO2", "EX13", as on the schematic), a `group` ("mcu",
"chain") and `caps`: whether it can be read, pulled up or down, drive low or high, and whether it has a weaker drive.
A point's index in the list is its id in `set` and `read`. `POINTS` is a [`const` item](../api/properties.md#const-items);
a server built from a table provides it as a `fn` (`const_model = "POINTS=fn"`, `fn pins_points(&self)` for
`pins: PinDiag`).

`set` takes a point away from its normal function until `release_all`, which gives every point back and returns the
board to the firmware's safe state. `read` gives the level on the pad, also while the point drives: a point driving
high that reads low is shorted to something stronger. A host sets and reads one point per call; the framer packs
many small calls into one frame, so there are no multi-point methods.

## Safety stays in the firmware

How a point is made, and what is safe to do with it, is the firmware's knowledge. A shift register output is a point
like any other: the host asks for a level, the firmware checks its board's rules and shifts the chain, or refuses.
The trait carries only the outcome:

| Error | When |
|---|---|
| `NoSuchPoint` | no point with that id |
| `NotCapable` | the point can never do this (see its caps): reading a shift register output, driving an input-only pin |
| `Common(InUse)` | a function holds the point now |
| `Refused { rule }` | the board's rules forbid it in the current state of the other points: "two VREF sources" |

A change of several points is several `set`s, each checked on its own. A host orders them so that each step is safe
(turning a source off before turning another on); a step through an unsafe state is refused, never done.

Drive strength and slew rate are the firmware's choice. `Drive::Weak` asks for the weakest drive the point has: a
short-to-GND test drives high weakly and reads back, so a dead short wins and an external pull-down doesn't.

## Names as interned strings

Names and refusal rules are [`ww_interned::Interned`](#ww_interned): an index into the firmware's string table
(`Id`), resolved by the host from the firmware's ELF, or the text itself (`Text`) from a device without a table. A
host that can't resolve an id shows `#17`.

## ww_interned

```rust
pub enum Interned<'i> {
    Id(UNib32),     // index into the firmware's string table
    Text(&'i str),  // the text itself
}
```

Text costs flash and bytes on the wire; an id costs a byte or two. The text lives only in the firmware's ELF, and a
host finds the ELF by the device's build id, as `ww counters` and `ww report` do. On embedded the plan is defmt's own
table: `defmt::intern!` already places strings in the ELF's `.defmt` section, and an accessor for a `defmt::Str`'s
index is being proposed upstream. Until then a device sends `Text`.

## What a host does with it

- **Pulls**: read with the internal pull-up, then the pull-down (after driving the opposite level, so input latches
  such as RP2350-E9 can't hold a stale level): external pull-up, external pull-down, or floating.
- **Shorts to GND / supply**: drive high weakly and read back (low: shorted to GND), drive low and read back (high:
  shorted to the supply).
- **Shorts between points**: drive each point alone, low with the others pulled up, then high with the others pulled
  down; the points that follow are shorted to it.
- **Connectivity**: drive an expander output and see which MCU pins follow.

The test pair `tests/pin_diag` runs these against a mock board with a shorted pair, an external pull-up, an
input-only pin, two interlocked shift register outputs and a pin held by a function.

## `ww pins`: the tests on any device

`ww pins test` does all of the above on any device serving `PinDiag`, found through its introspection data wherever
the API has it, so a board needs no test code of its own on the host:

```
$ ww pins list
$ ww pins test GPIO0,GPIO5,P8,GPIO2..=GPIO7,'chain.*'
$ ww pins test --plan b200a.toml
$ ww pins test GPIO0..=GPIO9 --save b200a.toml
```

Each point is tested only with what its caps allow; what can't be checked says *not checked*, a refusal says
*refused* with the device's reason, and neither stops the run. `release_all` ends every run, also on Ctrl-C and
errors. A plan file adds expectations per point (`expect = "pull_up"`), narrower `allow` lists, and the points that
are one net on the board; `--save` writes what a good board shows into it. The options, the selection syntax and the
plan format: [command reference](../cli_reference.md#ww-pins).

The tests are a library, `ww_pin_probe`, so a test bench uses them without the CLI: `probe::run` over anything
implementing its `Pins` trait (`device::DevicePins` for a connected device, `mock::MockBoard` for tests), with
`select::select` for the point list and `plan::Plan` / `plan::save` for plan files.
