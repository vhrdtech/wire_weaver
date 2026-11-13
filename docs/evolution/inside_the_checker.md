---
level: nomicon
---

# Inside the checker

[The evolution checker](checker_tool.md) gives a verdict; this page is about how it gets there where that isn't
obvious: which padding bits a new field may take, how a snapshot names the definitions of other crates, and how a
snapshot saved long ago is still read the way it was meant. It is for changing the checker, for a `ww_stdlib` crate
whose snapshots other crates depend on, and for making sense of a surprising `note:`.

## New fields in padding bits

Padding inside a type, before a field aligned to a nibble or a byte, is always written as zero bits and skipped when
reading. A new field of any type, `Unsized` or not, can take such bits, anywhere between the old fields, if:

* none of the old fields moves and the size doesn't change: the new field only takes bits that were padding,
* all zero bits are a valid value of it, as that is what it reads from old data: `bool`, numbers, arrays and
  non-`Unsized` structs of them, or an enum without data that has a variant with discriminant 0,
* it's fixed size (not `UNib32`, `Option`, an enum with data, an `Unsized` type, ...).

Old fields are then matched by name. Where padding is depends on the bit offset the type starts at. An `Unsized` type
always starts at a byte boundary, so its padding is always in the same place:

```rust
#[derive_shrink_wrap]
pub struct U {
    pub f: bool,
    // 7 bits of padding before `b`
    #[since = "w1.1"]
    pub g: u7, // added in w1.1, reads as 0 from old data
    pub b: u8,
    pub s: String,
}
```

An `Unsized` type can gain fields with `#[default]` at the end as well, in the same version. Bits after its last field
are not free though: lengths of variable size fields are written there.

`final_structure`, `self_describing` and `sized` types are packed tightly into their parent, so they can't grow at the
end, and they can start at any bit inside the parent, so the checker requires that the new field fits for every start
offset:

```rust
#[derive_shrink_wrap(sized)]
pub struct S {
    pub a: u8,
    pub f: bool,
    // 7 bits of padding before `b`, wherever S starts, as `a` is byte-aligned
    #[since = "w1.1"]
    pub g: u3, // added in w1.1, reads as 0 from old data
    pub b: u8,
}
```

Without `a`, the padding before `b` is 7 bits if `S` starts at a byte boundary, but none if it starts at bit offset 7,
so `g` can't be added. Padding after the last field is never free: whatever follows the type in its parent starts
right there. Enum variants with data are not padded to the same size, so there are no free bits after a shorter
variant either.

All of this was checked against real bytes: old and new versions read each other's data, with the new field reading
as zero from old data. The client uses the same rules when comparing its API with a device's.

## Snapshots and signatures

