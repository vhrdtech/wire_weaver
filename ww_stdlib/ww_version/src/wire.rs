//! The wire version of an API, `w2.3` (EVO-24): what link setup and `#[since]` decide on, apart from the crate
//! version, which is Cargo's alone.

use core::fmt::{Debug, Display, Formatter};
use core::str::FromStr;
use shrink_wrap::prelude::*;

/// Wire version of an API root, written `w2.3` and said "wire 2.3": a line and a revision.
///
/// - The line (`2`) counts wire breaks: peers on different lines don't talk.
/// - The revision (`3`) counts wire-compatible additions on a line (a new method, property or stream, a field or a
///   trailing argument with a default): peers on one line always do, and `#[since = "w2.3"]` names the revision that
///   added an item.
///
/// The `w` is part of the notation everywhere (source attributes, CLI output, logs, docs), so a crate version can't
/// be taken for one: [WireVersion::from_str] refuses `2.3`, `w2.3.0` and `0.2.0`. Ordered by line, then revision.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    cfg_attr_borrowed(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    derive(PartialEq, Eq, Copy, Clone, Hash, PartialOrd, Ord),
    final_structure
)]
pub struct WireVersion {
    pub line: UNib32,
    pub revision: UNib32,
}

impl WireVersion {
    pub const fn new(line: u32, revision: u32) -> Self {
        WireVersion {
            line: UNib32(line),
            revision: UNib32(revision),
        }
    }

    /// On the same line: peers on one line always talk, whatever their revisions.
    pub const fn same_line(&self, other: &Self) -> bool {
        self.line.0 == other.line.0
    }

    /// Line 0, the design line (EVO-41): a root on `w0` is an API still being designed. Breaks are accepted
    /// without a new line, no snapshots are saved, `#[since]` is refused, and a client checks the device's
    /// introspection data instead of trusting a revision. A first release starts at `w1.0`.
    pub const fn is_design_line(&self) -> bool {
        self.line.0 == 0
    }

    /// The next revision on this line, for a wire-compatible addition.
    pub const fn next_revision(&self) -> Self {
        WireVersion::new(self.line.0, self.revision.0 + 1)
    }

    /// The first revision of the next line, for a wire break.
    pub const fn next_line(&self) -> Self {
        WireVersion::new(self.line.0 + 1, 0)
    }
}

impl Display for WireVersion {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "w{}.{}", self.line.0, self.revision.0)
    }
}

impl Debug for WireVersion {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        Display::fmt(self, f)
    }
}

/// Why a string is not a wire version, see [WireVersion::from_str]. `Display` says how to write it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireVersionError {
    Empty,
    /// No `w` in front: `2.3`, or a crate version like `0.2.0` pasted by mistake.
    NoPrefix,
    /// `W2.3`: the prefix is a lower-case `w`.
    UpperCasePrefix,
    /// `w2.3.0`: three numbers are a crate version, a wire version has a line and a revision.
    ThreeParts,
    /// `w2`: no revision.
    NoRevision,
    /// More than three parts, or nothing between the dots.
    Malformed,
    /// A part that is not a decimal number fitting `u32`, or has a leading zero (`w02.3`).
    BadNumber,
}

impl Display for WireVersionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let why = match self {
            WireVersionError::Empty => "empty",
            WireVersionError::NoPrefix => {
                "no `w` in front: a wire version is written `w2.3` (line 2, revision 3), so it can't be \
                 mixed up with a crate version; a crate version is Cargo's and not a wire version"
            }
            WireVersionError::UpperCasePrefix => "the prefix is a lower-case `w`: `w2.3`",
            WireVersionError::ThreeParts => {
                "three numbers are a crate version: a wire version has a line and a revision, `w2.3`"
            }
            WireVersionError::NoRevision => {
                "no revision: write `w2.0` for the first revision of line 2"
            }
            WireVersionError::Malformed => "expected `w<line>.<revision>`, like `w2.3`",
            WireVersionError::BadNumber => {
                "line and revision are decimal numbers without leading zeros, like `w2.3`"
            }
        };
        write!(f, "not a wire version: {why}")
    }
}

impl core::error::Error for WireVersionError {}

impl FromStr for WireVersion {
    type Err = WireVersionError;

