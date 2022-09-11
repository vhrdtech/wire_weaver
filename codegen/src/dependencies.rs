use semver::VersionReq;
use std::fmt::Write;

pub trait Depends {
    fn dependencies(&self) -> Dependencies;
}

pub struct Dependencies {
    /// Creates/modules/packages to depend on
    pub depends: Vec<Package>,
    /// Things to use/include/import from crates
    pub uses: Vec<Import>,
}

#[derive(Hash, Eq, PartialEq, Clone, Debug)]
pub enum Package {
    RustCrate(RustCrateSource, VersionReq),
}

#[derive(Hash, Eq, PartialEq, Clone, Debug)]
pub enum RustCrateSource {
    Crates(String),
    Github(String),
    Path(String),
}

#[derive(Clone, Eq, PartialEq)]
pub enum Import {
    Entity(&'static str),
    EntityAs(&'static str, &'static str),
    Submodule(&'static str, Vec<Import>),
}

impl Import {
    pub fn render(&self /*language: */) -> String {
        let mut s = String::new();
        match self {
            Import::Entity(e) => {
                write!(s, "use {};", e).unwrap();
            }
            Import::EntityAs(e, r#as) => {
                write!(s, "use {} as {};", e, r#as).unwrap();
            }
            Import::Submodule(r#mod, e) => {
                write!(s, "use {}::{{", r#mod).unwrap();
                for (i, import) in e.iter().enumerate() {
                    write!(s, "{}", import.render_internal()).unwrap();
                    if i < e.len() - 1 {
                        write!(s, ", ").unwrap();
                    }
                }
                write!(s, "}};").unwrap();
            }
        }
        s
    }

    fn render_internal(&self) -> String {
        let mut s = String::new();
        match self {
            Import::Entity(e) => {
                write!(s, "{}", e).unwrap();
            }
            Import::EntityAs(e, r#as) => {
                write!(s, "{} as {}", e, r#as).unwrap();
            }
            Import::Submodule(r#mod, e) => {
                write!(s, "{}::{{", r#mod).unwrap();
                for (i, import) in e.iter().enumerate() {
                    write!(s, "{}", import.render_internal()).unwrap();
                    if i < e.len() - 1 {
                        write!(s, ", ").unwrap();
                    }
                }
                write!(s, "}}").unwrap();
            }
        }
        s
    }
}

pub struct ImportMerger {
    uses: Vec<Import>,
}

impl ImportMerger {
    pub fn new() -> Self {
        Self { uses: Vec::new() }
    }

    pub fn merge(&mut self, other: &Vec<Import>) {
        for import in other {
            Self::merge_one(&mut self.uses, import)
        }
    }

    fn merge_one(to: &mut Vec<Import>, import: &Import) {
        match import {
            i @ Import::Entity(_) | i @ Import::EntityAs(_, _) => {
                if !to.contains(i) {
                    to.push(i.clone());
                }
            }
            Import::Submodule(name_merging, entities_merging) => {
                let mut found = false;
                for i in to.iter_mut() {
                    if let Import::Submodule(name_exists, entities) = i {
                        if name_merging == name_exists {
                            for e in entities_merging {
                                Self::merge_one(entities, e);
                            }
                            found = true;
                            break;
                        }
                    }
                }
                if !found {
                    to.push(Import::Submodule(name_merging, entities_merging.clone()))
                }
            }
        }
    }

    pub fn render(&self /*language: Language*/) -> String {
        let mut s = String::new();
        for i in &self.uses {
            write!(s, "{}", i.render()).unwrap();
        }
        s
    }
}
