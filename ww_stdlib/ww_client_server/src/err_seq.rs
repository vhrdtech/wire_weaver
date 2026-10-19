//! Numbers of the error sites in a generated server: the `err_seq` of an [Error](crate::Error) (SRV-22).
//!
//! An error a device answers with is identified by the firmware build (its build id or git sha), the request's path
//! (the host knows it: every answer carries the request's seq), the [ErrorKind](crate::ErrorKind) and `err_seq`.
//! `err_seq` says which site of the generated code raised it, in one of four ranges:
//!
//! - **Item sites**, `0..ITEM_END`: the steps of an item (method, property, stream, sink), numbered by what the
//!   step does, in the order of the steps and the same for every item: [KIND_NOT_TAKEN] 0, [DECODE] 1, [HANDLER] 2,
//!   [ENCODE] 3, ... (the modules [method], [property], [constant], [stream], [sink] name them per kind of item).
//!   The host knows the item's kind and the request's kind, and the error kind tells the rest apart: one number
//!   never stands for two sites of the same item, request and error kind. A step an item doesn't have keeps its
//!   number for the others, so numbers don't move when an item changes, and the same step of any two items is the
//!   same code.
//! - **Deferred return sites**, `DEFERRED_*`: `<method>_ser_return_event` of a deferred method, which the firmware
//!   calls later; same numbers for every method.
//! - **Level sites**, `LEVEL_*`: errors of a trait level's dispatch rather than of an item (no item with that id, an
//!   index that isn't valid, ...), same numbers on every level; the request path says where it stopped.
//! - **Request sites**, `REQUEST_*`, `MULTI_*`, `OBSERVE_*`, `INTROSPECT`: errors before the path is followed or of
//!   a whole request (a global trait path that doesn't resolve, a path too deep, multi requests, observe requests,
//!   introspection).
//!
//! [GENERIC] (`u32::MAX`) is the answer of `ww_device` when the server couldn't even encode
//! an error. [Error::site](crate::Error::site) sorts an `err_seq` into these ranges, and an error's `Display` shows
//! it.

/// Item sites are below this.
pub const ITEM_END: u32 = 32;

/// An item doesn't take this request kind (a write to a method, a call on a property).
pub const KIND_NOT_TAKEN: u32 = 0;
/// The request's arguments or data don't decode.
pub const DECODE: u32 = 1;
/// The user's handler answered `Unimplemented` (or deferred where it can't).
pub const HANDLER: u32 = 2;
/// The answer (a value, a status, a sideband reply) didn't fit.
pub const ENCODE: u32 = 3;
/// The handler's own error didn't fit into the answer.
pub const USER_ERROR: u32 = 4;
/// A stream's store failed.
pub const STORE: u32 = 5;
/// A stream can't be resumed (no such session, or its store failed).
pub const RESUME: u32 = 6;
/// The data is longer than the item's bound.
pub const TOO_LONG: u32 = 7;
/// A credit sink's write position doesn't decode (API-28).
pub const CREDIT_POS: u32 = 8;
/// A deferred method in a multi request (CLT-15), refused before the handler runs.
pub const DEFERRED_IN_MULTI: u32 = 9;

/// Sites of a method, by step.
pub mod method {
    pub use super::{DEFERRED_IN_MULTI, KIND_NOT_TAKEN};
    /// The arguments don't decode.
    pub const ARGS: u32 = super::DECODE;
    /// The handler answered `Unimplemented`.
    pub const HANDLER: u32 = super::HANDLER;
    /// The return value didn't fit into the answer.
    pub const OUTPUT: u32 = super::ENCODE;
}

/// Sites of a property, by step.
pub mod property {
    pub use super::{KIND_NOT_TAKEN, USER_ERROR};
    /// Write: the value doesn't decode.
    pub const WRITE_DECODE: u32 = super::DECODE;
    /// Write: the setter answered `Unimplemented`; read: the getter did.
    pub const HANDLER: u32 = super::HANDLER;
    /// Read: the value didn't fit into the answer.
    pub const VALUE: u32 = super::ENCODE;
}

/// Sites of a `const` item (API-36).
pub mod constant {
    pub use super::KIND_NOT_TAKEN;
    /// The value didn't fit into the answer.
    pub const VALUE: u32 = super::ENCODE;
}

