//! Checking a crate against its `api_snapshots/`, from source: what `ww api check` and an API crate's own
//! evolution test run.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

pub use roots::{RemovedRoot, RootOutcome, RootsOutcome, SharedChange};
use wire_weaver_core::SourceNotes;
pub use wire_weaver_core::{CrateCache, UnitHint, UnreasonedSilence};
use ww_self::{ApiBundleOwned, snapshot};
use ww_version::{VersionOwned, WireVersion};

use crate::recorded::Recorded;
use crate::snapshot_files::{
    CrateFile, SNAPSHOTS_DIR, Snapshot, crate_files, latest_snapshot, read, root_file_name,
    root_of_file_name, root_snapshots, triplet,
};
use crate::superseded::{self, Superseded};
use crate::{Change, Move, Report, Rule, compare_recorded, own_traits, verify_move};

mod roots;

/// Checks a crate, or a snapshot file, against an earlier snapshot.
///
/// A crate is parsed from source and compared with its newest snapshot in `api_snapshots/` with a version not newer
/// than the one in its `Cargo.toml`: the snapshot of the current version if it was saved already (then nothing may
/// have changed), else the one of the previous version (then the version must be bumped enough for what changed).
/// A snapshot file is compared with the previous snapshot in the same directory.
///
/// With [require_current](Self::require_current), the snapshot of the current version has to be saved and match the
/// source, and it is compared with the previous version's snapshot.
///
/// ```no_run
/// // tests/evolution.rs of an API crate: fails when the snapshot of the current version is missing or differs from
/// // the source, and on a change without the version bump it needs; `WW_API_SAVE=1 cargo test --test evolution`
/// // saves the snapshot first
/// wire_weaver_evolution::Check::new(env!("CARGO_MANIFEST_DIR")).require_current(true).assert();
/// ```
#[derive(Clone, Debug)]
pub struct Check {
    path: PathBuf,
    against: Option<PathBuf>,
    strict: bool,
    require_current: bool,
    snapshots_dir: Option<PathBuf>,
    cache: CrateCache,
}

/// Environment variable that makes [Check::assert] save the snapshot of the current version first, when set to
/// anything but empty or `0`.
pub const SAVE_ENV: &str = "WW_API_SAVE";

