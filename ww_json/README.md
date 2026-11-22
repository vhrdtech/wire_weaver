# ww_json

JSON for any WireWeaver API, from the API model, never written per command.

- `Codec`: `ValueOwned` to and from `serde_json::Value`, also the arguments of a method as one object by name. Quantities are text with their unit (`"5 ms"`), converted exactly on input; integers beyond ±2⁵³ are strings; errors name the JSON path (`args.timeout`, `items[2].name`).

An MCP server turns an agent's JSON arguments into the values of a call and the typed results back into JSON with it. See the "JSON for any API" page of the docs.
