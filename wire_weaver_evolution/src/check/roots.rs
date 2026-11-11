//! [Check] of a crate with several API roots (EVO-37): each root against its own latest snapshot, a removed root
//! against the crate version, a changed shared item named with every root it changed on (EVO-39).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use ww_self::{ApiBundleOwned, qualified_name};
use ww_version::{FullVersionOwned, VersionOwned, WireVersion};

use super::{
    Check, Comparison, NoSnapshot, Outcome, SaveStatus, Saved, UnitHint, compare_checked,
    design_line_warnings, key, own_moves, save_command, unreached_moves,
};
use crate::recorded::Recorded;
use crate::snapshot_files::{
    CrateFile, NamedFile, RootFileName, RootSnapshot, crate_files, read, removed_file_name,
    root_of_file_name, root_snapshots, triplet,
};
use crate::{Bump, Change, Move, own_traits, own_type_paths, suggest};
use wire_weaver_core::CrateCache;

/// What [Check::run] found for a crate with several API roots (`[package.metadata.wire_weaver.roots]`, EVO-37):
/// each root compared with its own latest snapshot (EVO-39).
#[derive(Debug)]
pub struct RootsOutcome {
    pub crate_id: String,
    pub version: VersionOwned,
    pub crate_path: PathBuf,
    /// Where snapshots were looked for.
    pub dir: PathBuf,
    /// One per root of the `roots` table, in its order, each [Outcome::Compared] or [Outcome::NoSnapshot]; with
    /// [Check::against], only the root that snapshot is of.
    pub roots: Vec<RootOutcome>,
    /// Roots with snapshots that the `roots` table no longer lists.
    pub removed: Vec<RemovedRoot>,
    /// The crate's own types and traits reached by more than one root that changed, each with the roots it changed
    /// on.
    pub shared: Vec<SharedChange>,
    /// Unit-suffix hints found in the source (TOOL-22), once each for all roots: a note, never part of the verdict.
    pub hints: Vec<UnitHint>,
    /// What looked wrong but doesn't change the verdict, once each for all roots: a root on w0, the design line, in a
    /// crate that can be published (EVO-41).
    pub warnings: Vec<String>,
    /// Checked with [Check::require_current]: a removal has to be recorded.
    pub current_required: bool,
}

/// One API root of a crate with several, as [Check::run] found it.
#[derive(Debug)]
pub struct RootOutcome {
    /// `wire_v2::Thermo`.
    pub root: String,
    /// Its wire version, from the `roots` table.
    pub wire: WireVersion,
    /// [Outcome::Compared] or [Outcome::NoSnapshot], without the hints and warnings, which [RootsOutcome] has.
    pub outcome: Outcome,
}

/// An API root with snapshots in the crate's snapshot directory that the `roots` table no longer lists: a line
/// leaving the crate, a breaking crate bump (docs/evolution/rules.md#several-lines-in-one-crate). `ww api save`
/// records the removal ([removed_file_name]), so a later version isn't asked for the bump again.
#[derive(Debug)]
pub struct RemovedRoot {
    pub root: String,
    /// Its newest snapshot.
    pub last: PathBuf,
    /// The wire version of that snapshot.
    pub wire: WireVersion,
    /// The crate version the removal needs a breaking bump of: the newest one with a saved snapshot, `None` when the
    /// root was only ever saved by the version being worked on.
    pub since: Option<VersionOwned>,
    /// The file recording its removal, `None` while it isn't.
    pub recorded: Option<PathBuf>,
}

/// A type or trait of the crate that more than one root reaches (a `common` module's), changed since a root's
/// snapshot.
#[derive(Debug, PartialEq, Eq)]
pub struct SharedChange {
    /// Its path in the crate, `common::Code`.
    pub item: String,
    /// The roots it changed on, with their wire versions.
    pub roots: Vec<(String, WireVersion)>,
    /// The other roots that reach it, with nothing to compare it with: a root on w0, the design line (EVO-41), or
    /// without a snapshot.
    pub also: Vec<(String, WireVersion)>,
}

