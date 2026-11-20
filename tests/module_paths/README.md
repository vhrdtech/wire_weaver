# module_paths

Tests of types in modules (API-46) through generated server and client code: each type is named by the path
other crates see (`module_paths_api::geometry::shapes::Label`, a root re-export, a re-export under another
name), and equal definitions in different modules stay apart.
