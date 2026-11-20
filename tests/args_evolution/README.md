# args_evolution

Tests EVO-22 (arguments that evolve): a trailing argument with a default is read as its default when an older client leaves it out, and a `#[flatten]` struct argument serializes to the same bytes as its fields passed separately.

Exercises `args_evolution_api` through the generated client and the dynamic client, with `no_alloc` both on and off.
