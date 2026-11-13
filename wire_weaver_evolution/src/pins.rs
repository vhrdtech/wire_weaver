//! Pinned references of a frozen snapshot brought in line with the source (EVO-70): the signatures by which a
//! snapshot names other crates' traits and types it leaves out (`SkippedFullVersion`), and the versions of those
//! crates (`ext_crates`), rewritten to what the crate's current source gives, everything else of the file kept byte
//! for byte. For a fixture that stands for a released API but follows the live `ww_stdlib` by path on purpose, such
//! as `wire_weaver_evolution`'s `nucleo_g0b1re_api` with its old-format snapshot (EVO-13): a stdlib change reaches it
//! at once, and its pins are refreshed by this tool, never by hand. Saving the snapshot again would rewrite it in the
//! current format and lose what it stands for.
//!
//! The pins are refreshed only while the source stays wire-compatible with the snapshot: a break of an embedded
//! definition is refused, naming what broke, unless it is accepted on purpose (a stated wire break of the stdlib).

use crate::snapshot_files;
use crate::{Change, Check, CrateCache, Outcome, Report, WireBump};
use std::path::{Path, PathBuf};
use ww_self::definition_cache::Kind;
use ww_self::{ApiBundleOwned, ApiLevelLocationOwned, TypeLocationOwned, signature};

/// One pinned reference or crate version that changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refreshed {
    /// `trait ww_timebase::Clock`, `type ww_error::CommonError` or `crate ww_dfu`.
    pub what: String,
    /// The pin as it was and as it is now: signature bytes in hex, or a version.
    pub from: String,
    pub to: String,
}

/// What [refresh] did.
#[derive(Debug)]
pub struct RefreshedPins {
    /// The snapshot file.
    pub path: PathBuf,
    /// The pins that changed, empty when all were current.
    pub refreshed: Vec<Refreshed>,
    /// The comparison of the source with the snapshot, before the refresh: an [Outcome::Compared].
    pub outcome: Outcome,
}

/// Whether `report` keeps the wire: no breaking change, no new wire line. A compatible change of an embedded
/// definition is one: a typed number with the same bytes, or a wire-compatible addition (a raised max length, a new
/// wire revision).
pub fn wire_compatible(report: &Report) -> bool {
    report.change != Change::Breaking && report.wire_bump != WireBump::Line
}

