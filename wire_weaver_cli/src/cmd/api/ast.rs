use anyhow::Result;
use std::path::PathBuf;
use wire_weaver_core::load_v2;

pub(crate) fn print_ast(crate_path: PathBuf, trait_name: Option<String>) -> Result<()> {
    let api_bundle = load_v2(&crate_path, trait_name, false)?;
    let ron = ron::ser::to_string_pretty(
        &api_bundle,
        ron::ser::PrettyConfig::default().compact_structs(true),
    )?;
    println!("{}", ron);
    Ok(())
}