impl Check {
    /// The root, crate and wire version the snapshot file checked is named after, if it is a root's.
    pub(super) fn root_file(&self) -> Option<RootFileName> {
        root_of_file_name(self.path.file_name()?.to_str()?)
    }

    /// [Check::run] of a crate with several API roots: each root from source against its own latest snapshot.
    pub(super) fn run_roots(
        &self,
        roots: &BTreeMap<String, WireVersion>,
        cache: &CrateCache,
    ) -> Result<Outcome, String> {
        let dir = self.crate_snapshots_dir();
        let mut loaded = vec![];
        let mut hints = vec![];
        let mut warnings = vec![];
        let mut moves = vec![];
        let mut modules = BTreeMap::new();
        let mut definitions = vec![];
        for (root, wire) in roots {
            let (bundle, notes) = cache
                .load_root_with_notes(&self.path, root)
                .map_err(|e| format!("{e:#}"))?;
            for hint in notes.hints.iter() {
                if !hints.contains(hint) {
                    hints.push(hint.clone());
                }
            }
            for moved in own_moves(&bundle, &notes) {
                if !moves.contains(&moved) {
                    moves.push(moved);
                }
            }
            if wire.is_design_line() {
                for warning in design_line_warnings(&self.path, &bundle)? {
                    if !warnings.contains(&warning) {
                        warnings.push(warning);
                    }
                }
            }
            modules.insert(root.clone(), notes.trait_modules);
            definitions.push(notes.definitions);
            loaded.push((root.clone(), *wire, bundle));
        }
        let bundles: Vec<&ApiBundleOwned> = loaded.iter().map(|(_, _, b)| b).collect();
        // what every root's walk saw of other crates
        let definitions: Vec<&ApiBundleOwned> = definitions.iter().collect();
        warnings.extend(unreached_moves(&bundles, &moves));
        let own = loaded[0]
            .2
            .crate_version(0)
            .map_err(|e| e.to_string())?
            .clone();
        let mut outcome = RootsOutcome {
            crate_id: own.crate_id.clone(),
            version: own.version.clone(),
            crate_path: self.path.clone(),
            dir: dir.clone(),
            roots: vec![],
            removed: vec![],
            shared: vec![],
            hints,
            warnings,
            current_required: self.require_current,
        };
        if let Some(against) = &self.against {
            outcome
                .roots
                .push(self.against_root(against, loaded, &own, &moves, &definitions)?);
            return Ok(Outcome::Roots(Box::new(outcome)));
        }
        // what each root reaches, w0 roots and roots without a snapshot too, by path in the crate: two roots may
        // each reach a `Channel` of their own
        let mut items = BTreeMap::new();
        let mut reached: BTreeMap<String, Vec<(String, WireVersion)>> = BTreeMap::new();
        for (root, wire, bundle) in &loaded {
            let own = own_items(bundle, &modules[root], &own.crate_id)?;
            for path in own.values() {
                reached
                    .entry(path.clone())
                    .or_default()
                    .push((root.clone(), *wire));
            }
            items.insert(root.clone(), own);
        }
        let files = crate_files(&dir, &own.crate_id)?;
        let saved_at_current = files.iter().any(|f| f.version == key(&own.version));
        for (root, wire, bundle) in loaded {
            let all = root_snapshots(&dir, &own.crate_id, &root, wire.line.0)?;
            let one = self.root_outcome(
                &root,
                bundle,
                all,
                saved_at_current,
                &dir,
                &moves,
                &definitions,
            )?;
            outcome.roots.push(RootOutcome {
                root,
                wire,
                outcome: one,
            });
        }
        outcome.removed = removed_roots(&files, roots, &own.version);
        outcome.shared = shared_changes(&outcome.roots, &reached, &items)?;
        Ok(Outcome::Roots(Box::new(outcome)))
    }

