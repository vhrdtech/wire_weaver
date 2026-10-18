//! `ww stream <path> [--batches]` (API-41 piece 6): prints items of a plain, counted or reliable stream without
//! generated code, one per line (`--batches`: whole batches instead), until Ctrl+C.
//!
//! Stream functions with arguments aren't supported yet: the CLI has no value-parsing convention for method or
//! stream arguments anywhere (CLT-23-adjacent), so this errors out clearly instead of half-building one.

use crate::util::{display_path, resolve};
use anyhow::{Context, Result, anyhow, bail};
use clap::Args;
use console::style;
use wire_weaver_client::ww_self::{ApiBundleOwned, TypeOwned, ValueOwned};
use wire_weaver_client::{
    AckMode, Batch, BatchItem, DynClient, DynResource, DynResourceKind, DynStreamOpen, StreamEvent,
    seconds,
};

#[derive(Args)]
pub(crate) struct StreamArgs {
    /// Path to a stream, e.g. "motor.position" or "bank[0].samples"
    path: String,

    /// Print whole batches (API-41's batched streams) instead of one item per line
    #[arg(long)]
    batches: bool,
}

pub(crate) async fn stream(args: StreamArgs, device: &DynClient) -> Result<()> {
    let bundle = device
        .device_introspect()
        .ok_or_else(|| anyhow!("device did not provide introspection data"))?
        .api_bundle
        .clone();
    let resource = resolve(DynResource::root(bundle), &args.path)
        .with_context(|| format!("'{}'", display_path(&args.path)))?;
    let ty = match resource.kind()? {
        DynResourceKind::Stream {
            ty, is_up: true, ..
        } => ty.clone(),
        DynResourceKind::Stream { is_up: false, .. } => {
            bail!("'{}' is a sink, not a stream", display_path(&args.path))
        }
        _ => bail!("'{}' is not a stream", display_path(&args.path)),
    };
    if let Some((stream_args, _)) = resource.stream_fn()
        && !stream_args.is_empty()
    {
        bail!(
            "'{}' is a stream function with arguments: 'ww stream' doesn't parse argument values yet",
            display_path(&args.path)
        );
    }
    println!("{} {}", style("streaming").dim(), display_path(&args.path));
    println!("{}", style("Ctrl+C to stop").dim());

    match resource.delivery() {
        None => {
            let mut stream = resource
                .stream(device.cmd())
                .await
                .context("opening the stream")?;
            if args.batches {
                loop {
                    let batch = stream.recv_batch().await.context("receiving a batch")?;
                    print_plain_batch(&batch, &ty, resource.bundle());
                }
            } else {
                loop {
                    let value = stream.recv().await.context("receiving an item")?;
                    println!("{}", fmt(&value, &ty, resource.bundle()));
                }
            }
        }
        Some(_) => {
            let open = resource
                .positioned_stream(device.cmd(), &[], AckMode::OnReceive, None)
                .await
                .context("opening the stream")?;
            let mut stream = match open {
                DynStreamOpen::Rejected(err) => {
                    bail!("rejected: {}", fmt(&err, &ty, resource.bundle()))
                }
                DynStreamOpen::Open { stream, .. } => stream,
            };
            if args.batches {
                loop {
                    let batch = stream.recv_batch().await.context("receiving a batch")?;
                    print_positioned_batch(&batch, &ty, resource.bundle());
                }
            } else {
                loop {
                    let event = stream.recv_event().await.context("receiving an event")?;
                    print_event(event, &ty, resource.bundle());
                }
            }
        }
    }
}

fn fmt(value: &ValueOwned, ty: &TypeOwned, bundle: &ApiBundleOwned) -> String {
    crate::value_fmt::fmt_value(value, Some(ty), bundle)
}

/// `t@value, t@value, ..` of a batch's items, `t` left out for an untimed one.
fn fmt_items(items: &[BatchItem<ValueOwned>], ty: &TypeOwned, bundle: &ApiBundleOwned) -> String {
    items
        .iter()
        .map(|item| match &item.t {
            Some(t) => format!("{:.6}s@{}", seconds(t), fmt(&item.value, ty, bundle)),
            None => fmt(&item.value, ty, bundle),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn print_plain_batch(batch: &[BatchItem<ValueOwned>], ty: &TypeOwned, bundle: &ApiBundleOwned) {
    println!(
        "{} {}: [{}]",
        style("batch").dim(),
        batch.len(),
        fmt_items(batch, ty, bundle)
    );
}

fn print_positioned_batch(batch: &Batch<ValueOwned>, ty: &TypeOwned, bundle: &ApiBundleOwned) {
    println!(
        "{} {}..{}: [{}]",
        style("batch").dim(),
        batch.pos,
        batch.pos + batch.items.len() as u64,
        fmt_items(&batch.items, ty, bundle)
    );
}

fn print_event(event: StreamEvent<ValueOwned>, ty: &TypeOwned, bundle: &ApiBundleOwned) {
    match event {
        StreamEvent::Item { pos, t, value, .. } => match t {
            Some(t) => println!("{pos}  {:.6}s: {}", seconds(&t), fmt(&value, ty, bundle)),
            None => println!("{pos}: {}", fmt(&value, ty, bundle)),
        },
        StreamEvent::Gap {
            at, units, cause, ..
        } => println!("{} at {at}, {units} units ({cause:?})", style("gap").red()),
        StreamEvent::Resumed {
            cursor,
            session,
            ended,
        } => println!(
            "{} {cursor:?}, session {session}{}",
            style("resumed").dim(),
            ended
                .map(|s| format!(" (session {s} ended)"))
                .unwrap_or_default()
        ),
        StreamEvent::BackfillDone => println!("{}", style("backfill done").dim()),
        StreamEvent::Disconnected => println!("{}", style("disconnected").yellow()),
        StreamEvent::Closed => println!("{}", style("closed").dim()),
    }
}
