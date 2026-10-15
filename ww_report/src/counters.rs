//! The device's counters through `ww_counters::Counters`, decoded with its ELF.

use crate::clients::CountersClient;
use crate::info::Part;
use anyhow::{Result, anyhow};
use cnt_core::{Counters, Storage};
use serde::Serialize;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;
use wire_weaver_client::CallError;
use ww_counters::host::{CountersDevice, WwSource};
use ww_counters::{CounterBuffer, CountersInfoOwned};

/// The calls `WwSource` makes, blocking: run from `spawn_blocking`.
pub struct BlockingCounters(pub Arc<Mutex<CountersClient>>);

fn call_error<E: std::fmt::Debug>(e: CallError<E>) -> anyhow::Error {
    match e {
        CallError::Device(e) => anyhow!("the device refused: {e:?}"),
        CallError::Unimplemented { resource } => anyhow!("the device doesn't implement {resource}"),
        CallError::Client(e) => e.into(),
    }
}

impl CountersDevice for BlockingCounters {
    fn read(&mut self, buffer: CounterBuffer, offset: u16) -> Result<Vec<u32>> {
        let page = self
            .0
            .blocking_lock()
            .read(buffer, offset)
            .blocking_call()
            .map_err(call_error)?;
        Ok(page.iter().map(|w| w.0).collect())
    }

    fn reset(&mut self, buffer: CounterBuffer) -> Result<()> {
        self.0
            .blocking_lock()
            .reset(buffer)
            .blocking_call()
            .map_err(call_error)
    }
}

/// One counter and its value.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CounterValue {
    /// `RAM` or `BKP`
    pub buffer: String,
    /// `group/name`
    pub name: String,
    pub value: u64,
    pub unit: String,
    pub severity: String,
}

/// What `ww report` shows of the counters.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CountersReport {
    pub elf: String,
    pub counters: Vec<CounterValue>,
}

/// `INFO` of the device, its used words, or why it can't be read.
pub async fn read_info(client: &Mutex<CountersClient>) -> Part<CountersInfoOwned> {
    Part::from_result(client.lock().await.read_INFO().read().await)
}

/// Read all counters of the device and decode them with `elf`, checking the ELF against the device's `INFO`.
pub async fn read(
    client: &Arc<Mutex<CountersClient>>,
    info: CountersInfoOwned,
    elf: &Path,
) -> Result<CountersReport> {
    let mut counters = Counters::load_elf(elf)?;
    let mut source = WwSource::new(BlockingCounters(client.clone()), info);
    source.check(&counters, elf)?;
    let counters = tokio::task::spawn_blocking(move || -> Result<Counters> {
        counters.read(&mut source)?;
        Ok(counters)
    })
    .await??;
    let mut out = vec![];
    for block in counters.blocks() {
        for (c, value) in block.values() {
            out.push(CounterValue {
                buffer: match block.storage() {
                    Storage::Ram => "RAM".to_string(),
                    Storage::Bkp => "BKP".to_string(),
                },
                name: c.qualified_name(),
                value: value.to_u64(),
                unit: c.unit.clone(),
                severity: format!("{:?}", c.severity).to_lowercase(),
            });
        }
    }
    Ok(CountersReport {
        elf: elf.display().to_string(),
        counters: out,
    })
}
