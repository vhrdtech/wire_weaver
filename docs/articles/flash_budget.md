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
- `Globals8` / `Globals16` (SRV-53): 8 or 16 traits of one method each (`GlobalTraitN`), built with introspection off, so the server answers `GlobalFull` paths to them itself: the cost of one more trait in its global path lookup, its level included. Excluded from the mean too.

Two things keep the numbers honest. The request the server gets and every value a handler returns go through `core::hint::black_box`, so the compiler can't drop an arm it would know is never taken. Each handler is `#[inline(never)]` with a body of its own (a static it touches): identical handlers would be merged by LLVM, and the arms calling them with them.

Comparing two sizes that both have resources at the root matters. Against `BaseOnly`, whose root holds only a nested level, adding resources changes the root level's code shape too, and the inliner's choices with it: the server function shrank by 560 B when 16 calls were added in one early try.

The run also checks servers for code a firmware should not carry unless it asks for it: the largest one for the `Series` codec of observe (SRV-54) and `core::fmt` `Debug` of `shrink_wrap::Error` with its variant names (SW-69), `Globals16` for the global trait path lookup by name and the crate and trait names in flash (SRV-53).

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

The replies are the same bytes. Nearly all of the increase is `compiler_builtins`' `memcpy`, a byte loop at `opt-level = "z"`: 262 → 597 instructions for the call with an argument. It copies each level's 16-20 B `Result<WrAction, Error>`, now built by an out-of-line path and moved up through the levels. [SRV-52](#errors-written-in-place-srv-52) writes errors in place and returns a small `WrAction`, which removes those copies.

## Errors written in place (SRV-52)

Every level of the dispatcher returned `Result<WrAction, Error>` and passed it up with `?`, and the root wrote the error. That `Result` is 16-20 B, so each level copied it into its caller's, and LLVM spent `Try::branch`, `map_err` and the copies on every error site and every return. Now a level gets a small context, `ww_client_server::dispatch::Dispatch` (the request, the path, the writer's state where the reply starts, whether it is an element of a multi request), and returns a one or two byte `WrAction`. An error is written where it happens by one cold function, `dispatch::err`, that rewinds the writer to where the reply starts and writes the `Error` there; the level returns `WrAction::WrittenErr`. A level no longer carries the event kind builder up either: the root knows where the reply starts and fills in the kind's nibble there. In a multi request each element's errors are written where the element starts. Only when not even the error fits does the root return `WrAction::Failed`'s `shrink_wrap::Error`, as it did before. The reply bytes are the same (TEST-31).

Measured 3 Nov 2026 against SRV-51 (commit `6059e1a8`) and the SRV-51 baseline above, in the same worktree, bytes:

| thumbv6m                           | z, sync SRV-51 | SRV-52 | z, async SRV-51 | SRV-52 | 3, sync SRV-51 | SRV-52 | 3, async SRV-51 | SRV-52 |
|------------------------------------|------:|------:|------:|------:|------:|------:|------:|------:|
| floor, one method                  | 14502 | 14506 | 15730 | 15638 | 20434 | 20450 | 21758 | 21574 |
| 8 resources, two of each kind      | 17686 | 17370 | 18758 | 18258 | 24054 | 23694 | 25198 | 24598 |
| mean of the four shared-type kinds |    86 |    86 |    87 |    88 |    94 |   104 |    97 |    96 |

Against the baseline before SRV-51, the 8-resource server is now 1068 B smaller at `z` sync and 840 B async, and the same size at `fw-3` (+24 B sync, −76 B async). The saving grows with the number of levels a request passes: these servers have two.

Instructions per request, `just dispatch-budget` (Cortex-M3 under QEMU), warm run, before SRV-51 → after SRV-52:

