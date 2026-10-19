## Unreleased

### 🚀 Features

- Sync handlers in an async server (SRV-21): `GenServerConfig::handler_model` / `ww_codegen!(.., handler_model = "..")` (`HandlerModel`, `HandlerKind`) says per item whether its handlers are `async fn` or plain `fn` (regex over the dotted item path, `_` the default, `use_async` without one). Sync handlers are called without `.await`, from `poll_properties` too, and a trait level whose items are all sync gets a plain `fn` dispatcher; every call goes through `wire_weaver::handler::sync` / `r#async`, so a handler of the other kind is a compile error. Patterns that match no item, and `async` in a blocking server, are compile errors. `ServerScaffoldConfig::handler_model` scaffolds `fn` / `async fn` to match.

- A cheaper observe poll (API-52): a cells level's `<Trait>Row` implements `LevelRowCells::copy_columns`, so `poll_properties` copies only the time and the observed columns out of the ring; the generated plan and update writing look a batch's columns up with `LevelBatch::column_after`, one walk over them per update instead of one per property. With ww_stream's outbox changes the FOC poll that encodes a batch takes half the instructions (Cortex-M4F, opt 3), the same bytes.

- `ObserveBatches::init(slot, policy, ..)` (API-39) builds the batches in place, in a `static` (`ObserveBatches::init(CELL.uninit(), DEFAULT_POLICY)` with a `static_cell::StaticCell`): the stores of every level and a cells level's staging store are written where they stay, never on the stack. `ObserveBatches::new` went through by-value stores, a 31.6 KB stack frame for the 21-property FOC level of the motor demo on a Cortex-M4F at `opt-level = 3` (35 KB at `z`, which also has an 8.3 KB frame for the row store alone); `new` goes through `init`. `ObserveState::init(slot, limits, batches)` does the same for the state (its tables and outboxes, an outbox being `observe_outbox` bytes per level), so a firmware sets observe up in its statics with 8 B of stack at `opt-level = 3`, 72 B at `z`; the bench's frame left (7.4 KB) is its own message buffers. `ObserveBatchSource` for `&mut B`, so an `ObserveState` can borrow batches kept in a `static`. The generated `<Trait>Samples` / `<Trait>Row` stores have `init_store`, and their `set` is `#[inline(always)]`: `<Level>Sampler::sample` at `opt-level = "z"` takes 412 cycles on an STM32H725 instead of 1398 (FOC level, 21 properties).

### ⚠️ Breaking

- `GenServerConfig`, `GenServerConfigRaw` and `ServerScaffoldConfig` have a `handler_model` field (SRV-21): code building them literally adds `handler_model: HandlerModel::default()` (`String::new()` for the scaffold), which keeps every handler as `use_async` says.
- `ServerScaffoldConfig` has an `observe_cells` field (API-50): code building it literally adds
  `observe_cells: vec![]`.

* `Delta<'i, T>`, `DeltaOfDelta<'i, T>`, `XorFloat<'i, T>` (and `..Owned`) are no longer built-in API types (removed
  from shrink_wrap): use `Series<'i, T>`, which codegen already maps to `ww_self::Type::Series`.
- Generated std clients of APIs with `+observe+batched` properties (API-39): their slots in `<Trait>Update` are
  `BatchedOwned<T>`, `<Trait>Latest` keeps the last sample, `<Trait>Observe::samples(max_age)` observes every
  sample of each batched property of the level, `observe_<name>` yields one value per sample, its time from the
  update's `sample_times` (API-49). `LevelUpdate::update_latest` takes the update's `sample_times`.

- Batched observe on the server (API-39): an API with `+observe+batched` properties gets `ObserveState<B =
  ObserveBatches>` (`new(limits, batches)`, one `Outbox` per level with such properties), `ObserveBatches`
  (`new(policy)`, `split()` into `ObserveSamplers` and `ObserveBatchConsumers` for samplers in an interrupt or
  another thread, `samplers()` to sample from the polling task), a `<Level>Sampler` per level
  (`sample(t, <Trait>Samples { .. })`, O(1), `is_sampling()`), a `<Trait>Row` per level (the time and the values
  of one sample, kept in one batcher per level: one gate, one time column per update, API-49), the
  `ObserveBatchSource` trait, and
  `OBSERVE_BATCH` / `OBSERVE_OUTBOX` (codegen arguments `observe_batch = N`, default 32, and `observe_outbox =
  N`, default 4096). `poll_properties` of such an API takes a `scratch: &mut ww_stream::batch::Scratch<SB>`: it
  encodes sealed batches into the outboxes (the scratch holds one column: the biggest of `Second` and the batched
  property types, `OBSERVE_BATCH` of them), then sends updates while batches wait (up to `MAX_UPDATES_PER_POLL` per
  level), each batch whole with its times as the event's `sample_times`, with no head `t` when an update carries
  only batches. Batched slots of the
  `<Trait>Update` types are `Batched<'i, T>` (`BatchedOwned<T>`). The scaffold writes `observe_batch` and says how
  to build the state. APIs without `+batched` properties keep their code.

