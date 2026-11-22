//! JSON for any WireWeaver API (INT-14), guided by the API model: [ValueOwned](ww_self::ValueOwned) to and from
//! [serde_json::Value] ([value]), generic over every type, never written per command.
//!
//! A consumer that turns an agent's JSON arguments into the values of a call and the typed results back into JSON
//! (an MCP server) uses [Codec].

mod error;
pub mod value;

pub use error::{ErrorKind, JsonError, JsonPath, Step};
pub use value::{Codec, EXACT_INT, args_from_json, args_to_json, from_json, to_json};