impl Check {
    /// `path` is a crate directory or a snapshot file (`.ron`).
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Check {
            path: path.into(),
            against: None,
            strict: false,
            require_current: false,
            snapshots_dir: None,
            cache: CrateCache::default(),
        }
    }

    /// Compare with this snapshot file instead of the newest fitting one.
    pub fn against(mut self, snapshot: impl Into<PathBuf>) -> Self {
        self.against = Some(snapshot.into());
        self
    }

    /// A crate without a snapshot to compare with fails, instead of passing unchecked.
    pub fn strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }

    /// The snapshot of the crate's current version has to be saved, and the source must not differ from it; it is
    /// compared with the previous version's snapshot to check the bump. Implies [strict](Self::strict). For a crate
    /// that saves the snapshot of the version it works on as it goes ([save](Self::save)), so a version can't be
    /// released without one. Not used for a snapshot file or with [against](Self::against).
    pub fn require_current(mut self, require_current: bool) -> Self {
        self.require_current = require_current;
        self
    }

    /// Snapshots of the crate are in `dir` instead of its `api_snapshots/`: for a crate whose source isn't where its
    /// snapshots are kept (exact_units, from its own repository, saved in wire_weaver_snapshots).
    pub fn snapshots_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.snapshots_dir = Some(dir.into());
        self
    }

    /// Load the crate with `cache`, shared with other checks: a [save](Self::save) and a [run](Self::run), or checks
    /// of crates with the same dependencies, walk the dependencies' source once. A cache keeps a load only while
    /// the files it came from are unchanged, so a run after an edit sees it. Each `Check` has a cache of its own
    /// otherwise, which its clones share.
    pub fn cache(mut self, cache: CrateCache) -> Self {
        self.cache = cache;
        self
    }

    /// Where the crate's snapshots are.
    fn crate_snapshots_dir(&self) -> PathBuf {
        self.snapshots_dir
            .clone()
            .unwrap_or_else(|| self.path.join(SNAPSHOTS_DIR))
    }

    fn is_snapshot(&self) -> bool {
        self.path.extension().is_some_and(|ext| ext == "ron")
    }

    /// Loads both versions and compares them. Whether the version is bumped enough is up to [Outcome::verdict].
    pub fn run(&self) -> Result<Outcome, String> {
        // a crate's roots walk their dependencies once (EVO-39), and a run again of an unchanged crate loads nothing
        let cache = &self.cache;
        let is_snapshot = self.is_snapshot();
        if !is_snapshot {
            let roots = wire_weaver_core::api_roots(&self.path).map_err(|e| format!("{e:#}"))?;
            if !roots.is_empty() {
                return self.run_roots(&roots, cache);
            }
        }
        let (new, new_recorded, new_label, dir, hints, unreasoned, moves, definitions) =
            if is_snapshot {
                let dir = self.path.parent().unwrap_or(Path::new(".")).to_path_buf();
                let new = read(&self.path)?;
                (
                    new.bundle,
                    new.recorded,
                    self.path.display().to_string(),
                    dir,
                    vec![],
                    vec![],
                    vec![],
                    None,
                )
            } else {
                let (new, notes) = cache
                    .load_crate_with_notes(&self.path)
                    .map_err(|e| format!("{e:#}"))?;
                let dir = self.crate_snapshots_dir();
                let moves = own_moves(&new, &notes);
                let label = "source".to_string();
                let definitions = Some(notes.definitions);
                (
                    new,
                    Recorded::default(),
                    label,
                    dir,
                    notes.hints,
                    notes.unreasoned,
                    moves,
                    definitions,
                )
            };
        let definitions: Vec<&ApiBundleOwned> = definitions.iter().collect();
        // w0, the design line (EVO-41): no snapshot is required, what one shows is information
        let design_line = new.wire.is_some_and(|w| w.is_design_line());
        let mut warnings = match design_line && !is_snapshot {
            true => design_line_warnings(&self.path, &new)?,
            false => vec![],
        };
        warnings.extend(unreached_moves(&[&new], &moves));
        if self.require_current && !is_snapshot && self.against.is_none() && !design_line {
            return self.run_current(new, hints, unreasoned, &moves, &definitions);
        }
        let old = match &self.against {
            Some(against) => read(against)?,
            None => {
                let version = &new.crate_version(0).map_err(|e| e.to_string())?.version;
                let previous = match is_snapshot.then(|| self.root_file()).flatten() {
                    // a root's snapshot file, compared with the previous one of the same root
                    Some(name) => {
                        root_snapshots(&dir, &name.crate_id, &name.root, name.wire.line.0)?
                            .into_iter()
                            .rfind(|s| key(&s.version) < key(version))
                            .map(|s| s.snapshot)
                    }
                    None => latest_snapshot(&dir, &new, version, !is_snapshot)?,
                };
                match previous {
                    Some(previous) => previous,
                    None => {
                        let own = &new.ext_crates[0];
                        return Ok(Outcome::NoSnapshot(Box::new(NoSnapshot {
                            crate_id: own.crate_id.clone(),
                            version: own.version.clone(),
                            dir,
                            crate_path: (!is_snapshot).then(|| self.path.clone()),
                            strict: self.strict && !design_line,
                            current_required: false,
                            previous: None,
                            hints,
                            unreasoned,
                            design_line,
                            warnings,
                            previous_line: None,
                        })));
                    }
                }
            }
        };
        let report = compare_checked(
            &dir,
            &old.bundle,
            &old.recorded,
            &new,
            &new_recorded,
            &moves,
            &definitions,
        )?;
        Ok(Outcome::Compared(Box::new(Comparison {
            old_path: old.path,
            new_label,
            crate_path: (!is_snapshot).then(|| self.path.clone()),
            report,
            old: old.bundle,
            new,
            current_required: false,
            hints,
            unreasoned,
            design_line,
            warnings,
            unchanged_since: None,
            root: None,
        })))
    }

    /// [run](Self::run) with [require_current](Self::require_current), `source` loaded from the crate.
    fn run_current(
        &self,
        source: ApiBundleOwned,
        hints: Vec<UnitHint>,
        unreasoned: Vec<UnreasonedSilence>,
        moves: &[Move],
        definitions: &[&ApiBundleOwned],
    ) -> Result<Outcome, String> {
        let dir = self.crate_snapshots_dir();
        let own = source.crate_version(0).map_err(|e| e.to_string())?;
        let warnings = unreached_moves(&[&source], moves);
        let current = current_snapshot(&dir, &source)?;
        let previous = latest_snapshot(&dir, &source, &own.version, false)?;
        let crate_path = Some(self.path.clone());
        let Some(current) = current else {
            return Ok(Outcome::NoSnapshot(Box::new(NoSnapshot {
                crate_id: own.crate_id.clone(),
                version: own.version.clone(),
                dir,
                crate_path,
                strict: true,
                current_required: true,
                previous: previous.map(|s| s.path),
                hints,
                unreasoned,
                design_line: false,
                warnings: warnings.clone(),
                previous_line: None,
            })));
        };
        let report = compare_checked(
            &dir,
            &current.bundle,
            &current.recorded,
            &source,
            &Recorded::default(),
            moves,
            definitions,
        )?;
        let (old, new_label, new, report) = match previous {
            // the source is checked against the snapshot of its version first, then that snapshot against the previous
            Some(previous) if report.change == Change::None => {
                // the current snapshot is the source's, with the same `#[moved_from]`
                let report = compare_checked(
                    &dir,
                    &previous.bundle,
                    &previous.recorded,
                    &current.bundle,
                    &current.recorded,
                    moves,
                    definitions,
                )?;
                let label = current.path.display().to_string();
                (previous, label, current.bundle, report)
            }
            _ => (current, "source".to_string(), source, report),
        };
        Ok(Outcome::Compared(Box::new(Comparison {
            old_path: old.path,
            new_label,
            crate_path,
            report,
            old: old.bundle,
            new,
            current_required: true,
            hints,
            unreasoned,
            design_line: false,
            warnings: warnings.clone(),
            unchanged_since: None,
            root: None,
        })))
    }

    /// Saves a snapshot of the crate's current version into its `api_snapshots/`, as `ww api save` does: writes it, or
    /// refreshes it if it differs from the source. A snapshot of an older version is never written. Refuses a version
    /// not bumped enough since the previous version's snapshot, unless `force` is true.
    ///
    /// A crate with several API roots (`[package.metadata.wire_weaver.roots]`, EVO-37) gets one snapshot per root and
    /// wire version instead ([root_file_name], EVO-38), each checked against that root's previous one: one [Saved]
    /// per root, in the table's order. Nothing is written when any root is refused.
    ///
    /// A crate or root on w0, the design line (EVO-41), gets no snapshot: [SaveStatus::DesignLine]. Its first one
    /// is saved once it moves to `w1.0`, its first release.
    ///
    /// The snapshot of a released version never changes, which this can't know: a pre-commit hook refusing changes to
    /// committed snapshots of released versions does (docs/evolution/checker_tool.md).
    pub fn save(&self, force: bool) -> Result<Vec<Saved>, String> {
        if self.is_snapshot() {
            return Err(format!(
                "{} is a snapshot file, saving needs a crate directory",
                self.path.display()
            ));
        }
        let roots = wire_weaver_core::api_roots(&self.path).map_err(|e| format!("{e:#}"))?;
        let cache = &self.cache;
        let planned = if roots.is_empty() {
            vec![self.plan_crate(cache, force)?]
        } else {
            let mut planned = roots
                .iter()
                .map(|(root, wire)| self.plan_root(cache, root, *wire, force))
                .collect::<Result<Vec<_>, _>>()?;
            let own = planned[0]
                .0
                .bundle
                .crate_version(0)
                .map_err(|e| e.to_string())?
                .clone();
            planned.extend(self.plan_removed(&roots, &own, force)?);
            planned
        };
        let mut saved = vec![];
        for (one, contents) in planned {
            if let Some(contents) = contents {
                let dir = one.path.parent().unwrap_or(Path::new("."));
                fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
                fs::write(&one.path, contents)
                    .map_err(|e| format!("writing {}: {e}", one.path.display()))?;
            }
            for kept in &one.superseded {
                let dir = kept.path.parent().unwrap_or(Path::new("."));
                fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
                fs::write(&kept.path, &kept.bytes)
                    .map_err(|e| format!("writing {}: {e}", kept.path.display()))?;
            }
            saved.push(one);
        }
        Ok(saved)
    }

    /// The snapshot of the whole crate at its version, and the text to write, `None` when it is up to date.
    fn plan_crate(
        &self,
        cache: &CrateCache,
        force: bool,
    ) -> Result<(Saved, Option<String>), String> {
        let (bundle, notes) = cache
            .load_crate_with_notes(&self.path)
            .map_err(|e| format!("{e:#}"))?;
        let moves = own_moves(&bundle, &notes);
        let own = bundle.crate_version(0).map_err(|e| e.to_string())?;
        let dir = self.crate_snapshots_dir();
        let path = dir.join(format!("{}.ron", own.filename_friendly()));
        if bundle.wire.is_some_and(|w| w.is_design_line()) {
            let warnings = design_line_warnings(&self.path, &bundle)?;
            return Ok(not_saved(path, bundle, None, warnings));
        }
        // with the hash of the shrink_wrap bytes the RON stands for: the build of wire_weaver_snapshots re-encodes the
        // RON and fails when it differs, which proves the round trip lossless
        let contents = wire_weaver_snapshots::render_ron(&bundle)?;
        let previous = latest_snapshot(&dir, &bundle, &own.version, false)?;
        let previous = self.check_bump(
            previous,
            &bundle,
            &moves,
            &[&notes.definitions],
            None,
            force,
        )?;
        let status = existing_status(&path, &bundle, &contents)?;
        // a refresh in place keeps the definitions it replaces (EVO-68)
        let (superseded, warnings) = match status {
            SaveStatus::Refreshed | SaveStatus::Reformatted => match read(&path) {
                Ok(old) => superseded::superseded(
                    &dir.join(superseded::SUPERSEDED_DIR),
                    &old.bundle,
                    &notes.definitions,
                ),
                Err(_) => (vec![], vec![]),
            },
            _ => (vec![], vec![]),
        };
        let (mut saved, contents) = planned(path, status, previous, bundle, None, contents);
        saved.superseded = superseded
            .into_iter()
            .filter(|kept| !kept.path.exists())
            .collect();
        saved.warnings.extend(warnings);
        Ok((saved, contents))
    }

    /// The snapshot of the API root `root` (`wire_v2::Thermo`) at its wire version `wire`, and the text to write,
    /// `None` when it is up to date. Only the current crate version's file is ever written: a root that didn't change
    /// since its previous snapshot keeps that one, which an older crate version saved.
    fn plan_root(
        &self,
        cache: &CrateCache,
        root: &str,
        wire: WireVersion,
        force: bool,
    ) -> Result<(Saved, Option<String>), String> {
        let (bundle, notes) = cache
            .load_root_with_notes(&self.path, root)
            .map_err(|e| format!("{e:#}"))?;
        let moves = own_moves(&bundle, &notes);
        let own = bundle.crate_version(0).map_err(|e| e.to_string())?.clone();
        let version = own.version.clone();
        let dir = self.crate_snapshots_dir();
        let path = dir.join(root_file_name(&own, root, wire));
        if wire.is_design_line() {
            let warnings = design_line_warnings(&self.path, &bundle)?;
            return Ok(not_saved(path, bundle, Some(root.to_string()), warnings));
        }
        let contents = wire_weaver_snapshots::render_root_ron(&bundle, root)?;
        let all = root_snapshots(&dir, &own.crate_id, root, wire.line.0)?;
        if let Some(newer) = all.iter().find(|s| key(&s.version) > key(&version)) {
            return Err(format!(
                "{} has `{root}` of {} {} (wire {}), newer than the source's {}: a snapshot of an older version is \
                 never written",
                newer.snapshot.path.display(),
                own.crate_id,
                triplet(&newer.version),
                newer.wire,
                triplet(&version)
            ));
        }
        let mut warnings = vec![];
        let previous = all
            .into_iter()
            .rfind(|s| key(&s.version) < key(&version))
            .map(|s| s.snapshot);
        if previous.is_none() {
            warnings.extend(no_previous_warning(
                &dir,
                &own.crate_id,
                &version,
                root,
                wire,
            )?);
        }
        let previous = self.check_bump(
            previous,
            &bundle,
            &moves,
            &[&notes.definitions],
            Some(root),
            force,
        )?;
        let unchanged = previous
            .as_ref()
            .is_some_and(|(_, report)| report.change == Change::None);
        let (path, status) = match previous.as_ref() {
            // the previous crate version's snapshot of the root stays its latest
            Some((previous_path, _)) if unchanged && !path.exists() => {
                (previous_path.clone(), SaveStatus::UpToDate)
            }
            _ => {
                let status = existing_status(&path, &bundle, &contents)?;
                (path, status)
            }
        };
        let (mut saved, contents) = planned(
            path,
            status,
            previous,
            bundle,
            Some(root.to_string()),
            contents,
        );
        saved.warnings = warnings;
        Ok((saved, contents))
    }

    /// Compares `bundle` (with its `#[moved_from]`, `moves`) with the snapshot `previous`, refusing a version or wire
    /// version not bumped enough unless `force`. `root` names the API root of a crate with several.
    fn check_bump(
        &self,
        previous: Option<Snapshot>,
        bundle: &ApiBundleOwned,
        moves: &[Move],
        definitions: &[&ApiBundleOwned],
        root: Option<&str>,
        force: bool,
    ) -> Result<Option<(PathBuf, Report)>, String> {
        let Some(previous) = previous else {
            return Ok(None);
        };
        let report = compare_checked(
            &self.crate_snapshots_dir(),
            &previous.bundle,
            &previous.recorded,
            bundle,
            &Recorded::default(),
            moves,
            definitions,
        )?;
        let of = root.map_or(String::new(), |root| format!("API root `{root}`: "));
        if let Err(e) = report.check_version()
            && !force
        {
            return Err(format!(
                "{of}{e}.\nNot saved: bump {} to {} or later (docs/evolution/rules.md#your-apis-version) and save \
                 again.",
                version_declared_at(Some(&self.path)),
                triplet(&report.minimal_version())
            ));
        }
        if let Err(e) = report.check_wire()
            && !force
        {
            let at = wire_declared_at(Some(&self.path), root);
            return Err(format!(
                "{of}{e}.\nNot saved: {} and save again.",
                wire_hint_at(&report, &at)
            ));
        }
        Ok(Some((previous.path, report)))
    }

    /// [run](Self::run) and [verdict](Outcome::verdict), printing what was found; panics with what is wrong and what
    /// to do about it. For an API crate's own test.
    ///
    /// With [SAVE_ENV] (`WW_API_SAVE=1 cargo test --test evolution`), [saves](Self::save) the snapshot of the current
    /// version first, so the lock file pins the saver too.
    ///
    /// With [strict](Self::strict) (not [require_current](Self::require_current), which doesn't imply this: the
    /// documented `tests/evolution.rs` template uses it and external users aren't forced into writing a reason),
    /// also panics on a bare or empty `#[no_unit_hint]` (TOOL-35); opt in with `.strict(true)`.
    pub fn assert(&self) {
        if std::env::var_os(SAVE_ENV).is_some_and(|v| !v.is_empty() && v != "0") {
            match self.save(false) {
                Ok(saved) => {
                    for saved in &saved {
                        println!("{saved}");
                        for warning in &saved.warnings {
                            println!("warning: {warning}");
                        }
                    }
                }
                Err(e) => panic!("\n{e}"),
            }
        }
        let outcome = self
            .run()
            .unwrap_or_else(|e| panic!("evolution check failed: {e}"));
        if self.strict
            && let Some(message) = unreasoned_message(outcome.unreasoned())
        {
            panic!("\n{outcome}\n{message}");
        }
        match outcome.verdict() {
            Ok(ok) => println!("{outcome}\n{ok}"),
            Err(e) => panic!("\n{outcome}\n{e}"),
        }
    }
}

