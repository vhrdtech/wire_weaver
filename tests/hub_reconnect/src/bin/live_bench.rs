//! Live link handover (CLT-44) on a real board. `live_bench hold SERIAL METHOD SECS` holds a WireWeaver USB device
//! directly for SECS seconds with 20 calls of METHOD (a method without arguments, e.g. `gpio.pin[0].output_level`)
//! in flight all the time, printing a `state` line every 200 ms and the connection events it gets; `live_bench ask
//! SERIAL METHOD` connects to the same device as another program, through the hub, and calls METHOD once. Started as
//! the hub (`WW_HUB_EXE`), it hosts the hub.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use wire_weaver_client::hub::HubMode;
use wire_weaver_client::{ClientConfig, ClientKind, ClientMeta, DynClient, DynResource};

fn main() -> Result<(), String> {
    // `LIVE_BENCH_LOG=debug` shows frame prefix reports, also in a hub this program hosts (`WW_HUB_EXE` set to it)
    let level = match std::env::var("LIVE_BENCH_LOG").as_deref() {
        Ok("debug") => tracing::Level::DEBUG,
        Ok("trace") => tracing::Level::TRACE,
        _ => tracing::Level::INFO,
    };
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_max_level(level)
        .with_writer(std::io::stderr)
        .init();
    // first, before any runtime: the hub runs its own
    ww_hub::main_if_hub();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [op, serial, method, secs] if op == "hold" => {
            rt.block_on(hold(serial, method, secs.parse().map_err(|_| "SECS")?))
        }
        [op, serial, method] if op == "ask" => rt.block_on(ask(serial, method)),
        _ => Err("usage: live_bench hold SERIAL METHOD SECS | live_bench ask SERIAL METHOD".into()),
    }
}

fn config(serial: &str, program: &str) -> ClientConfig {
    ClientConfig::new()
        .usb()
        .serial_eq(serial.to_string())
        .client_meta(ClientMeta::new().kind(ClientKind::Cli).program(program))
}

/// `a.b[1].c`: the resource of a dotted path with indices.
async fn resolve(client: &DynClient, path: &str) -> Result<DynResource, String> {
    let bundle = client
        .cmd()
        .introspect()
        .get()
        .await
        .map_err(|e| format!("{e:#}"))?
        .ok_or("the device has no introspection data")?;
    let mut r = DynResource::root(Arc::new(bundle));
    for part in path.split('.') {
        let (name, index) = match part.split_once('[') {
            Some((name, i)) => (
                name,
                Some(i.trim_end_matches(']').parse::<u32>().map_err(|_| part)?),
            ),
            None => (part, None),
        };
        r = r.child(name).map_err(|e| format!("{e:?}"))?;
        if let Some(i) = index {
            r = r.index(i).map_err(|e| format!("{e:?}"))?;
        }
    }
    Ok(r)
}

async fn hold(serial: &str, method: &str, secs: u64) -> Result<(), String> {
    let client = DynClient::from_config(config(serial, "live_bench hold"))
        .connect()
        .await
        .map_err(|e| format!("{e:#}"))?;
    let method = Arc::new(resolve(&client, method).await?);
    let mut events = client.cmd().connection_events();
    let (ok, failed, max_gap_us) = (
        Arc::new(AtomicU32::new(0)),
        Arc::new(AtomicU32::new(0)),
        Arc::new(AtomicU64::new(0)),
    );
    let started = Instant::now();
    // the longest time without a call completing: what the app sees of a handover
    let last_done = Arc::new(AtomicU64::new(0));
    let mut workers = vec![];
    for _ in 0..20 {
        let (cmd, method) = (client.cmd().clone(), method.clone());
        let (ok, failed, max_gap_us, last_done) = (
            ok.clone(),
            failed.clone(),
            max_gap_us.clone(),
            last_done.clone(),
        );
        workers.push(tokio::spawn(async move {
            while started.elapsed() < Duration::from_secs(secs) {
                let r = method.call(&cmd, &[]).await;
                let now = started.elapsed().as_micros() as u64;
                let before = last_done.swap(now, Ordering::Relaxed);
                let gap = now.saturating_sub(before);
                max_gap_us.fetch_max(gap, Ordering::Relaxed);
                if gap > 20_000 {
                    println!("gap {} ms at {} ms", gap / 1000, now / 1000);
                    tracing::info!("live_bench: no reply for {} ms until now", gap / 1000);
                }
                match r {
                    Ok(_) => _ = ok.fetch_add(1, Ordering::Relaxed),
                    Err(e) => {
                        failed.fetch_add(1, Ordering::Relaxed);
                        println!("failed at {} ms: {e:?}", now / 1000);
                    }
                }
            }
        }));
    }
    println!("holding");
    while started.elapsed() < Duration::from_secs(secs) {
        while let Ok(event) = events.try_recv() {
            println!("event {event:?}");
        }
        println!(
            "state {} {} {}",
            ok.load(Ordering::Relaxed),
            failed.load(Ordering::Relaxed),
            max_gap_us.load(Ordering::Relaxed)
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    for w in workers {
        let _ = w.await;
    }
    println!(
        "done {} calls, {} failed, longest gap without a reply {:?}",
        ok.load(Ordering::Relaxed),
        failed.load(Ordering::Relaxed),
        Duration::from_micros(max_gap_us.load(Ordering::Relaxed))
    );
    Ok(())
}

async fn ask(serial: &str, method: &str) -> Result<(), String> {
    let started = Instant::now();
    let client = DynClient::from_config(config(serial, "live_bench ask").hub(HubMode::Prefer))
        .connect()
        .await
        .map_err(|e| format!("{e:#}"))?;
    let connected = started.elapsed();
    let method = resolve(&client, method).await?;
    let value = method
        .call(client.cmd(), &[])
        .await
        .map_err(|e| format!("{e:?}"))?;
    println!("asked: connected in {connected:?}, {value:?}");
    Ok(())
}
