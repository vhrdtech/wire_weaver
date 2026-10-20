//! Checks of `wire_weaver_gid.json` that need nothing but this crate: unique ids and names, reserved ranges, and
//! that no id ever assigned changes or vanishes (`assigned_ids.lock`, append-only).

use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use ww_global::{ENTRIES, EntryKind, NO_ID, RESERVED};

const LOCK: &str = "assigned_ids.lock";

#[test]
fn ids_and_names_are_unique() {
    let mut ids = HashSet::new();
    let mut names = HashMap::new();
    for e in ENTRIES {
        assert!(ids.insert(e.id), "id {} is assigned twice", e.id);
        for name in core::iter::once(&e.name).chain(e.aliases) {
            if let Some(other) = names.insert((e.kind, *name), e.id) {
                panic!(
                    "`{name}` is the name or alias of both id {other} and id {}",
                    e.id
                );
            }
        }
        match e.kind {
            EntryKind::Crate => {
                assert!(!e.name.contains("::"), "crate entry {}: `{}`", e.id, e.name)
            }
            EntryKind::Trait => {
                let (_, trait_name) = e.split();
                assert!(
                    trait_name.is_some_and(|t| !t.is_empty() && !t.contains("::")),
                    "trait entry {} is not `crate::Trait`: `{}`",
                    e.id,
                    e.name
                );
            }
        }
        assert!(
            !NO_ID.contains(&e.name),
            "`{}` has id {} and is in no_id",
            e.name,
            e.id
        );
    }
}

#[test]
fn reserved_ranges_stay_free() {
    for e in ENTRIES {
        for (start, end, reason) in RESERVED {
            assert!(
                !(*start..=*end).contains(&e.id),
                "`{}` takes id {}, reserved ({start}..={end}): {reason}",
                e.name,
                e.id
            );
        }
    }
}

fn kind_str(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Crate => "crate",
        EntryKind::Trait => "trait",
    }
}

/// Every (id, name) ever assigned is in the lock: it must stay in the registry with the same id (retired is fine,
/// a rename keeps the old name as an alias), and a new id goes into the lock with `just gid-lock`.
#[test]
fn assigned_ids() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(LOCK);
    let lock = std::fs::read_to_string(&path).unwrap();
    let mut locked = HashSet::new();
    let mut errors = String::new();
    for line in lock
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
    {
        let mut parts = line.split_whitespace();
        let (Some(id), Some(kind), Some(name), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            panic!("{LOCK}: bad line `{line}`, expected `<id> <crate|trait> <name>`");
        };
        let id: u32 = id
            .parse()
            .unwrap_or_else(|_| panic!("{LOCK}: bad id in `{line}`"));
        locked.insert(id);
        match ENTRIES.iter().find(|e| e.id == id) {
            None => writeln!(errors, "id {id} ({kind} {name}) vanished from wire_weaver_gid.json: ids are never removed, mark it `retired`").unwrap(),
            Some(e) if kind_str(e.kind) != kind || (e.name != name && !e.aliases.contains(&name)) => writeln!(
                errors,
                "id {id} was {kind} `{name}`, the registry has {} `{}` (a rename keeps the old name in `aliases`)",
                kind_str(e.kind),
                e.name
            )
            .unwrap(),
            Some(_) => {}
        }
        if let Some(e) = ENTRIES.iter().find(|e| {
            e.id != id && (e.name == name || e.aliases.contains(&name)) && kind_str(e.kind) == kind
        }) {
            writeln!(
                errors,
                "{kind} `{name}` had id {id}, the registry gives it id {}: ids never change",
                e.id
            )
            .unwrap();
        }
    }
    let new: Vec<_> = ENTRIES.iter().filter(|e| !locked.contains(&e.id)).collect();
    if !new.is_empty() && std::env::var("GID_LOCK").as_deref() == Ok("update") && errors.is_empty()
    {
        let mut lock = lock;
        for e in &new {
            writeln!(lock, "{} {} {}", e.id, kind_str(e.kind), e.name).unwrap();
        }
        std::fs::write(&path, lock).unwrap();
    } else {
        for e in new {
            writeln!(
                errors,
                "id {} ({} {}) is not in {LOCK}: run `just gid-lock` to append it",
                e.id,
                kind_str(e.kind),
                e.name
            )
            .unwrap();
        }
    }
    assert!(
        errors.is_empty(),
        "{LOCK} and wire_weaver_gid.json disagree:\n{errors}"
    );
}