/// `items`, one per line, if there are any: what `--strict` refuses a bare or empty `#[no_unit_hint]` with, for
/// [Check::assert] and `ww api check --strict`.
pub fn unreasoned_message(items: &[UnreasonedSilence]) -> Option<String> {
    if items.is_empty() {
        return None;
    }
    Some(
        items
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// What [Check::run] found.
#[derive(Debug)]
pub enum Outcome {
    Compared(Box<Comparison>),
    /// Nothing to compare with.
    NoSnapshot(Box<NoSnapshot>),
    /// A crate with several API roots (EVO-39): each root against its own latest snapshot.
    Roots(Box<RootsOutcome>),
}

/// Two versions compared.
#[derive(Debug)]
pub struct Comparison {
    /// The snapshot compared with.
    pub old_path: PathBuf,
    /// `source` or the path of the newer snapshot.
    pub new_label: String,
    /// The crate checked, `None` if a snapshot file was.
    pub crate_path: Option<PathBuf>,
    pub report: Report,
    /// The snapshot as loaded.
    pub old: ApiBundleOwned,
    /// The newer version as loaded.
    pub new: ApiBundleOwned,
    /// Checked with [Check::require_current]: either the source against the snapshot of its version (`new_label` is
    /// `source`), or that snapshot against the previous version's.
    pub current_required: bool,
    /// Unit-suffix hints found in the source (TOOL-22), empty for a snapshot file: a note, never part of the
    /// verdict.
    pub hints: Vec<UnitHint>,
    /// `#[no_unit_hint]`s found with no reason or an empty one (TOOL-22), empty for a snapshot file: what `--strict`
    /// refuses, never part of the version/wire verdict.
    pub unreasoned: Vec<UnreasonedSilence>,
    /// The newer side is on w0, the design line (EVO-41): breaks are accepted, the report is information and the
    /// verdict passes.
    pub design_line: bool,
    /// What looked wrong but doesn't change the verdict: a crate that can be published with its root on w0.
    pub warnings: Vec<String>,
    /// An API root's snapshot saved by this older crate version, compared as the snapshot of the crate's version:
    /// that version saved other roots, so this one was unchanged in it (EVO-39). `None` otherwise.
    pub unchanged_since: Option<VersionOwned>,
    /// The API root compared (`wire_v2::Thermo`), of a crate with several; `None` for a whole crate.
    pub root: Option<String>,
}

/// No snapshot of the crate at or before its version.
#[derive(Debug)]
pub struct NoSnapshot {
    pub crate_id: String,
    pub version: VersionOwned,
    /// Where snapshots were looked for.
    pub dir: PathBuf,
    /// The crate checked, `None` if a snapshot file was.
    pub crate_path: Option<PathBuf>,
    pub strict: bool,
    /// Checked with [Check::require_current]: the snapshot of the current version is missing, `previous` is the newest
    /// older one, if any.
    pub current_required: bool,
    pub previous: Option<PathBuf>,
    /// Unit-suffix hints found in the source (TOOL-22), empty for a snapshot file.
    pub hints: Vec<UnitHint>,
    /// `#[no_unit_hint]`s found with no reason or an empty one (TOOL-22), empty for a snapshot file: what
    /// `--strict` refuses.
    pub unreasoned: Vec<UnreasonedSilence>,
    /// The crate is on w0, the design line (EVO-41): it keeps no snapshots, so none is missing.
    pub design_line: bool,
    /// What looked wrong but doesn't change the verdict: a crate that can be published with its root on w0.
    pub warnings: Vec<String>,
    /// An API root that begins a new line, compared with the latest snapshot of the previous line's root of the same
    /// name for its moves (EVO-40): the snapshot and the report. Only its `#[moved_from]` errors fail the check.
    pub previous_line: Option<(PathBuf, Box<Report>)>,
}

/// What [Check::save] did.
#[derive(Debug)]
pub struct Saved {
    /// The snapshot of the current version. For an API root that didn't change since its previous snapshot, that
    /// one (saved by an older crate version), which stays its latest.
    pub path: PathBuf,
    pub status: SaveStatus,
    /// The previous version's snapshot and what changed since, if there is one.
    pub previous: Option<(PathBuf, Report)>,
    /// The crate as loaded from source, or the API root with [root](Self::root).
    pub bundle: ApiBundleOwned,
    /// The API root (`wire_v2::Thermo`) of a crate with several, `None` for a whole-crate snapshot.
    pub root: Option<String>,
    /// What looked wrong but didn't stop the save: an API root saved without an earlier snapshot to check it
    /// against, while the crate has other snapshots.
    pub warnings: Vec<String>,
    /// The definitions of the crate's own that a refresh in place replaced, kept by their old signatures (EVO-68).
    pub superseded: Vec<Superseded>,
}

/// Whether [Check::save] wrote the snapshot.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SaveStatus {
    /// There was none for this version.
    Saved,
    /// It differed from the source, and is overwritten.
    Refreshed,
    /// Same definitions, written again in the current format.
    Reformatted,
    /// Nothing written.
    UpToDate,
    /// Nothing written: the crate or root is on w0, the design line (EVO-41), which keeps no snapshots.
    DesignLine,
    /// An API root the crate no longer has: its removal is recorded
    /// ([removed_file_name](crate::snapshot_files::removed_file_name)), checked against the crate version (EVO-39).
    Removed,
}

impl fmt::Display for SaveStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            SaveStatus::Saved => "saved",
            SaveStatus::Refreshed => "refreshed",
            SaveStatus::Reformatted => "reformatted",
            SaveStatus::UpToDate => "up to date",
            SaveStatus::DesignLine => "not saved, w0 is the design line",
            SaveStatus::Removed => "removed",
        })
    }
}

