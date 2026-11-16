//! A set of snapshots compiled into an application, written by [crate::embed] at build time.
//!
//! An application that knows which API crates its devices use (the layer-7 `ww` and the Python wheel for the
//! standard library, a product's host tool for its own API) embeds their snapshots so that skipped definitions
//! resolve with nothing on disk and no network. The set holds the shrink_wrap bytes of each snapshot (the RON was
//! checked and encoded at build time) and the superseded definitions kept next to them (EVO-68); each is decoded on
//! its first use, so a client talking to a device that uses two of fifty crates doesn't decode fifty.

use crate::file;
use std::sync::OnceLock;
use tracing::error;
use ww_self::ApiBundleOwned;
use ww_self::definition_cache::{self, Kind};
use ww_self::signature::{Match, Signatures};
use ww_self::snapshot;
use ww_version::FullVersionOwned;

/// A decoded superseded definition: its canonical bundle and the signatures computed from its bytes.
type Superseded = (ApiBundleOwned, Signatures);

/// Snapshots and superseded definitions embedded in a binary, see the [module docs](self).
pub struct Set {
    /// (file name as `ww api save` wrote it, its shrink_wrap bytes)
    snapshots: &'static [(&'static str, &'static [u8])],
    /// (file name as [definition_cache::file_name] names it, canonical bytes)
    superseded: &'static [(&'static str, &'static [u8])],
    decoded: OnceLock<Box<[OnceLock<Option<ApiBundleOwned>>]>>,
    superseded_decoded: OnceLock<Box<[OnceLock<Option<Superseded>>]>>,
}

impl Set {
    /// A set over the bytes [crate::embed] wrote; `static` in the application, registered with [crate::embed()]
    /// or given to a [crate::Resolver].
    pub const fn new(
        snapshots: &'static [(&'static str, &'static [u8])],
        superseded: &'static [(&'static str, &'static [u8])],
    ) -> Self {
        Set {
            snapshots,
            superseded,
            decoded: OnceLock::new(),
            superseded_decoded: OnceLock::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.snapshots.is_empty() && self.superseded.is_empty()
    }

    /// The snapshot of exactly this crate version, if embedded.
    pub fn get(&self, version: &FullVersionOwned) -> Option<&ApiBundleOwned> {
        let file_name = file::file_name(version);
        let index = self
            .snapshots
            .iter()
            .position(|(name, _)| *name == file_name)?;
        self.decoded(index).as_ref()
    }

    /// Every crate version the set has a snapshot of, from the file names.
    pub fn versions(&self) -> impl Iterator<Item = FullVersionOwned> + '_ {
        self.snapshots
            .iter()
            .filter_map(|(name, _)| file::parse_file_name(name))
    }

    /// File names of the embedded snapshots.
    pub fn file_names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.snapshots.iter().map(|(name, _)| *name)
    }

    /// File names of the embedded superseded definitions.
    pub fn superseded_file_names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.superseded.iter().map(|(name, _)| *name)
    }

    fn decoded(&self, index: usize) -> &Option<ApiBundleOwned> {
        let slots = self
            .decoded
            .get_or_init(|| (0..self.snapshots.len()).map(|_| OnceLock::new()).collect());
        slots[index].get_or_init(|| {
            let (file_name, bytes) = self.snapshots[index];
            match snapshot::decode(bytes) {
                Ok(bundle) if file::parse_file_name(file_name) == bundle.ext_crates.first().cloned() => {
                    Some(bundle)
                }
                Ok(_) => {
                    error!("embedded API snapshot {file_name} is not of the crate version its name says");
                    None
                }
                Err(e) => {
                    error!("embedded API snapshot {file_name}: {e:#}");
                    None
                }
            }
        })
    }

    /// The canonical bundle of the trait or type `name` of `version` that `reference` (a `Skipped*` location's
    /// signature bytes) names, kept when the snapshot of that version was refreshed in place and the definition
    /// changed (EVO-68). One with the same docs is preferred to one with the same shape only.
    pub fn superseded(
        &self,
        kind: Kind,
        version: &FullVersionOwned,
        name: &str,
        reference: &[u8],
    ) -> Option<&ApiBundleOwned> {
        // `ww_error_0_1_0-type-CommonError-`: the signature in the name is the one it was kept under, not trusted
        let prefix = definition_cache::file_name(kind, version, name, &[]);
        let prefix = prefix.strip_suffix(".ww").unwrap_or(&prefix);
        let mut same_shape = None;
        for (index, (file_name, _)) in self.superseded.iter().enumerate() {
            if !file_name
                .strip_prefix(prefix)
                .is_some_and(|rest| !rest.contains('-'))
            {
                continue;
            }
            let Some((bundle, signatures)) = self.superseded_decoded(index, kind, name) else {
                continue;
            };
            match signatures.matches(reference) {
                Match::Exact => return Some(bundle),
                Match::No => {}
                Match::DocsDiffer | Match::Shape => same_shape = same_shape.or(Some(bundle)),
            }
        }
        same_shape
    }

    fn superseded_decoded(&self, index: usize, kind: Kind, name: &str) -> Option<&Superseded> {
        let slots = self.superseded_decoded.get_or_init(|| {
            (0..self.superseded.len())
                .map(|_| OnceLock::new())
                .collect()
        });
        slots[index]
            .get_or_init(|| {
                let (file_name, bytes) = self.superseded[index];
                match definition_cache::decode_with_signatures(kind, name, bytes) {
                    Ok(decoded) => Some(decoded),
                    Err(e) => {
                        error!("embedded superseded definition {file_name}: {e:#}");
                        None
                    }
                }
            })
            .as_ref()
    }
}

impl std::fmt::Debug for Set {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Set")
            .field("snapshots", &self.snapshots.len())
            .field("superseded", &self.superseded.len())
            .finish()
    }
}