    /// The root `against`, a root's snapshot file, is of, compared with it.
    fn against_root(
        &self,
        against: &Path,
        loaded: Vec<(String, WireVersion, ApiBundleOwned)>,
        own: &FullVersionOwned,
        moves: &[Move],
        definitions: &[&ApiBundleOwned],
    ) -> Result<RootOutcome, String> {
        let Some(name) = against
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(root_of_file_name)
        else {
            return Err(format!(
                "{} has several API roots: compare one of them with a snapshot of that root \
                 ({}_*.<module path>.<Trait>@w*.ron), {} is not one",
                self.path.display(),
                own.crate_id,
                against.display()
            ));
        };
        let Some((root, wire, bundle)) = loaded.into_iter().find(|(root, ..)| *root == name.root)
        else {
            return Err(format!(
                "{} is a snapshot of the API root `{}`, which `[package.metadata.wire_weaver.roots]` of {} doesn't \
                 list",
                against.display(),
                name.root,
                self.path.join("Cargo.toml").display()
            ));
        };
        let old = read(against)?;
        let report = compare_checked(
            &self.crate_snapshots_dir(),
            &old.bundle,
            &old.recorded,
            &bundle,
            &Recorded::default(),
            moves,
            definitions,
        )?;
        let outcome = Outcome::Compared(Box::new(Comparison {
            old_path: old.path,
            new_label: "source".to_string(),
            crate_path: Some(self.path.clone()),
            report,
            old: old.bundle,
            new: bundle,
            current_required: false,
            hints: vec![],
            design_line: wire.is_design_line(),
            warnings: vec![],
            unchanged_since: None,
            root: Some(root.clone()),
        }));
        Ok(RootOutcome {
            root,
            wire,
            outcome,
        })
    }

    /// One root from source (`source`) against `all` its snapshots ([root_snapshots]). `saved_at_current`: the
    /// crate's version has snapshots saved already, so a root whose latest snapshot is older was saved unchanged by
    /// it, and that snapshot stands for the crate's version.
    #[allow(clippy::too_many_arguments)]
    fn root_outcome(
        &self,
        root: &str,
        source: ApiBundleOwned,
        all: Vec<RootSnapshot>,
        saved_at_current: bool,
        dir: &Path,
        moves: &[Move],
        definitions: &[&ApiBundleOwned],
    ) -> Result<Outcome, String> {
        let own = source.crate_version(0).map_err(|e| e.to_string())?.clone();
        let version = own.version.clone();
        let crate_path = Some(self.path.clone());
        // w0, the design line (EVO-41): no snapshot is required, what one shows is information
        let design_line = source.wire.is_some_and(|w| w.is_design_line());
        let require_current = self.require_current && !design_line;
        let mut older: Vec<_> = all
            .into_iter()
            .filter(|s| key(&s.version) <= key(&version))
            .collect();
        older.sort_by_key(|s| (key(&s.version), (s.wire.line.0, s.wire.revision.0)));
        let Some(latest) = older.pop() else {
            let previous_line = match design_line {
                true => None,
                false => previous_line(dir, root, &source, moves, definitions)?,
            };
            return Ok(Outcome::NoSnapshot(Box::new(NoSnapshot {
                crate_id: own.crate_id,
                version,
                dir: dir.to_path_buf(),
                crate_path,
                strict: (self.strict || require_current) && !design_line,
                current_required: require_current,
                previous: None,
                hints: vec![],
                design_line,
                warnings: vec![],
                previous_line,
            })));
        };
        let at_current = key(&latest.version) == key(&version);
        let unchanged_since = (!at_current && saved_at_current).then(|| latest.version.clone());
        let mut old = latest.snapshot;
        if unchanged_since.is_some() {
            // compared as the snapshot of the crate's version, which kept it for this root
            old.bundle.ext_crates[0].version = version.clone();
        }
        let report = compare_checked(
            dir,
            &old.bundle,
            &old.recorded,
            &source,
            &Recorded::default(),
            moves,
            definitions,
        )?;
        let compared = |old: super::Snapshot,
                        new_label: String,
                        new: ApiBundleOwned,
                        report,
                        unchanged_since| {
            Outcome::Compared(Box::new(Comparison {
                old_path: old.path,
                new_label,
                crate_path: crate_path.clone(),
                report,
                old: old.bundle,
                new,
                current_required: require_current,
                hints: vec![],
                design_line,
                warnings: vec![],
                unchanged_since,
                root: Some(root.to_string()),
            }))
        };
        if !require_current {
            return Ok(compared(
                old,
                "source".into(),
                source,
                report,
                unchanged_since,
            ));
        }
        // as for a whole crate: the source against the snapshot of its version first, then that one against the
        // previous
        if at_current
            && report.change == Change::None
            && let Some(previous) = older.pop()
        {
            let report = compare_checked(
                dir,
                &previous.snapshot.bundle,
                &previous.snapshot.recorded,
                &old.bundle,
                &old.recorded,
                moves,
                definitions,
            )?;
            let label = old.path.display().to_string();
            return Ok(compared(previous.snapshot, label, old.bundle, report, None));
        }
        if at_current || unchanged_since.is_some() || report.change == Change::None {
            return Ok(compared(
                old,
                "source".into(),
                source,
                report,
                unchanged_since,
            ));
        }
        // changed since an older version's snapshot, and the crate's version has none yet
        Ok(Outcome::NoSnapshot(Box::new(NoSnapshot {
            crate_id: own.crate_id,
            version,
            dir: dir.to_path_buf(),
            crate_path,
            strict: true,
            current_required: true,
            previous: Some(old.path),
            hints: vec![],
            design_line: false,
            warnings: vec![],
            previous_line: None,
        })))
    }