impl fmt::Display for Saved {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.status)
    }
}

impl Outcome {
    /// `Ok` with a line saying why it passed, or `Err` naming the version needed and what to run.
    pub fn verdict(&self) -> Result<String, String> {
        match self {
            Outcome::Compared(c) => c.verdict(),
            Outcome::NoSnapshot(n) => n.verdict(),
            Outcome::Roots(r) => r.verdict(),
        }
    }

    /// Unit-suffix hints found in the source (TOOL-22), never affecting the verdict.
    pub fn hints(&self) -> &[UnitHint] {
        match self {
            Outcome::Compared(c) => &c.hints,
            Outcome::NoSnapshot(n) => &n.hints,
            Outcome::Roots(r) => &r.hints,
        }
    }

    /// `#[no_unit_hint]`s found with no reason or an empty one (TOOL-22): what `ww api check --strict` refuses,
    /// never part of [verdict](Self::verdict) itself.
    pub fn unreasoned(&self) -> &[UnreasonedSilence] {
        match self {
            Outcome::Compared(c) => &c.unreasoned,
            Outcome::NoSnapshot(n) => &n.unreasoned,
            Outcome::Roots(r) => &r.unreasoned,
        }
    }

    /// What looked wrong but doesn't change the verdict (a w0 root in a crate that can be published, EVO-41).
    pub fn warnings(&self) -> &[String] {
        match self {
            Outcome::Compared(c) => &c.warnings,
            Outcome::NoSnapshot(n) => &n.warnings,
            Outcome::Roots(r) => &r.warnings,
        }
    }
}

