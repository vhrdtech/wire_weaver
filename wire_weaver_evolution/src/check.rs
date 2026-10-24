//! Checking a crate against its `api_snapshots/`, from source: what `ww api check` and an API crate's own
//! evolution test run.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use ww_self::{ApiBundleOwned, snapshot};
use ww_version::VersionOwned;

use crate::snapshot_files::{SNAPSHOTS_DIR, latest_snapshot, parse, triplet};
use crate::{Change, Report, compare};

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

    fn is_snapshot(&self) -> bool {
        self.path.extension().is_some_and(|ext| ext == "ron")
    }

    /// Loads both versions and compares them. Whether the version is bumped enough is up to [Outcome::verdict].
    pub fn run(&self) -> Result<Outcome, String> {
        let is_snapshot = self.is_snapshot();
        if self.require_current && !is_snapshot && self.against.is_none() {
            return self.run_current();
        }
        let (new, new_label, dir) = if is_snapshot {
            let dir = self.path.parent().unwrap_or(Path::new(".")).to_path_buf();
            (parse(&self.path)?, self.path.display().to_string(), dir)
        } else {
            let new = wire_weaver_core::load_crate(&self.path).map_err(|e| format!("{e:#}"))?;
            (new, "source".to_string(), self.path.join(SNAPSHOTS_DIR))
        };
        let (old_path, old) = match &self.against {
            Some(against) => (against.clone(), parse(against)?),
            None => {
                let version = &new.crate_version(0).map_err(|e| e.to_string())?.version;
                match latest_snapshot(&dir, &new, version, !is_snapshot)? {
                    Some(previous) => previous,
                    None => {
                        let own = &new.ext_crates[0];
                        return Ok(Outcome::NoSnapshot(NoSnapshot {
                            crate_id: own.crate_id.clone(),
                            version: own.version.clone(),
                            dir,
                            crate_path: (!is_snapshot).then(|| self.path.clone()),
                            strict: self.strict,
                            current_required: false,
                            previous: None,
                        }));
                    }
                }
            }
        };
        let report = compare(&old, &new)?;
        Ok(Outcome::Compared(Box::new(Comparison {
            old_path,
            new_label,
            crate_path: (!is_snapshot).then(|| self.path.clone()),
            report,
            old,
            new,
            current_required: false,
        })))
    }

    /// [run](Self::run) with [require_current](Self::require_current).
    fn run_current(&self) -> Result<Outcome, String> {
        let source = wire_weaver_core::load_crate(&self.path).map_err(|e| format!("{e:#}"))?;
        let dir = self.path.join(SNAPSHOTS_DIR);
        let own = source.crate_version(0).map_err(|e| e.to_string())?;
        let current = current_snapshot(&dir, &source)?;
        let previous = latest_snapshot(&dir, &source, &own.version, false)?;
        let crate_path = Some(self.path.clone());
        let Some((current_path, current)) = current else {
            return Ok(Outcome::NoSnapshot(NoSnapshot {
                crate_id: own.crate_id.clone(),
                version: own.version.clone(),
                dir,
                crate_path,
                strict: true,
                current_required: true,
                previous: previous.map(|(path, _)| path),
            }));
        };
        let report = compare(&current, &source)?;
        let (old_path, old, new_label, new, report) = match previous {
            // the source is checked against the snapshot of its version first, then that snapshot against the previous
            Some((previous_path, previous)) if report.change == Change::None => {
                let report = compare(&previous, &current)?;
                let label = current_path.display().to_string();
                (previous_path, previous, label, current, report)
            }
            _ => (current_path, current, "source".to_string(), source, report),
        };
        Ok(Outcome::Compared(Box::new(Comparison {
            old_path,
            new_label,
            crate_path,
            report,
            old,
            new,
            current_required: true,
        })))
    }

    /// Saves a snapshot of the crate's current version into its `api_snapshots/`, as `ww api save` does: writes it, or
    /// refreshes it if it differs from the source. A snapshot of an older version is never written. Refuses a version
    /// not bumped enough since the previous version's snapshot, unless `force` is true.
    ///
    /// The snapshot of a released version never changes, which this can't know: a pre-commit hook refusing changes to
    /// committed snapshots of released versions does (docs/evolution/checker_tool.md).
    pub fn save(&self, force: bool) -> Result<Saved, String> {
        if self.is_snapshot() {
            return Err(format!(
                "{} is a snapshot file, saving needs a crate directory",
                self.path.display()
            ));
        }
        let bundle = wire_weaver_core::load_crate(&self.path).map_err(|e| format!("{e:#}"))?;
        let own = bundle.crate_version(0).map_err(|e| e.to_string())?;
        let dir = self.path.join(SNAPSHOTS_DIR);
        let path = dir.join(format!("{}.ron", own.filename_friendly()));
        // with the hash of the shrink_wrap bytes the RON stands for: the build of wire_weaver_snapshots re-encodes the
        // RON and fails when it differs, which proves the round trip lossless
        let contents = wire_weaver_snapshots::render_ron(&bundle)?;

        let previous = match latest_snapshot(&dir, &bundle, &own.version, false)? {
            Some((previous_path, previous)) => {
                let report = compare(&previous, &bundle)?;
                if let Err(e) = report.check_version()
                    && !force
                {
                    return Err(format!(
                        "{e}.\nNot saved: bump `version` in {} to {} or later (docs/evolution/rules.md#your-apis-version) \
                         and save again.",
                        self.path.join("Cargo.toml").display(),
                        triplet(&report.minimal_version())
                    ));
                }
                Some((previous_path, report))
            }
            None => None,
        };

        let status = match fs::read_to_string(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => SaveStatus::Saved,
            Err(e) => return Err(format!("reading {}: {e}", path.display())),
            Ok(text) => {
                let same = parse(&path).ok().is_some_and(|old| {
                    old.ww_self_version == bundle.ww_self_version
                        && compare(&old, &bundle).is_ok_and(|r| r.change == Change::None)
                });
                // the same definitions in another ww_self format, or a file without (or with another) hash, are
                // saved again
                if !same {
                    SaveStatus::Refreshed
                } else if snapshot::saved_hash(&text) != snapshot::saved_hash(&contents) {
                    SaveStatus::Reformatted
                } else {
                    SaveStatus::UpToDate
                }
            }
        };
        if status != SaveStatus::UpToDate {
            fs::create_dir_all(&dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
            fs::write(&path, contents).map_err(|e| format!("writing {}: {e}", path.display()))?;
        }
        Ok(Saved {
            path,
            status,
            previous,
            bundle,
        })
    }

    /// [run](Self::run) and [verdict](Outcome::verdict), printing what was found; panics with what is wrong and what
    /// to do about it. For an API crate's own test.
    ///
    /// With [SAVE_ENV] (`WW_API_SAVE=1 cargo test --test evolution`), [saves](Self::save) the snapshot of the current
    /// version first, so the lock file pins the saver too.
    pub fn assert(&self) {
        if std::env::var_os(SAVE_ENV).is_some_and(|v| !v.is_empty() && v != "0") {
            match self.save(false) {
                Ok(saved) => println!("{saved}"),
                Err(e) => panic!("\n{e}"),
            }
        }
        let outcome = self
            .run()
            .unwrap_or_else(|e| panic!("evolution check failed: {e}"));
        match outcome.verdict() {
            Ok(ok) => println!("{outcome}\n{ok}"),
            Err(e) => panic!("\n{outcome}\n{e}"),
        }
    }
}

