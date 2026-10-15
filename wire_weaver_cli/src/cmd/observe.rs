//! `ww observe <path> [--rate ..]` (TOOL-11): observes a trait level's `+observe` properties without generated
//! code, printing each update's present values, the update rate and the link bytes/s it costs.

use anyhow::{Context, Result, anyhow, bail};
use clap::Args;
use console::style;
use human_repr::HumanCount;
use std::time::Instant;
use wire_weaver::shrink_wrap::UNib32;
use wire_weaver_client::observe::ObserveMode;
use wire_weaver_client::{DynClient, DynResource};

#[derive(Args)]
pub(crate) struct ObserveArgs {
    /// Path to a trait level, e.g. "motor" or "bank[0]"; empty observes the API root
    #[arg(default_value = "")]
    path: String,

    /// Observe every property of the level periodically at this rate instead of on every change
    #[arg(long, value_name = "MS")]
    rate: Option<u64>,
}

pub(crate) async fn observe(args: ObserveArgs, device: &DynClient) -> Result<()> {
    let bundle = device
        .device_introspect()
        .ok_or_else(|| anyhow!("device did not provide introspection data"))?
        .api_bundle
        .clone();
    let level = resolve(DynResource::root(bundle), &args.path)?;
    let props = level
        .observable_properties()
        .with_context(|| format!("'{}'", display_path(&args.path)))?;
    if props.is_empty() {
        bail!(
            "'{}' has no `+observe` properties",
            display_path(&args.path)
        );
    }
    println!(
        "{} {} ({}): {}",
        style("observing").dim(),
        display_path(&args.path),
        if props.len() == 1 {
            "1 property".to_string()
        } else {
            format!("{} properties", props.len())
        },
        props
            .iter()
            .map(|(_, name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join(", "),
    );
    let mode = match args.rate {
        Some(ms) => ObserveMode::Periodic {
            every_us: UNib32((ms.saturating_mul(1000)).min(u32::MAX as u64) as u32),
        },
        None => ObserveMode::OnChange { min_us: UNib32(0) },
    };
    let entries = props.iter().map(|(index, ..)| (*index, mode)).collect();
    let mut observer = level
        .observe(device.cmd(), entries)
        .await
        .context("observe request")?;
    if let Some(granted) = observer.granted() {
        println!(
            "{} {}/s",
            style("budget:").dim(),
            granted.budget.human_count_bytes()
        );
    }
    println!("{}", style("Ctrl+C to stop").dim());

    let start = Instant::now();
    let mut count: u64 = 0;
    let mut bytes: u64 = 0;
    loop {
        let update = observer.recv().await.context("receiving update")?;
        count += 1;
        bytes += update.data_len as u64;
        let elapsed = start.elapsed().as_secs_f64().max(f64::EPSILON);
        let values = update
            .values
            .iter()
            .map(|(name, value)| format!("{name}: {}", crate::value_fmt::fmt_value(value)))
            .collect::<Vec<_>>()
            .join(", ");
        let missed = if update.missed > 0 {
            format!("  {} {}", style("missed").red(), update.missed)
        } else {
            String::new()
        };
        println!(
            "{{{values}}}  {} {:.1}/s  {} {}/s{missed}",
            style("rate").dim(),
            count as f64 / elapsed,
            style("link").dim(),
            (bytes as f64 / elapsed) as u64,
        );
    }
}

/// `path`, or the API root when empty.
fn display_path(path: &str) -> &str {
    if path.is_empty() { "API root" } else { path }
}

/// Walk `path` (e.g. `motor` or `bank[0].level`) from `root` by names and indices, the same syntax
/// `DynResource::path()` prints.
fn resolve(root: DynResource, path: &str) -> Result<DynResource> {
    let mut res = root;
    if path.is_empty() {
        return Ok(res);
    }
    for segment in path.split('.') {
        let name_end = segment.find('[').unwrap_or(segment.len());
        let (name, mut rest) = (&segment[..name_end], &segment[name_end..]);
        if !name.is_empty() {
            res = res.child(name)?;
        }
        while let Some(after_bracket) = rest.strip_prefix('[') {
            let close = after_bracket
                .find(']')
                .ok_or_else(|| anyhow!("'{segment}': missing ']'"))?;
            let index: u32 = after_bracket[..close]
                .parse()
                .with_context(|| format!("'{segment}': expected an integer index"))?;
            res = res.index(index)?;
            rest = &after_bracket[close + 1..];
        }
        if !rest.is_empty() {
            bail!("'{segment}': unexpected text after the index");
        }
    }
    Ok(res)
}