impl Comparison {
    fn verdict(&self) -> Result<String, String> {
        let report = &self.report;
        if self.design_line {
            return Ok(design_line_verdict(
                report.new_wire,
                "the differences above are information",
            ));
        }
        let wire = report.check_wire().err().map(|e| {
            format!(
                "{e}.\n{} (docs/evolution/rules.md#the-wire-version).",
                capitalized(&wire_hint_at(
                    report,
                    &wire_declared_at(self.crate_path.as_deref(), self.root.as_deref())
                ))
            )
        });
        let version = self.version_verdict();
        match (version, wire) {
            (Ok(ok), None) => Ok(ok),
            (Ok(_), Some(wire)) => Err(wire),
            (Err(version), None) => Err(version),
            (Err(version), Some(wire)) => Err(format!("{version}\n{wire}")),
        }
    }

    /// [Self::verdict] of the crate version alone.
    fn version_verdict(&self) -> Result<String, String> {
        let report = &self.report;
        if let Err(e) = report.check_version() {
            let same_version = report.old.version == report.new.version;
            let mut msg = format!("{e}.");
            if self.current_required {
                return Err(msg + &self.current_hint());
            }
            if let Some(since) = &self.unchanged_since {
                msg.push_str(&format!(
                    "\n{} {} has snapshots, saved with this root unchanged since {} ({}), so it is released and never \
                     changes: a change needs a new version.",
                    report.new.crate_id,
                    triplet(&report.new.version),
                    triplet(since),
                    self.old_path.display()
                ));
            } else if same_version {
                msg.push_str(&format!(
                    "\n{} {} has a snapshot ({}), so it is released and never changes: a change needs a new version.",
                    report.new.crate_id,
                    triplet(&report.old.version),
                    self.old_path.display()
                ));
            }
            if let Some(path) = &self.crate_path {
                msg.push_str(&format!(
                    "\nBump {} to {} or later (docs/evolution/rules.md#your-apis-version); once that version is \
                     released, record it with `ww api save {}` and commit {SNAPSHOTS_DIR}/.",
                    version_declared_at(Some(path)),
                    triplet(&report.minimal_version()),
                    path.display()
                ));
            }
            return Err(msg);
        }
        Ok(match report.change {
            Change::None => "no changes".to_string(),
            _ => "version is bumped enough".to_string(),
        })
    }
}

impl Comparison {
    /// What to do when a check with [Check::require_current] fails.
    fn current_hint(&self) -> String {
        let report = &self.report;
        let version = format!("{} {}", report.new.crate_id, triplet(&report.new.version));
        let declared_at = version_declared_at(self.crate_path.as_deref());
        if self.new_label == "source" {
            format!(
                "\nThe source differs from {}, the snapshot of {version}{}.\nIf {version} is not released yet, save \
                 it again with {}. A released version's snapshot never changes: bump {declared_at} \
                 instead (docs/evolution/rules.md#your-apis-version) and save the new version's snapshot.",
                self.old_path.display(),
                self.unchanged_since
                    .as_ref()
                    .map_or(String::new(), |since| format!(
                        " (unchanged since {}, so no file of its own)",
                        triplet(since)
                    )),
                save_command(self.crate_path.as_deref())
            )
        } else {
            format!(
                "\nBump {declared_at} to {} or later (docs/evolution/rules.md#your-apis-version) and save \
                 its snapshot with {}; {} goes then, as {version} was never released.",
                triplet(&report.minimal_version()),
                save_command(self.crate_path.as_deref()),
                self.new_label
            )
        }
    }
}

/// What to change for [Report::check_wire]: the wire version to declare, and where it is declared, `at`
/// ([wire_declared_at]).
fn wire_hint_at(report: &Report, at: &str) -> String {
    match (report.minimal_wire(), report.new_wire) {
        (Some(min), Some(new)) if new < min => {
            format!("bump the wire version in {at} to {min} or later")
        }
        (Some(min), None) => format!("declare the wire version in {at}, {min} or later"),
        _ => format!("fix the #[since] markers or bump the wire version in {at}"),
    }
}