- `property!(ro+observe+batched ..)` / `rw+observe+batched` (API-39): `+batched` after `+observe` (alone it is a
  parse error saying so) sets ww_self's `ApiItemKind::Property::batched` to `Batching::SharedClock` (API-49), so
  API hashes change.
- Introspection data changes (API-46): each struct and enum carries its module path (ww_self `ItemStruct::path`),
  so API hashes change and devices and hosts are rebuilt together; snapshots (`ww api save`) take
  `#[derive_shrink_wrap]` types and unit marker structs (`pub struct Volt;`) from `pub mod`s too, and know each by
  its path and name (`units::Volt` and a root `Volt` are two definitions). A type no other crate can reach is an
  error naming it, and one re-exported under another name (`pub use inner::Inner as Renamed`) is named `Renamed`.
- **Breaking:** generated credit-sink servers drop a write whose position is 2^28 or more
  (`CreditVerdict::Invalid`, API-42); positions wrap on the wire, see `ww_client_server`.

- Generated std clients (CLT-28): `observe_<name>()` of a `+observe` property is the observe request now,
  `observe_<name>(mode)` returns a `PreparedObserve` (`.subscribe().await` gives an `Observer`) instead of a `Stream`
  of stream data; properties without `+observe` keep the old `observe_<name>()`. The modules of the generated levels
  (`<api crate>_<trait>`) are `pub`, so their observe types can be named.

### 🐛 Fixes

- `ObserveState::reset()` (the link went down) drops the rows taken before it (API-39, API-50): rows waiting in a sampler's batch or in a cells level's ring went to the next host that observed the level in `Samples` mode, stamped with their old times (seen on Donguru: 18 rows 88 s old at the start of a `dg power watch --samples`). Test `reset_drops_waiting_rows` in `tests/observe_batched`.

- `ww_codegen!` in test code finds an API crate that is only a dev-dependency (e.g. a test server in a binary crate's tests): the crate walker looked in `[dependencies]` alone and failed with `Dependency not found`.
- `fixed_layout` types (SW-44) can be used in an API: the crate walker rejected the directive (`unsupported
  directive 'fixed_layout'`), so `load` and `ww_codegen!` failed on any API using such a type. It now makes a struct
  without a size assumption `sized` (`self_describing` with an `Option` / `Result` last), as the derive does, so a
  nested one is written without a size. A `head_tail` type (SW-46) in an API is an error saying it has no
  ShrinkWrap serdes. Found by the conformance matrix (TEST-23).
- Generated `poll_properties` sends a pending `missed` count in an update with no properties when nothing else is
  due (API-39): before, samples lost after the last batch of a burst were never reported.

- Drop the unused `console` and `relative-path` dependencies (P2401, cargo-machete).
- `load`, `load_dep` and the codegen macros parse each module file of a walked crate once instead of on every path
  through it (`ww_si`'s `unit.rs` was parsed over a hundred times per API): loading an API with `ww_si` quantities
  takes ~50 ms instead of ~700 ms in debug, `ww_codegen!` and the snapshot checks get faster too.
- An `#[optional(group)]` named like an item of the same trait (group `format`, method `format`) is a compile error
  naming both, instead of `omit` silently dropping only the item.
- Snapshots (`ww api save`) take generic marker structs too: a public struct whose fields are all `PhantomData`
  (`ww_si::units::Per<A, B>`), like a unit struct, so a `Quantity<Per<..>>` in an API is skipped as known (STD-38).
- In a path type with arguments (`units_lib::Quantity<units_lib::units::Volt>`) the arguments are converted where
  they are written, not in the crate the path leads to ("Dependency not found" before) (API-46).
- The walker parses each crate once per load (its crate cache was never filled), `snapshots_are_up_to_date` takes
  4 s instead of 11 s.
- Deferred return helpers (`<method>_send_return`, `_send_return_blocking`, `_ser_return_event`) are generated for
  deferred methods on nested trait levels too (SRV-20), named with the trait item prefix like the handler
  (`plug_vbus_cycle_send_return`); `method_model` patterns and the scaffold's deferred hint match the prefixed name.

### 🚀 Features

- Observe cells (API-50): `ww_codegen!(.., observe_cells = "root_foc, ..")` puts the named levels on a ring of
  atomics instead of `<Level>Sampler::sample`. Per level `<Level>Cells<R>` (`const fn new`, for a `static`, or an
  `Arc` with `CellsReader::shared`), `writer()` handing out the one `<Level>Writer` with a setter per `+batched`
  property (relaxed stores into the row, where the value is computed) and `commit(t)`, and `reader()` for
  `ObserveBatches::new(policy, ..)`, which then is `ObserveBatches<'c>`. `poll_properties` drains the ring into one
  staging batch and sends it through the same outbox: the wire is the same as the sampler's (tested byte for byte,
  and every `tests/observe_batched` scenario runs on both). Rows lapped by the writer count as `missed`. Other
  levels keep their samplers; a name that isn't a level with `+batched` properties is a compile error listing them.
  `ww api scaffold --observe-cells` (`ServerScaffoldConfig::observe_cells`) writes the statics and the wiring.
  The setters are `#[inline(always)]`: a row of the motor demo's 21 FOC values takes 357 cycles on an STM32H725 at
  `opt-level = "z"` against 1400 through the sampler (342 against 423 at `opt-level = 3`), see
  `docs/api/batching.md`.

- Batched streams (API-41): `#[stream(T, delivery, batched)]` / `batched(untimed)` and `#[batched]` /
  `#[batched(untimed)]` next to `stream!`, for plain, counted and reliable streams. Generated servers keep their
  state in a `ww_stream::batch::Batched` (`stream_<name>()` returns it), grant the host's `max_age_us` on open and
  resume, and `poll_streams` (now also for blocking servers with batched counted or plain streams) seals batches by
  age, encodes each once and sends counted and plain ones right away, reliable ones through the store; it takes a
  `ww_stream::batch::Scratch` when the API has batched streams. `stream_data_ser().<name>_push(to, value, t)` and
  `<name>_push_many(to, t0, dt, values)` are O(1) per item and never encode (an interrupt may call them);
  `_push_many` stamps item `i` with `t0 + i * dt`, exact in `t0`'s prefix and number type and wrapping like the
  device clock (`ww_si::Quantity::steps`), and returns `None`, pushing nothing, when `dt` is not a whole number of
  `t0`'s prefix (P2401#17oct-2214; it took `(t, value)` pairs before). The scaffold writes
  `Batched<.., 32>` states. Generated clients unpack batches into items. A batched item type that borrows is a
  compile error. A batched timed stream's model item gets `batch_item_ty` (the `Timed<T>` instance, for a dynamic
  decoder): resolved from `ww_si` directly (not from anything the trait's own source names), so the item type's own
  crate doesn't need a `use` of `Timed` — only `ww_si` as a dependency (needed anyway for `Timed<T>`'s `t: Second`).

