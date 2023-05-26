use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    pub info: Info,
    pub main: Main,
    pub dependencies: Dependencies,
    pub gen: GenerateTargets,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Info {
    pub name: String,
    pub version: String,
    pub authors: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Main {
    pub src: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Dependencies {

}

#[derive(Serialize, Deserialize, Debug)]
pub struct GenerateTargets {
    pub rust: TargetRust,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TargetRust {
    pub core: TargetRustCore,
    pub xpi: TargetRustXpi
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TargetRustCore {
    #[serde(rename = "crate")]
    pub target_crate: String,
    #[serde(rename = "derive")]
    pub add_derives: Vec<String>,
    pub serdes: Vec<RustSerDes>,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum RustSerDes {
    #[serde(rename = "wfd")]
    Wfd,
    #[serde(rename = "wfs")]
    Wfs,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TargetRustXpi {
    #[serde(rename = "crate")]
    pub target_crate: String,
    pub client: bool,
    pub server: bool,
    pub flavor: RustXpiFlavor,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum RustXpiFlavor {
    #[serde(rename = "nostd_sync")]
    NostdSync,
}
