//! `ww observe <path> [--rate ..]` (TOOL-11): observes a trait level's `+observe` properties without generated
//! code, printing each update's present values, the update rate and the link bytes/s it costs. With `--samples`,
//! `+batched` properties come sample by sample, one line each with its device time (API-39).

use crate::untrusted::escape;
use crate::util::{display_path, resolve};
use anyhow::{Context, Result, anyhow, bail};
use clap::Args;
use console::style;
use human_repr::HumanCount;
use std::collections::HashMap;
use std::time::Instant;
use wire_weaver::shrink_wrap::UNib32;
use wire_weaver_client::observe::{ObserveMode, ObserveRefused};
use wire_weaver_client::ww_client_server::ww_si::Micros;
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

/// The entries a device didn't apply, by property name: `reading: ModeNotSupported`.
fn refusals(props: &[(u32, String, TypeOwned)], refused: &[ObserveRefused]) -> Vec<String> {
    refused
        .iter()
        .map(|r| {
            let name = props.iter().find(|(index, ..)| *index == r.index.get().0);
            match name {
                Some((_, name, _)) => format!("{}: {:?}", escape(name), r.reason),
                None => format!("#{}: {:?}", r.index.get().0, r.reason),
            }
        })
        .collect()
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
            .map(|(_, name, _)| escape(name))
            .collect::<Vec<_>>()
            .join(", "),
    );
    let mode = match args.rate {
        Some(ms) => ObserveMode::Periodic { every: micros(ms) },
        None => ObserveMode::OnChange {
            min: Micros::new(UNib32(0)),
        },
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
                    every: Micros::new(UNib32(0)),
                    max_age: micros(ms),
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
        let refused = refusals(&props, &granted.refused);
        if args.samples.is_some() && !refused.is_empty() {
            bail!(
                "'{}': the device refused {}: a firmware without samples (`observe_samples = false`) refuses `Samples`",
                display_path(&args.path),
                refused.join(", ")
            );
        }
        for refused in refused {
            eprintln!("{} {refused}", style("refused:").yellow());
        }
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
                println!("  {} {}: {value}", style(t).dim(), escape(&samples.name));
            }
        }
        let values = update
            .values
            .iter()
            .filter(|(name, _)| !update.samples.iter().any(|s| s.name == *name))
            .map(|(name, value)| {
                let ty = types.get(name.as_str()).copied();
                let value = crate::value_fmt::fmt_value(value, ty, level.bundle());
                format!("{}: {value}", escape(name))
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

fn micros(ms: u64) -> Micros<UNib32> {
    Micros::new(UNib32(ms.saturating_mul(1000).min(u32::MAX as u64) as u32))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wire_weaver_client::observe::ObserveRefusal;
    use wire_weaver_client::ww_self::TypeOwned;

    /// A refused entry is named by its property, and by its index when the level doesn't list it.
    use wire_weaver_client::ww_client_server::ResourceIndex;

    #[test]
    fn refusals_name_the_property() {
        let props = vec![(1, "reading".to_string(), TypeOwned::Bool)];
        let refused = |index| ObserveRefused {
            index: ResourceIndex::new(UNib32(index)),
            reason: ObserveRefusal::ModeNotSupported,
        };
        assert_eq!(
            refusals(&props, &[refused(1), refused(7)]),
            ["reading: ModeNotSupported", "#7: ModeNotSupported"]
        );
    }
}