/// Sites of a stream (device to host), by step.
pub mod stream {
    pub use super::{KIND_NOT_TAKEN, RESUME, STORE};
    /// Open: the arguments don't decode.
    pub const ARGS: u32 = super::DECODE;
    /// Open: the handler answered `Unimplemented`, or deferred (which a stream can't).
    pub const OPEN: u32 = super::HANDLER;
    /// Open or resume: the answer didn't fit.
    pub const OPENED: u32 = super::ENCODE;
    /// Sideband: the handler's sideband answer didn't fit.
    pub const SIDEBAND: u32 = super::ENCODE;
    /// Read: the status didn't fit into the answer.
    pub const STATUS: u32 = super::ENCODE;
}

/// Sites of a sink (host to device), by step.
pub mod sink {
    pub use super::{CREDIT_POS, KIND_NOT_TAKEN};
    /// Write: the data is longer than the sink's bound.
    pub const DATA_TOO_LONG: u32 = super::TOO_LONG;
    /// Write: the data doesn't decode.
    pub const DATA: u32 = super::DECODE;
    /// Sideband: the answer didn't fit.
    pub const SIDEBAND: u32 = super::ENCODE;
}

/// `<method>_ser_return_event`: the return value didn't fit into the scratch buffer.
pub const DEFERRED_OUTPUT_SER: u32 = 32;
/// `<method>_ser_return_event`: finishing the return value failed.
pub const DEFERRED_OUTPUT_FINISH: u32 = 33;
/// `<method>_ser_return_event`: the event didn't fit into the event buffer.
pub const DEFERRED_EVENT_SER: u32 = 34;
/// `<method>_ser_return_event`: finishing the event failed.
pub const DEFERRED_EVENT_FINISH: u32 = 35;

/// The level has no item with the next id of the path.
pub const LEVEL_BAD_PATH: u32 = 40;
/// The item is `#[optional]` and left out of this server (API-16).
pub const LEVEL_OMITTED: u32 = 41;
/// The index isn't one of the array's valid indices.
pub const LEVEL_BAD_INDEX: u32 = 42;
/// The path ends at an array without an index, and the request isn't a read of its valid indices.
pub const LEVEL_INDEX_EXPECTED: u32 = 43;
/// The path ends at a trait level, which doesn't take this request kind.
pub const LEVEL_NOT_SUPPORTED: u32 = 44;
/// The valid indices of an array didn't fit into the answer.
pub const LEVEL_INDICES_SER: u32 = 45;
/// The answer's header didn't fit into the scratch buffer.
pub const REPLY_START: u32 = 46;

/// A global trait path to a trait this device doesn't implement in a compatible version (API-20).
pub const REQUEST_TRAIT_NOT_FOUND: u32 = 48;
/// The path is deeper than the server takes.
pub const REQUEST_PATH_TOO_DEEP: u32 = 49;
/// Multi request: the path of an index is deeper than the server takes.
pub const MULTI_PATH_TOO_DEEP: u32 = 50;
/// Multi request: the arguments (or data) of the indices don't decode, or there are fewer than indices.
pub const MULTI_ARGS: u32 = 51;
/// Multi request: a deferred method can't answer inside one (CLT-15).
pub const MULTI_DEFERRED: u32 = 52;
/// Multi request: the answers didn't fit into the scratch buffer.
pub const MULTI_OVERFLOW: u32 = 53;
/// Observe request: an entry doesn't decode.
pub const OBSERVE_MALFORMED: u32 = 54;
/// Observe request: an entry names a property that can't be observed.
pub const OBSERVE_REFUSED: u32 = 55;
/// Observe request: the answer didn't fit.
pub const OBSERVE_SER: u32 = 56;
/// Observe request: a level under a resource array can't be observed yet (API-29).
pub const OBSERVE_ARRAY: u32 = 57;
/// Observe request: the path is no level with observable properties.
pub const OBSERVE_PATH: u32 = 58;
/// Introspection: an event of the introspection data couldn't be sent.
pub const INTROSPECT: u32 = 59;

/// `ww_device`'s answer when the server failed without an error to send.
pub const GENERIC: u32 = u32::MAX;

/// Which part of the generated server an `err_seq` comes from, see the [module docs](self).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Site {
    /// Step `n` of the item at the request's path.
    Item(u32),
    /// A deferred method's return helper, one of the `DEFERRED_*` sites.
    DeferredReturn(u32),
    /// The dispatch of a trait level on the request's path, one of the `LEVEL_*` sites (and [REPLY_START]).
    Level(u32),
    /// The request as a whole, one of the `REQUEST_*`, `MULTI_*`, `OBSERVE_*` sites or [INTROSPECT].
    Request(u32),
    /// [GENERIC].
    Generic,
    /// Outside of every range: a server built with another numbering.
    Unknown(u32),
}

