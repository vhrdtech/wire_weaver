# Changelog

## Unreleased

### 🚀 Features

- Depends on `ww_quantity`, not `wire_weaver_client`: no client runtime, codegen or snapshot fetch (`ureq`, `rustls`) in its tree.
- `ww_json::toml`: values to and from `toml_edit` by the JSON form's rules, `decode_onto` for settings levels, `write_onto` keeping comments (INT-18).
- `ww_json::toml`'s `decode_onto` reads `{ unset_to_none = true }` (an `Option` field is `None` over any base value) and `{ use_parent = true }` (keeps the base's value, same as leaving the key out); `write_onto` writes the first where a left-out key would read back wrong and leaves either alone when unchanged (INT-18).
- New crate: `ww_json::Codec` (`to_json`, `from_json`, `args_to_json`, `args_from_json`, `raw`) turns the values of any API type into JSON and back, guided by the API model; quantities are text with their unit (`"5 ms"`), integers beyond ±2⁵³ strings, `Vec<u8>` hex. `JsonError` names the JSON path and what was expected (INT-14).
- `schema::item_schemas`, `type_schema` and `args_schema`: a JSON Schema (draft 2020-12) of the arguments, result, value and stream item of every API item, with doc comments, bounds, `x-unit`, `x-min` and `x-max` for quantities and `$defs` for recursive types (INT-15).
