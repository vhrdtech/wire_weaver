# wire_weaver_evolution

> WireWeaver evolution checker: compares an API crate with its saved snapshots and tells which version
> position to bump

Compares a crate's traits and types against a snapshot of an earlier version of the same crate (saved by
`ww api save`) and reports which version position has to move, following the project's
[evolution rules](https://github.com/vhrdtech/wire_weaver/blob/master/docs/evolution/rules.md): a breaking
change (minor before 1.0, major after) when a trait, resource or type is removed, renamed, or can no longer be
used with the other version in either direction; a compatible change (patch before 1.0, minor after) for
everything else. Resources are matched by id inside each trait, traits and types by name. The wire version
(`[package.metadata.wire_weaver] wire = "w2.3"`, EVO-31) is checked apart from the crate version.

See the [main repo](https://github.com/vhrdtech/wire_weaver) for the full picture and
[documentation](https://vhrdtech.github.io/wire_weaver) for the step-by-step guide.