    /// The removal of each root `roots` no longer lists and that isn't recorded yet, for [Check::save]: refused when
    /// the crate version isn't bumped enough, unless `force`.
    pub(super) fn plan_removed(
        &self,
        roots: &BTreeMap<String, WireVersion>,
        own: &FullVersionOwned,
        force: bool,
    ) -> Result<Vec<(Saved, Option<String>)>, String> {
        let dir = self.crate_snapshots_dir();
        let files = crate_files(&dir, &own.crate_id)?;
        let mut planned = vec![];
        for removed in removed_roots(&files, roots, &own.version) {
            if removed.recorded.is_some() {
                continue;
            }
            if let Some(min) = removed.required()
                && key(&own.version) < key(&min)
                && !force
            {
                return Err(format!(
                    "{}.\nNot saved: bump `version` in {} to {} or later (docs/evolution/rules.md#several-lines-in-one-crate) \
                     and save again.",
                    removed.needs(&own.crate_id),
                    self.path.join("Cargo.toml").display(),
                    triplet(&min)
                ));
            }
            let path = dir.join(removed_file_name(own, &removed.root, removed.wire));
            let last = removed
                .last
                .file_name()
                .map_or(String::new(), |n| n.to_string_lossy().to_string());
            let contents = format!(
                "// {} {}: API root {} removed, its last snapshot {last} at wire {}\n",
                own.crate_id,
                triplet(&own.version),
                removed.root,
                removed.wire
            );
            let saved = Saved {
                path,
                status: SaveStatus::Removed,
                previous: None,
                bundle: read(&removed.last)?.bundle,
                root: Some(removed.root),
                warnings: vec![],
            };
            planned.push((saved, Some(contents)));
        }
        Ok(planned)
    }
}

