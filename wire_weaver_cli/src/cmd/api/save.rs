use super::check::{SNAPSHOTS_DIR, latest_snapshot, print_report, triplet};
use anyhow::{Context, Result, anyhow};
use console::style;
use ron::ser::{PrettyConfig, to_string_pretty};
use std::fs;
use std::path::PathBuf;
use wire_weaver_client::evolution::{self, Change};

pub(crate) fn save(crate_path: PathBuf, force: bool) -> Result<()> {
    let bundle = wire_weaver_core::load_crate(&crate_path)?;
    let version = bundle.crate_version(0)?;
    let dir = crate_path.join(SNAPSHOTS_DIR);
    let file_path = dir.join(format!("{}.ron", version.filename_friendly()));
    let crate_version = format!("{} {}", version.crate_id, triplet(&version.version));
    let header = format!(
        "// {crate_version} traits and types, saved by `ww api save`.\n// Do not edit, bump the crate version and save a new snapshot instead.\n"
    );
    let contents = header + &to_string_pretty(&bundle, PrettyConfig::new().compact_structs(true))?;

    // compared with the snapshot of this version if there is one, otherwise with the previous version
    let status = match latest_snapshot(&dir, &bundle, &version.version, true)? {
        Some((old_path, old)) => {
            let report = evolution::compare(&old, &bundle).map_err(|e| anyhow!(e))?;
            let same_version = old_path == file_path;
            if same_version && report.change == Change::None {
                println!("{} is up to date", file_path.display());
                return Ok(());
            }
            if report.change != Change::None {
                print_report(&report, &old_path, "source");
            }
            match report.check_version() {
                Ok(()) => "saved",
                Err(_) if force && same_version => "overwritten",
                Err(_) if force => "saved",
                Err(e) if same_version => {
                    return Err(anyhow!(
                        "{e}.\nBump the crate version (see docs/evolution/rules.md) and save a new snapshot, \
                        or pass --force if {crate_version} was never published."
                    ));
                }
                Err(e) => {
                    return Err(anyhow!(
                        "{e}.\nBump the crate version (see docs/evolution/rules.md), or pass --force to save anyway."
                    ));
                }
            }
        }
        None => "saved",
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