/// Where the crate at `crate_path` declares its wire version: `[package.metadata.wire_weaver] wire` in its
/// `Cargo.toml` (every crate, EVO-31), or the attribute of a test crate whose root says `#[ww_api_root(wire = ..)]`
/// instead, or for the API root `root` of a crate with several, its entry in `[package.metadata.wire_weaver.roots]`.
/// The only place the checker decides this.
fn wire_declared_at(crate_path: Option<&Path>, root: Option<&str>) -> String {
    if let Some(root) = root {
        let cargo_toml = crate_path.map_or("Cargo.toml".into(), |p| p.join("Cargo.toml"));
        return format!(
            "`\"{root}\"` of `[package.metadata.wire_weaver.roots]` in {}",
            cargo_toml.display()
        );
    }
    let Some(crate_path) = crate_path else {
        // a snapshot file
        return "`[package.metadata.wire_weaver] wire` in the crate's `Cargo.toml`".to_string();
    };
    let cargo_toml = crate_path.join("Cargo.toml");
    let in_manifest = fs::read_to_string(&cargo_toml)
        .is_ok_and(|text| text.contains("[package.metadata.wire_weaver]"));
    let in_attribute = fs::read_to_string(crate_path.join("src/lib.rs"))
        .is_ok_and(|text| text.contains("#[ww_api_root(wire"));
    if in_attribute && !in_manifest {
        "`#[ww_api_root(wire = ..)]`".to_string()
    } else {
        format!(
            "`[package.metadata.wire_weaver] wire` of {}",
            cargo_toml.display()
        )
    }
}

/// Where the crate version at `crate_path` is declared, for a hint to bump it: `version` in its `Cargo.toml`, or
/// `[workspace.package] version` of the workspace's `Cargo.toml` for a crate that inherits it
/// (`version.workspace = true`, the framework crates such as `ww_link`).
pub(crate) fn version_declared_at(crate_path: Option<&Path>) -> String {
    let Some(crate_path) = crate_path else {
        return "`version` in the crate's `Cargo.toml`".to_string();
    };
    let cargo_toml = crate_path.join("Cargo.toml");
    let inherited = fs::read_to_string(&cargo_toml).is_ok_and(|text| {
        text.lines().any(|line| {
            let line: String = line.chars().filter(|c| !c.is_whitespace()).collect();
            line == "version.workspace=true" || line.starts_with("version={workspace=true")
        })
    });
    let workspace = inherited
        .then(|| {
            let crate_path = crate_path.canonicalize().ok()?;
            crate_path
                .ancestors()
                .skip(1)
                .map(|dir| dir.join("Cargo.toml"))
                .find(|path| {
                    fs::read_to_string(path)
                        .is_ok_and(|text| text.lines().any(|l| l.trim() == "[workspace]"))
                })
        })
        .flatten();
    match workspace {
        Some(workspace) => format!(
            "`[workspace.package] version` in {} (inherited by {})",
            workspace.display(),
            cargo_toml.display()
        ),
        None => format!("`version` in {}", cargo_toml.display()),
    }
}

fn capitalized(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// How to save the snapshot of the current version: the template's recipe, the test it runs, or the CLI.
fn save_command(crate_path: Option<&Path>) -> String {
    let path = crate_path.map_or("<crate>".to_string(), |p| p.display().to_string());
    format!(
        "`just api-save` (`{SAVE_ENV}=1 cargo test --test evolution` in the crate, or `ww api save {path}`)"
    )
}

/// Whether `crate_path` (an API crate directory, not a snapshot file) has an evolution test of its own: a
/// `tests/evolution.rs` that calls [Check]. `ww api check` and `ww api save` warn with this when it doesn't, so a
/// wire break is caught by `cargo test` and a pre-commit hook (docs/evolution/checker_tool.md), not only when
/// someone remembers to run `ww api check` by hand.
pub fn missing_test_warning(crate_path: &Path) -> Option<String> {
    let test_path = crate_path.join("tests").join("evolution.rs");
    let has_test = fs::read_to_string(&test_path)
        .is_ok_and(|contents| contents.contains("wire_weaver_evolution::Check"));
    if has_test {
        return None;
    }
    Some(format!(
        "no {} calling wire_weaver_evolution::Check: add the wire_weaver_evolution dev-dependency and this test \
         (docs/evolution/checker_tool.md), so a wire break is caught by `cargo test`, not only by `ww api check` \
         run by hand",
        test_path.display()
    ))
}

/// The warning for an API root saved with no earlier snapshot of its own while `dir` has snapshots of older versions
/// of the crate: a root renamed, moved to another module or to another line is not matched by them (EVO-40), and
/// would otherwise be saved unchecked without a word. `None` when the crate has no older snapshots (a new crate, or
/// only this version's files of its roots).
fn no_previous_warning(
    dir: &Path,
    crate_id: &str,
    version: &VersionOwned,
    root: &str,
    wire: WireVersion,
) -> Result<Option<String>, String> {
    let trait_name = root.rsplit("::").next().unwrap_or(root);
    let mut others = vec![];
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("reading {}: {e}", dir.display())),
    };
    for entry in entries {
        let path = entry
            .map_err(|e| format!("reading {}: {e}", dir.display()))?
            .path();
        let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if path.extension().is_none_or(|ext| ext != "ron") {
            continue;
        }
        if let Some(name) = root_of_file_name(file_name) {
            if name.crate_id == crate_id && name.version < key(version) {
                others.push(format!("{file_name} (root `{}`)", name.root));
            }
            continue;
        }
        let snapshot = read(&path)?;
        let own = snapshot
            .bundle
            .crate_version(0)
            .map_err(|e| e.to_string())?;
        if own.crate_id != crate_id || key(&own.version) >= key(version) {
            continue;
        }
        let roots: Vec<_> = snapshot
            .bundle
            .root
            .items
            .iter()
            .filter(|item| matches!(item.kind, ww_self::ApiItemKindOwned::Trait { .. }))
            .map(|item| format!("`{}`", item.ident))
            .collect();
        let wire = snapshot
            .bundle
            .wire
            .map_or("no wire version".to_string(), |w| w.to_string());
        others.push(format!(
            "{file_name} (whole crate, {wire}, traits {})",
            if roots.is_empty() {
                "none".to_string()
            } else {
                roots.join(", ")
            }
        ));
    }
    if others.is_empty() {
        return Ok(None);
    }
    others.sort();
    Ok(Some(format!(
        "API root `{root}` has no earlier snapshot, so it is saved unchecked: looked for its own files \
         ({crate_id}_*.{}@w*.ron) and for whole-crate snapshots on line {} with a root trait `{trait_name}`, and \
         {} has only {}. A root renamed, moved to another module or to another line is not matched by name \
         (EVO-40); compare it by hand with `ww api diff <snapshot> --against <its old snapshot>` once saved",
        root.replace("::", "."),
        wire.line.0,
        dir.display(),
        others.join(", ")
    )))
}

