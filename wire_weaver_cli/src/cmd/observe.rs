//! `ww observe <path> [--rate ..]` (TOOL-11): observes a trait level's `+observe` properties without generated
//! code, printing each update's present values, the update rate and the link bytes/s it costs. With `--samples`,
//! `+batched` properties come sample by sample, one line each with its device time (API-39).

use crate::util::{display_path, resolve};
use anyhow::{Context, Result, anyhow, bail};
use clap::Args;
use console::style;
use human_repr::HumanCount;
use std::collections::HashMap;
use std::time::Instant;
use wire_weaver::shrink_wrap::UNib32;
use wire_weaver_client::observe::ObserveMode;
use wire_weaver_client::ww_self::TypeOwned;
use wire_weaver_client::{DynClient, DynResource, seconds};

#[derive(Args)]
pub(crate) struct ObserveArgs {
    /// Path to a trait level, e.g. "motor" or "bank\[0\]"; empty observes the API root
    #[arg(default_value = "")]
    path: String,

    /// Observe every property of the level periodically at this rate instead of on every change
    #[arg(long, value_name = "MS")]
    rate: Option<u64>,

    /// Observe every sample of the level's `+batched` properties, sent in batches at most this old
    #[arg(long, value_name = "MS")]
    samples: Option<u64>,
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
            every_us: micros(ms),
        },
        None => ObserveMode::OnChange { min_us: UNib32(0) },
    };
    let batched = level.batched_properties()?;
    if args.samples.is_some() && batched.is_empty() {
        bail!(
            "'{}' has no `+batched` properties to observe every sample of",
            display_path(&args.path)
        );
    }
    let entries = props
        .iter()
        .map(|(index, ..)| match args.samples {
            Some(ms) if batched.contains(index) => (
                *index,
                ObserveMode::Samples {
                    every_us: UNib32(0),
                    max_age_us: micros(ms),
                },
            ),
            _ => (*index, mode),
        })
        .collect();
    let types: HashMap<&str, &TypeOwned> = props
        .iter()
        .map(|(_, name, ty)| (name.as_str(), ty))
        .collect();
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
        for samples in &update.samples {
            let ty = types.get(samples.name.as_str()).copied();
            for (value, t) in &samples.items {
                let value = crate::value_fmt::fmt_value(value, ty, level.bundle());
                let t = t.map_or("-".to_string(), |t| format!("{:.6}", seconds(&t)));
                println!("  {} {}: {value}", style(t).dim(), samples.name);
            }
        }
        let values = update
            .values
            .iter()
            .filter(|(name, _)| !update.samples.iter().any(|s| s.name == *name))
            .map(|(name, value)| {
                let ty = types.get(name.as_str()).copied();
                let value = crate::value_fmt::fmt_value(value, ty, level.bundle());
                format!("{name}: {value}")
            })
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

fn micros(ms: u64) -> UNib32 {
    UNib32(ms.saturating_mul(1000).min(u32::MAX as u64) as u32)
}
