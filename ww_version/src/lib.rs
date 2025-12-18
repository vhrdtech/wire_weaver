#![cfg_attr(not(feature = "std"), no_std)]

use core::fmt::{Debug, Formatter};
use shrink_wrap::prelude::*;

#[cfg(feature = "semver")]
pub use semver;

/// SemVer version as defined by <https://semver.org> in ShrinkWrap format.
/// The minimum size is 2 bytes, when major, minor and patch are less than 8 and pre and build are None.
/// [VersionOwned] is automatically generated from this definition as well and uses String instead.
#[derive_shrink_wrap]
#[derive(PartialEq, Eq, Clone, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[owned = "std"]
#[final_structure]
pub struct Version<'i> {
    pub major: UNib32,
    pub minor: UNib32,
    pub patch: UNib32,
    #[flag]
    build: bool,
    /// Optional pre-release identifier on a version string. This comes after - in a SemVer version, like 1.0.0-alpha.1
    /// Used in compatibility checks.
    pub pre: Option<&'i str>,
    /// Optional build metadata identifier. This comes after + in a SemVer version, as in 0.8.1+zstd.1.5.0.
    /// Not used in compatibility checks, only treated as additional metadata.
    pub build: Option<&'i str>,
}

#[derive_shrink_wrap]
#[derive(PartialEq, Eq, Clone, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[owned = "std"]
#[final_structure]
pub struct FullVersion<'i> {
    pub crate_id: &'i str,
    pub version: Version<'i>,
    // TODO: Add type name
}

/// Compact version for traits-based requests that are made often or through limited bandwidth interfaces.
/// Type id is globally unique across all crates, tracked manually via [ww_global registry](https://github.com/vhrdtech/wire_weaver/tree/master/ww_global).
#[derive_shrink_wrap]
#[derive(PartialEq, Eq, Copy, Clone, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[final_structure]
pub struct CompactVersion {
    pub global_type_id: UNib32,
    pub major: UNib32,
    pub minor: UNib32,
    pub patch: UNib32,
}

impl<'i> Version<'i> {
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Version {
            major: UNib32(major),
            minor: UNib32(minor),
            patch: UNib32(patch),
            pre: None,
            build: None,
        }
    }

    pub const fn full(
        major: u32,
        minor: u32,
        patch: u32,
        pre: Option<&'i str>,
        build: Option<&'i str>,
    ) -> Self {
        Version {
            major: UNib32(major),
            minor: UNib32(minor),
            patch: UNib32(patch),
            pre,
            build,
        }
    }

    pub fn is_protocol_compatible(&self, other: &Self) -> bool {
        if self.major.0 >= 1 && other.major.0 >= 1 {
            self.major == other.major
        } else {
            if self.major != other.major {
                return false;
            }
            if self.minor != other.minor {
                return false;
            }
            true
        }
    }
}

impl<'i> FullVersion<'i> {
    pub const fn new(crate_id: &'i str, version: Version<'i>) -> Self {
        FullVersion { crate_id, version }
    }

    pub fn is_protocol_compatible(&self, other: &Self) -> bool {
        if self.crate_id != other.crate_id {
            return false;
        }
        self.version.is_protocol_compatible(&other.version)
    }
}

impl Debug for Version<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}.{}.{}", self.major.0, self.minor.0, self.patch.0)?;
        if let Some(pre) = self.pre {
            write!(f, "-{}", pre)?;
        }
        if let Some(build) = self.build {
            write!(f, "+{}", build)?;
        }
        Ok(())
    }
}

impl Debug for FullVersion<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{} {:?}", self.crate_id, self.version)
    }
}

#[cfg(feature = "std")]
impl Debug for VersionOwned {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?}", self.as_ref())
    }
}

#[cfg(feature = "std")]
impl Debug for FullVersionOwned {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{} {:?}", self.crate_id, self.version)
    }
}

#[cfg(feature = "std")]
impl FullVersionOwned {
    pub const fn new(crate_id: String, version: VersionOwned) -> Self {
        FullVersionOwned { crate_id, version }
    }
}

#[cfg(feature = "std")]
impl VersionOwned {
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        VersionOwned {
            major: UNib32(major),
            minor: UNib32(minor),
            patch: UNib32(patch),
            pre: None,
            build: None,
        }
    }

    pub const fn full(
        major: u32,
        minor: u32,
        patch: u32,
        pre: Option<String>,
        build: Option<String>,
    ) -> Self {
        VersionOwned {
            major: UNib32(major),
            minor: UNib32(minor),
            patch: UNib32(patch),
            pre,
            build,
        }
    }

    pub fn as_ref(&self) -> Version<'_> {
        Version {
            major: self.major,
            minor: self.minor,
            patch: self.patch,
            pre: self.pre.as_deref(),
            build: self.build.as_deref(),
        }
    }
}