/// Roots with a snapshot in `files` (of one crate, [crate_files]) not newer than `version` that `roots` doesn't
/// list. [Check::run] and [Check::save] see them alike: the removal needs a breaking bump of the last crate version
/// that shipped the root, the newest one saved before `version` while the root had a snapshot; none when it only
/// ever had snapshots of `version`, which is being worked on. A record of the removal at or after the root's last
/// snapshot ([removed_file_name]) is the removal already checked and kept.
fn removed_roots(
    files: &[NamedFile],
    roots: &BTreeMap<String, WireVersion>,
    version: &VersionOwned,
) -> Vec<RemovedRoot> {
    let current = key(version);
    // the oldest and the newest snapshot of each root no longer listed
    struct Known<'a> {
        first: (u32, u32, u32),
        last: &'a NamedFile,
        wire: WireVersion,
    }
    let mut known: BTreeMap<&str, Known> = BTreeMap::new();
    for file in files {
        if let CrateFile::Root { root, wire } = &file.kind
            && !roots.contains_key(root)
            && file.version <= current
        {
            let entry = known.entry(root.as_str()).or_insert(Known {
                first: file.version,
                last: file,
                wire: *wire,
            });
            entry.first = entry.first.min(file.version);
            if entry.last.version <= file.version {
                (entry.last, entry.wire) = (file, *wire);
            }
        }
    }
    let previous = files
        .iter()
        .map(|f| f.version)
        .filter(|v| *v < current)
        .max();
    known
        .into_iter()
        .map(|(root, Known { first, last, wire })| {
            let recorded = files.iter().find_map(|f| match &f.kind {
                CrateFile::Removed { root: r, .. }
                    if r == root && f.version >= last.version && f.version <= current =>
                {
                    Some(f.path.clone())
                }
                _ => None,
            });
            RemovedRoot {
                root: root.to_string(),
                last: last.path.clone(),
                wire,
                since: previous
                    .filter(|p| *p >= first)
                    .map(|(major, minor, patch)| VersionOwned::new(major, minor, patch)),
                recorded,
            }
        })
        .collect()
}

/// The crate's own traits and types in `bundle`, its root trait aside (two lines' roots may share a name, never a
/// definition), by the name the checker's findings give them (`Channel`, `Code`, `geometry::Point` where two types
/// share a name), each with its path in the crate (`common::Channel`): what tells two roots' items apart. A trait's
/// module is in `trait_modules`, what the root's load noted ([wire_weaver_core::SourceNotes::trait_modules]); a type
/// carries its path in the model (API-46).
fn own_items(
    bundle: &ApiBundleOwned,
    trait_modules: &BTreeMap<(String, String), Vec<String>>,
    crate_id: &str,
) -> Result<BTreeMap<String, String>, String> {
    let root = bundle.root.items.first().map(|item| item.ident.as_str());
    let mut items = BTreeMap::new();
    for name in own_traits(bundle).into_keys() {
        if Some(name) == root {
            continue;
        }
        let modules = trait_modules
            .get(&(crate_id.to_string(), name.to_string()))
            .ok_or_else(|| {
                format!("internal: the load of {crate_id} noted no module for its trait {name}")
            })?;
        items.insert(name.to_string(), qualified_name(Some(modules), name));
    }
    for (key, (path, _)) in own_type_paths(bundle) {
        items.insert(key, path);
    }
    Ok(items)
}

/// The crate's own types and traits that more than one root reaches (`reached`, by every root, w0 roots and roots
/// without a snapshot too, by path in the crate) and that changed on a root compared with its snapshot. `items`:
/// each root's [own_items], to name what a finding is about by its path.
fn shared_changes(
    roots: &[RootOutcome],
    reached: &BTreeMap<String, Vec<(String, WireVersion)>>,
    items: &BTreeMap<String, BTreeMap<String, String>>,
) -> Result<Vec<SharedChange>, String> {
    let mut changed_on: BTreeMap<String, Vec<(String, WireVersion)>> = BTreeMap::new();
    for r in roots {
        let Outcome::Compared(c) = &r.outcome else {
            continue;
        };
        let changed: BTreeSet<String> = crate::diff(&c.old, &c.new)?
            .iter()
            .filter_map(|d| item_of(&items[&r.root], d.path()))
            .filter(|path| reached.get(*path).is_some_and(|by| by.len() > 1))
            .cloned()
            .collect();
        for name in changed {
            changed_on
                .entry(name)
                .or_default()
                .push((r.root.clone(), r.wire));
        }
    }
    Ok(changed_on
        .into_iter()
        .map(|(item, roots)| {
            let also = reached[&item]
                .iter()
                .filter(|by| !roots.contains(by))
                .cloned()
                .collect();
            SharedChange { item, roots, also }
        })
        .collect())
}

