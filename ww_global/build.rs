//! Turns `wire_weaver_gid.json` into Rust: a const per crate entry and the table of all entries, written into
//! `OUT_DIR/gid.rs` and included by `src/lib.rs`.

use convert_case::{Case, Casing};
use serde_json::Value;
use std::fmt::Write;
use std::path::PathBuf;
use std::{env, fs};

fn main() {
    println!("cargo:rerun-if-changed=wire_weaver_gid.json");
    let json = fs::read_to_string("wire_weaver_gid.json").unwrap();
    let json: Value = serde_json::from_str(&json).expect("wire_weaver_gid.json is not valid JSON");

    let mut out = String::new();
    let mut entries = String::new();
    for entry in json["ids"].as_array().expect("`ids` list") {
        let id = entry["id"].as_u64().expect("`id` number") as u32;
        let (kind, name) = match (entry["crate"].as_str(), entry["trait"].as_str()) {
            (Some(name), None) => ("Crate", name),
            (None, Some(name)) => ("Trait", name),
            _ => panic!("id {id}: an entry has exactly one of `crate` or `trait`"),
        };
        if kind == "Crate" {
            let constant = name.to_case(Case::Constant);
            writeln!(out, "/// Global id of the `{name}` crate.").unwrap();
            writeln!(
                out,
                "pub const {constant}: GlobalTypeId = GlobalTypeId::new({id});"
            )
            .unwrap();
        }
        let aliases = entry["aliases"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|a| a.as_str().expect("alias string"))
                    .collect()
            })
            .unwrap_or_else(Vec::new);
        let retired = match entry["retired"].as_str() {
            Some(reason) => format!("Some({reason:?})"),
            None => "None".into(),
        };
        writeln!(
            entries,
            "    Entry {{ id: {id}, kind: EntryKind::{kind}, name: {name:?}, aliases: &{aliases:?}, retired: {retired} }},"
        )
        .unwrap();
    }
    writeln!(out, "\n/// Every entry of the registry, in file order.").unwrap();
    writeln!(out, "pub static ENTRIES: &[Entry] = &[\n{entries}];").unwrap();

    let mut reserved = String::new();
    for range in json["reserved"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[])
    {
        let ids = range["ids"].as_array().expect("`ids` pair");
        let (start, end) = (ids[0].as_u64().unwrap(), ids[1].as_u64().unwrap());
        let reason = range["reason"].as_str().expect("`reason` string");
        writeln!(reserved, "    ({start}, {end}, {reason:?}),").unwrap();
    }
    writeln!(
        out,
        "\n/// Id ranges kept free (first, last, reason).\npub static RESERVED: &[(u32, u32, &str)] = &[\n{reserved}];"
    )
    .unwrap();

    let no_id: Vec<&str> = json["no_id"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|a| a.as_str().expect("no_id string"))
                .collect()
        })
        .unwrap_or_default();
    writeln!(
        out,
        "\n/// Public traits deliberately left without an id (`crate::Trait`).\npub static NO_ID: &[&str] = &{no_id:?};"
    )
    .unwrap();

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap()).join("gid.rs");
    fs::write(out_path, out).unwrap();
}