/// What [Check::run] found.
#[derive(Debug)]
pub enum Outcome {
    Compared(Box<Comparison>),
    /// Nothing to compare with.
    NoSnapshot(NoSnapshot),
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
}

/// What [Check::save] did.
#[derive(Debug)]
pub struct Saved {
    /// The snapshot of the current version.
    pub path: PathBuf,
    pub status: SaveStatus,
    /// The previous version's snapshot and what changed since, if there is one.
    pub previous: Option<(PathBuf, Report)>,
    /// The crate as loaded from source.
    pub bundle: ApiBundleOwned,
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
}

impl fmt::Display for SaveStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            SaveStatus::Saved => "saved",
            SaveStatus::Refreshed => "refreshed",
            SaveStatus::Reformatted => "reformatted",
            SaveStatus::UpToDate => "up to date",
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
        }
    }
}

impl Comparison {
    fn verdict(&self) -> Result<String, String> {
        let report = &self.report;
        if let Err(e) = report.check_version() {
            let same_version = report.old.version == report.new.version;
            let mut msg = format!("{e}.");
            if self.current_required {
                return Err(msg + &self.current_hint());
            }
            if same_version {
                msg.push_str(&format!(
                    "\n{} {} has a snapshot ({}), so it is released and never changes: a change needs a new version.",
                    report.new.crate_id,
                    triplet(&report.old.version),
                    self.old_path.display()
                ));
            }
            if let Some(path) = &self.crate_path {
                msg.push_str(&format!(
                    "\nBump `version` in {} to {} or later (docs/evolution/rules.md#your-apis-version); once that \
                     version is released, record it with `ww api save {}` and commit {SNAPSHOTS_DIR}/.",
                    path.join("Cargo.toml").display(),
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
        let cargo_toml = match &self.crate_path {
            Some(path) => path.join("Cargo.toml").display().to_string(),
            None => "Cargo.toml".to_string(),
        };
        if self.new_label == "source" {
            format!(
                "\nThe source differs from {}, the snapshot of {version}.\nIf {version} is not released yet, save \
                 it again with {}. A released version's snapshot never changes: bump `version` in {cargo_toml} \
                 instead (docs/evolution/rules.md#your-apis-version) and save the new version's snapshot.",
                self.old_path.display(),
                save_command(self.crate_path.as_deref())
            )
        } else {
            format!(
                "\nBump `version` in {cargo_toml} to {} or later (docs/evolution/rules.md#your-apis-version) and save \
                 its snapshot with {}; {} goes then, as {version} was never released.",
                triplet(&report.minimal_version()),
                save_command(self.crate_path.as_deref()),
                self.new_label
            )
        }
    }
}

/// How to save the snapshot of the current version: the template's recipe, the test it runs, or the CLI.
fn save_command(crate_path: Option<&Path>) -> String {
    let path = crate_path.map_or("<crate>".to_string(), |p| p.display().to_string());
    format!(
        "`just api-save` (`{SAVE_ENV}=1 cargo test --test evolution` in the crate, or `ww api save {path}`)"
    )
}

/// The snapshot of exactly the version of `bundle` in `dir`.
fn current_snapshot(
    dir: &Path,
    bundle: &ApiBundleOwned,
) -> Result<Option<(PathBuf, ApiBundleOwned)>, String> {
    let version = &bundle.crate_version(0).map_err(|e| e.to_string())?.version;
    Ok(latest_snapshot(dir, bundle, version, true)?
        .filter(|(_, s)| triplet(&s.ext_crates[0].version) == triplet(version)))
}

impl NoSnapshot {
    fn verdict(&self) -> Result<String, String> {
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
            Outcome::NoSnapshot(n) => write!(
                f,
                "{} {}: no snapshot to compare with in {}",
                n.crate_id,
                triplet(&n.version),
                n.dir.display()
            ),
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
        for finding in &report.breaking {
            write!(f, "\n  breaking: {finding}")?;
        }
        for change in &report.compatible {
            write!(f, "\n  compatible: {change}")?;
        }
        for warning in &report.warnings {
            write!(f, "\n  warning: {warning}")?;
        }
        Ok(())
    }
}
