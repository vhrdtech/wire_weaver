//! [Pins] over the `ww_gpio::Bank`s of a connected device, for a device that serves GPIO banks but not
//! `ww_pin_diag::PinDiag` (TOOL-21): every pin of every bank is a point, found through the device's introspection
//! data, arrays of banks and pins by their valid indices.
//!
//! A pin set for the first time has its mode and pull saved, and [Pins::release_all] puts them back: GPIO has no
//! "give the pin back to its function" call, and no board rules either. What the device refuses comes back as a
//! refusal (`Unsupported` as not capable), but nothing stops a drive the board can't take: that is what `PinDiag`
//! is for.

use crate::{Level, PinError, Pins, Point, PointCaps, PointState, Pull};
use anyhow::{Result, anyhow, bail};
use wire_weaver::prelude::{CommonErrorOwned, UNib32};
use wire_weaver_client::{CallError, Commander, DynClient, DynResource, DynResourceKind};
use ww_gpio::{GpioErrorOwned, Mode};
use ww_pin_diag::{InternedOwned, PinDiagErrorOwned};

/// Client of `ww_gpio::Pin` at an absolute path.
struct PinClient {
    cmd: Commander,
}

/// Client of `ww_gpio::Bank` at an absolute path.
struct BankClient {
    cmd: Commander,
}

mod pin_client {
    use super::*;
    wire_weaver::ww_codegen!(
        ww_gpio::Pin for PinClient,
        client = "trait_client",
        no_alloc = false,
        use_async = true,
    );
}

mod bank_client {
    use super::*;
    wire_weaver::ww_codegen!(
        ww_gpio::Bank for BankClient,
        client = "trait_client",
        no_alloc = false,
        use_async = true,
    );
}

struct BankPin {
    client: PinClient,
    /// Mode and pull before the first `set`, put back by `release_all`.
    saved: Option<(Mode, Pull)>,
}

/// [Pins] of a device's GPIO banks, see the [module](self) documentation.
pub struct BankPins {
    pins: Vec<BankPin>,
}

impl BankPins {
    /// Every pin of every `ww_gpio::Bank` the device serves, with its point: named as the pin's capabilities name
    /// it (`PB5`), or bank name and index (`PA3`) when that name isn't unique; the bank's name is the group.
    pub async fn find(device: &DynClient) -> Result<(Self, Vec<Point>)> {
        let Some(bundle) = device.device_introspect() else {
            bail!("no introspection data from the device, needed to find its GPIO banks");
        };
        let mut banks = vec![];
        collect_banks(
            device.cmd(),
            DynResource::root(bundle.api_bundle.clone()),
            &mut banks,
            0,
        )
        .await?;
        if banks.is_empty() {
            bail!("the device serves neither ww_pin_diag::PinDiag nor ww_gpio::Bank");
        }
        let mut pins = vec![];
        let mut found = vec![];
        for bank in banks {
            let client = BankClient {
                cmd: commander(device, bank.index_chain()),
            };
            let bank_name = client
                .name()
                .call()
                .await
                .map_err(|e| anyhow!("{}: name: {e}", bank.path()))?;
            let array = bank.child("pin")?;
            let indices = array
                .valid_indices(device.cmd())
                .await
                .map_err(|e| anyhow!("{}: valid indices: {e}", array.path()))?;
            for i in indices.iter() {
                let pin = array.index(i)?;
                let client = PinClient {
                    cmd: commander(device, pin.index_chain()),
                };
                let caps = client
                    .capabilities()
                    .call()
                    .await
                    .map_err(|e| anyhow!("{}: capabilities: {e}", pin.path()))?;
                found.push((bank_name.clone(), i, caps));
                pins.push(BankPin {
                    client,
                    saved: None,
                });
            }
        }
        let names: Vec<&str> = found.iter().map(|(_, _, c)| c.name.as_str()).collect();
        let points = found
            .iter()
            .enumerate()
            .map(|(id, (bank, i, caps))| {
                let own = caps.name.as_str();
                let unique = !own.is_empty() && names.iter().filter(|n| **n == own).count() == 1;
                let modes = caps.modes.as_slice();
                let pulls = caps.pulls.as_slice();
                Point {
                    id: id as u16,
                    name: if unique {
                        own.to_string()
                    } else {
                        format!("{bank}{i}")
                    },
                    group: bank.clone(),
                    caps: PointCaps {
                        read: true,
                        pull_up: pulls.contains(&Pull::Up),
                        pull_down: pulls.contains(&Pull::Down),
                        drive_low: modes.contains(&Mode::PushPullOutput)
                            || modes.contains(&Mode::OpenDrainOutput),
                        drive_high: modes.contains(&Mode::PushPullOutput),
                        weak_drive: false,
                    },
                }
            })
            .collect();
        Ok((BankPins { pins }, points))
    }

