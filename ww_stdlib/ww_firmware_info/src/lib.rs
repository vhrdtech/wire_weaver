#![cfg_attr(not(feature = "std"), no_std)]
//! Which firmware runs and why the device last reset (STD-24).
//!
//! [FirmwareInfo] answers what `--version` of our CLIs prints (`b200_usb_io 0.5.0 (a1b2c3d-dirty, built 3 Oct 2026
//! 18:20)`) and gives the **GNU build id**, the firmware's identity: the linker hashes the linked output, so the id
//! changes with every byte of code, unlike a git commit (dirty builds, two profiles of one commit) or a version. A
//! host finds the firmware's ELF by it, which defmt logs, `cnt` counters and crash records all need.
//! [LastReset] reports why the device last reset and, after a crash, what the firmware saved before it.
//!
//! # Producing the summary
//!
//! In the firmware's `Cargo.toml` enable the `build` feature for build dependencies:
//!
//! ```toml
//! [dependencies]
//! ww_firmware_info = { version = "0.1", default-features = false, features = ["build-id"] }
//! [build-dependencies]
//! ww_firmware_info = { version = "0.1", features = ["build"] }
//! ```
//!
//! `build.rs` calls `build::emit`, the firmware builds the value with [firmware_summary!]:
//!
//! ```ignore
//! // build.rs
//! fn main() { ww_firmware_info::build::emit(); }
//!
//! // firmware: in the handler of `FirmwareInfo::FIRMWARE`
//! let summary = ww_firmware_info::firmware_summary!();
//! ```
//!
//! # Build id
//!
//! Pass `-C link-arg=--build-id=sha1` to the linker and keep the note in flash with one line in `memory.x`
//! (a `SECTIONS` block that also defines the symbols `build_id` reads):
//!
//! ```text
//! SECTIONS {
//!   .note.gnu.build-id : ALIGN(4) {
//!     __build_id_start = .;
//!     KEEP(*(.note.gnu.build-id))
//!     __build_id_end = .;
//!   } > FLASH
//! } INSERT AFTER .rodata;
//! ```
//!
//! Without the note `FirmwareInfo::BUILD_ID` is empty and hosts fall back to the newest ELF they find, with a
//! warning.

use wire_weaver::prelude::*;
pub use ww_date_time::DateTime;
pub use ww_si::Second;
pub use ww_version::Version;
#[cfg(feature = "std")]
pub use ww_version::VersionOwned;

#[cfg(feature = "build")]
pub mod build;
mod build_id;
mod last_reset;

#[cfg(all(feature = "build-id", not(feature = "std")))]
pub use build_id::build_id;
pub use build_id::parse_build_id_note;
pub use last_reset::*;

/// Longest build id carried on the wire (sha1 is 20 bytes, `--build-id=sha256` 32).
pub const BUILD_ID_MAX_LEN: usize = 32;

/// Which firmware runs: what `--version` prints, and how to find its ELF.
///
/// All items are constant for the life of a firmware image.
#[ww_trait]
pub trait FirmwareInfo {
    /// Name, version, git commit, build time and profile.
    const FIRMWARE: FirmwareSummary<'i>;

    /// GNU build id of the image (20 bytes with sha1), empty when the firmware didn't keep the note. A host finds
    /// the ELF by it.
    const BUILD_ID: MaxLen<RefVec<'i, u8>, BUILD_ID_MAX_LEN>;

    /// The `bedrock_build_info` compact record as stored in flash (header with magic, length and CRC-32 included):
    /// crate features, dependencies, compiler, target. The full record is in the ELF only.
    #[optional]
    const BUILD_RECORD: MaxLen<RefVec<'i, u8>, 512>;
}

/// Why the device last reset and what the firmware saved before it, see [LastReset::last_reset].
///
/// Capturing the record is firmware infrastructure (a fault or panic handler writing into RAM that survives a
/// reset); this trait only reports it.
#[ww_trait]
pub trait LastReset {
    /// Cause of the last reset and, after a crash, the record saved before it.
    fn last_reset() -> ResetRecord<'i>;

    /// Boots since the boot counter was cleared. With the cause it tells a reset loop from a single crash.
    fn boot_count() -> u32;

    /// Forget the crash record and zero the boot counter, after the host saved them.
    fn clear();
}

/// What `--version` shows of a firmware, see `FirmwareInfo::FIRMWARE`.
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq, Eq))]
pub struct FirmwareSummary<'i> {
    /// Package or product firmware name, `b200_usb_io`.
    pub name: MaxLen<&'i str, 32>,
    /// Package version.
    pub version: Version<'i>,
    /// `None` when built outside git.
    pub git: Option<GitInfo<'i>>,
    /// Build time (UTC); `SOURCE_DATE_EPOCH` when set, for reproducible builds.
    pub built: Option<DateTime>,
    /// Cargo profile the firmware was built with.
    pub profile: BuildProfile<'i>,
}