- Generated code names each user type by the path other crates see (API-46): the shortest one through `pub mod`s and
  `pub use` re-exports (by name, renamed, glob), recorded in the definition, instead of `<crate>::<Name>`. A type in
  a `pub mod` no longer needs a root re-export, and a marker `units::Volt` next to an alias `Volt` (the ww_si 0.2
  layout) is the marker. Equal definitions at different paths are separate entries of the types array.
- The walker follows renamed imports (`use units::Volt as V;`) to their definition (API-46).
- Type aliases in APIs (API-45): `pub type Volt = Quantity<VoltUnit>;` and generic ones (`type Reading<T> =
  Timed<Quantity<T>>`, const parameters too) are found like structs and enums (in lib.rs, modules, `use` imports and
  globs) and stand for what they name: the right-hand side is converted in the alias' crate, with each parameter
  replaced by its argument. An alias is not a definition of its own in the API model. They were an error ("Type .. not
  found").
- `PhantomData<T>` fields are left out of a struct's definition in the API model (zero bits on the wire, DER-15); the
  parameter stays in the definition's generics, used by no field.
- `Series<'i, T>` / `SeriesOwned<T>` in APIs (SW-60, CLT-29): the API model has them as `Type::Series`, with
  `#[series(opaque)]` fields marked (`Field::series_opaque`); generated servers and clients use the types as written.
  The stream serializers name the value's lifetime (`'i`): `Series` is invariant over it.
- A cached bundle in `~/.wire_weaver/` that doesn't read back in this ww_self format is written again (it was kept,
  and hosts couldn't use it).
- Self-referential types in APIs (API-44): `enum Expr<'i> { Neg(RefBox<'i, Expr<'i>>), Lit(i32) }`, `struct Tree<'i>
  { children: RefVec<'i, Tree<'i>> }`, directly or through other types, were an error ("Self-referential type .. is
  not supported yet"). A definition goes into the types array once, its references to itself point at its own index.
  A cycle through a generic definition is an error naming the cycle.

- Generic user types in APIs (API-43): `Timed<Ampere>`, `Window<4>`, `Reading<i16>`, `Tagged<'i, T>` as arguments,
  return values, properties, stream items and fields, through `crate_walker` into dependency crates. A generic
  definition goes into the types array once with its parameters, each use is an instance with its arguments (types,
  integer and bool literals, `const` items, a const parameter passed on, defaults); generated code uses the types as
  written (`TaggedOwned<String>` on the owned side). `load_crate` (snapshots) includes generic definitions, so
  `ww_si::Timed` is left out of device introspection data. A const parameter that isn't an integer or `bool` is an
  error naming the type.

- The model parser accepts the `validate = path` directive of `#[derive_shrink_wrap]` (SW-58) and ignores it.
- The model parser accepts the `series` directive of `#[derive_shrink_wrap]` (SW-60) and ignores it.
- A crate's API snapshot leaves out derived types with type parameters (DER-14, `ww_si::Timed<T>`): the API model has
  none yet.

- Server and client codegen store every definition left out of the introspection data in the definition cache
  `~/.wire_weaver/definitions/` (EVO-9), so a host on the same machine can put back definitions of a device built from
  another revision of an unpublished crate.
- Observed properties in std clients (CLT-28): per trait level with `+observe` properties (not under a resource
  array) the client module has `<Trait>Update` (`<Trait>UpdateOwned` with owned fields, the sparse struct of the
  level's `PropertyUpdate`), `<Trait>Latest` (latest value per property with its receive time) and `<Trait>Observe`
  (`Option<ObserveMode>` per property, with setters), and the level client gets `observe(cfg)`. See
  `docs/api/properties.md#observing`.

- Observed properties on the server (SRV-18): for an API with `+observe` properties the generated server answers
  `RequestKind::Observe` on a trait level (validated in full and clamped by `ww_stream::observe`, entries refused one
  by one, a malformed request or more than 16 refused entries fail as a whole with nothing changed, a level under a
  resource array answers `OperationNotSupported`, an answer that doesn't fit the reply buffer undoes the request)
  and generates `ObserveState` (one table per level, sized at compile time), a sparse `<Trait>Update` per level, `mark_changed_<name>()` (called after a `value_on_changed` write that
  changed the value) and `poll_properties(out, medium, now_us, buf)`, which sends one `PropertyUpdate` per level per
  call, earliest deadline first within the level's budget. The user's server provides
  `fn observe_state(&mut self) -> &mut api_impl::ObserveState` and depends on `ww_stream`; `ww api scaffold` prints
  both. See `docs/api/properties.md#observing-changes`.
- `sparse` (DER-13) is accepted in `#[derive_shrink_wrap(..)]` of API types; the API model now carries it
  (`ItemStruct::sparse`, `Field::slot`, TOOL-11) instead of describing such a struct as a plain struct of `Option`s,
  so a dynamic decoder reads it correctly.
- Generated clients read a `const` item once per connection (API-36): `read_<NAME>()` uses
  `Commander::prepare_const_read`, later reads answer from the client's cache.
- `#[errors(Common(InUse, Refused), Nack, ..)]` on a trait method (API-32): parsed and checked against the variants
  of the method's error enum (and of the enum in `Common(..)`), an unknown or repeated name, a list on a method without
  `Result` or on another item is an `ErrorListError` that `#[ww_trait]` shows on the name. The list goes into the
  model (`ApiItemKind::Method::errors`). Generated clients document it (`# Errors`, each entry with its variant's doc
  line) and check it: a device error outside the list logs a warning (`PreparedCall::with_error_list`). Nothing
  changes on the device or on the wire. `internal::ww_self` re-exported for `wire_weaver_derive`.
- `#[errors(..)]` also on stream functions (`#[stream(..)] fn`, about the `Err` of the open's `Result`) and above a
  `property!` with an error type (about that type, for reads and writes) (API-32), with the same checks, into
  `ApiItemKind::StreamFn::errors` / `Property::errors`. Generated clients document them (`# Errors` on the stream
  function and on `write_<property>`) and check them: `PreparedStreamOpen::with_error_list`,
  `PreparedWrite::with_error_list`. `#[errors(..)]` on `stream!`, `sink!`, a `const` or `ww_impl!` stays an error, now
  saying why.
- Accepts the `display` directive of `#[derive_shrink_wrap(..)]` (DER-12): a type using it is no longer rejected
  with "unsupported directive 'display'" when introspected from a trait. It has no effect on the API model or
  generated server/client code (`core::fmt::Display`/`core::error::Error` are Rust-only, not wire-defining).

### 🐛 Fixes

- `multi_req = true` servers no longer panic on host input (CLT-15): a multi request on a path at the depth limit
  (16) is answered with `BadPath` instead of indexing out of bounds, a deferred method inside a multi request with
  `OperationNotSupported` (a method the `method_model` defers is not called at all) instead of `todo!()`. An index
  that fails is answered with an error in its own slot and the others go on; answers that don't fit into the scratch
  buffer fail the request with `ResponseSerFailed`.
- `MultiCall` and `MultiWrite` are dispatched as calls and writes with their `MultiArgs` (they ran as reads), each
  index answered like in `MultiRead`; `MultiWrite` answers with `EventKind::Value`. `MultiArgs::Different` is a
  serialized `Vec<Vec<u8>>` with the arguments of each index.
- Server code of a stream function whose error type borrows (`fn frames() -> Result<(), LogError<'i>>`) compiles with
  the handler signature `ww api scaffold` prints (`RpcResult<Result<(), LogError<'_>>>`) (API-35): the generated
  `StreamOpen` dispatch kept the handler's result, which borrows `self`, alive across `new_stream_session()` and
  `stream_<name>().open(..)` (E0499), so handlers had to return `'static` errors. The `Ok` value is now taken out of
  the result before the stream's state is touched. Nothing changes on the wire.
- Bit-sized numbers `U1`..`U64` / `I2`..`I64` (`u7`, `ub10`, `i2`, ..) used directly as a method argument, return
  value, property, stream or sink type generate `wire_weaver::shrink_wrap::un::U7` etc. in server and client code;
  `ww_codegen!` panicked with `not yet implemented` (SW-48). Types that only contained them were fine.
- Trait clients (`client = "trait_client"`) name the client's own trait in global paths of nested levels too; they
  named the nested trait with an index chain starting at the outer one.
- A crate with `version.workspace = true` (`wire_weaver` itself) can be walked: a type reached through its
  `prelude` failed with `No version found in Cargo.toml`. Crates only passed through on a path are no longer listed in
  the API bundle's `ext_crates`.
- Stream items and property notifications (`stream_data_ser().<stream>()` and the `_send` / `_send_blocking`
  variants) of a type that doesn't end on a byte boundary or has sizes of its own (a `bool` or nibble enum at the end,
  a `&str` or `RefVec` inside) are written on their own (`to_ww_bytes`) between the event header and the event's
  sizes. Written in place, the item's last bits shared a byte with the event's path length and were lost, and its
  sizes mixed with the event's, so the host could not read it (e.g., `ww_dfu::PlanAck`). Byte-aligned items without
  sizes of their own are sent exactly as before; devices built with older code send the broken form for the others
  and need a rebuild.
- Fixed-size arrays in method arguments and return values are generated as `[T; N]` (they were `[N; T]`, which
  didn't compile), in server and client code and in `ww api scaffold` output.
- A method returning a borrowed type inside another type (`Result<RefVec<'i, T>, E>`) keeps its lifetime in the
  generated server (`RefVec<'_, T>`; it was `RefVec<, T>`).
- `sink!` of a borrowed struct (`sink!(plan: PlanSegment<'i>)`) generates a valid turbofish in the server's write
  handler (`PlanSegment::<'_>::des_shrink_wrap`).
- The std client's argument struct of a method that takes a `Vec` or `String` is named `<Method>ArgsOwned`, as
  `derive_shrink_wrap` requires for owned types; such methods failed to compile.
- Generated code names `wire_weaver::shrink_wrap::RefVec` (and `RefBox`, `Nibble`, `UNib32`) by their full path, so a
  crate that depends only on `wire_weaver` compiles.
- `ww api scaffold` names `wire_weaver::shrink_wrap::tail_bytes::TailBytes` by its full path in byte sink handlers.

### ⚠️ Breaking

- `const` items of a `#[ww_trait]` are served without a `get_<name>` handler (API-36): the generated server code
  declares `pub trait <RootTrait>Consts` and reads each value from the server struct's impl of it, an associated
  const (`const BOARD_BOARD: BoardSummary<'static> = ..;`, a Rust const) or a `fn` (`fn board_board(&self) ->
  BoardSummary<'_>`, a value fixed at boot or link time) as picked by the new `ConstModel` (`const_model = "..."`).
  Servers move their `get_<path>_<NAME>` handlers into that impl, `ww api scaffold` prints it. `GenServerConfig`,
  `GenServerConfigRaw` and `ServerScaffoldConfig` have a new field `const_model`.
- Stream functions (API-31): every stream is opened by calling it with `RequestKind::StreamOpen`. Servers
  implement, for every `stream!`, the open handler named like the stream (`fn <name>(&mut self, cx, ..) ->
  RpcResult<()>`, return `Ready(())` to accept) and `close_<name>(cx)`; sideband `Close` goes to the latter,
  `sideband_<name>` only gets the other commands. Clients open with `<name>(args).call()` / `.blocking_call()`
  instead of `<name>().await` / `<name>_blocking()`. `ww api scaffold` prints the new handlers.
- Generated std clients (CLT-24): a method returning `Result<T, E>` returns `PreparedCall<T, CallError<E>>` (was
  `PreparedCall<Result<T, E>>`), a property with a user error type `PreparedWrite<CallError<E>>` (was
  `PreparedWrite<E>`), one without `PreparedWrite<Error>` (was `PreparedWrite<()>`). See `wire_weaver_client`.
- `GenServerConfig`, `GenServerConfigRaw` and `ServerScaffoldConfig` have a new `omit` field (API-16): struct
  literals in `build.rs` scripts need `omit: vec![]`.
- Every generated server handler takes `cx: &mut wire_weaver::Context<'_, impl EventOut, Medium>` as the first argument
  after `&mut self` (`BlockingEventOut` for `use_async = false`): methods and `sideband_*` instead of
  `msg_tx: &mut impl MessageSink`, and newly `get_*`, `set_*`, `changed_*` and `write_*`. `valid_indices_*` don't.
  Run `ww api scaffold` to see the expected signatures.
- Generated `process_request_bytes(bytes, scratch, out, medium)` takes `out: &mut impl EventOut` (or
  `BlockingEventOut`) and the medium instead of `msg_tx`.
- `GenServerConfig` and `GenServerConfigRaw` have a `medium` field, `ServerScaffoldConfig` too.
- Generated client `write_<property>` returns `PreparedWrite<UserError>` (`PreparedWrite<()>` without one) instead of
  `PreparedWrite<Result<(), UserError>>`, see `wire_weaver_client` changelog.

- Generated servers follow `ww_client_server`'s seq change: `Request::seq` is `UVlq32Backfill` and `Event::seq` is
  `UVlq32`, deferred `*_ser_return_event()` methods take a `u32` seq. Wire-incompatible with clients and devices built
  before this change.
- The API hash (`API_HASH_NO_DOCS`/`API_HASH_WITH_DOCS`) is calculated over the introspection data as a device sends
  it, with known traits and types left out, so it changes for APIs using `ww_stdlib` traits or types. A device and a
  client built with `wire_weaver_core` versions that know different snapshots report different hashes for the same
  API, which falls back to the per-resource compatibility check.
- The API hash covers `ww_self_version`, which now follows the `ww_self` crate version: a device and a client built
  with different `ww_self` versions report different hashes for the same API, with the same fallback.
- `#[flag] name: bool` fields are recorded as `TypeOwned::Flag` in introspection data and snapshots, they were a
  plain `bool` and didn't say that the `Option` or `Result` field `name` has no flag of its own, so dynamic clients
  misread such types. Introspection data, API hashes and signatures of types with relocated flags change
  (`ww_version::Version` and `FullVersion`, `ww_si::SIExp` and the types containing them). A device and a client
  built on different sides of this change report such types as incompatible (`bool` vs `flag`).

### 🚀 Features

- Stream functions and counted / reliable streams in codegen (API-31): `#[stream(T, counted | reliable)] fn
  name(args) -> Result<A, E>` and `#[counted]` / `#[reliable] stream!`. Server: open with typed arguments and result,
  resume by cursor, close, `Ack` / `ResendFrom` into `ww_stream`, a read returns `StreamStatus`, state through
  `stream_<name>()`, sessions from `new_stream_session()`; `stream_data_ser().<name>_send` (counted), `<name>_push`
  (reliable) and `<name>_event`, and `poll_streams(out, now_ms, buf, budget)` on the server sending stored events by
  priority and the device's `Query`s. Client: `<name>(args)`, `<name>_resume(cursor)`, `<name>_status()`. Scaffold
  and introspection too (`docs/api/stream_reliability.md`, `tests/streams_reliable`).
- `Delta<'i, T>`, `DeltaOfDelta<'i, T>` and `XorFloat<'i, T>` (and their `..Owned<T>` forms) in `#[ww_trait]` APIs
  (SW-50): arguments, return values, properties, stream and sink items and fields of API types, as
  `ww_self::Type::Delta` / `DeltaOfDelta` / `XorFloat` in introspection data. `T` is checked: `u8`..`u64`,
  `i8`..`i64` for the first two, `f32` or `f64` for `XorFloat`.
- Optional items (API-16): `#[optional]` / `#[optional(group)]` on trait methods, properties and streams go into the
  model (`ApiItem::optional`); `GenServerConfig::omit` / `GenServerConfigRaw::omit` (`Vec<codegen::omit::Omit>`,
  `Omit::always(path)`, `Omit::when(cfg, path)`) leave them out of a server: no dispatch arm, no handler, the level's
  fallback arm answers `Unimplemented` for ids of the trait, the introspection data lists them (one variant per
  combination of `omit_if` conditions, under `#[cfg]`). Only optional items can be left out, any other path is a
  compile error naming the trait's optional items and groups. `ServerScaffoldConfig::omit` leaves their stubs out.
  `#[optional]` on `ww_impl!` is rejected. See `docs/api/optional_items.md`.
- Types imported with a glob (`use wire_weaver::prelude::*`) are found, after the ones defined or imported by name
  (STD-16, for `CommonError` from the prelude).
- Generated code names types of `shrink_wrap`, `ww_version` and `ww_error` through `wire_weaver::`, so a server or
  client crate needs no dependency of its own on them.
- An API crate can name `ww_error` types (`use ww_error::CommonError`) with only `wire_weaver` as a dependency, as
  for `shrink_wrap` and `ww_version` (STD-16).
- Types with the `no_future_version` and `future_version = <Name>` arguments of `derive_shrink_wrap` (SW-47) are
  read from API crates; the variant stays out of the API model like a written `#[future_version]` one.
- `NumericBaseType::UVlq32` / `UVlq64` map to `shrink_wrap::UVlq32` / `UVlq64` in generated type definitions
  (STD-23).
- `UVlq32` / `UVlq64` fields are numbers in the API model (`NumericBaseType::UVlq32` / `UVlq64`), and map back to
  `shrink_wrap::UVlq32` / `UVlq64` in generated type definitions (STD-23); they were unresolved user types.
- `Delta<'i, T>`, `DeltaOfDelta<'i, T>` and `XorFloat<'i, T>` (and their `..Owned<T>` forms) in `#[ww_trait]` APIs
  (SW-50): arguments, return values, properties, stream and sink items and fields of API types, as
  `ww_self::Type::Delta` / `DeltaOfDelta` / `XorFloat` in introspection data. `T` is checked: `u8`..`u64`,
  `i8`..`i64` for the first two, `f32` or `f64` for `XorFloat`.
- A `#[future_version]` enum variant (SW-16) is left out of the API model (`ww_self`): it is no variant on the
  wire, adding it later changes neither introspection data nor snapshots.
- Servers serve requests addressed by a global trait path (API-20, `PathKind::GlobalFull`): the trait is looked up
  wherever it sits in the API tree, its crate version checked for protocol compatibility, and the request continues
  from there; `ErrorKind::TraitNotFound` otherwise. Global paths don't depend on the user API version, so a host can
  reach `ww_dfu::BootControl` or info traits on a device with any version of its API. Traits under a resource array
  can't be reached this way; when a trait is implemented several times, the first one (depth first) answers.
  `PathKind::GlobalCompact` is answered with `TraitNotFound` for now: compact ids of traits are not in the API model
  yet.
- `MaxLen<T, N>` (SW-19) in `#[ww_trait]` method arguments, return types, properties, `stream!`/`sink!` items and
  types, `N` an integer literal, `{literal}` or a `const` of the API crate's `lib.rs`. Carried into introspection as
  `ww_self::Type::MaxLen`. Byte streams and sinks with a bound keep their raw `TailBytes` wire form; the server
  rejects a longer sink write (`ArgsDesFailed`), a stream send of a longer value returns `MaxLenExceeded`.
- Generated `no_alloc` servers have `<METHOD>_REQUEST_MAX_LEN` and `<METHOD>_RESPONSE_MAX_LEN: Option<usize>` per
  method, the longest request and reply message, `None` when a part is unbounded (first step of API-18). Check a
  message buffer against them at compile time.
- `#[credit] sink!(..)`: credit-based flow control for a sink (API-28, `docs/api/streams.md`). Generated servers call a
  new handler `credit_<name>(&mut self[, index_chain]) -> &mut ww_client_server::credit::SinkCredit`, check the
  position of each write, call `write_<name>` only for writes in order, answer `CreditQuery` themselves and send
  `Credit`/`Resend` events (credit counts bytes of items, the serialized value of each write); the server scaffold
  includes the handler and a `SinkCredit` field. Each credit sink gets `<SINK>_MAX_ITEM_BYTES: Option<u32>` on its
  level's `<Level>StreamSerializer` (and `stream_data_ser().<sink>_max_item_bytes()`, `stream_data_ser()` is a
  `const fn`), the largest item, from `MAX_SIZE` (for a byte sink, its `MaxLen` bound), None when unbounded, for
  `SinkCredit::sized_for()`. A bounded byte
  credit sink checks the length before the write takes a position. Generated clients open
  credit sinks with `Commander::prepare_credit_sink()`.
- `medium` server codegen option: type (usually an enum) handlers get from `cx.medium()`, `()` if not set.
- `stream_data_ser()` has `<name>_send(.., out)` and `<name>_send_blocking(.., out)` for every stream, sending through a
  handler's `cx` or `server.sink()`, and the same serializers for `ro` / `rw` properties: property updates are sent
  as stream data on the property's path. Generated clients get `observe_<property>()` /
  `observe_<property>_blocking()` returning a `Stream` of them.
- Deferred methods get `<method>_send_return(out, seq, ..)` and `<method>_send_return_blocking(..)`, serializing the
  reply in one pass, `seq` from `cx.reply_to()` of the deferred call.
- Introspection works for `use_async = false` servers as well (sent through `cx`).
- `load_crate()` loads all `#[ww_trait]`/`#[ww_api_root]` traits and `#[derive_shrink_wrap]` types defined in a crate
  into an `ApiBundleOwned`, with traits and types from other crates replaced by `SkippedFullVersion` references
  carrying the signature of the left out definition. Types re-exported from the crate's modules with `pub use` are
  included too. Used by `ww api save` to save crate snapshots.

- Introspection data sent by a device leaves out traits and types known from the snapshots embedded in
  `wire_weaver_snapshots` (`ww_global` and `ww_stdlib` crates), referring to them by crate version, name and signature
  instead (e.g. `examples/all_gpio_api` goes from 4363 to 72 bytes). Only definitions identical to the snapshot are
  left out. `transform::skip_known()` does it. Generated clients embed their own API in the same form.

- Server codegen emits `API_HASH_NO_DOCS`, `API_HASH_WITH_DOCS` and the compile-time `API_ID` string for USB
  identity strings.

- Generated `stream_data_ser()` has `<name>_sideband(sideband, scratch)`, `<name>_sideband_send(sideband, out)` and
  `<name>_sideband_send_blocking` for every `stream!` and `sink!`: the device sends a sideband event on its own, from a
  handler (pass `cx`) or from the event loop.
- `gen_server_scaffold()` with `ServerScaffoldConfig` generates the user side of a server as Rust source: a server
  struct (with fields for `value_on_changed` properties), a stub for every handler the server codegen expects,
  with the resource's doc comments, and the matching `ww_codegen!` invocation. Method, getter and setter stubs return
  `Unimplemented`, valid indices stubs allow no index. Used by `ww api scaffold`.

### 🐛 Fixes

- A stream sideband request whose `sideband_<name>` handler returns `None` is answered with `EventKind::Written`
  instead of nothing, so the host doesn't keep its seq until a timeout. Hosts built before this change accept the
  answer as a reply to the request.
- `method_model = "..=deferred"` servers compile again: generated `<method>_ser_return_event` used an undefined
  `request` for unit methods, `RefVec` instead of `TailBytes` for the return value and `Error` without a lifetime.

- Types and traits can be referred to through modules: `mod ty; use ty::Ty;`, `crate::`/`self::` paths, and paths
  longer than `ext_crate::Ty` (e.g. `ext_crate::module::Ty`). `ww_version` and `shrink_wrap` used through
  `wire_weaver`'s re-exports (e.g. `use wire_weaver::prelude::*;`) resolve without a direct dependency on them.
  All of these failed with "Dependency not found" or "Only support `MyType` and `ext_crate::MyType`".
- Generated client methods no longer declare an unused 128-byte `args_scratch` buffer when `no_alloc` is not set.

- Loading a self-referential type (e.g., containing `RefBox<Self>`) fails with an error instead of overflowing the
  stack.

- `#[default = ..]` on fields of API types accepts any expression (e.g. `#[default = None]` as in the docs), not only
  string literals, which failed to load. Only the presence of a default is recorded in introspection data for now.

- Generated std client sets `client_version` to the API crate's `<TRAIT>_FULL_GID`, so the host and device check
  version compatibility during link setup instead of treating the client as dynamic.

### 📚 Documentation

- Docs: product names written WireWeaver and ShrinkWrap in prose.

## 0.4.0 - 07 Jan 2026

### 🚀 Features

- Property_model support with get_set and value_on_changed options.
- Method_model deferred passes seq number to method and uses Option<return ty> to determine whether to answer
  immediately or not.
- Serialize return value for deferred methods.
- Harden USB link implementation.
- #[derive(ShrinkWrap)]
- Implement repr un enums.
- External types support in shrink_wrap attr.
- U1..=U63 support, #[fixed_size] and #[dynamic_size] attributes for derive_shrink_wrap attribute macro.
- I2, I3, .., I63 support.
- Propagate ident spans, among other things makes output look better in IDE
- #[owned = "feature"] attribute to generate TyOwned from Ty<'i> and serdes code for it.
- Subtype scaffold.
- Replace #[final_evolution] with #[final_structure], add #[self_describing] and #[sized] attributes, implement const
  asserts.
- Handle #[default = None] on evolved types.
- ww_impl! proc macro that generates ww-trait server or client implementation in place.
- ww_trait support in separate files, multiple API levels.
- Implement resource array support for methods, properties, streams and API traits.
- Implement RefBox<'i, T>
- Check that flag order is LIFO.
- Property access mode.
- ww_si, ww_can_bus, ww_numeric and other library types
- Trait client structs
- Use the index chain in client codegen as well.
- Tuple and array support.
- Ww_trait: emit proper compiler error if lifetime on a referenced type is incorrect.
- Const properties
- Global trait addressing support
- Client: split methods into 2 - one with default timeout and the second with explicit value.
- Treat [T] and &[T] as Vec/RefVec<T> in API
- Stream sideband channel, document ww_client_server
- Partial stream client support
- Stream client subscribe
- Methods and streams tests
- Trait array server, client and integration test
- Support super and crate in ww_api macro.
- Error sequence ID in generated server code to help identify exact errors
- Blocking client for methods
- Generate connect_raw() and connect_raw_blocking()
- Reserved resource kind
- Scaffold introspect
- Stream and array of streams data serializers at any depth

### 🐛 Bug Fixes

- *(api)* Support derive with paths.
- User types returned from methods directly.
- Option in argument position.
- Collect methods and streams doc comments.
- Use ww_repr instead of repr for clarity, generate discriminant fn in ww_repr attr macro.
- Always add ww_repr in wire_weaver_api macro
- Treat strings as Unsized
- Handle Unsized in Option and Result
- Automatically switch to Owned type in client codegen if no_alloc == false, and add a use statement.
- Multi level traits
- Make whole API level owned when no_alloc = false
- Generate connect client methods only if async_worker+usb is used
- Array of streams on client
