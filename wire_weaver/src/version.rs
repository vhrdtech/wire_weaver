use core::fmt::{Debug, Formatter};
use shrink_wrap::prelude::*;
use wire_weaver_derive::derive_shrink_wrap;

/// SemVer version as defined by <https://semver.org> in ShrinkWrap format.
/// The minimum size is 2 bytes, when major, minor and patch are less than 8 and pre and build are None.
/// [VersionOwned] is automatically generated from this definition as well and uses String instead.
#[derive_shrink_wrap]
#[derive(PartialEq, Eq, Clone)]
#[shrink_wrap(no_alloc)]
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

impl<'i> Version<'i> {
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Version {
            major: UNib32(major),
            minor: UNib32(minor),
            patch: UNib32(patch),
            pre: None,
            build: None,
        }
    }

    pub fn full(
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

#[cfg(feature = "std")]
impl Debug for VersionOwned {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?}", self.as_ref())
    }
}

#[cfg(feature = "std")]
impl VersionOwned {
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        VersionOwned {
            major: UNib32(major),
            minor: UNib32(minor),
            patch: UNib32(patch),
            pre: None,
            build: None,
        }
    }

    pub fn full(
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

#[cfg(feature = "std")]
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

#[cfg(feature = "std")]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{from_slice, to_slice};
    use hex_literal::hex;

    #[test]
    fn v1_compatibility() {
        let version = Version::new(0, 1, 2);
        let mut buf = [0u8; 8];
        let bytes = to_slice(&mut buf, &version).unwrap();
        assert_eq!(bytes, hex!("01 20"));

        let version_des: Version = from_slice(bytes).unwrap();
        assert_eq!(version_des, version);
    }

    #[test]
    fn v1_compatibility_full() {
        let version = Version::full(0, 1, 2, Some("pre"), Some("build"));
        let mut buf = [0u8; 32];
        let bytes = to_slice(&mut buf, &version).unwrap();
        println!("{:02x?}", bytes);
        assert_eq!(bytes, hex!("01 2C 707265 6275696C64 5 3"));

        let version_des: Version = from_slice(bytes).unwrap();
        assert_eq!(version_des, version);
    }
}
