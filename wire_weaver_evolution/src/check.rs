//! Checking a crate against its `api_snapshots/`, from source: what `ww api check` and an API crate's own
//! evolution test run.

use std::fmt;
use std::path::{Path, PathBuf};

use ww_self::ApiBundleOwned;
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
/// ```no_run
/// // tests/evolution.rs of an API crate: fails on a change without the version bump it needs, and in strict mode
/// // also when the crate has no snapshot yet
/// wire_weaver_evolution::Check::new(env!("CARGO_MANIFEST_DIR")).strict(true).assert();
/// ```
#[derive(Clone, Debug)]
pub struct Check {
    path: PathBuf,
    against: Option<PathBuf>,
    strict: bool,
}

impl Check {
    /// `path` is a crate directory or a snapshot file (`.ron`).
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Check {
            path: path.into(),
            against: None,
            strict: false,
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

    /// Loads both versions and compares them. Whether the version is bumped enough is up to [Outcome::verdict].
    pub fn run(&self) -> Result<Outcome, String> {
        let is_snapshot = self.path.extension().is_some_and(|ext| ext == "ron");
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
        })))
    }

    /// [run](Self::run) and [verdict](Outcome::verdict), printing what was found; panics with what is wrong and what
    /// to do about it. For an API crate's own test.
    pub fn assert(&self) {
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

impl NoSnapshot {
    fn verdict(&self) -> Result<String, String> {
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