/// The git state a firmware was built from.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Debug, PartialEq, Eq)
)]
pub struct GitInfo<'i> {
    /// Commit id, 20 bytes; hosts print the first 7 hex digits.
    pub commit: MaxLen<RefVec<'i, u8>, 20>,
    /// Uncommitted changes were present at build time.
    pub dirty: bool,
}

/// Cargo profile of a build.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Debug, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum BuildProfile<'i> {
    Release,
    Debug,
    /// A custom profile, by name.
    Other(MaxLen<&'i str, 16>),
}

/// Truncate `s` to at most `N` bytes at a char boundary, for values taken from the environment at build time.
pub fn bounded_str<const N: usize>(s: &str) -> MaxLen<&str, N> {
    let mut end = s.len().min(N);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    MaxLen::new_unchecked(&s[..end])
}

/// Decode a hex string at compile time into 20 bytes (a git commit), zeros when the string is not 40 hex digits.
/// Used by [firmware_summary!].
#[doc(hidden)]
pub const fn hex20(s: &str) -> ([u8; 20], bool) {
    let b = s.as_bytes();
    let mut out = [0u8; 20];
    if b.len() != 40 {
        return (out, false);
    }
    let mut i = 0;
    while i < 20 {
        let (hi, lo) = (nibble(b[2 * i]), nibble(b[2 * i + 1]));
        if hi > 15 || lo > 15 {
            return ([0; 20], false);
        }
        out[i] = (hi << 4) | lo;
        i += 1;
    }
    (out, true)
}

const fn nibble(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => 255,
    }
}

/// Build time from the `WW_FW_BUILT` variable of `build::emit` (`YYYY-MM-DDTHH:MM:SS`), `None` when absent or
/// malformed. Used by [firmware_summary!].
#[doc(hidden)]
pub fn parse_built(s: &str) -> Option<DateTime> {
    let b = s.as_bytes();
    if b.len() != 19 {
        return None;
    }
    let num = |from: usize, to: usize| -> Option<u32> { s.get(from..to)?.parse().ok() };
    DateTime::from_ymd_hms_utc_opt(
        num(0, 4)? as i32,
        num(5, 7)? as u8,
        num(8, 10)? as u8,
        num(11, 13)? as u8,
        num(14, 16)? as u8,
        num(17, 19)? as u8,
        0,
    )
}

/// Build a [FirmwareSummary] from the variables `build::emit` set in the firmware's `build.rs`.
///
/// Not a `const`: the build time is converted at run time. Call it from the handler of
/// `FirmwareInfo::FIRMWARE`. Values that don't fit their bounds are cut.
///
/// `$crate::...` needs a firmware crate that depends on `ww_firmware_info`; the variables are read with `env!`, so a
/// missing `build.rs` call is a compile error.
#[macro_export]
macro_rules! firmware_summary {
    () => {{
        const COMMIT: ([u8; 20], bool) = $crate::hex20(env!("WW_FW_GIT_COMMIT"));
        $crate::FirmwareSummary {
            name: $crate::bounded_str(env!("WW_FW_NAME")),
            version: $crate::Version::full(
                $crate::parse_u32(env!("WW_FW_VERSION_MAJOR")),
                $crate::parse_u32(env!("WW_FW_VERSION_MINOR")),
                $crate::parse_u32(env!("WW_FW_VERSION_PATCH")),
                if env!("WW_FW_VERSION_PRE").is_empty() {
                    None
                } else {
                    Some(env!("WW_FW_VERSION_PRE"))
                },
                None,
            ),
            git: if COMMIT.1 {
                Some($crate::GitInfo {
                    commit: $crate::MaxLen::new_unchecked($crate::RefVec::new_bytes(&COMMIT.0)),
                    dirty: !env!("WW_FW_GIT_DIRTY").is_empty(),
                })
            } else {
                None
            },
            built: $crate::parse_built(env!("WW_FW_BUILT")),
            profile: match env!("WW_FW_PROFILE") {
                "release" => $crate::BuildProfile::Release,
                "debug" => $crate::BuildProfile::Debug,
                other => $crate::BuildProfile::Other($crate::bounded_str(other)),
            },
        }
    }};
}

/// Parse a decimal `u32` at compile time (a Cargo version component), 0 when malformed. Used by [firmware_summary!].
#[doc(hidden)]
pub const fn parse_u32(s: &str) -> u32 {
    let b = s.as_bytes();
    let mut i = 0;
    let mut v = 0u32;
    while i < b.len() {
        if b[i] < b'0' || b[i] > b'9' {
            return 0;
        }
        v = v * 10 + (b[i] - b'0') as u32;
        i += 1;
    }
    v
}

#[doc(hidden)]
pub use wire_weaver::prelude::{MaxLen, RefVec};
