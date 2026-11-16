//! Live completion values of the standard library commands.

use clap_complete::CompletionCandidate;
use std::collections::BTreeMap;

/// Point names of the device `ww pins` would open (selection from env variables and ww.toml, flags aren't seen
/// here), with `group.*` for each group; empty when there's no single PinDiag to ask.
pub fn pin_points() -> Vec<CompletionCandidate> {
    let Ok(rt) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return vec![];
    };
    let points = rt.block_on(async {
        let selection = wire_weaver_cli::registry::selection_from_env().ok()?;
        let connect =
            wire_weaver_client::DynClient::from_config(selection.client_config()).connect();
        let device = tokio::time::timeout(std::time::Duration::from_secs(3), connect)
            .await
            .ok()?
            .ok()?;
        let mut pins = ww_pin_probe::device::DevicePins::find(&device, None).ok()?;
        let raw = pins.raw_points().await.ok();
        let _ = device.disconnect().asynch().await;
        Some(ww_pin_probe::device::Names::default().points(&raw?))
    });
    let points = points.unwrap_or_default();
    let mut out: Vec<CompletionCandidate> = points
        .iter()
        .map(|p| {
            let help = format!("P{}, {}", p.id, p.group);
            CompletionCandidate::new(&p.name).help(Some(help.into()))
        })
        .collect();
    let mut groups: BTreeMap<&str, usize> = BTreeMap::new();
    for p in &points {
        *groups.entry(&p.group).or_default() += 1;
    }
    for (group, n) in groups {
        let help = format!("all {n} points of {group}");
        out.push(CompletionCandidate::new(format!("{group}.*")).help(Some(help.into())));
    }
    out
}
