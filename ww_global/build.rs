use convert_case::{Case, Casing};
use std::collections::HashMap;
use std::io::Write;

fn main() {
    let ids = std::fs::read_to_string("wire_weaver_gid.json").unwrap();
    let ids: HashMap<String, u32> = serde_json::from_str(&ids).unwrap();
    let mut ids: Vec<_> = ids.into_iter().collect();
    ids.sort_unstable_by(|a, b| a.1.cmp(&b.1));
    let mut wr = Vec::new();
    write!(&mut wr, "#![no_std]\n\n").unwrap();
    write!(&mut wr, "use shrink_wrap::UNib32;\n\n").unwrap();
    for (crate_name, gid) in ids {
        let crate_name = crate_name.to_case(Case::Constant);
        writeln!(&mut wr, "pub const {crate_name}: UNib32 = UNib32({gid});").unwrap();
    }
    std::fs::write("src/lib.rs", wr).unwrap();
}
