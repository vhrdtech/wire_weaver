use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use toml::Value;

#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    pub info: Info,
    pub dependencies: Option<Dependencies>,
    pub gen: Option<GenerateTargets>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Info {
    pub name: String,
    pub version: String,
    pub authors: Vec<String>,
    pub src: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Dependencies {
    #[serde(flatten)]
    deps: HashMap<String, Value>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GenerateTargets {
    pub rust: Option<TargetRust>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TargetRust {
    pub core: Vec<TargetRustCore>,
    pub xpi: Vec<TargetRustXpi>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TargetRustCore {
    #[serde(rename = "crate")]
    pub target_crate: String,
    #[serde(rename = "derive")]
    pub add_derives: Option<Vec<String>>,
    pub serdes: Option<Vec<RustSerDes>>,
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
    #[serde(rename = "async")]
    Async,
}