/// The passing verdict of a crate on w0, the design line (EVO-41).
fn design_line_verdict(wire: Option<WireVersion>, what: &str) -> String {
    let wire = wire.map_or("w0".to_string(), |w| w.to_string());
    format!(
        "{wire} is the design line: breaks are accepted and {what}; at its first release the API moves to w1.0, and \
         `ww api save` keeps its first snapshot (docs/evolution/rules.md#w0-the-design-line)"
    )
}

/// The warnings for a crate at `crate_path` with a root on w0, the design line (EVO-41): one when it can be
/// published, since others then build on an API that may still break without a new line.
fn design_line_warnings(crate_path: &Path, bundle: &ApiBundleOwned) -> Result<Vec<String>, String> {
    if !wire_weaver_core::publishable(crate_path).map_err(|e| format!("{e:#}"))? {
        return Ok(vec![]);
    }
    let own = bundle.crate_version(0).map_err(|e| e.to_string())?;
    Ok(vec![format!(
        "{} can be published (its Cargo.toml has no `publish = false`), and its API is on {}, the design line, \
         where breaks need no new line: move it to w1.0 before publishing (docs/evolution/rules.md#w0-the-design-line)",
        own.crate_id,
        bundle.wire.map_or("w0".to_string(), |w| w.to_string())
    )])
}

/// [Saved] of a crate or root on the design line: nothing to write.
fn not_saved(
    path: PathBuf,
    bundle: ApiBundleOwned,
    root: Option<String>,
    warnings: Vec<String>,
) -> (Saved, Option<String>) {
    let saved = Saved {
        path,
        status: SaveStatus::DesignLine,
        previous: None,
        bundle,
        root,
        warnings,
        superseded: vec![],
    };
    (saved, None)
}

/// [compare_recorded] of a crate with its `#[moved_from]` (`moves`), each one the comparison couldn't check
/// ([Report::unverified_moves]) verified against the latest snapshot in `dir` of the line it names: what doesn't hold
/// goes to [Report::moved_from_errors], one without a snapshot of its line to [Report::warnings]. `definitions` as for
/// [compare_recorded], those of the source a side was loaded from.
pub(crate) fn compare_checked(
    dir: &Path,
    old: &ApiBundleOwned,
    old_recorded: &Recorded,
    new: &ApiBundleOwned,
    new_recorded: &Recorded,
    moves: &[Move],
    definitions: &[&ApiBundleOwned],
) -> Result<Report, String> {
    let mut report = compare_recorded(old, old_recorded, new, new_recorded, moves, definitions)?;
    if report.unverified_moves.is_empty() {
        return Ok(report);
    }
    let crate_id = &new.crate_version(0).map_err(|e| e.to_string())?.crate_id;
    // every snapshot of the crate, whole-crate and per root, with its wire version
    let mut snapshots = vec![];
    for file in crate_files(dir, crate_id)? {
        if !matches!(file.kind, CrateFile::Crate | CrateFile::Root { .. }) {
            continue;
        }
        let snapshot = read(&file.path)?;
        if let Some(wire) = snapshot.bundle.wire {
            snapshots.push((wire, file.version, snapshot));
        }
    }
    snapshots.sort_by_key(|(wire, version, _)| (wire.line.0, wire.revision.0, *version));
    for moved in std::mem::take(&mut report.unverified_moves) {
        let on_line: Vec<_> = snapshots
            .iter()
            .rev()
            .filter(|(wire, ..)| wire.same_line(&moved.wire) && !wire.is_design_line())
            .collect();
        // the newest of the line that has the resource named, else the newest, which says it has none
        let has = |s: &&&(WireVersion, (u32, u32, u32), Snapshot)| {
            own_traits(&s.2.bundle)
                .get(moved.from_trait.as_str())
                .is_some_and(|level| level.items.iter().any(|i| i.ident == moved.from_ident))
        };
        let Some((_, _, line)) = on_line.iter().find(has).or(on_line.first()) else {
            report.warnings.push(format!(
                "{moved}: not verified, {} has no snapshot of line {}",
                dir.display(),
                moved.wire.line.0
            ));
            continue;
        };
        if let Err(errors) = verify_move(&line.bundle, new, &moved) {
            for e in errors {
                report
                    .moved_from_errors
                    .push(format!("{e} (as in {})", line.path.display()));
            }
        }
    }
    Ok(report)
}

/// A warning for each of `moves` on a resource no bundle of `roots` has: an attribute on a trait no API root
/// reaches is checked by nothing.
pub(crate) fn unreached_moves(roots: &[&ApiBundleOwned], moves: &[Move]) -> Vec<String> {
    moves
        .iter()
        .filter(|moved| {
            !roots.iter().any(|bundle| {
                own_traits(bundle)
                    .get(moved.trait_name.as_str())
                    .is_some_and(|level| level.items.iter().any(|i| i.ident == moved.ident))
            })
        })
        .map(|moved| {
            format!(
                "{moved}: no API root reaches {}, so the attribute is not checked",
                moved.trait_name
            )
        })
        .collect()
}

/// The `#[moved_from]` (EVO-40) of `bundle`'s own crate among `notes`.
pub(crate) fn own_moves(bundle: &ApiBundleOwned, notes: &SourceNotes) -> Vec<Move> {
    let own = bundle.ext_crates.first().map(|c| c.crate_id.as_str());
    notes
        .moved_from
        .iter()
        .filter(|m| Some(m.crate_id.as_str()) == own)
        .map(Move::from)
        .collect()
}

/// Whether the snapshot file at `path`, of the same version as `bundle`, needs writing.
fn existing_status(
    path: &Path,
    bundle: &ApiBundleOwned,
    contents: &str,
) -> Result<SaveStatus, String> {
    Ok(match fs::read_to_string(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => SaveStatus::Saved,
        Err(e) => return Err(format!("reading {}: {e}", path.display())),
        Ok(text) => {
            let old = read(path).ok();
            let same = old.as_ref().is_some_and(|old| {
                old.bundle.ww_self_version == bundle.ww_self_version
                    && compare_recorded(
                        &old.bundle,
                        &old.recorded,
                        bundle,
                        &Recorded::default(),
                        &[],
                        // any other signature of a dependency is another snapshot
                        &[],
                    )
                    .is_ok_and(|r| r.change == Change::None)
            });
            let complete = old.is_some_and(|old| old.recorded.unrecorded().is_empty());
            // the same definitions in another ww_self format or with fields it didn't record yet, or a file
            // without (or with another) hash, are saved again
            if !same {
                SaveStatus::Refreshed
            } else if !complete || snapshot::saved_hash(&text) != snapshot::saved_hash(contents) {
                SaveStatus::Reformatted
            } else {
                SaveStatus::UpToDate
            }
        }
    })
}

