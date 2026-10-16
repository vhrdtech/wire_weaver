# ww_self

`ww_self` is WireWeaver's own AST: a self-describing model of an API (its traits, methods, properties, streams)
and of the data types it uses, serializable with `shrink_wrap` like any other wire type. The codegen builds one
from a `#[ww_trait]`/`#[ww_api_root]` definition at compile time, a device can send its own as introspection data,
and tools (the dynamic client, the `ww` CLI, the Python bindings) walk it to call methods, decode values and show
an API without any generated code for it. It lives in the `wire_weaver` repo root, not `ww_stdlib/`, but is
versioned and published the same way; add it with:

```toml
[dependencies]
ww_self = "0.2"
```

## The API model

* `ApiBundle<'i>` - one API as serialized/introspected: `magic` (`ww_self::MAGIC`), `ww_self_version`
  (the `ww_self` version the bundle was built with, so a reader knows which format it's in), `root: ApiLevel<'i>`
  (the entry point), a deduplicated `types: RefVec<TypeLocation<'i>>` and `traits: RefVec<ApiLevelLocation<'i>>`
  (referred to by index from elsewhere in the model), `ext_crates: RefVec<FullVersion<'i>>` (every external crate a
  type or trait in the bundle comes from), and `root_omitted` (ids of the root's
  [optional items](../api/optional_items.md) this device leaves out).
* `ApiLevel<'i>` - one trait's worth of API: `docs`, `crate_idx` (into `ext_crates`), `trait_name`, and
  `items: RefVec<ApiItem<'i>>`.
* `ApiItem<'i>` - one item of a level: `id`, `kind: ApiItemKind<'i>`, `multiplicity` (`Flat` or `Array {
  index_type_idx }` for a `ww_impl!(x[]: ..)` array), `since` (the version it was added in), `ident`, `docs`, and
  `optional` (`#[optional]`/`#[optional(group)]`, see [optional items](../api/optional_items.md)).
* `ApiItemKind<'i>` - what kind of item: `Method { args, return_ty, errors }`, `Property { ty, access, write_err_ty
  }`, `Stream { ty, is_up }`, `Trait { trait_idx, omitted }` (a nested level), `FlowSink { ty, flow }` (a credit
  sink), or `StreamFn { ty, delivery, args, return_ty }` (a stream opened by calling it, with its own arguments and
  `Delivery`: `Plain`, `Counted` or `Reliable`).