impl Site {
    pub fn of(err_seq: u32) -> Site {
        match err_seq {
            0..ITEM_END => Site::Item(err_seq),
            DEFERRED_OUTPUT_SER..=DEFERRED_EVENT_FINISH => Site::DeferredReturn(err_seq),
            LEVEL_BAD_PATH..=REPLY_START => Site::Level(err_seq),
            REQUEST_TRAIT_NOT_FOUND..=INTROSPECT => Site::Request(err_seq),
            GENERIC => Site::Generic,
            _ => Site::Unknown(err_seq),
        }
    }

    /// Short description of the site, None outside of the ranges.
    pub fn name(&self) -> Option<&'static str> {
        let seq = match self {
            Site::Item(s) => {
                return Some(match *s {
                    KIND_NOT_TAKEN => "request kind not taken",
                    DECODE => "decode",
                    HANDLER => "handler",
                    ENCODE => "encode",
                    USER_ERROR => "user error encode",
                    STORE => "stream store",
                    RESUME => "stream resume",
                    TOO_LONG => "data too long",
                    CREDIT_POS => "credit position",
                    DEFERRED_IN_MULTI => "deferred method in a multi request",
                    _ => return None,
                });
            }
            Site::DeferredReturn(s) | Site::Level(s) | Site::Request(s) => *s,
            Site::Generic => return Some("no error could be encoded"),
            Site::Unknown(_) => return None,
        };
        Some(match seq {
            DEFERRED_OUTPUT_SER => "value too big",
            DEFERRED_OUTPUT_FINISH => "value not finished",
            DEFERRED_EVENT_SER => "event too big",
            DEFERRED_EVENT_FINISH => "event not finished",
            LEVEL_BAD_PATH => "no such item",
            LEVEL_OMITTED => "item left out of this firmware",
            LEVEL_BAD_INDEX => "index not valid",
            LEVEL_INDEX_EXPECTED => "array index expected",
            LEVEL_NOT_SUPPORTED => "a trait level doesn't take this request",
            LEVEL_INDICES_SER => "valid indices too big",
            REPLY_START => "answer header too big",
            REQUEST_TRAIT_NOT_FOUND => "global trait not found",
            REQUEST_PATH_TOO_DEEP => "path too deep",
            MULTI_PATH_TOO_DEEP => "multi request: path too deep",
            MULTI_ARGS => "multi request: arguments don't decode",
            MULTI_DEFERRED => "multi request: deferred method",
            MULTI_OVERFLOW => "multi request: answers too big",
            OBSERVE_MALFORMED => "observe request: entry doesn't decode",
            OBSERVE_REFUSED => "observe request: not observable",
            OBSERVE_SER => "observe request: answer too big",
            OBSERVE_ARRAY => "observe request: level in a resource array",
            OBSERVE_PATH => "observe request: no observable level there",
            INTROSPECT => "introspection: event not sent",
            _ => return None,
        })
    }
}

/// `item site 2 (handler)`, `level site 40 (no such item)`.
impl core::fmt::Display for Site {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let (range, n) = match self {
            Site::Item(n) => ("item", n),
            Site::DeferredReturn(n) => ("deferred return", n),
            Site::Level(n) => ("level", n),
            Site::Request(n) => ("request", n),
            Site::Generic => return write!(f, "generic error (no error could be encoded)"),
            Site::Unknown(n) => return write!(f, "site {n}"),
        };
        match self.name() {
            Some(name) => write!(f, "{range} site {n} ({name})"),
            None => write!(f, "{range} site {n}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        assert_eq!(Site::of(0), Site::Item(0));
        assert_eq!(Site::of(ITEM_END - 1), Site::Item(31));
        assert_eq!(Site::of(DEFERRED_EVENT_SER), Site::DeferredReturn(34));
        assert_eq!(Site::of(LEVEL_BAD_INDEX), Site::Level(42));
        assert_eq!(Site::of(MULTI_ARGS), Site::Request(51));
        assert_eq!(Site::of(GENERIC), Site::Generic);
        assert_eq!(Site::of(1000), Site::Unknown(1000));
        assert_eq!(Site::of(HANDLER).to_string(), "item site 2 (handler)");
        assert_eq!(
            Site::of(LEVEL_BAD_PATH).to_string(),
            "level site 40 (no such item)"
        );
        // every site in a range has a name
        for seq in (0..=DEFERRED_IN_MULTI).chain(ITEM_END..=INTROSPECT) {
            let site = Site::of(seq);
            if !matches!(site, Site::Unknown(_)) {
                assert!(site.name().is_some(), "{seq}");
            }
        }
    }
}