/// The path in the crate of the trait or type a [crate::Difference]'s path is in, from a root's [own_items]:
/// `common::Code` of `Code: field 0`, `common::Channel` of `Channel::reset` and `trait Channel`, `other::Point` of
/// `type other::Point`. `None` for what the root doesn't have (a type it no longer reaches).
fn item_of<'i>(items: &'i BTreeMap<String, String>, path: &str) -> Option<&'i String> {
    let path = path
        .strip_prefix("trait ")
        .or_else(|| path.strip_prefix("type "))
        .unwrap_or(path);
    items
        .iter()
        .filter(|(key, _)| {
            path.strip_prefix(key.as_str()).is_some_and(|rest| {
                rest.is_empty() || rest.starts_with(':') || rest.starts_with(' ')
            })
        })
        .max_by_key(|(key, _)| key.len())
        .map(|(_, path)| path)
}

impl RemovedRoot {
    /// The smallest crate version for the removal, `None` when it needs no bump.
    pub fn required(&self) -> Option<VersionOwned> {
        self.since.as_ref().map(|s| suggest(s, Bump::Breaking))
    }

    /// What the removal needs, for crate `crate_id`.
    fn needs(&self, crate_id: &str) -> String {
        let since = self.since.as_ref().map_or("?".to_string(), triplet);
        let min = self.required().map_or("?".to_string(), |v| triplet(&v));
        format!(
            "API root `{}` is removed (its last snapshot is {}, at wire {}): a line leaving the crate is a breaking \
             change, {crate_id} {since} -> {min} or later",
            self.root,
            self.last.display(),
            self.wire
        )
    }
}

impl RootsOutcome {
    /// The smallest crate version for every root's changes and every removal: the largest of what each needs. A
    /// root on w0, the design line (EVO-41), informs only and needs nothing. `None` when there is nothing to compare
    /// with.
    pub fn minimal_version(&self) -> Option<VersionOwned> {
        let roots = self.roots.iter().filter_map(|r| match &r.outcome {
            Outcome::Compared(c) if !c.design_line => Some(c.report.minimal_version()),
            _ => None,
        });
        let removed = self
            .removed
            .iter()
            .filter(|r| r.recorded.is_none())
            .filter_map(RemovedRoot::required);
        roots.chain(removed).max_by_key(key)
    }

    pub(super) fn verdict(&self) -> Result<String, String> {
        let mut errors = vec![];
        let mut ok = vec![];
        for r in &self.roots {
            match r.outcome.verdict() {
                Ok(line) => ok.push(format!("`{}`: {line}", r.root)),
                Err(e) => errors.push(format!("API root `{}` ({}): {e}", r.root, r.wire)),
            }
        }
        for removed in &self.removed {
            if let Some(record) = &removed.recorded {
                ok.push(format!(
                    "`{}`: removed, recorded in {}",
                    removed.root,
                    record.display()
                ));
                continue;
            }
            match removed.required() {
                Some(min) if key(&self.version) < key(&min) => {
                    let mut msg = format!("{}.", removed.needs(&self.crate_id));
                    if removed.since.as_ref().map(key) == Some(key(&self.version)) {
                        msg.push_str(&format!(
                            "\n{} {} has snapshots saved with the root still in it, so it is released and never \
                             changes.",
                            self.crate_id,
                            triplet(&self.version)
                        ));
                    }
                    msg.push_str(&format!(
                        "\nBump `version` in {} to {} or later (docs/evolution/rules.md#several-lines-in-one-crate) \
                         and record the removal with {}.",
                        self.crate_path.join("Cargo.toml").display(),
                        triplet(&min),
                        save_command(Some(&self.crate_path))
                    ));
                    errors.push(msg);
                }
                _ if self.current_required => errors.push(format!(
                    "API root `{}` is removed, and the removal is not recorded: record it with {}.",
                    removed.root,
                    save_command(Some(&self.crate_path))
                )),
                _ => ok.push(format!(
                    "`{}`: removed, version is bumped enough",
                    removed.root
                )),
            }
        }
        match errors.is_empty() {
            true => Ok(ok.join("; ")),
            false => Err(errors.join("\n")),
        }
    }

