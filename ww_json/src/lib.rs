//! JSON and TOML for any WireWeaver API (INT-14, INT-15, INT-18), guided by the API model:
//! [ValueOwned](ww_self::ValueOwned) to and from [serde_json::Value] ([value]) and [toml_edit] items ([toml]), and a
//! JSON Schema (draft 2020-12) of every argument list, result, property and stream item ([schema]). All are generic
//! over every type, never written per command; JSON and TOML share one walk and its rules.
//!
//! A consumer that turns an agent's JSON arguments into the values of a call and the typed results back into JSON
//! (an MCP server) uses [Codec] and the schema of the same API item, which describes exactly what the codec accepts
//! and writes.

mod doc;
mod error;
pub mod schema;
pub mod toml;
pub mod value;

pub use doc::EXACT_INT;
pub use error::{ErrorKind, JsonError, JsonPath, Step};
/// The TOML crate the [toml] form reads and writes, so callers parse and print with the same version.
pub use toml_edit;
pub use value::{Codec, args_from_json, args_to_json, from_json, to_json};