* `TypeLocation<'i>` / `ApiLevelLocation<'i>` - either `InLine` (the actual `Type`/`ApiLevel`, plus which
  `ext_crates` entry it's from) or `Skipped{FullVersion,CompactVersion}` (name/id and version only, plus a
  [`signature`](#signatures-and-putting-skipped-definitions-back) hash) - how a device leaves out a definition the
  host is expected to already know (from a published crate), to save flash, while still letting the host check it's
  the exact same definition.

## The type and value model

* `Type<'i>` - every shape a value on the wire can have: `Bool`, `NumericAny` (any `ww_numeric` number type),
  `String`, `Vec`, `Array { len, ty }`, `Tuple`, `Struct(ItemStruct)`, `Enum(ItemEnum)`, `Option { some_ty }`,
  `Result { ok_ty, err_ty }`, `Box` (self-referential types), `Range`/`RangeInclusive`, `MaxLen { max_len, ty }`,
  `Delta`/`DeltaOfDelta`/`XorFloat` (the compressed number sequences of `shrink_wrap`, SW-31/SW-32), `OutOfLine {
  type_idx }` (a reference into `ApiBundle::types`, how recursive and shared types stay out of the inline tree),
  and `Flag` (a relocated `Option`/`Result` presence bit, [`#[flag]`](../serdes/showcase.md#relocating-presence-flags-with-flag) -
  marks a named field, never appears in a value).
* `Value<'i>` - a value matching some `Type`: `Bool`, `Numeric(NumericValue)`, `String`, `Vec`/`Array`/`Tuple` (of
  `Value`s), `Struct { fields: FieldsValue }`, `Enum { variant, fields }`, `Option`, `Result`, `Range`/
  `RangeInclusive` (of `NumericValue`). This is what a dynamic client builds from wire bytes plus a `Type`, without
  generated Rust types for the API at hand.
* `ItemStruct<'i>` / `ItemEnum<'i>` - a struct's/enum's own `size: ElementSize` (the same
  [size assumption](../serdes/size_assumptions.md) `#[derive_shrink_wrap]`
  picks), `crate_idx`, `docs`, `ident`, and `fields`/(`repr: Repr`, `variants: RefVec<Variant>`).
* `Field<'i>` / `Variant<'i>` - `ident`, `ty`, `docs`, `since`; a `Field` also carries its `default: Option<Value>`
  (for a `#[default = ..]` field) and relocates its own presence flags with `#[flag]` the same way `ww_version::Version`
  does.

Every enum in this model is `no_future_version` (SW-47): rather than tolerating an unknown future variant the usual
shrink_wrap way, a bundle names its own `ww_self_version` and a reader is expected to walk the whole model, so a
newer kind of item or type can't silently be skipped - that tolerance is handled one layer up instead
(introspection version negotiation, INT-8).

## Dynamic access helpers

With the `std` feature, owned types (`TypeOwned`, `ValueOwned`, `ApiBundleOwned`, ...) get methods for walking the
model without generated code:

* `ApiBundleOwned::get_item(id_chain)` / `get_trait` / `get_ty` / `crate_name` / `crate_version` - resolve an item
  path, a nested trait, an out-of-line type, or an `ext_crates` entry by index.
* `TypeOwned::get_in_line(bundle)` - follows an `OutOfLine` reference to the actual `Type`.
* `TypeOwned::human_name` / `human_definition` - a Rust-like name (`"Option<Vec<u8>>"`) or full definition, with or
  without the owning crate's name, for display in a tool.
* `TypeOwned::is_lifetime` / `is_unsized` / `is_byte_slice` / `max_len` - what the type needs structurally (does it
  borrow, is its size written to the FIFO, is it `Vec<u8>`/`MaxLen<Vec<u8>, N>`, what bound a `MaxLen` carries).
* `TypeOwned::is_common_error` / `ValueOwned::common_error` / `doc_message` - recognise `ww_error::CommonError`
  (by crate and type name, any version, even when its definition was left out) inside an API's own error enum, and
  turn an error variant's first doc line into a message with its fields filled in (`doc_message`, used for
  `Display` on generated error types; `fill` in `ww_self::common` does the `{field}` substitution).
* `ww_self::errors` - per-method `#[errors(..)]` lists (API-32): `ApiItemKindOwned::error_list`,
  `error_list_docs` (one doc line per entry, `"any CommonError"` for a bare `Common`), `error_list_contains`
  (whether a returned error value matches the list), `error_value_name`.
* `ApiItemKindOwned::as_stream_fn` / `as_stream` - see any device-to-host stream (a plain `stream!`, a `#[stream]`
  method, or a `StreamFn`) uniformly as a `StreamFnRef` (item type, `Delivery`, arguments, return type), or as a
  stream/sink with its direction and flow control.

## Signatures, and putting skipped definitions back

`ww_self::signature` computes a signature for a trait or type: a truncated SHA-256 (`SIGNATURE_LEN`, 8 bytes) of a
*canonical* bundle holding just that definition and everything it refers to, in-line, with indices renumbered in
the order of first use - so the signature depends only on the definition, not on where it sat in the original
bundle or what else was alongside it. `trait_signature`/`type_signature` take a `Resolve` callback to look up
`Skipped*` definitions (e.g. from a crate's published [snapshot](../evolution/checker_tool.md)) along the way.

`ww_self::inline::inline_skipped` uses the same signatures to put `Skipped*` locations back: it resolves each one,
checks its signature matches what was stored, and replaces it in place (appending whatever the restored
definition itself refers to). Anything not found, or found with a different signature, comes back as a
`NotInlined` (kind, crate/version if known, name, reason) instead of being substituted silently.

A signature is an honest description of the definition a device was built with, docs included: a crate version
that is not published yet changes under the same version, and so does a signature when the `ww_self` model gets a
field. A device built from another revision than the host then sends a signature the host's snapshot doesn't have.

### The definition cache

For that case, codegen keeps every definition it leaves out of a device's introspection data in
`~/.wire_weaver/definitions/` on the machine it builds on: the canonical bundle's bytes as they were hashed, in a file
named after crate, version, kind, name and signature (`ww_si-0_1_0-type-Volt-049a2a94f6d68bb5.ww`). A host
(`wire_weaver_client`, the `ww` CLI, Python's `connect()`) that doesn't find a skipped definition in its snapshots
under its signature looks it up there with `ww_self::inline::inline_skipped_or_cached`, which checks the bytes against
the signature before using them, so an entry written by an older `ww_self` still reads. The definition is put back
in-line and the API check then compares it with the host's by the [evolution rules](../evolution/rules.md), with a
warning that the device has another revision of it:

```text
Device API has type ww_si::Volt 0.1.0 with signature 049a2a94f6d68bb5 (this host's is 0af8195a2e0069fe): likely a
build from another revision of an unpublished crate, its definition is from the definition cache and is compared by
the evolution rules
```

Only when no definition with that signature is found anywhere does a resource that uses it fail the check, with
both signatures, the likely cause and the fix: rebuild (reflash) both sides from one revision, or build the
device's firmware once on the host's machine so that its definitions get into the cache. Entries are a few hundred
bytes each and are never changed; deleting the directory only loses the ability to talk to such devices.

## Visiting the model

`ww_self::visit::Visit` (read-only) and `ww_self::visit_mut::VisitMut` (mutable) are exhaustive visitor traits
over every node kind above, generated from one `define_visitor!` macro definition: each has a default per-node
method (`visit_type`, `visit_api_item_kind`, ...) that recurses into its children, plus leaf methods
(`visit_ident`, `visit_docs`) that do nothing by default - override just the node kinds you care about and call
the matching free function (`visit::visit_api_bundle`, ...) to keep recursing into the rest. `Canonical` (behind
`signature`) and the remapping pass in `inline` are both built this way, overriding only `visit_type`/
`visit_api_item_kind`/etc. to rewrite the handful of index fields (`type_idx`, `trait_idx`, `crate_idx`) that need
remapping.

## How it relates to the rest

`wire_weaver_core`'s codegen (`transform`/`codegen`) builds an `ApiBundle` from the trait source it parses and
hands it to both the server (introspection data a device can send) and client codegen; `wire_weaver_client` uses
it for the dynamic client, compatibility/evolution checks against a cached bundle, and the generated client's own
layout; `wire_weaver_cli` and `wire_weaver_py` walk it to print an API tree, introspect a live device, and expose
it to Python. `wire_weaver_snapshots` stores published crates' bundles so devices can skip their definitions and
hosts can still resolve and verify them via the signatures above.