    /// Whether any root not on w0 changed, or one is removed and not recorded yet.
    pub fn changed(&self) -> bool {
        self.roots.iter().any(|r| match &r.outcome {
            Outcome::Compared(c) => c.report.change != Change::None && !c.design_line,
            _ => false,
        }) || self.removed.iter().any(|r| r.recorded.is_none())
    }
}

/// Each root's report under its name and wire version, then the removed roots, the shared items that changed and the
/// crate version all of it needs.
impl fmt::Display for RootsOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {}: {} API roots, each against its own latest snapshot in {}",
            self.crate_id,
            triplet(&self.version),
            self.roots.len(),
            self.dir.display()
        )?;
        for r in &self.roots {
            write!(f, "\nAPI root `{}` ({}): {}", r.root, r.wire, r.outcome)?;
        }
        for removed in &self.removed {
            match &removed.recorded {
                Some(record) => write!(
                    f,
                    "\nAPI root `{}` removed, recorded in {}",
                    removed.root,
                    record.display()
                )?,
                None => write!(f, "\n{}", removed.needs(&self.crate_id))?,
            }
        }
        for shared in &self.shared {
            write!(f, "\n{shared}")?;
        }
        if self.changed()
            && let Some(min) = self.minimal_version()
        {
            write!(
                f,
                "\ncrate: {} or later, the largest its roots need",
                triplet(&min)
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

/// `shared `common::Code` changed on `wire_v1::Api` (w1.0), `wire_v2::Api` (w2.0)`.
impl fmt::Display for SharedChange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let roots: Vec<_> = self
            .roots
            .iter()
            .map(|(root, wire)| format!("`{root}` ({wire})"))
            .collect();
        write!(f, "shared `{}` changed on {}", self.item, roots.join(", "))?;
        if !self.also.is_empty() {
            let also: Vec<_> = self
                .also
                .iter()
                .map(|(root, wire)| format!("`{root}` ({wire})"))
                .collect();
            write!(f, ", also reached by {}", also.join(", "))?;
        }
        Ok(())
    }
}

/// A root that begins a new line (no snapshot on its own line, `source`) against the latest snapshot of the root of
/// the same trait name on the highest line below, for the moves across that break (EVO-40): what its `#[moved_from]`
/// (`moves`) names is checked, and resources of the same name elsewhere are proposed. Information besides a
/// [NoSnapshot]: a new line has nothing to keep compatible with. `None` without such a snapshot.
fn previous_line(
    dir: &Path,
    root: &str,
    source: &ApiBundleOwned,
    moves: &[Move],
    definitions: &[&ApiBundleOwned],
) -> Result<Option<(PathBuf, Box<crate::Report>)>, String> {
    let (Some(wire), Ok(own)) = (source.wire, source.crate_version(0)) else {
        return Ok(None);
    };
    let trait_name = |root: &str| root.rsplit("::").next().map(str::to_string);
    let files = crate_files(dir, &own.crate_id)?;
    let latest = files
        .iter()
        .filter_map(|file| match &file.kind {
            CrateFile::Root { root: r, wire: w }
                if w.line.0 < wire.line.0
                    && !w.is_design_line()
                    && trait_name(r) == trait_name(root)
                    && file.version <= key(&own.version) =>
            {
                Some(((w.line.0, file.version, w.revision.0), file))
            }
            _ => None,
        })
        .max_by_key(|(order, _)| *order);
    let Some((_, file)) = latest else {
        return Ok(None);
    };
    let old = read(&file.path)?;
    let report = compare_checked(
        dir,
        &old.bundle,
        &old.recorded,
        source,
        &Recorded::default(),
        moves,
        definitions,
    )?;
    Ok(Some((file.path.clone(), Box::new(report))))
}
