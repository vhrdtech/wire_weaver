#![cfg_attr(not(feature = "std"), no_std)]
//! Strings a device sends as a small number instead of text (STD-48).
//!
//! Names and reasons (a pin's name, why a request was refused) cost flash and bytes on the wire when sent as text.
//! An [Interned] carries an index into the firmware's string table instead; the text lives only in the firmware's
//! ELF, and a host resolves it there, finding the ELF by the device's build id the same way `ww counters` and
//! `ww report` decode counters and crashes. A device without such a table (a simulator, a Linux server) sends the
//! text itself, and a host without the ELF shows an id as `#<id>`.
//!
//! How a firmware builds its table is open (see the docs page): the type only fixes what travels.

use wire_weaver::prelude::*;

/// A string sent as an index into the firmware's string table, or as text.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Debug, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum Interned<'i> {
    /// Index into the string table of the firmware's ELF.
    Id(UNib32),
    /// The text itself, from a device without a string table.
    Text(&'i str),
}

/// The text, or `#<id>` for an id not resolved yet.
impl core::fmt::Display for Interned<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Interned::Id(id) => write!(f, "#{id}"),
            Interned::Text(text) => f.write_str(text),
            Interned::Unknown(d) => write!(f, "#?{d}"),
        }
    }
}

/// The text, or `#<id>` for an id not resolved yet.
#[cfg(feature = "std")]
impl core::fmt::Display for InternedOwned {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            InternedOwned::Id(id) => write!(f, "#{id}"),
            InternedOwned::Text(text) => f.write_str(text),
            InternedOwned::Unknown(d) => write!(f, "#?{d}"),
        }
    }
}
