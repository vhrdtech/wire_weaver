#![cfg_attr(not(feature = "std"), no_std)]

mod test;

pub use shrink_wrap;
use shrink_wrap::{BufReader, BufWriter};
pub use wire_weaver_derive::{derive_shrink_wrap, full_version, ww_api, ww_repr, ww_trait};

pub mod prelude {
    pub use shrink_wrap::prelude::*;
    pub use wire_weaver_derive::{derive_shrink_wrap, full_version, ww_api, ww_repr, ww_trait};
}

/// User protocol ID and version. Only major and minor numbers are used and checked.
/// Protocols are compatible if IDs are equal and if major versions matches for major >= 1.
/// So all 1.x and 1.y series are considered compatible, so that older firmwares can talk to newer
/// host software and older host software can talk to newer firmwares.
///
/// If major == 0, then only minor versions are compared. I.e. 0.1 and 0.2 are incompatible and can
/// be used during development.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
// #[deprecated]
pub struct ProtocolInfo {
    pub protocol_id: u32,
    pub major_version: u8,
    pub minor_version: u8,
    // TODO: Add patch version?
}

impl ProtocolInfo {
    pub const fn size_bytes() -> usize {
        6
    }

    // TODO: use derive(shrink_wrap)
    pub fn write(&self, wr: &mut BufWriter) -> Result<(), shrink_wrap::Error> {
        wr.write_u32(self.protocol_id)?;
        wr.write_u8(self.major_version)?;
        wr.write_u8(self.minor_version)?;
        Ok(())
    }

    pub fn read(rd: &mut BufReader) -> Result<ProtocolInfo, shrink_wrap::Error> {
        Ok(ProtocolInfo {
            protocol_id: rd.read_u32()?,
            major_version: rd.read_u8()?,
            minor_version: rd.read_u8()?,
        })
    }

    pub fn is_compatible(&self, other: &ProtocolInfo) -> bool {
        if self.protocol_id != other.protocol_id {
            false
        } else if self.major_version == 0 && other.major_version == 0 {
            self.minor_version == other.minor_version
        } else {
            // not comparing minor versions, protocols are supposed to be backwards and forwards compatible after 1.0
            self.major_version == other.major_version
        }
    }
}
