//! [Pins] over a connected device: `PinDiag` found wherever the device's API has it, from its introspection data.

use crate::{Level, PinError, Pins, Point, PointState};
use anyhow::{Result, anyhow, bail};
use std::path::PathBuf;
use wire_weaver_client::{CallError, Commander, DynClient};
use ww_pin_diag::{InternedOwned, PinDiagErrorOwned, PointOwned};
use ww_report::clients::FirmwareInfoClient;
use ww_report::path::{TraitAt, find_all_traits};

/// Client of `ww_pin_diag::PinDiag` at an absolute path.
pub struct PinDiagClient {
    cmd: Commander,
}

mod pin_diag_client {
    use super::*;
    wire_weaver::ww_codegen!(
        ww_pin_diag::PinDiag for PinDiagClient,
        client = "trait_client",
        no_alloc = false,
        use_async = true,
    );
}

/// Where the device serves `PinDiag`: `name` is the path's item names joined with `.` (`diag.pins`), empty for the
/// API root itself. Needs the device's introspection data (not a traits-only connection).
pub fn locations(device: &DynClient) -> Result<Vec<TraitAt>> {
    let Some(bundle) = device.device_introspect() else {
        bail!("no introspection data from the device, needed to find PinDiag in its API");
    };
    Ok(find_all_traits(
        &bundle.api_bundle,
        "ww_pin_diag",
        "PinDiag",
    ))
}

/// Shows a location: `diag.pins`, `(root)` for the API root.
pub fn location_name(at: &TraitAt) -> &str {
    if at.name.is_empty() {
        "(root)"
    } else {
        &at.name
    }
}

/// [Pins] of a connected device.
pub struct DevicePins {
    client: PinDiagClient,
    pub at: TraitAt,
}

impl DevicePins {
    /// `PinDiag` at `at` (`diag.pins`, see [locations]), or the only one the device has.
    pub fn find(device: &DynClient, at: Option<&str>) -> Result<Self> {
        let all = locations(device)?;
        let names = || {
            all.iter()
                .map(|a| location_name(a).to_string())
                .collect::<Vec<_>>()
                .join(", ")
        };
        let at = match at {
            Some(at) => {
                let at = at.trim_matches('.');
                all.iter()
                    .find(|a| a.name == at || (at == "(root)" && a.name.is_empty()))
                    .cloned()
                    .ok_or_else(|| match all.is_empty() {
                        true => anyhow!("the device doesn't serve ww_pin_diag::PinDiag"),
                        false => anyhow!("no PinDiag at '{at}', the device has it at: {}", names()),
                    })?
            }
            None => match all.as_slice() {
                [] => bail!("the device doesn't serve ww_pin_diag::PinDiag"),
                [one] => one.clone(),
                _ => bail!(
                    "the device serves PinDiag more than once, choose one with --at (or target = in a plan): {}",
                    names()
                ),
            },
        };
        let mut cmd = device.cmd().clone();
        cmd.set_base_path(at.path.clone());
        Ok(DevicePins {
            client: PinDiagClient { cmd },
            at,
        })
    }

    /// The device's `POINTS`, names and groups as it sends them.
    pub async fn raw_points(&mut self) -> Result<Vec<PointOwned>> {
        self.client
            .read_POINTS()
            .read()
            .await
            .map_err(|e| anyhow!("reading POINTS: {e}"))
    }
}

fn failed_or_refused(e: CallError<PinDiagErrorOwned>) -> PinError {
    match e {
        CallError::Device(e) => PinError::Refused(e),
        other => PinError::Failed(other.to_string()),
    }
}

impl Pins for DevicePins {
    async fn set(&mut self, point: u16, state: PointState) -> Result<(), PinError> {
        self.client
            .set(point, state)
            .call()
            .await
            .map_err(failed_or_refused)
    }

    async fn read(&mut self, point: u16) -> Result<Level, PinError> {
        self.client
            .read(point)
            .call()
            .await
            .map_err(failed_or_refused)
    }

    async fn release_all(&mut self) -> Result<(), PinError> {
        self.client
            .release_all()
            .call()
            .await
            .map_err(|e| PinError::Failed(e.to_string()))
    }
}

/// Turns [InternedOwned] names into text: `Text` as it is, an `Id` from the firmware's ELF found by build id.
#[derive(Clone, Debug, Default)]
pub struct Names {
    /// The firmware's ELF, found by the build id the device reports.
    pub elf: Option<PathBuf>,
    /// What to tell the user about names sent as ids.
    pub note: Option<String>,
}

impl Names {
    /// For a device whose points are `raw`: looks for the ELF only when a name or group is an id.
    pub async fn for_device(device: &DynClient, raw: &[PointOwned]) -> Self {
        let ids = raw.iter().any(|p| is_id(&p.name) || is_id(&p.group));
        if !ids {
            return Names::default();
        }
        let build_id = FirmwareInfoClient::new(device)
            .read_BUILD_ID()
            .read()
            .await
            .map(|id| id.as_slice().to_vec())
            .unwrap_or_default();
        let elf = (!build_id.is_empty())
            .then(|| cnt_core::elf::find(&build_id))
            .flatten();
        // TODO(STD-48): look the ids up in the ELF's string table once defmt has an accessor for a Str's index
        // (Table::get_str, proposed upstream); until then they show as #<id>.
        let note = Some(match &elf {
            Some(path) => format!(
                "names sent as ids show as #<id>: the firmware's ELF is {}, reading ids from it is pending (STD-48)",
                path.display()
            ),
            None if build_id.is_empty() => {
                "names sent as ids show as #<id>: the device reports no build id to find its ELF by"
                    .into()
            }
            None => format!(
                "names sent as ids show as #<id>: no ELF with build id {} ('ww elf add' adds it)",
                cnt_core::elf::hex(&build_id)
            ),
        });
        Names { elf, note }
    }

    pub fn text(&self, name: &InternedOwned) -> String {
        name.to_string()
    }

    /// The device's points with their names as text.
    pub fn points(&self, raw: &[PointOwned]) -> Vec<Point> {
        raw.iter()
            .enumerate()
            .map(|(id, p)| Point {
                id: id as u16,
                name: self.text(&p.name),
                group: self.text(&p.group),
                caps: p.caps,
            })
            .collect()
    }
}

fn is_id(name: &InternedOwned) -> bool {
    matches!(name, InternedOwned::Id(_))
}
