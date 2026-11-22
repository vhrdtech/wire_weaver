# Changelog

## Unreleased

### 🚀 Features

- New crate: `ww_json::Codec` (`to_json`, `from_json`, `args_to_json`, `args_from_json`, `raw`) turns the values of any API type into JSON and back, guided by the API model; quantities are text with their unit (`"5 ms"`), integers beyond ±2⁵³ strings, `Vec<u8>` hex. `JsonError` names the JSON path and what was expected (INT-14).