fn planned(
    path: PathBuf,
    status: SaveStatus,
    previous: Option<(PathBuf, Report)>,
    bundle: ApiBundleOwned,
    root: Option<String>,
    contents: String,
) -> (Saved, Option<String>) {
    let write = (status != SaveStatus::UpToDate).then_some(contents);
    let saved = Saved {
        path,
        status,
        previous,
        bundle,
        root,
        warnings: vec![],
        superseded: vec![],
    };
    (saved, write)
}

fn key(v: &VersionOwned) -> (u32, u32, u32) {
    (v.major.0, v.minor.0, v.patch.0)
}

/// The snapshot of exactly the version of `bundle` in `dir`.
fn current_snapshot(dir: &Path, bundle: &ApiBundleOwned) -> Result<Option<Snapshot>, String> {
    let version = &bundle.crate_version(0).map_err(|e| e.to_string())?.version;
    Ok(latest_snapshot(dir, bundle, version, true)?
        .filter(|s| triplet(&s.bundle.ext_crates[0].version) == triplet(version)))
}

impl NoSnapshot {
    fn verdict(&self) -> Result<String, String> {
        if self.design_line {
            return Ok(design_line_verdict(None, "no snapshot is kept or needed"));
        }
        if let Some((path, report)) = &self.previous_line
            && !report.moved_from_errors.is_empty()
        {
            return Err(format!(
                "{}.\n#[moved_from] names a resource of the previous line as it is in {} (docs/evolution/rules.md#moving-a-resource-to-another-place).",
                report.moved_from_errors.join(".\n"),
                path.display()
            ));
        }
        if self.current_required {
            let version = format!("{} {}", self.crate_id, triplet(&self.version));
            let previous = match &self.previous {
                Some(path) => format!(" (the previous one is {})", path.display()),
                None => String::new(),
            };
            return Err(format!(
                "no snapshot of {version} in {}{previous}, and the current version's snapshot is required.\nSave \
                 it with {} and commit {SNAPSHOTS_DIR}/ (see docs/evolution/checker_tool.md).",
                self.dir.display(),
                save_command(self.crate_path.as_deref())
            ));
        }
        let versions = if self.crate_path.is_some() {
            format!("{} {} or earlier", self.crate_id, triplet(&self.version))
        } else {
            format!("{} earlier than {}", self.crate_id, triplet(&self.version))
        };
        let found = format!("no snapshot of {versions} in {}", self.dir.display());
        if !self.strict {
            return Ok(format!("{found}, nothing to compare with"));
        }
        let save = match &self.crate_path {
            Some(path) => format!("`ww api save {}`", path.display()),
            None => "`ww api save <crate>`".to_string(),
        };
        Err(format!(
            "{found}, and strict mode needs one to check the crate against.\nRecord the released version with \
             {save} and commit {SNAPSHOTS_DIR}/ (see docs/evolution/checker_tool.md)."
        ))
    }
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Outcome::Compared(c) => c.fmt(f),
            Outcome::Roots(r) => r.fmt(f),
            Outcome::NoSnapshot(n) => {
                write!(
                    f,
                    "{} {}: no snapshot to compare with in {}",
                    n.crate_id,
                    triplet(&n.version),
                    n.dir.display()
                )?;
                for hint in &n.hints {
                    write!(f, "\n  note: {hint}")?;
                }
                for warning in &n.warnings {
                    write!(f, "\n  warning: {warning}")?;
                }
                if let Some((path, report)) = &n.previous_line {
                    write!(f, "\n  the line before: {}", path.display())?;
                    for finding in report
                        .breaking
                        .iter()
                        .filter(|f| f.rule == Rule::MovedAcross)
                    {
                        write!(f, "\n    moved: {}", finding.message)?;
                    }
                    for proposal in &report.proposals {
                        write!(f, "\n    proposal: {proposal}")?;
                    }
                }
                Ok(())
            }
        }
    }
}

/// The report as plain text: what was compared, then every breaking (with its rule) and compatible change and
/// warning on a line of its own.
impl fmt::Display for Comparison {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let report = &self.report;
        write!(
            f,
            "{} {} ({}) -> {} ({}): {}",
            report.new.crate_id,
            triplet(&report.old.version),
            self.old_path.display(),
            triplet(&report.new.version),
            self.new_label,
            report.change
        )?;
        write!(f, "\n  needs: {}", report.required())?;
        let (groups, rest) = report.breaking_groups();
        for (root, findings) in &groups {
            write!(f, "\n  {} broken because {root} changed:", findings.len())?;
            for finding in findings {
                write!(f, "\n    breaking: {finding}")?;
            }
        }
        for finding in rest {
            write!(f, "\n  breaking: {finding}")?;
        }
        for change in &report.compatible {
            write!(f, "\n  compatible: {change}")?;
        }
        for warning in &report.warnings {
            write!(f, "\n  warning: {warning}")?;
        }
        for note in &report.notes {
            write!(f, "\n  note: {note}")?;
        }
        for proposal in &report.proposals {
            write!(f, "\n  proposal: {proposal}")?;
        }
        if !report.unrecorded.is_empty() {
            let fields: Vec<_> = report.unrecorded.iter().map(|u| u.to_string()).collect();
            write!(
                f,
                "\n  not compared: {}, which a snapshot saved by an older wire_weaver doesn't have",
                fields.join(", ")
            )?;
        }
        for hint in &self.hints {
            write!(f, "\n  note: {hint}")?;
        }
        for warning in &self.warnings {
            write!(f, "\n  warning: {warning}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod hint_tests {
    use super::version_declared_at;
    use std::path::Path;

    /// A crate that inherits the workspace version is told to bump `[workspace.package] version`, one with its own
    /// `version` that one.
    #[test]
    fn where_the_version_is_declared() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let ww_link = version_declared_at(Some(&root.join("ww_link")));
        assert!(
            ww_link.starts_with("`[workspace.package] version` in ")
                && ww_link.contains("(inherited by "),
            "{ww_link}"
        );
        let own = version_declared_at(Some(&root.join("ww_stdlib/ww_gpio")));
        assert!(own.starts_with("`version` in "), "{own}");
    }
}
