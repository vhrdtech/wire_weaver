//! [SettingsGroup]: what `#[derive(Settings)]` implements on a settings struct.

use shrink_wrap::Error;

/// Highest slot id a field can have: slot sets are kept in a 256-bit set.
pub const MAX_SLOT: u32 = 255;

/// A settings struct: plain fields with a default each, stored as a record of the fields that differ from their
/// default. Implemented by [`#[derive(Settings)]`](derive@crate::Settings), not by hand.
///
/// Each field has a slot id that never changes. A record holds the shrink_wrap bytes of each field that isn't at its
/// default, as a byte blob per slot, so a reader can step over any slot it doesn't know and keep it (see the
/// [record format](crate::record)).
pub trait SettingsGroup: Clone + PartialEq {
    /// Version of the crate that defines the struct, written into every record (`CARGO_PKG_VERSION`).
    const VERSION: Version;
    /// The fields, in increasing slot order.
    const SLOTS: &'static [Slot];
    /// Slots of removed fields: never used again. A record that still holds one drops it on the next write.
    const RETIRED: &'static [u32];

    /// Every field at its default.
    fn defaults() -> Self;

    /// Whether the field at `slot` differs from its default. `false` for a slot that isn't a field.
    fn is_set(&self, slot: u32) -> bool;

    /// The shrink_wrap bytes of the field at `slot`, written into `buf`.
    fn write_slot<'b>(&self, slot: u32, buf: &'b mut [u8]) -> Result<&'b [u8], Error>;

    /// Set the field at `slot` from its shrink_wrap bytes. The field is left as it was on an error.
    fn read_slot(&mut self, slot: u32, bytes: &[u8]) -> Result<(), Error>;

    /// Whether `bytes` decode to the current value of the field at `slot`.
    fn slot_eq(&self, slot: u32, bytes: &[u8]) -> bool;

    /// Put the field at `slot` back to its default.
    fn reset_slot(&mut self, slot: u32);

    /// Whether `slot` is one of the fields.
    fn is_known(slot: u32) -> bool {
        Self::SLOTS.binary_search_by_key(&slot, |s| s.id).is_ok()
    }

    /// Whether `slot` belongs to a removed field.
    fn is_retired(slot: u32) -> bool {
        Self::RETIRED.contains(&slot)
    }

    /// The field at `slot`.
    fn slot(slot: u32) -> Option<&'static Slot> {
        Self::SLOTS
            .binary_search_by_key(&slot, |s| s.id)
            .ok()
            .map(|i| &Self::SLOTS[i])
    }
}

/// One field of a [SettingsGroup].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Slot {
    /// Slot id, stable for the life of the field.
    pub id: u32,
    /// Field name, for logs and dump decoders.
    pub name: &'static str,
}

/// `major.minor.patch` of the crate that wrote a record.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Version {
            major,
            minor,
            patch,
        }
    }

    /// Parse `"1.2.3"` at compile time (from `env!("CARGO_PKG_VERSION")`), a pre-release or build suffix is
    /// ignored. Panics (a compile error in a const) on anything else.
    pub const fn parse(s: &str) -> Self {
        let b = s.as_bytes();
        let mut parts = [0u32; 3];
        let mut part = 0;
        let mut i = 0;
        let mut digits = 0;
        while i < b.len() {
            let c = b[i];
            if c == b'.' {
                assert!(digits > 0 && part < 2, "bad version");
                part += 1;
                digits = 0;
            } else if c.is_ascii_digit() {
                parts[part] = parts[part] * 10 + (c - b'0') as u32;
                digits += 1;
            } else if c == b'-' || c == b'+' {
                break;
            } else {
                panic!("bad version");
            }
            i += 1;
        }
        assert!(part == 2 && digits > 0, "bad version");
        Version::new(parts[0], parts[1], parts[2])
    }
}

impl core::fmt::Display for Version {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// A set of slot ids `0..=MAX_SLOT`.
#[derive(Copy, Clone, Default, PartialEq, Eq)]
pub(crate) struct SlotSet([u32; 8]);

impl SlotSet {
    pub(crate) fn insert(&mut self, slot: u32) -> bool {
        if slot > MAX_SLOT {
            return false;
        }
        self.0[(slot / 32) as usize] |= 1 << (slot % 32);
        true
    }

    pub(crate) fn words(&self) -> &[u32] {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_version() {
        assert_eq!(Version::parse("0.5.0"), Version::new(0, 5, 0));
        assert_eq!(Version::parse("12.34.56-rc.1"), Version::new(12, 34, 56));
        assert_eq!(Version::parse("1.0.0+build"), Version::new(1, 0, 0));
    }
}
