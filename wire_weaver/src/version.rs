use shrink_wrap::prelude::*;
use wire_weaver_derive::derive_shrink_wrap;

/// SemVer version as defined by <https://semver.org> in ShrinkWrap format.
/// The minimum size is 2 bytes, when major, minor and patch are less than 8 and pre and build are None.
/// [VersionOwned] is automatically generated from this definition as well and uses String instead.
#[derive_shrink_wrap]
#[derive(Debug, PartialEq, Eq, Clone)]
#[shrink_wrap(no_alloc)]
#[owned = "std"]
pub struct Version<'i> {
    pub major: UNib32,
    pub minor: UNib32,
    pub patch: UNib32,
    pub pre: Option<&'i str>,
    pub build: Option<&'i str>,
}

impl Version<'_> {
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Version {
            major: UNib32(major),
            minor: UNib32(minor),
            patch: UNib32(patch),
            pre: None,
            build: None,
        }
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
