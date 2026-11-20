# module_paths_api

API-46 test API: a type in a `pub mod` that isn't re-exported, one with a lifetime two modules deep, one
imported under another name, one re-exported at the root or under another name, two equal definitions with the
same name in different modules, and a unit marker next to a root type of another shape.

Used by `tests/module_paths`, never built on its own.