/// Refreshes the pins of the snapshot of the crate at `crate_dir`'s current version (or `snapshot`), see the
/// [module docs](self). `accept_breaking` refreshes them across a break too; without it a break is an error that
/// names it. `dry_run` changes no file. The source is loaded through `cache`, see [Check::cache].
pub fn refresh(
    crate_dir: &Path,
    snapshot: Option<&Path>,
    accept_breaking: bool,
    dry_run: bool,
    cache: &CrateCache,
) -> Result<RefreshedPins, String> {
    let snapshot = match snapshot {
        Some(snapshot) => snapshot.to_path_buf(),
        None => {
            let (bundle, _) = cache
                .load_crate_with_notes(crate_dir)
                .map_err(|e| format!("{e:#}"))?;
            let own = bundle.crate_version(0).map_err(|e| e.to_string())?;
            crate_dir
                .join(snapshot_files::SNAPSHOTS_DIR)
                .join(format!("{}.ron", own.filename_friendly()))
        }
    };
    if !snapshot.exists() {
        return Err(format!("{}: no such snapshot to refresh", snapshot.display()));
    }
    let outcome = Check::new(crate_dir)
        .against(&snapshot)
        .cache(cache.clone())
        .run()?;
    let comparison = match &outcome {
        Outcome::Compared(comparison) => comparison,
        Outcome::NoSnapshot(_) => {
            return Err(format!(
                "{}: no snapshot of the crate's current version to refresh",
                crate_dir.display()
            ));
        }
        Outcome::Roots(_) => {
            return Err(format!(
                "{}: a crate with several API roots, its pins are refreshed by saving its snapshots",
                crate_dir.display()
            ));
        }
    };
    let path = comparison.old_path.clone();
    if !wire_compatible(&comparison.report) && !accept_breaking {
        return Err(format!(
            "{}\nThe source no longer works with {} on the wire, so its pins are not refreshed. If the break is \
             intended (a stated wire break of a dependency, with its EVO item and CHANGELOG entry), refresh them \
             with --accept-breaking.",
            outcome,
            path.display()
        ));
    }
    let text =
        std::fs::read_to_string(&path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    let saved = snapshot_files::read(&path)?.bundle;
    let (text, refreshed) = refresh_text(&text, &saved, &comparison.new)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if !dry_run && !refreshed.is_empty() {
        std::fs::write(&path, text).map_err(|e| format!("writing {}: {e}", path.display()))?;
    }
    Ok(RefreshedPins {
        path,
        refreshed,
        outcome,
    })
}

/// A reference by what it names: kind, crate and name.
type Key = (Kind, String, String);

/// The references of `bundle` that leave a definition out, with their signature bytes.
fn references(bundle: &ApiBundleOwned) -> Result<Vec<(Key, Vec<u8>)>, String> {
    let crate_id = |idx: u32| {
        bundle
            .crate_version(idx)
            .map(|v| v.crate_id.clone())
            .map_err(|e| format!("{e:?}"))
    };
    let mut references = vec![];
    for location in &bundle.types {
        if let TypeLocationOwned::SkippedFullVersion {
            crate_idx,
            type_name,
            signature,
        } = location
        {
            let key = (Kind::Type, crate_id(crate_idx.0)?, type_name.clone());
            references.push((key, signature.clone()));
        }
    }
    for location in &bundle.traits {
        if let ApiLevelLocationOwned::SkippedFullVersion {
            crate_idx,
            trait_name,
            signature,
        } = location
        {
            let key = (Kind::Trait, crate_id(crate_idx.0)?, trait_name.clone());
            references.push((key, signature.clone()));
        }
    }
    Ok(references)
}

/// `text`, the RON of `saved`, with its references and dependency versions set to those of `source`. A reference
/// keeps its form: 8 bytes saved before EVO-69 (the signature with docs) stay 8 bytes.
fn refresh_text(
    text: &str,
    saved: &ApiBundleOwned,
    source: &ApiBundleOwned,
) -> Result<(String, Vec<Refreshed>), String> {
    let current = references(source)?;
    let mut refreshed = vec![];
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    for (key, old) in references(saved)? {
        let Some((_, new)) = current.iter().find(|(k, _)| k == &key) else {
            return Err(format!(
                "{} {}::{} is no longer left out by the source, refresh by hand is not possible: the snapshot and \
                 the source differ in more than their pins",
                key.0.name(),
                key.1,
                key.2
            ));
        };
        let new = match old.len() {
            signature::SIGNATURE_LEN => signature::docs_part(new)
                .unwrap_or_else(|| signature::shape_part(new))
                .to_vec(),
            _ => new.to_vec(),
        };
        // the next reference in the file is this one: both list types, then traits, in order
        let field = match key.0 {
            Kind::Type => "type_name",
            Kind::Trait => "trait_name",
        };
        let head = format!("{field}: {:?}, signature: [", key.2);
        let start = rest
            .find(&head)
            .ok_or_else(|| format!("no `{head}` in the file"))?
            + head.len();
        let len = rest[start..]
            .find(']')
            .ok_or_else(|| format!("`{head}` not closed"))?;
        out.push_str(&rest[..start]);
        let list = &rest[start..start + len];
        if new == old {
            out.push_str(list);
        } else {
            out.push_str(&format_bytes(list, &new));
            refreshed.push(Refreshed {
                what: format!("{} {}::{}", key.0.name(), key.1, key.2),
                from: hex(&old),
                to: hex(&new),
            });
        }
        rest = &rest[start + len..];
    }
    out.push_str(rest);

    // versions of the other crates, one line each: `(crate_id: "ww_dfu", version: (major: 0, minor: 1, ...)),`
    let mut text = String::with_capacity(out.len());
    for line in out.split_inclusive('\n') {
        let Some((crate_id, version)) = ext_crate_line(line) else {
            text.push_str(line);
            continue;
        };
        let new = source.ext_crates[1..]
            .iter()
            .find(|v| v.crate_id == crate_id)
            .map(|v| &v.version);
        match new {
            Some(new) if snapshot_files::triplet(new) != version.0 => {
                let new_text = format!(
                    "major: {}, minor: {}, patch: {}",
                    new.major.0, new.minor.0, new.patch.0
                );
                refreshed.push(Refreshed {
                    what: format!("crate {crate_id}"),
                    from: version.0,
                    to: snapshot_files::triplet(new),
                });
                text.push_str(&line.replacen(&version.1, &new_text, 1));
            }
            _ => text.push_str(line),
        }
    }
    Ok((text, refreshed))
}

/// The crate and version of a line of `ext_crates`: the version as `major.minor.patch` and as it is written.
fn ext_crate_line(line: &str) -> Option<(String, (String, String))> {
    let line = line.trim().strip_prefix("(crate_id: \"")?;
    let (crate_id, rest) = line.split_once('"')?;
    let rest = rest.strip_prefix(", version: (")?;
    let written = &rest[..rest.find(", pre:")?];
    let mut parts = written.split(", ").map(|p| p.split_once(": ").map(|(_, n)| n));
    let (major, minor, patch) = (parts.next()??, parts.next()??, parts.next()??);
    Some((
        crate_id.to_string(),
        (format!("{major}.{minor}.{patch}"), written.to_string()),
    ))
}

/// `bytes` written as the saved `list` writes its numbers: one per line with its indent, or on one line.
fn format_bytes(list: &str, bytes: &[u8]) -> String {
    let Some(first) = list.find(|c: char| c.is_ascii_digit()) else {
        return bytes
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(", ");
    };
    if !list.contains('\n') {
        return bytes
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(", ");
    }
    let indent = &list[list[..first].rfind('\n').map_or(0, |n| n + 1)..first];
    let close = &list[list.rfind('\n').unwrap_or(list.len())..];
    let mut out = String::new();
    for b in bytes {
        out.push('\n');
        out.push_str(indent);
        out.push_str(&b.to_string());
        out.push(',');
    }
    out.push_str(close);
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
