use semver::VersionReq;

pub trait Depends {
    fn dependencies(&self) -> Dependencies;
}

pub struct Dependencies {
    /// Creates/modules/packages to depend on
    pub depends: Vec<Package>,
    /// Things to use/include/import from crates
    pub uses: Vec<Import>
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

pub enum Import {
    Entity(&'static str),
    EntityAs(&'static str, &'static str),
    Entities(Vec<Import>),
    Submodule(&'static str, Box<Import>)
}