| request                         | z, sync before | after | z, async before | after | 3, sync before | after | 3, async before | after |
|---------------------------------|------:|------:|------:|------:|------:|------:|------:|------:|
| call, no arguments or output    |  2488 |  2284 |  2641 |  2428 |  1375 |  1375 |  1552 |  1559 |
| call, an argument and an output |  2723 |  2742 |  2869 |  2891 |  1647 |  1624 |  1806 |  1807 |
| property read                   |  2466 |  2268 |  2619 |  2412 |  1373 |  1388 |  1534 |  1554 |
| property write                  |  2596 |  2399 |  2748 |  2543 |  1458 |  1475 |  1636 |  1659 |
| stream open                     |  3541 |  3326 |  3695 |  3472 |  1979 |  1974 |  2143 |  2144 |
| stream close                    |  2260 |  2252 |  2405 |  2398 |  1375 |  1374 |  1538 |  1544 |
| error: bad path                 |  2712 |  2724 |  2858 |  2874 |  1679 |  1697 |  1835 |  1881 |
| error: kind not taken           |  2490 |  2501 |  2636 |  2653 |  1512 |  1532 |  1653 |  1699 |

At `z` the answers take 6-8 % fewer instructions than before SRV-51, the error paths 0.4-0.6 % more; at `fw-3` everything is within 3 %. The future of `process_request_bytes` in the `use_async` server is 528 B, 16 B more than before (512 B): the context the root keeps across its await.

One kind got dearer through SRV-51 and SRV-52, measured on master with SW-72's `MaxLenBounds` kind: a `MaxLen<&str, N>` property with a different `N` each costs 309 B at `z` sync (236 B before), 316 B async (206 B), 526 B at `fw-3` sync (324 B) and 516 B async (292 B). The shared paths are generic over the value type, so every distinct type gets its own `des`, `reply_value` and `reply_get_unit_err`: 164 B per type at `z`, 360 B at `fw-3`, where `des` inlines the whole `&str` decoder. Calls with types of their own got cheaper all the same (300 → 96 B at `z`), as their serdes is per type anyway. Making the value paths take erased serializers is [SRV-55](#reply-paths-without-a-copy-per-type-srv-55); making `des` a plain `#[inline]` was tried and is no fix (`MaxLen` 526 → 478 B at `fw-3`, but a call with its own types 107 → 196 B).

## Reply paths without a copy per type (SRV-55)

After SRV-51 and SRV-52 the shared reply paths (`reply_call`, `reply_value`, `reply_get`, ...) were generic over the value type, so a firmware had one copy of each path per type, with the type's serializer inlined in it. In Donguru (typec_dongle) that was 177 functions and 25.8 KB, and the firmware was 14.2 KB larger than before SRV-51. Three changes take that back:

1. The generic entry points in `ww_client_server::dispatch` are `#[inline(always)]`. They only match the handler's result and pass the value on as an `ErasedValue`: a pointer to it and a `fn` pointer to a small thunk around its type's `ser_shrink_wrap`. Each path behind them is compiled once per server mode. `SerializeShrinkWrap` has associated constants and is not dyn-compatible, and a `&dyn` of a helper trait would add a 16 B vtable per type, so the handle is a plain `fn` pointer. Whether the type is unsized is a separate `bool` argument, not a field: LLVM drops it from every path of a server without `multi_req`.
2. `MaxLen<T, N>` decodes through `des_bounded::<T>(rd, N)` (`#[inline(never)]` in `shrink_wrap`), one copy per `T` instead of one per bound: a `&str` decoder used to be inlined into each `N`'s `des`.
3. `ww_device::Server::handle` polls its state machine in a function of its own (`PollOutOfLine`). Once the dispatcher got smaller, LLVM inlined `handle` into Donguru's server task, and with it what both call (`poll_properties`, `observe_request`, the writers): the task went from 24.6 KB to 45.0 KB while `handle`'s 14.2 KB went away, 6.2 KB more in all.

Making the per-type wrappers `#[inline(never)]` instead (one small function per type that matches and calls the erased path) was tried and is 7.5 KB worse in Donguru: LLVM inlines the serializer thunks back into them. `des` stays generic: it is the type's decoder and a `BufReader`, nothing to share.

Real firmware, text + data in bytes, every build in the same paths (so that panic locations weigh the same):

| firmware                                      | c440491 (before SRV-51) | f5f88ea (SRV-52) |  SRV-55 |
|-----------------------------------------------|------------------------:|-----------------:|--------:|
| Donguru, typec_dongle 1965884, release (`z`)  |                 317,996 |          332,184 | 314,972 |
| Donguru, its dev profile (`opt-level = 3`), FLASH overflowed by |   6,148 |           16,532 |     960 |
| donguru_template 856c389, release (`z`)       |                         |          135,676 | 132,164 |