A snapshot (`api_snapshots/<crate>_<major>_<minor>_<patch>.ron`) holds the crate's own traits and types in full. A
trait or type of another crate that the API uses (a `ww_stdlib` trait, a `ww_si` unit) is not copied in: the snapshot
names it by its crate, the crate's version and a signature, and leaves the definition out. A device's introspection
data does the same, so a device doesn't spend flash on definitions every host already knows. The definitions are
found again by signature: in the snapshots built into the checker and `ww` (`wire_weaver_snapshots`), in the source
the checker walked, and in the [definition cache](../std_library/ww_self.md#the-definition-cache) of a host.

That makes the signature the identity of a definition, and three things follow from it. A definition can change
without its crate version changing; the snapshot that defined it is refreshed and the old definition would be gone; a
snapshot saved by an older wire_weaver has signatures made over other bytes. The sections below take them in turn.

### Doc comments of a dependency (EVO-69)

A definition has two signatures (`ww_self::signature::Signatures`,
[signatures](../std_library/ww_self.md#signatures-and-putting-skipped-definitions-back)):

* the **shape signature**, over the definition without its docs, is its identity: the definition is the same when its
  names, fields and wire shape are, whatever its docs say;
* the **signature with docs** tells whether the text changed too.

A reference to another crate's definition (a `Skipped*` item in the model) is its shape signature followed by its
signature with docs: 16 bytes, or 8, the shape alone, in introspection data without docs. A reference saved before
EVO-69 is a signature with docs alone, 8 bytes, and is matched as such (`Signatures::matches`).

Everything that looks a definition up goes by shape:

* Devices leave a known definition out when the shape matches (`skip_known`), and hosts put it back by shape and list
  other docs (`Inlined::docs_differ`).
* A device rebuilt with other docs and the same API connects as the same API, and the client logs "documentation
  updated" (`DeviceApiInfo::docs_updated_since`).
* The definition cache is keyed by shape.
* The checker compares references by shape only. When only the docs differ, the report gets a note
  (`Report::notes`) and the verdict doesn't change:

```text
note: type exact_units::IndexFrom1 0.1.0: doc comments changed since the old version was saved, the definition is the same
```

An API's own doc comments are different: they are part of its snapshot, and a change needs the compatible bump
([doc comments](versions.md#doc-comments)).

A snapshot saved before EVO-69 holds the signature with docs alone. The checker finds the definition it names among
the ones it knows (the snapshot of that crate version, the source's, the [superseded](#superseded-definitions-evo-68)
ones) and reads its shape from it, so an old snapshot is never edited: it is understood as it was saved. The skipped
references in introspection data and the API hashes changed once with EVO-69; the `signatures` cases of the
[conformance corpus](../serdes/conformance.md) hold the bytes.

### A dependency changed without a version bump (EVO-67)

A crate version that is not published yet can change without a bump (`ww_error` 0.1.0's `CommonError` gaining a
typed index), and then the snapshot of a crate that uses it names one signature while the source names another. The
checker looks both definitions up: the snapshot's in the snapshots built into it, the source's in what the walk of the
source saw (`wire_weaver_core::SourceNotes::definitions`). When it finds both, it compares them as any two versions of
a type, so `arg: u8` -> `arg: Index<Argument, u8>` is `u8 -> Index<Argument, u8, Unbounded>, same bytes` in every
crate that embeds the type, and a change of the bytes is a break named at the field. This holds for generic types
(`exact_units::IndexFrom1`) as for plain ones.

A definition found nowhere (a snapshot saved against a revision of the dependency that no snapshot holds any more) is
still reported by its two signatures. Either way the snapshot of the checking crate's current version differs from its
source: save it again while the version is not published (`just save-snapshots` for `ww_stdlib`).

### Superseded definitions (EVO-68)

Saving the dependency again refreshes its snapshot in place, and the definition the other snapshots name by the old
signature would then be gone: an API line frozen in a snapshot of its own (`thermo_api`'s `wire_v1::Thermo`) and a
device built before still name it. So `ww api save` keeps each definition of the crate's own whose signature with docs
a refresh in place changes, in `api_snapshots/superseded/`, one file per definition (a docs-only change is kept too: a
snapshot saved before EVO-69 names the definition by its signature with docs):

```text
ww_stdlib/ww_error/api_snapshots/superseded/ww_error_0_1_0-type-CommonError-bb2c2d7eb9d06d70.ww
ww_stdlib/ww_indication/api_snapshots/superseded/ww_indication_0_1_0-trait-Indication-e1b197f7361f73a6.ww
```

* A trait or type whose own source didn't change is kept too when its signature did: `Indication`'s method returns a
  `CommonError`.
* A file holds the same bytes the [definition cache](../std_library/ww_self.md#the-definition-cache) keeps for a
  device's definitions: the definition and everything it refers to, in-line, which its signature is the hash of, so a
  file that was changed fails to load.
* `just save-snapshots` copies them into `wire_weaver_snapshots`, which builds them in next to the snapshots
  (`wire_weaver_snapshots::superseded`), and the checker finds a definition by its signature there when the snapshot
  of its crate version has another one: a trait changed in place is then compared resource by resource, a type field
  by field, as above. A client putting a device's definitions back finds them the same way.
* Commit the files with the refreshed snapshots.
* The old definitions of the other crates it refers to must be known when the refresh runs (the snapshots built into
  `ww`, or what an earlier refresh kept): otherwise `ww api save` warns that the definition can't be kept.

A refresh that happened before these files existed is made up for from git history: take the earlier snapshot and
keep what the current one no longer has.

```sh
git show 8446b3979:wire_weaver_snapshots/other_repos/exact_units_0_1_0.ron > old.ron
ww api keep-superseded old.ron --current wire_weaver_snapshots/other_repos/exact_units_0_1_0.ron \
    --into wire_weaver_snapshots/other_repos/superseded
```

The earlier snapshot must parse with today's model, and the definitions of other crates it refers to must be known,
as for `ww api save`.

### A snapshot saved by an older wire_weaver (EVO-70)

A newer wire_weaver may record more about an API than the one that saved a snapshot. A field of the API model that a
snapshot file has nowhere was not recorded by its writer, and the checker compares both sides without it, saying so
(`not compared: ItemEnum::future_version, ...`, see
[the current version's snapshot](checker_tool.md#the-current-versions-snapshot)).

Signatures are the harder case. A snapshot saved before a field of the model that signatures include
(`ApiBundle::wire`, EVO-24) holds signatures made over other bytes, so the checker doesn't require the two sides'
signatures to be equal. It still compares a definition whose signature changed when it knows both definitions: a
signature that names a known definition (the snapshot of its crate version, the source's, a superseded one) was made
over the same bytes. A dependency changed in place is then a compatible change or a break named at the field, as in a
snapshot of today's format. Only a signature that names nothing known is let through unchecked.

### Frozen fixtures and `ww api refresh-pins`

A frozen snapshot that stands for a released API but follows the live `ww_stdlib` by path on purpose, as the
evolution checker's `nucleo_g0b1re_api` fixture does, keeps its pins in line with the source without being saved
again (saving would rewrite it in today's format):

```sh
ww api refresh-pins wire_weaver_evolution/tests/fixtures/nucleo_g0b1re_api
```

* It rewrites only the signatures by which the snapshot of the crate's current version names other crates' traits and
  types, and those crates' versions; an 8-byte reference stays 8 bytes.
* It refuses when the source no longer works with the snapshot on the wire, printing the report, unless
  `--accept-breaking` (a stated break of the dependency). `--dry-run` prints what would change.
* In this repository `just refresh-fixture-pins` runs it, and `just save-snapshots` calls that at its end.
* No signature bytes are edited by hand: the fixture's tests require a wire-compatible verdict against the live
  `ww_stdlib` (not "no changes"), the older-format checks run on a copy with refreshed pins, and
  `wire_weaver_snapshots`' `signatures_from_before_marker_definitions` reads its pins from the snapshot files.
* The tests `old_format_snapshot_passes_against_unchanged_source`, `compatible_stdlib_change_passes_the_fixture` and
  `breaking_stdlib_change_fails_the_fixture` in `wire_weaver_evolution` hold this in place.

## Where the checks live

The checks live in `wire_weaver_evolution` and are the same for `ww api check`, `Check` in an API crate's own test,
and `wire_weaver_client` at connect time: `compat::TypeCheck` for types and markers (including
[units by meaning](units_by_meaning.md#in-code)), `Rule` for the rule each finding names, `superseded` for the
kept definitions, `pins` for `refresh-pins`. Its tests are grouped by topic: `tests::in_place` (EVO-67),
`tests::superseded` (EVO-68), `tests::by_meaning` (EVO-17), `tests::moved_from`, `tests::root_cause` (EVO-21).
