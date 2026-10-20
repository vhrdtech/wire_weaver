use crate::{ENTRIES, GlobalTypeId};

/// What a registry id stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntryKind {
    /// A whole crate, `name` is the crate name.
    Crate,
    /// One trait, `name` is `crate::Trait`.
    Trait,
}

/// One entry of the registry (`wire_weaver_gid.json`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub id: u32,
    pub kind: EntryKind,
    /// Crate name, or `crate::Trait` for a trait.
    pub name: &'static str,
    /// Earlier names of the same crate or trait (after a rename or a move to another crate), found like `name`.
    pub aliases: &'static [&'static str],
    /// `Some(reason)` when the crate or trait is gone: the id stays taken and is never matched or reused.
    pub retired: Option<&'static str>,
}

impl Entry {
    /// Crate name and trait name (`None` for a crate entry).
    pub fn split(&self) -> (&'static str, Option<&'static str>) {
        split(self.name)
    }

    fn is(&self, kind: EntryKind, name_matches: impl Fn(&str) -> bool) -> bool {
        self.kind == kind
            && self.retired.is_none()
            && (name_matches(self.name) || self.aliases.iter().any(|a| name_matches(a)))
    }
}

fn split(name: &str) -> (&str, Option<&str>) {
    match name.split_once("::") {
        Some((crate_name, trait_name)) => (crate_name, Some(trait_name)),
        None => (name, None),
    }
}

/// Id of the trait `trait_name` in crate `crate_name`, found by its name or one of its aliases.
pub fn trait_id(crate_name: &str, trait_name: &str) -> Option<GlobalTypeId> {
    ENTRIES
        .iter()
        .find(|e| {
            e.is(EntryKind::Trait, |n| {
                split(n) == (crate_name, Some(trait_name))
            })
        })
        .map(|e| GlobalTypeId::new(e.id))
}

/// Id of the crate `crate_name`, found by its name or one of its aliases.
pub fn crate_id(crate_name: &str) -> Option<GlobalTypeId> {
    ENTRIES
        .iter()
        .find(|e| e.is(EntryKind::Crate, |n| n == crate_name))
        .map(|e| GlobalTypeId::new(e.id))
}

/// The entry with this id, retired ones included.
pub fn entry(id: u32) -> Option<&'static Entry> {
    ENTRIES.iter().find(|e| e.id == id)
}