    /// `w<line>.<revision>`, nothing else: no spaces, no upper-case `W`, no third number.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            return Err(WireVersionError::Empty);
        }
        let rest = match s.as_bytes()[0] {
            b'w' => &s[1..],
            b'W' => return Err(WireVersionError::UpperCasePrefix),
            _ => return Err(WireVersionError::NoPrefix),
        };
        let mut parts = rest.split('.');
        let line = parts.next().unwrap_or_default();
        let (Some(revision), third, fourth) = (parts.next(), parts.next(), parts.next()) else {
            return Err(if line.is_empty() {
                WireVersionError::Malformed
            } else {
                number(line).err().unwrap_or(WireVersionError::NoRevision)
            });
        };
        match (third, fourth) {
            (None, _) => {}
            (Some(_), None) => return Err(WireVersionError::ThreeParts),
            (Some(_), Some(_)) => return Err(WireVersionError::Malformed),
        }
        if line.is_empty() || revision.is_empty() {
            return Err(WireVersionError::Malformed);
        }
        Ok(WireVersion::new(number(line)?, number(revision)?))
    }
}

fn number(s: &str) -> Result<u32, WireVersionError> {
    if !s.bytes().all(|b| b.is_ascii_digit()) || (s.len() > 1 && s.starts_with('0')) {
        return Err(WireVersionError::BadNumber);
    }
    s.parse().map_err(|_| WireVersionError::BadNumber)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex_literal::hex;

    fn parse(s: &str) -> Result<WireVersion, WireVersionError> {
        s.parse()
    }

    #[test]
    fn parses_and_displays() {
        assert_eq!(parse("w2.3"), Ok(WireVersion::new(2, 3)));
        assert_eq!(parse("w0.0"), Ok(WireVersion::new(0, 0)));
        assert_eq!(parse("w10.42"), Ok(WireVersion::new(10, 42)));
        assert_eq!(WireVersion::new(2, 3).to_string(), "w2.3");
        assert_eq!(format!("{:?}", WireVersion::new(1, 0)), "w1.0");
        for s in ["w0.0", "w2.3", "w4294967295.1"] {
            assert_eq!(parse(s).unwrap().to_string(), s);
        }
    }

    #[test]
    fn line_0_is_the_design_line() {
        assert!(WireVersion::new(0, 0).is_design_line());
        assert!(WireVersion::new(0, 7).is_design_line());
        assert!(!WireVersion::new(1, 0).is_design_line());
    }

    #[test]
    fn refuses_crate_versions_and_typos() {
        use WireVersionError::*;
        let cases = [
            ("", Empty),
            ("2.3", NoPrefix),
            ("0.2.0", NoPrefix),
            ("v2.3", NoPrefix),
            (" w2.3", NoPrefix),
            ("W2.3", UpperCasePrefix),
            ("w2.3.0", ThreeParts),
            ("w0.1.1", ThreeParts),
            ("w2", NoRevision),
            ("w", Malformed),
            ("w.3", Malformed),
            ("w2.", Malformed),
            ("w1.2.3.4", Malformed),
            ("w2.3 ", BadNumber),
            ("w02.3", BadNumber),
            ("w2.-3", BadNumber),
            ("w2.+3", BadNumber),
            ("w4294967296.0", BadNumber),
            ("w2w3", BadNumber),
        ];
        for (s, expected) in cases {
            assert_eq!(parse(s), Err(expected), "{s:?}");
        }
    }

    #[test]
    fn errors_say_why() {
        let msg = |s: &str| parse(s).unwrap_err().to_string();
        assert!(msg("0.2.0").contains("crate version"), "{}", msg("0.2.0"));
        assert!(msg("w0.2.0").contains("crate version"), "{}", msg("w0.2.0"));
        assert!(msg("w2").contains("w2.0"), "{}", msg("w2"));
        assert!(msg("W2.3").contains("lower-case"), "{}", msg("W2.3"));
    }

    #[test]
    fn order_and_lines() {
        let v = |s: &str| parse(s).unwrap();
        assert!(v("w2.3") < v("w2.4"));
        assert!(v("w2.10") > v("w2.9"));
        assert!(v("w2.9") < v("w3.0"));
        assert!(v("w2.3").same_line(&v("w2.0")));
        assert!(!v("w2.3").same_line(&v("w3.3")));
        assert_eq!(v("w2.3").next_revision(), v("w2.4"));
        assert_eq!(v("w2.3").next_line(), v("w3.0"));
    }

    #[test]
    fn wire_bytes() {
        let mut buf = [0u8; 16];
        let bytes = WireVersion::new(2, 3).to_ww_bytes(&mut buf).unwrap();
        assert_eq!(bytes, hex!("23"));
        assert_eq!(
            WireVersion::from_ww_bytes(bytes).unwrap(),
            WireVersion::new(2, 3)
        );
        let bytes = WireVersion::new(1, 200).to_ww_bytes(&mut buf).unwrap();
        assert_eq!(
            WireVersion::from_ww_bytes(bytes).unwrap(),
            WireVersion::new(1, 200)
        );
    }
}