#[cfg(all(feature = "std", feature = "semver"))]
impl TryFrom<semver::Version> for VersionOwned {
    type Error = &'static str;

    fn try_from(value: semver::Version) -> Result<Self, Self::Error> {
        const ERR: &str = "failed to convert u64 to u32";
        let major = u32::try_from(value.major).map_err(|_| ERR)?;
        let minor = u32::try_from(value.minor).map_err(|_| ERR)?;
        let patch = u32::try_from(value.patch).map_err(|_| ERR)?;
        let pre_present = !value.pre.is_empty();
        let pre = pre_present.then_some(value.pre.as_str().to_owned());
        let build_present = !value.build.is_empty();
        let build = build_present.then_some(value.build.as_str().to_owned());
        Ok(Self {
            major: UNib32(major),
            minor: UNib32(minor),
            patch: UNib32(patch),
            pre,
            build,
        })
    }
}

#[cfg(all(feature = "std", feature = "semver"))]
impl TryInto<semver::Version> for VersionOwned {
    type Error = semver::Error;

    fn try_into(self) -> Result<semver::Version, Self::Error> {
        use semver::{BuildMetadata, Prerelease};
        let pre = if let Some(pre) = self.pre {
            Prerelease::new(&pre)?
        } else {
            Prerelease::EMPTY
        };
        let build = if let Some(build) = self.build {
            BuildMetadata::new(&build)?
        } else {
            BuildMetadata::EMPTY
        };
        Ok(semver::Version {
            major: self.major.0 as u64,
            minor: self.minor.0 as u64,
            patch: self.patch.0 as u64,
            pre,
            build,
        })
    }
}

// impl<'i> std::borrow::Borrow<Version<'i>> for VersionOwned {
//     fn borrow(&self) -> &Version<'i> {
//         &Version {
//             major: self.major,
//             minor: self.minor,
//             patch: self.patch,
//             pre: self.pre.as_ref().map(|pre| pre.as_str()),
//             build: self.build.as_ref().map(|build| build.as_str()),
//         }
//     }
// }

#[cfg(feature = "std")]
impl Version<'_> {
    pub fn make_owned(&self) -> VersionOwned {
        VersionOwned {
            major: self.major,
            minor: self.minor,
            patch: self.patch,
            pre: self.pre.map(|pre| pre.to_string()),
            build: self.build.map(|build| build.to_string()),
        }
    }
}

#[cfg(feature = "std")]
impl FullVersion<'_> {
    pub fn make_owned(&self) -> FullVersionOwned {
        FullVersionOwned {
            crate_id: self.crate_id.to_string(),
            version: self.version.make_owned(),
        }
    }
}

#[cfg(feature = "std")]
impl From<FullVersion<'_>> for FullVersionOwned {
    fn from(value: FullVersion<'_>) -> Self {
        FullVersionOwned {
            crate_id: value.crate_id.to_string(),
            version: value.version.make_owned(),
        }
    }
}

#[cfg(feature = "std")]
impl FullVersionOwned {
    pub fn as_ref(&self) -> FullVersion<'_> {
        FullVersion {
            crate_id: &self.crate_id,
            version: self.version.as_ref(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex_literal::hex;

    #[test]
    fn v1_compatibility() {
        let version = Version::new(0, 1, 2);
        let mut buf = [0u8; 8];
        let bytes = version.to_ww_bytes(&mut buf).unwrap();
        assert_eq!(bytes, hex!("01 20"));

        let version_des = Version::from_ww_bytes(bytes).unwrap();
        assert_eq!(version_des, version);
    }

    #[test]
    fn v1_compatibility_full() {
        let version = Version::full(0, 1, 2, Some("pre"), Some("build"));
        let mut buf = [0u8; 32];
        let bytes = version.to_ww_bytes(&mut buf).unwrap();
        // println!("{:02x?}", bytes);
        assert_eq!(bytes, hex!("01 2C 707265 6275696C64 5 3"));

        let version_des = Version::from_ww_bytes(bytes).unwrap();
        assert_eq!(version_des, version);
    }
}
