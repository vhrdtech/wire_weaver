use anyhow::{Context, Result, anyhow};
use console::style;
use ron::ser::{PrettyConfig, to_string_pretty};
use shrink_wrap::SerializeShrinkWrapOwned;
use std::fs;
use std::path::{Path, PathBuf};
use wire_weaver_client::ww_self::ApiBundleOwned;
use wire_weaver_client::ww_self::visit_mut::VisitMut;

/// Directory inside a crate where its snapshots are kept.
const SNAPSHOTS_DIR: &str = "api_snapshots";

pub(crate) fn save(crate_path: PathBuf, force: bool) -> Result<()> {
    let bundle = wire_weaver_core::load_crate(&crate_path)?;
    let version = bundle.crate_version(0)?;
    let dir = crate_path.join(SNAPSHOTS_DIR);
    let file_path = dir.join(format!("{}.ron", version.filename_friendly()));
    let v = &version.version;
    let crate_version = format!(
        "{} {}.{}.{}",
        version.crate_id, v.major.0, v.minor.0, v.patch.0
    );
    let header = format!(
        "// {crate_version} traits and types, saved by `ww api save`.\n// Do not edit, bump the crate version and save a new snapshot instead.\n"
    );
    let contents = header + &to_string_pretty(&bundle, PrettyConfig::new().compact_structs(true))?;

    let status = match fs::read_to_string(&file_path) {
        Ok(existing) => match compare(&existing, &bundle, &file_path)? {
            Diff::Same => {
                println!("{} is up to date", file_path.display());
                return Ok(());
            }
            Diff::DocsOnly | Diff::Changed if force => "overwritten",
            Diff::DocsOnly => {
                return Err(anyhow!(
                    "{crate_version} doc comments changed since {} was saved.\n\
                    Bump the crate's compatible version position (patch before 1.0, minor after) and save a new snapshot, \
                    so that newer docs can be told apart, or pass --force if that version was never published.",
                    file_path.display()
                ));
            }
            Diff::Changed => {
                return Err(anyhow!(
                    "{crate_version} traits or types changed since {} was saved.\n\
                    Bump the crate version (see docs/evolution/rules.md) and save a new snapshot, \
                    or pass --force if that version was never published.",
                    file_path.display()
                ));
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "saved",
        Err(e) => return Err(e).context(format!("reading {}", file_path.display())),
    };
    fs::create_dir_all(&dir)?;
    fs::write(&file_path, contents).context(format!("writing {}", file_path.display()))?;
    println!(
        "{} {}, {}",
        style(status).green(),
        file_path.display(),
        crate::api_tree::summary(&bundle)
    );
    Ok(())
}

enum Diff {
    Same,
    DocsOnly,
    Changed,
}

fn compare(existing: &str, bundle: &ApiBundleOwned, file_path: &Path) -> Result<Diff> {
    let mut existing: ApiBundleOwned =
        ron::from_str(existing).context(format!("parsing {}", file_path.display()))?;
    let mut bundle = bundle.clone();
    if existing.to_ww_bytes_owned()? == bundle.to_ww_bytes_owned()? {
        return Ok(Diff::Same);
    }
    DropDocs.visit_api_bundle(&mut existing);
    DropDocs.visit_api_bundle(&mut bundle);
    if existing.to_ww_bytes_owned()? == bundle.to_ww_bytes_owned()? {
        Ok(Diff::DocsOnly)
    } else {
        Ok(Diff::Changed)
    }
}

struct DropDocs;

impl VisitMut for DropDocs {
    fn visit_docs(&mut self, docs: &mut Vec<String>) {
        docs.clear();
    }
}
