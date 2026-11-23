# ww_json

JSON and TOML for any WireWeaver API, from the API model, never written per command.

- `Codec`: `ValueOwned` to and from `serde_json::Value`, also the arguments of a method as one object by name. Quantities are text with their unit (`"5 ms"`), converted exactly on input; integers beyond ±2⁵³ are strings; errors name the JSON path (`args.timeout`, `items[2].name`).
- `toml`: the same codec for TOML files on `toml_edit` (INT-18): integers up to `i64`, unit enum variants snake_case, `decode_onto` sets a settings level onto a base value, `write_onto` writes changes back into a parsed document, keeping its comments.
- `schema`: a JSON Schema (draft 2020-12) of the same forms (INT-15).

An MCP server turns an agent's JSON arguments into the values of a call and the typed results back into JSON with it. See the "JSON and TOML for any API" page of the docs.
