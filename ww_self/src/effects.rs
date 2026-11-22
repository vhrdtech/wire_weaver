//! What calling a method or writing a property does (API-58), `#[effect(..)]` in the API: [Effects].

use shrink_wrap::prelude::*;

/// The effects of an API item (API-58), a set of bits: what a call, a property write or a sink's data does to the
/// device and the world around it. `#[effect(..)]` on the item, or on its trait for every item without one.
///
/// An empty set means none was declared: a property read, a const and a stream open are [Effects::READ] by their
/// kind ([ApiItemOwned::effects](crate::ApiItemOwned::effects)). A bit this build doesn't know (an effect added
/// later) is kept; [Effects::unknown] has them, and a consumer treats such a set as the most restrictive case.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    derive(Clone, Copy, Debug, PartialEq, Eq, Hash),
    transparent
)]
pub struct Effects(pub UNib32);

impl Effects {
    /// None declared.
    pub const NONE: Effects = Effects(UNib32(0));
    /// `read`: changes no state. Combines with nothing else.
    pub const READ: Effects = Effects(UNib32(1 << 0));
    /// `edit`: changes state that can be undone or overwritten (a setpoint, a mode, a volatile buffer).
    pub const EDIT: Effects = Effects(UNib32(1 << 1));
    /// `persist`: writes non-volatile storage: wears flash, survives a reboot.
    pub const PERSIST: Effects = Effects(UNib32(1 << 2));
    /// `destructive`: loses data (an erase, a factory reset, a log cleared).
    pub const DESTRUCTIVE: Effects = Effects(UNib32(1 << 3));
    /// `permanent`: an irreversible hardware change (OTP, fuses, readout protection).
    pub const PERMANENT: Effects = Effects(UNib32(1 << 4));
    /// `outbound`: reaches outside the system (sends a frame on a bus, publishes, calls out).
    pub const OUTBOUND: Effects = Effects(UNib32(1 << 5));
    /// `actuate`: switches power or outputs, moves things (a rail, a relay, a motor enable).
    pub const ACTUATE: Effects = Effects(UNib32(1 << 6));
    /// `reset`: reboots the device or drops the link.
    pub const RESET: Effects = Effects(UNib32(1 << 7));
    /// `job`: starts work that outlives the call.
    pub const JOB: Effects = Effects(UNib32(1 << 8));

    /// Every effect this build knows, with its name as `#[effect(..)]` writes it, in bit order.
    pub const NAMED: [(Effects, &'static str); 9] = [
        (Effects::READ, "read"),
        (Effects::EDIT, "edit"),
        (Effects::PERSIST, "persist"),
        (Effects::DESTRUCTIVE, "destructive"),
        (Effects::PERMANENT, "permanent"),
        (Effects::OUTBOUND, "outbound"),
        (Effects::ACTUATE, "actuate"),
        (Effects::RESET, "reset"),
        (Effects::JOB, "job"),
    ];

    const KNOWN: u32 = (1 << Effects::NAMED.len()) - 1;

    /// The raw bits.
    pub const fn bits(self) -> u32 {
        self.0.0
    }

    /// Both sets together.
    #[must_use]
    pub const fn union(self, other: Effects) -> Effects {
        Effects(UNib32(self.bits() | other.bits()))
    }

    /// Whether every effect of `other` is in this set.
    pub const fn contains(self, other: Effects) -> bool {
        self.bits() & other.bits() == other.bits()
    }

    /// No effect declared.
    pub const fn is_empty(self) -> bool {
        self.bits() == 0
    }

    /// Exactly `read`: the item changes nothing, so a GUI may call it freely and a peer with only the right to
    /// observe may call it (the API author's promise, nothing checks it).
    pub const fn is_read(self) -> bool {
        self.bits() == Effects::READ.bits()
    }

    /// The bits this build has no name for, an empty set without any.
    pub const fn unknown(self) -> Effects {
        Effects(UNib32(self.bits() & !Effects::KNOWN))
    }

    /// The effect named `name` as `#[effect(..)]` writes it, `None` for an unknown name.
    pub fn from_name(name: &str) -> Option<Effects> {
        Effects::NAMED
            .iter()
            .find(|(_, n)| *n == name)
            .map(|(e, _)| *e)
    }

    /// The names of the known effects in this set, in bit order.
    pub fn names(self) -> impl Iterator<Item = &'static str> {
        Effects::NAMED
            .into_iter()
            .filter(move |(e, _)| self.contains(*e))
            .map(|(_, n)| n)
    }
}

impl Default for Effects {
    fn default() -> Self {
        Effects::NONE
    }
}

impl core::ops::BitOr for Effects {
    type Output = Effects;

    fn bitor(self, rhs: Effects) -> Effects {
        self.union(rhs)
    }
}

/// As `#[effect(..)]` writes it: `edit, persist`; an unknown bit as `bit9`, an empty set as `none`.
impl core::fmt::Display for Effects {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.is_empty() {
            return f.write_str("none");
        }
        let mut first = true;
        let mut sep = |f: &mut core::fmt::Formatter<'_>| {
            let s = if first { "" } else { ", " };
            first = false;
            f.write_str(s)
        };
        for name in self.names() {
            sep(f)?;
            f.write_str(name)?;
        }
        let unknown = self.unknown().bits();
        for bit in 0..u32::BITS {
            if unknown & (1 << bit) != 0 {
                sep(f)?;
                write!(f, "bit{bit}")?;
            }
        }
        Ok(())
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    #[test]
    fn names_and_unknown_bits() {
        let e = Effects::EDIT | Effects::PERSIST;
        assert_eq!(e.to_string(), "edit, persist");
        assert!(!e.is_read());
        assert!(Effects::READ.is_read());
        assert_eq!(Effects::from_name("actuate"), Some(Effects::ACTUATE));
        assert_eq!(Effects::from_name("nope"), None);
        let later = Effects(UNib32(Effects::EDIT.bits() | 1 << 12));
        assert_eq!(later.unknown(), Effects(UNib32(1 << 12)));
        assert_eq!(later.to_string(), "edit, bit12");
        assert_eq!(Effects::NONE.to_string(), "none");
    }

    #[test]
    fn wire_is_the_unib32() {
        let mut buf = [0u8; 8];
        let bytes = (Effects::EDIT | Effects::PERSIST)
            .to_ww_bytes(&mut buf)
            .unwrap();
        let mut buf2 = [0u8; 8];
        assert_eq!(bytes, UNib32(0b110).to_ww_bytes(&mut buf2).unwrap());
    }
}
