use super::check::print_report;
use anyhow::{Context, Result, anyhow};
use console::style;
use std::fs;
use std::path::PathBuf;
use wire_weaver_client::ww_self::snapshot;
use wire_weaver_evolution::snapshot_files::{SNAPSHOTS_DIR, latest_snapshot, triplet};
use wire_weaver_evolution::{self as evolution, Change};

pub(crate) fn save(crate_path: PathBuf, force: bool) -> Result<()> {
    let bundle = wire_weaver_core::load_crate(&crate_path)?;
    let version = bundle.crate_version(0)?;
    let dir = crate_path.join(SNAPSHOTS_DIR);
    let file_path = dir.join(format!("{}.ron", version.filename_friendly()));
    let crate_version = format!("{} {}", version.crate_id, triplet(&version.version));
    // with the hash of the shrink_wrap bytes the RON stands for: the build of wire_weaver_snapshots re-encodes the RON
    // and fails when it differs, which proves the round trip lossless
    let contents = wire_weaver_snapshots::render_ron(&bundle).map_err(|e| anyhow!(e))?;
    let hash = snapshot::saved_hash(&contents).map(str::to_owned);

    // compared with the snapshot of this version if there is one, otherwise with the previous version
    let status = match latest_snapshot(&dir, &bundle, &version.version, true)
        .map_err(|e| anyhow!(e))?
    {
        Some((old_path, old)) => {
            let report = evolution::compare(&old, &bundle).map_err(|e| anyhow!(e))?;
            let same_version = old_path == file_path;
            // the same definitions in another ww_self format, or a file without (or with another) hash, are saved again
            let hash_saved = same_version
                && fs::read_to_string(&file_path)
                    .is_ok_and(|text| snapshot::saved_hash(&text) == hash.as_deref());
            if same_version
                && report.change == Change::None
                && old.ww_self_version == bundle.ww_self_version
                && hash_saved
            {
                println!("{} is up to date", file_path.display());
                return Ok(());
            }
            if report.change != Change::None {
                print_report(&report, &old_path, "source");
            }
            match report.check_version() {
                Ok(()) if same_version && report.change == Change::None => "reformatted",
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