    fn pin(&mut self, point: u16) -> Result<&mut BankPin, PinError> {
        self.pins
            .get_mut(point as usize)
            .ok_or(PinError::Refused(PinDiagErrorOwned::NoSuchPoint))
    }
}

fn commander(device: &DynClient, chain: &[UNib32]) -> Commander {
    let mut cmd = device.cmd().clone();
    cmd.set_base_path(chain.to_vec());
    cmd
}

/// Banks below `res`, arrays of them entered by their valid indices.
async fn collect_banks(
    cmd: &Commander,
    res: DynResource,
    out: &mut Vec<DynResource>,
    depth: usize,
) -> Result<()> {
    // trait implementations nest, but not without a bound
    if depth > 16 {
        return Ok(());
    }
    if is_bank(&res) {
        out.push(res);
        return Ok(());
    }
    let names: Vec<String> = res.children().iter().map(|i| i.ident.clone()).collect();
    for name in names {
        let Ok(child) = res.child(&name) else {
            continue;
        };
        if child.is_omitted() {
            continue;
        }
        match child.kind() {
            Ok(DynResourceKind::Level(_)) => {
                Box::pin(collect_banks(cmd, child, out, depth + 1)).await?
            }
            Ok(DynResourceKind::Array(item))
                if matches!(item.kind, wire_weaver_client::ww_self::ApiItemKindOwned::Trait { .. }) =>
            {
                let indices = child
                    .valid_indices(cmd)
                    .await
                    .map_err(|e| anyhow!("{}: valid indices: {e}", child.path()))?;
                for i in indices.iter() {
                    Box::pin(collect_banks(cmd, child.index(i)?, out, depth + 1)).await?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn is_bank(res: &DynResource) -> bool {
    let Ok(DynResourceKind::Level(level)) = res.kind() else {
        return false;
    };
    level.trait_name == "Bank"
        && res
            .bundle()
            .ext_crates
            .get(level.crate_idx.0 as usize)
            .is_some_and(|c| c.crate_id == "ww_gpio")
}

/// A GPIO error as `PinDiag` would say it: `Unsupported` is not capable, a bank whose pins share one mode a refusal
/// by that rule.
fn refused(e: CallError<GpioErrorOwned>) -> PinError {
    match e {
        CallError::Device(GpioErrorOwned::Common(CommonErrorOwned::Unsupported)) => {
            PinError::Refused(PinDiagErrorOwned::NotCapable)
        }
        CallError::Device(GpioErrorOwned::Common(c)) => {
            PinError::Refused(PinDiagErrorOwned::Common(c))
        }
        CallError::Device(GpioErrorOwned::DifferentModes) => {
            PinError::Refused(PinDiagErrorOwned::Refused {
                rule: InternedOwned::Text("the bank's pins share one mode".into()),
            })
        }
        CallError::Device(other) => PinError::Failed(other.to_string()),
        other => PinError::Failed(other.to_string()),
    }
}

fn failed<E: std::fmt::Display>(e: E) -> PinError {
    PinError::Failed(e.to_string())
}

impl Pins for BankPins {
    async fn set(&mut self, point: u16, state: PointState) -> Result<(), PinError> {
        let pin = self.pin(point)?;
        if pin.saved.is_none() {
            let mode = pin.client.mode().call().await.map_err(failed)?;
            let pull = pin.client.read_pull().read().await.map_err(failed)?;
            pin.saved = Some((mode, pull));
        }
        match state {
            PointState::Input { pull } => {
                pin.client.write_pull(pull).write().await.map_err(refused)?;
                pin.client
                    .set_mode(Mode::Input, None)
                    .call()
                    .await
                    .map_err(refused)
            }
            // GPIO has no weaker drive than its normal one: a weak drive is a normal one (weak_drive = false)
            PointState::Output { level, .. } => {
                pin.client
                    .set_mode(Mode::PushPullOutput, Some(level))
                    .call()
                    .await
                    .map_err(refused)
            }
            PointState::Unknown(_) => Err(PinError::Refused(PinDiagErrorOwned::NotCapable)),
        }
    }

    async fn read(&mut self, point: u16) -> Result<Level, PinError> {
        let pin = self.pin(point)?;
        pin.client.input_level().call().await.map_err(failed)
    }

    async fn release_all(&mut self) -> Result<(), PinError> {
        let mut errors = vec![];
        for (id, pin) in self.pins.iter_mut().enumerate() {
            let Some((mode, pull)) = pin.saved.take() else {
                continue;
            };
            if let Err(e) = pin.client.write_pull(pull).write().await {
                errors.push(format!("P{id} pull: {e}"));
            }
            if let Err(e) = pin.client.set_mode(mode, None).call().await {
                errors.push(format!("P{id} mode: {e}"));
            }
        }
        match errors.is_empty() {
            true => Ok(()),
            false => Err(PinError::Failed(format!(
                "not all pins put back: {}",
                errors.join("; ")
            ))),
        }
    }
}
