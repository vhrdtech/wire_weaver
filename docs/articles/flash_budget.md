---
level: nomicon
---

# What a server costs in flash

A WireWeaver server on a small MCU pays for three things: a fixed floor (request decoding, the reply envelope, the link), the dispatcher's code for each resource of the API, and the serdes of the types those resources use. This page is the harness that measures the first two on the same firmware every time, and the numbers it gave. Every change to server codegen, `shrink_wrap` or the device crates reports its effect here, before and after.

## Running it

```sh
just flash-budget                                   # Cortex-M0+ (thumbv6m-none-eabi)
just flash-budget --targets thumbv7em-none-eabihf   # other cores
```

It builds the `qemu-flash-*` bins of `examples_mcu/mcu_qemu` and reads the flash of each ELF: every allocated section with contents (code, read-only data, vector table). Nothing runs, so no QEMU and no board are needed. Each build is done at both profiles firmware ships with, `fw-z` (fat LTO, one codegen unit, `opt-level = "z"`) and `fw-3` (the same at `opt-level = 3`), and with both server flavours: `use_async = false`, and `use_async = true` with sync handlers (`handler_model = "_=sync"`), as Donguru's firmware is built.

`just dispatch-budget` is the same server in instructions: `qemu-dispatch-requests` runs the `BaseOnly` server under QEMU (Cortex-M3, `lm3s6965evb`, counted by `qemu_icount`) on one request of each kind (a call with and without arguments, a property read and write, a stream open and close, a bad path, a request kind the resource doesn't take) and prints the instructions from `process_request_bytes` to its reply, with the reply's length and hash, at `fw-z` and `fw-3`, sync and `use_async`. `--qemu-host HOST` runs QEMU on another PC.

## How it measures

The servers are the roots of `tests/flash_budget_api`:

- `Floor`: one method, what any server costs.
- `BaseOnly`: a trait level with two resources of each kind (calls with and without arguments, a read-write property, a stream).
- `Calls8` / `Calls16`, `CallsArgs8` / `CallsArgs16`, `Props8` / `Props16`, `Streams8` / `Streams16`: that level plus 8 or 16 resources of one kind at the root. They all use the same two types, so serdes is paid once and `(size(*16) - size(*8)) / 8` is the dispatcher's own cost of one more resource of that kind.
- `Typed8` / `Typed16`: calls with an argument and an output of their own types, as in a real API, so each one's serdes is in its cost too.
- `MaxLenBounds8` / `MaxLenBounds16` (SW-72): read-write properties of `MaxLen<&'i str, N>`, a different bound each (4, 8, .., 64), as a real API's bounded fields rarely share one `N`. Excluded from the mean below, like `Typed*`: its per-resource type differs by resource too.

Two things keep the numbers honest. The request the server gets and every value a handler returns go through `core::hint::black_box`, so the compiler can't drop an arm it would know is never taken. Each handler is `#[inline(never)]` with a body of its own (a static it touches): identical handlers would be merged by LLVM, and the arms calling them with them.

Comparing two sizes that both have resources at the root matters. Against `BaseOnly`, whose root holds only a nested level, adding resources changes the root level's code shape too, and the inliner's choices with it: the server function shrank by 560 B when 16 calls were added in one early try.

The run also checks the largest server for code a firmware should not carry unless it asks for it: the `Series` codec of observe (SRV-54), `core::fmt` `Debug` of `shrink_wrap::Error` with its variant names (SW-69), and the global trait path lookup by name (SRV-53).

## Baseline

Measured 3 Nov 2026 on wire_weaver master of that day (5e695078) plus the harness, bytes:

| thumbv6m, per resource            | z, sync | z, async | 3, sync | 3, async |
|-----------------------------------|--------:|---------:|--------:|---------:|
| floor, one method                 |   26286 |    27110 |   41158 |    41862 |
| call, no arguments or output      |      52 |       56 |     129 |       56 |
| call, an argument and an output   |     240 |       32 |     128 |       22 |
| property, read and write          |     193 |      172 |     263 |      158 |
| stream                            |     496 |      201 |     353 |      209 |
| call with types of its own        |     438 |       76 |     184 |       96 |
| mean of the four shared-type kinds |    245 |      115 |     218 |      111 |

The `Series` codec is linked in every server (36 symbols, 3796 B at `z`), though none of them observes anything. Neither the `Debug` names of `shrink_wrap::Error` nor the trait path lookup by name are in these servers: the first comes from a caller in a real firmware's stack, the second only with global traits in the API.

The per-kind numbers move with the inliner's choices, most in the `use_async` builds, where the arms of a level live inside one state machine. Compare the same cell before and after a change, and the mean for the overall trend. Compare only builds from the same checkout path: panic locations carry each source file's path, so a worktree at a shorter path is smaller by tens of bytes per file (one change looked 250 B better than it was, built in `/tmp`).

For scale: Donguru (typec_dongle, `use_async`, `fw-z` with `opt-level = "z"`, 43 resources of many types) spent about 616 B per resource under the generated dispatcher at wire_weaver 8b7b80ed, and 41.4 KB in all, measured from its ELF by inlined frames.

## SW-69: hand-written `Error` Debug

Measured 3 Nov 2026, before (commit `f8f78726`, the harness alone) against after (with SW-69's hand-written `Debug`), bytes:

| thumbv6m, floor, one method | z, sync | z, async | 3, sync | 3, async |
|------------------------------|--------:|---------:|--------:|---------:|
| before                       |   26378 |    27202 |   41210 |    41914 |
| after                        |   26346 |    27166 |   41174 |    41878 |
| change                       |     −32 |      −36 |     −36 |      −36 |

The per-resource numbers (the "8 resources" row and every "one more X" row) don't move beyond ±1 B rounding, and the SW-69 check still reports "not linked" both before and after: none of this harness's servers formats `shrink_wrap::Error` with `Debug` or `Display`, so the ~450 B of variant names the feature item describes never reaches these builds either way. What moved is a small, constant-size remnant of the derived `Debug` impl that LTO didn't fully strip even though nothing called it. The bytes SW-69 actually saves show up only where a real on-target caller formats the error (or a type nesting it) through `core::fmt::Debug`: fixed in this repo at `ww_settings::Error` / `LoadReport` (`examples_mcu/nucleo_{g0b1re,h743zi2}` `settings.rs`, both already `defmt::Format`) and `ww_stream::store::RamRingError` (`examples_mcu/rp2` `usb_log.rs`, now behind `ww_stream`'s new `defmt` feature); Donguru's own `settings.rs` has the same shape of fix to make, on its own branch.

These absolute floor numbers run a little higher than the "Baseline" table above (e.g. 26378 B against 26286 B at `fw-z, sync`), measured the same day: most likely the checkout path (see the note above this page's Baseline table), since this worktree's path (`/home/roman/git/.wt/flash-sw-debug/wire_weaver`) is longer than whatever path the baseline table was built from, and panic locations carry it per source file. The before/after pair here was measured back to back from the same checkout, so the diff between them is the number that matters regardless.

## Observe left out (SRV-54)

Server code stopped serializing whole `Event`s: the reply of a deferred method, introspection, stream sidebands and `ww_device`'s generic error are written in place, one writer per event kind (`ww_client_server::util`). `Event`'s serializer covers every `EventKind`, so any use of it linked the `Series<Second>` codec of observe's `sample_times`, the `StreamHead` writer and their `exact_units` code into every firmware. The body of `RequestKind::Observe` became a `RefBox<ObserveRequest>` (the same bytes), so only a server that observes links its decoder. Measured on f62c99f5 (before) and with the change (after), both built in the same worktree, bytes:

| thumbv6m                          | z, sync before | after | z, async before | after | 3, sync before | after | 3, async before | after |
|-----------------------------------|------:|------:|------:|------:|------:|------:|------:|------:|
| floor, one method                 | 26274 | 14510 | 27098 | 15502 | 40910 | 20086 | 41694 | 21118 |
| 8 resources, two of each kind     | 29482 | 18722 | 30454 | 19354 | 44458 | 23698 | 45194 | 24622 |
| call, no arguments or output      |    52 |    55 |    55 |    50 |   129 |   130 |    56 |    52 |
| call, an argument and an output   |   240 |   242 |    32 |    22 |   128 |   124 |    22 |    21 |
| property, read and write          |   193 |   173 |   172 |   179 |   263 |   226 |   158 |   212 |
| stream                            |   496 |   484 |   201 |   204 |   353 |   362 |   209 |   201 |
| call with types of its own        |   438 |   300 |    76 |    80 |   184 |   136 |    96 |    96 |
| mean of the four shared-type kinds|   245 |   238 |   115 |   114 |   218 |   210 |   111 |   122 |
| `Series` codec                    |  3796 |     0 |  3796 |     0 | 16286 |     0 | 16286 |     0 |

Of the floor's 11.8 KB at `z` (sync), the events written in place are about 11.2 KB and the lazy `Observe` body about 0.46 KB (0.87 KB at `3`), measured as two steps on ff395f1a. The 8-resource servers lose 1 KB less than the floor: their stream links the `StreamHead` writer again, which the floor carried before only through `Event`'s serializer. The per-resource cells stay within the inliner's noise. Firmware that observes is unchanged: `qemu-foc-observe` prints the same as before.

## SW-70: the bit helpers as hints

Measured 3 Nov 2026 against the harness alone (commit `67c1dd91`, with the SW-70 change on top of it): the private helpers of the bit writer and reader are `#[inline]` instead of `#[inline(always)]` unless the firmware asks otherwise with `--cfg shrink_wrap_hot` ([Hot paths at opt-level z](../serdes/code_size.md#hot-paths-at-opt-level-z)). The floor at `fw-z`: 26350 → 25610 B sync, 27174 → 26434 B async (−740 B), the `Series` codec in it 3796 → 3224 B; `fw-3` unchanged; every per-resource cell unchanged. With `shrink_wrap_hot="inline"` the same bytes as before, with `"optimize"` (nightly) +208 B. Lists of plain numbers share one non-generic length-and-bytes path: no list in these servers, Donguru 157728 → 157640 B at `z` and 229344 → 228452 B at `3`.

## SW-72: `MaxLen<_, N>`'s bound check through one helper

`tests/flash_budget_api` had no `MaxLen` field before this item, so it got `MaxLenBounds8` / `MaxLenBounds16` (above) to measure against. Before and after, measured in the same worktree (absolute path length affects panic-location strings, so a cross-worktree comparison is noise; the mean of the four shared-type kinds above is identical in both runs, confirming the rest of the build is deterministic):

| thumbv6m, one more `MaxLen<&str, N>` property | z, sync | z, async | 3, sync | 3, async |
|------------------------------------------------|--------:|---------:|--------:|---------:|
| before: `if len > N` written at each of the four call sites | 236 B | 206 B | 324 B | 292 B |
| after: shared `check_max_len(len, max)`, plain `fn` | 236 B | 206 B | 324 B | 292 B |
| after, `#[inline(never)]` on `check_max_len` (what the item asked for) | 246 B | 205 B | 377 B | 337 B |
| `#[inline(never)]` vs before | +10 B | −1 B | +53 B | +45 B |

The estimate (9 monomorphised copies on Donguru, "est. 200-250 B" in FEATURES.md) assumed the duplicated copies could be merged away. They can't: each copy still needs its own `N` to compare against, baked in as a call argument either way, so nothing byte-identical is left for the linker or `MergeFunctions` to fold. Forcing the check out of line with `#[inline(never)]` only adds the call's own overhead (argument setup, branch-and-link, the `?` on its `Result`) on top of a comparison that was already about as small as an immediate compare gets — and costs more of it at `fw-3`, where the optimizer would otherwise have inlined a function this small for free. Left as a plain `fn`, the compiler inlines it back at every site, landing on the exact bytes the duplicated form already had: one copy in the source to read and maintain, not a smaller binary. Shipped as the plain `fn`.

## Dispatcher arms: only what differs (SRV-51)

Every call, read and write arm of the generated dispatcher used to carry the whole reply inline: the event kind builder, a `map_err` per fallible step, the `seq != 0` check, the serializer call and the full `RpcResult` / `GetResult` / `SetResult` match. In a `no_alloc` server an arm now keeps only what differs per resource: it decodes the arguments or the value (`ww_client_server::dispatch::des`), calls the handler and hands its whole result to a shared reply path in `ww_client_server::dispatch`. The paths are `#[inline(never)]` and generic over the value and user error types, so a firmware has one copy per type, not one per resource, and every error site of a level goes through one cold constructor (`dispatch::err`). The reply bytes are the same (TEST-31), and so are the handler signatures.

What each kind of resource generates:

| resource, model                     | the arm                                                              | shared path                                  |
|-------------------------------------|----------------------------------------------------------------------|----------------------------------------------|
| method, `immediate` or `deferred`   | decode the arguments, call                                           | `reply_call`: the output, nothing for `Deferred` (answered later), or `Unimplemented` |
| property, `get_set`, write          | decode the value, call the setter                                    | `reply_set` (`reply_set_unit_err` without a user error type) |
| property, `get_set`, read           | call the getter                                                      | `reply_get` (`reply_get_unit_err`)           |
| property, `value_on_changed`, write | decode, compare, store, mark observed, call `changed_..`             | `reply_ok`                                   |
| property, `value_on_changed`, read  | nothing: the field is the answer                                     | `reply_value`                                |
| `const`                             | the constant or its `fn`                                             | `reply_value`                                |
| array of resources, no index        | `valid_indices_..`                                                   | `reply_value`                                |
| plain stream, open                  | decode the arguments, call                                           | `reply_open_plain`                           |
| counted or reliable stream, open    | decode, call, start the session                                      | `reply_opened`                               |
| stream or sink sideband             | call the sideband handler                                            | `reply_sideband`                             |
| sink write                          | decode, call                                                         | `reply_ok`                                   |

A variant the handler's type allows but its model doesn't expect still has its old answer, in the shared path: an `immediate` method that returns `Deferred` sends nothing now, a stream function that defers is refused. A `no_alloc = false` server (a host) keeps the inline arms: its replies grow into a buffer of their own (CLT-55).

Measured 3 Nov 2026, before (master c440491e plus the harness) and after, both built in the same worktree, bytes:

| thumbv6m                           | z, sync before | after | z, async before | after | 3, sync before | after | 3, async before | after |
|------------------------------------|------:|------:|------:|------:|------:|------:|------:|------:|
| floor, one method                  | 14230 | 14502 | 15322 | 15730 | 20054 | 20434 | 21166 | 21758 |
| 8 resources, two of each kind      | 18438 | 17686 | 19098 | 18758 | 23670 | 24054 | 24674 | 25198 |
| call, no arguments or output       |    55 |    56 |    52 |    45 |   130 |    59 |    52 |    56 |
| call, an argument and an output    |   242 |    22 |    21 |    26 |   124 |    16 |    23 |    23 |
| property, read and write           |   173 |   110 |   157 |   110 |   226 |   136 |   172 |   116 |
| stream                             |   484 |   154 |   196 |   168 |   362 |   166 |   202 |   192 |
| call with types of its own         |   300 |    96 |    64 |    96 |   136 |    60 |    96 |    96 |
| mean of the four shared-type kinds |   238 |    86 |   106 |    87 |   210 |    94 |   112 |    97 |

The per-resource cells include the handlers' own bodies (a property's getter and setter are 48 B of the 110). The floor grows by the shared paths a one-method server now links (270-590 B), so a server with more than a few resources is smaller; at `fw-3` the 8-resource server is still 380-520 B larger, as each shared path inlines its serializers there. Every cell is steadier than before: the arms no longer decide whether LLVM inlines a level into its caller.

The first step alone, the cold error constructor without the shared reply paths, was measured too and isn't worth it on its own: the 8-resource server lost 16-492 B, but the floor grew 220-460 B and the per-resource cells got worse (a stream 484 → 786 B at `z`, sync), as the arms still built their replies inline around the calls.

Instructions per request, `just dispatch-budget` (Cortex-M3 under QEMU), warm run:

| request                         | z, sync before | after | z, async before | after | 3, sync before | after | 3, async before | after |
|---------------------------------|------:|------:|------:|------:|------:|------:|------:|------:|
| call, no arguments or output    |  2488 |  2579 |  2641 |  2739 |  1375 |  1388 |  1552 |  1542 |
| call, an argument and an output |  2723 |  3096 |  2869 |  3276 |  1647 |  1693 |  1806 |  2089 |
| property read                   |  2466 |  2567 |  2619 |  2725 |  1373 |  1395 |  1534 |  1534 |
| property write                  |  2596 |  2694 |  2748 |  2854 |  1458 |  1483 |  1636 |  1637 |
| stream open                     |  3541 |  3627 |  3695 |  3788 |  1979 |  1990 |  2143 |  2130 |
| stream close                    |  2260 |  2528 |  2405 |  2684 |  1375 |  1374 |  1538 |  1523 |
| error: bad path                 |  2712 |  3077 |  2858 |  3259 |  1679 |  1765 |  1835 |  2162 |
| error: kind not taken           |  2490 |  2858 |  2636 |  3037 |  1512 |  1598 |  1653 |  1978 |

The replies are the same bytes. Nearly all of the increase is `compiler_builtins`' `memcpy`, a byte loop at `opt-level = "z"`: 262 → 597 instructions for the call with an argument. It copies each level's 16-20 B `Result<WrAction, Error>`, now built by an out-of-line path and moved up through the levels. SRV-52 writes errors in place and returns a small `WrAction`, which removes those copies.