In Donguru `ww_client_server::dispatch` went from 177 functions and 25,790 B to 116 and 15,556 B. 52 of them are the serializer thunks (7,978 B), 50 are `des` (6,576 B): what is left per type is the type's own serdes. In donguru_template the dispatch functions went from 11,260 B to 9,270 B.

TEST-30 (`just flash-budget`, thumbv6m), SRV-52 → SRV-55, bytes:

| thumbv6m                           | z, sync SRV-52 | SRV-55 | z, async SRV-52 | SRV-55 | 3, sync SRV-52 | SRV-55 | 3, async SRV-52 | SRV-55 |
|------------------------------------|------:|------:|------:|------:|------:|------:|------:|------:|
| floor, one method                  | 14514 | 14418 | 15646 | 15462 | 20458 | 20262 | 21582 | 21306 |
| 8 resources, two of each kind      | 17378 | 17402 | 18266 | 18250 | 23698 | 23606 | 24602 | 24530 |
| mean of the four shared-type kinds |    86 |    86 |    88 |    86 |   104 |   102 |    96 |   100 |
| call with types of its own         |    96 |    94 |    96 |    99 |   107 |   130 |    98 |    96 |
| `MaxLen<&str, N>` property, a different `N` each | 309 | 194 | 316 | 230 | 526 | 256 | 516 | 242 |

The `MaxLen` kind is below its cost before SRV-51 (236, 206, 324 and 292 B) except at `z` with `use_async`, 24 B above. A call with types of its own is 23 B dearer at `fw-3` sync. The harness's resources share their types, so the per-type copies SRV-55 removes are few there; a real API has a type per resource or two.

Instructions per request (`just dispatch-budget`, Cortex-M3 under QEMU) are within 1.5 % of SRV-52 in every cell. A call without arguments or output costs 17-23 more (2284 → 2307 at `z` sync, 1375 → 1395 at `fw-3` sync, 1.0-1.5 %); every other request is within 0.3 %. The thunk is one indirect call per reply. The reply bytes are the same (TEST-31 and the replies' hashes).

## Global trait paths by hash (SRV-53)

Every server had a function that matched the crate and trait names of a `GlobalFull` request against every trait of its API as strings, with a version check per arm, and the names in flash (862 B of code and 240 B of names in Donguru). Now a server with introspection has no global lookup at all: the client resolves a global path from the introspection data and sends the absolute one. A server without introspection keeps a `const` table of the reachable traits, each a 32-bit FNV-1a hash of crate and trait name, the crate's major and minor and the trait's path, looked up by one shared function, `ww_client_server::dispatch::global_full_path`, that hashes the request's names. See [Addressing](../api/addressing.md#who-resolves-a-global-path).

Measured 3 Nov 2026 against SRV-52 (master `f5f88ea4` plus the `Globals*` kind), in the same worktree, bytes:

| thumbv6m                                          | z, sync before | after | z, async before | after | 3, sync before | after | 3, async before | after |
|---------------------------------------------------|------:|------:|------:|------:|------:|------:|------:|------:|
| floor, one method (introspection on)              | 14566 | 14410 | 15694 | 15554 | 20474 | 20354 | 21598 | 21454 |
| 8 resources, two of each kind (introspection on)  | 17434 | 17166 | 18326 | 18082 | 23718 | 23330 | 24622 | 24366 |
| one more trait reached by a global path, introspection off |   120 |    80 |   128 |    87 |   131 |   114 |   280 |   268 |
| mean of the four shared-type kinds                |    86 |    89 |    88 |    88 |   104 |   104 |    96 |   100 |

The servers with introspection are 120-156 B smaller at the floor, 244-388 B with the eight resources, whose lookup had two traits (the root and `Base`) instead of one. Without introspection one more trait costs 80 B instead of 120 B at `z` sync, and no crate or trait name is in flash (`GlobalTrait15` and `flash_budget_api` were before). The means moved by 0-4 B, within the per-kind noise the baseline describes. Instructions per `GlobalFull` request were not measured again (no QEMU on the PC that ran this); the hash is one multiply per byte of the names, where the string match compared them with every trait's.
