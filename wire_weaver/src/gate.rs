//! The server's call gate (SRV-50): one check before every request's handler, so a device can refuse whole traits
//! in a state where it must not run them (a board guarded against reworks its firmware doesn't know serves identity,
//! info and update only).
//!
//! `ww_codegen!(.., server = true, call_gate = true)` makes the generated dispatcher ask the server struct, through
//! [CallGate::admit], before it decodes a request for an item: a method call, a property read or write, a stream
//! open or any other stream traffic, a sink write, and an observe request (asked for the trait level). A refusal is
//! answered with `ww_client_server::ErrorKind::Refused(reason)`, the handler doesn't run. Introspection and the
//! valid indices of nothing (the root) are never asked: a client can always learn what the device is.
//!
//! Without `call_gate` no code is generated for it.
//!
//! ```
//! use wire_weaver::gate::{CallGate, Op, Target, TraitId};
//!
//! const BOARD_INFO: TraitId = TraitId::new("ww_board_info", "BoardInfo");
//! const FIRMWARE_INFO: TraitId = TraitId::new("ww_firmware_info", "FirmwareInfo");
//!
//! struct Server {
//!     guarded: bool,
//! }
//!
//! impl CallGate for Server {
//!     fn admit(&self, target: &Target, _op: Op) -> Result<(), &str> {
//!         if !self.guarded || target.is_in(&FIRMWARE_INFO) {
//!             return Ok(());
//!         }
//!         if target.is_in(&BOARD_INFO) && target.item() != Some("set_reworks") {
//!             return Ok(());
//!         }
//!         Err("board guarded: unknown rework 9")
//!     }
//! }
//!
//! let server = Server { guarded: true };
//! let set = Target { trait_id: &BOARD_INFO, path: "board.set_reworks", level: false };
//! assert_eq!(set.item(), Some("set_reworks"));
//! assert!(server.admit(&set, Op::Call).is_err());
//! ```

/// A trait of an API by its crate and name: the identity of a global trait (`ww_board_info`, `BoardInfo`) wherever
/// it sits in an API, or of a project's own trait.
///
/// Equal when both names are: the same string literal (a firmware's `const` and the generated code's, merged by the
/// linker) is equal without comparing its bytes.
#[derive(Copy, Clone, Debug, Eq)]
pub struct TraitId {
    /// The crate the trait is defined in, as in its `FullVersion` (`ww_board_info`)
    pub crate_id: &'static str,
    /// The trait's name (`BoardInfo`)
    pub name: &'static str,
}

impl TraitId {
    pub const fn new(crate_id: &'static str, name: &'static str) -> Self {
        TraitId { crate_id, name }
    }
}

impl PartialEq for TraitId {
    fn eq(&self, other: &Self) -> bool {
        same_str(self.name, other.name) && same_str(self.crate_id, other.crate_id)
    }
}

/// `a == b`, without the byte compare when both are the same string.
#[inline(always)]
fn same_str(a: &str, b: &str) -> bool {
    a.len() == b.len() && (core::ptr::eq(a.as_ptr(), b.as_ptr()) || a == b)
}

/// The item a request reaches, as the generated dispatcher knows it. One `static` per item, made by the generated
/// code.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// The trait the item is defined in (for an observe request: the observed trait level)
    pub trait_id: &'static TraitId,
    /// The resource path from the API root, trait items and the item joined by dots (`board.set_reworks`, `gpio.set`
    /// for an item under a resource array, `board` for an observe request on that level, empty for the root
    /// level): the fallback for items of a project's own traits used more than once
    pub path: &'static str,
    /// The path ends at a trait level (an observe request), not at an item
    pub level: bool,
}

impl Target {
    /// The item's name in its trait (`set_reworks`, the last part of [Target::path]), None for a trait level.
    pub fn item(&self) -> Option<&'static str> {
        if self.level {
            return None;
        }
        Some(match self.path.rfind('.') {
            Some(dot) => &self.path[dot + 1..],
            None => self.path,
        })
    }

    /// The item is in trait `id`.
    pub fn is_in(&self, id: &TraitId) -> bool {
        *self.trait_id == *id
    }

    /// The item is `item` of trait `id`.
    pub fn is(&self, id: &TraitId, item: &str) -> bool {
        self.is_in(id) && self.item() == Some(item)
    }
}

/// What a request does to its [Target].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Op {
    /// A method call, also one index of a multi call
    Call,
    /// A property or constant read, the valid indices of a resource array, one index of a multi read
    Read,
    /// A property write, also one index of a multi write
    Write,
    /// A stream or sink open
    StreamOpen,
    /// Other stream traffic: close, acknowledgements and the other sideband messages
    Stream,
    /// A write into a sink
    SinkWrite,
    /// An observe request, for a trait level
    Observe,
    /// A request kind added after this list
    Other,
}

/// Implemented on the server struct of a `ww_codegen!(.., server = true, call_gate = true)`: asked before every
/// request's handler runs, see the [module docs](self).
pub trait CallGate {
    /// `Ok(())` serves the request, `Err(reason)` refuses it: the device answers
    /// `ww_client_server::ErrorKind::Refused(reason)` and the handler doesn't run. Keep the reason short, it is sent
    /// with every refusal (`board guarded: unknown rework 9`).
    fn admit(&self, target: &Target, op: Op) -> Result<(), &str>;
}
