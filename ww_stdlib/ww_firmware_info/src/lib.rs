#![no_std]

use wire_weaver::prelude::*;

#[ww_trait]
pub trait FirmwareInfo {
    property!(ro crc: (CrcKind, [u8]));
    property!(ro sha256: [u8]);

    /// Optional.
    /// ShrinkWrap serialized build information from [bedrock_build_info crate](https://github.com/romixlab/embedded_bedrock/tree/main/bedrock_build_info)
    /// Contains build timestamp, profile and optimization levels, crate information including enabled features and dependencies.
    /// Target info triple, compiler triple and version.
    /// Git info (sha, dirty, branch, tag).
    ///
    /// Some fields may be omitted from the version stored in device FLASH, while full version can be obtained from firmware ELF.
    stream!(bedrock_build_info: [u8]);
}

pub enum CrcKind {

}