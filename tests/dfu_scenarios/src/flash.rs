//! What survives a reset or a power cut: the flash of the simulated device (two banks, NOR semantics), its boot
//! record, the plan area with its apply journal and the live session record of the updater. The bootloader runs on it
//! between two firmware runs.

use ww_dfu::{BootState, Error, LastResult, Outcome, Profile, SlotSwitch};

use crate::plan::{self, CHUNK, CHUNKS, ERASE, Header, SLOT, Step};
use crate::record::Recorder;

/// Which firmware runs after the bootloader.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Firmware {
    /// The app with every update trait: receives updates itself (A/B, stored plan).
    App,
    /// The app of a device that updates in place live: `UpdateNode` and `BootControl` only.
    LeanApp,
    /// The bootloader with a transport (or a recovery app): receives and applies in place, live.
    Updater,
}

impl Firmware {
    pub fn name(self) -> &'static str {
        match self {
            Firmware::App => "app",
            Firmware::LeanApp => "app",
            Firmware::Updater => "updater",
        }
    }
}

/// The power failed while the bootloader worked.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PowerCut;

/// A power cut injected by a test.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Cut {
    /// The bootloader loses power while applying the stored plan, half way into step `n` (counted from 0).
    Apply(u32),
    /// The updater loses power half way into applying step `n` it just received.
    Download(u32),
}

/// The stored plan (in place, [ww_dfu::Target::Stored]).
#[derive(Clone, Debug, Default)]
pub struct PlanArea {
    pub header: Vec<u8>,
    /// The plan after the header, as the steps' byte form
    pub body: Vec<u8>,
    /// Steps stored so far (step by step)
    pub steps: u32,
    /// Checked by `Stage::finish`, waits for the bootloader
    pub complete: bool,
    /// Apply journal: steps the bootloader has finished. A real one clears one bit per step (no erase needed).
    pub applied: u32,
}

/// The updater's record of a live in-place session, written after every step: survives a power cut.
#[derive(Clone, Debug)]
pub struct LiveRecord {
    pub header: Vec<u8>,
    pub next_step: u32,
    /// Bytes of the plan after the header that the applied steps take (where a bulk session resumes)
    pub plan_bytes: u32,
}

#[derive(Clone, Debug)]
pub struct Flash {
    pub profile: Profile,
    /// Two banks. A/B: both slots; in place: bank 0 is the app slot, bank 1 is unused.
    pub banks: [Vec<u8>; 2],
    /// Remap: the bank that runs. Copy and in place: always 0.
    pub run_bank: usize,
    pub boot_state: BootState,
    pub last_result: LastResult,
    pub version_floor: u32,
    /// Digest the app slot must have to boot: the descriptor of the image the device last accepted.
    pub app_digest: [u8; 8],
    /// Version of a staged or trial image, for `last_result`
    pub pending_version: u32,
    /// The app asked to start the updater; stays set until a live in-place session finishes.
    pub enter_updater: bool,
    pub plan_area: PlanArea,
    /// Size of the plan area, 0 without one
    pub plan_area_size: u32,
    pub live_record: Option<LiveRecord>,
    pub cut: Option<Cut>,
}

impl Flash {
    /// A device with `image` confirmed in its app slot.
    pub fn new(profile: Profile, image: &[u8], plan_area_size: u32) -> Flash {
        let mut banks = [vec![0xFF; SLOT], vec![0xFF; SLOT]];
        banks[0].copy_from_slice(image);
        Flash {
            profile,
            banks,
            run_bank: 0,
            boot_state: BootState::Confirmed,
            last_result: LastResult {
                outcome: Outcome::Nothing,
                version: 0,
            },
            version_floor: 1,
            app_digest: plan::digest(image),
            pending_version: 0,
            enter_updater: false,
            plan_area: PlanArea::default(),
            plan_area_size,
            live_record: None,
            cut: None,
        }
    }

    pub fn is_ab(&self) -> bool {
        matches!(self.profile, Profile::AB(_))
    }

    /// The bank the app runs from.
    pub fn active(&self) -> &[u8] {
        &self.banks[self.run_bank]
    }

    /// Bank a live update writes to: the inactive one (A/B) or the app slot (in place).
    pub fn target_bank(&self) -> usize {
        if self.is_ab() { 1 - self.run_bank } else { 0 }
    }

    pub fn erase(&mut self, bank: usize, block: u16) -> Result<(), Error> {
        let at = block as usize * ERASE;
        if at + ERASE > SLOT {
            return Err(Error::OutOfRange);
        }
        self.banks[bank][at..at + ERASE].fill(0xFF);
        Ok(())
    }

    /// NOR programming: bits only go from 1 to 0, so programming over data that isn't erased corrupts it.
    pub fn program(&mut self, bank: usize, at: usize, data: &[u8]) {
        for (cell, b) in self.banks[bank][at..at + data.len()].iter_mut().zip(data) {
            *cell &= *b;
        }
    }

    /// Apply one step to the target bank, COPY reading from the running image. `half` cuts it half way, as a power
    /// cut would.
    pub fn apply(&mut self, step: &Step, half: bool) -> Result<(), Error> {
        let bank = self.target_bank();
        match step {
            Step::Erase { block } => {
                if half {
                    // an erase cut short leaves the block unreadable; here: half of it erased
                    let at = *block as usize * ERASE;
                    self.banks[bank][at..at + ERASE / 2].fill(0xFF);
                    return Ok(());
                }
                self.erase(bank, *block)
            }
            Step::Copy { dst, src, hash } => {
                let (dst, src) = (*dst as usize, *src as usize);
                if dst >= CHUNKS || src >= CHUNKS {
                    return Err(Error::OutOfRange);
                }
                let source: Vec<u8> = self.active()[src * CHUNK..(src + 1) * CHUNK].to_vec();
                if plan::chunk_hash(&source) != *hash {
                    return Err(Error::SourceMismatch);
                }
                self.write_chunk(bank, dst, &source, hash, half)
            }
            Step::Write { dst, hash, data } => {
                let dst = *dst as usize;
                if dst >= CHUNKS || data.len() != CHUNK {
                    return Err(Error::OutOfRange);
                }
                self.write_chunk(bank, dst, data, hash, half)
            }
        }
    }

    fn write_chunk(
        &mut self,
        bank: usize,
        dst: usize,
        data: &[u8],
        hash: &[u8; 8],
        half: bool,
    ) -> Result<(), Error> {
        let at = dst * CHUNK;
        if half {
            self.program(bank, at, &data[..CHUNK / 2]);
            return Ok(());
        }
        self.program(bank, at, data);
        if plan::chunk_hash(&self.banks[bank][at..at + CHUNK]) != *hash {
            // e.g., programmed over a chunk that wasn't erased
            return Err(Error::HashMismatch);
        }
        Ok(())
    }

    /// True if the app slot holds the image the device last accepted.
    pub fn app_bootable(&self) -> bool {
        plan::digest(self.active()) == self.app_digest
    }

    /// Parse and check a stored plan before anything is touched: well formed, as many steps and bytes as the header
    /// says, every COPY source still has its hash, every WRITE's data has its hash.
    pub fn check_stored_plan(&self) -> Result<(), Error> {
        let header = Header::parse(&self.plan_area.header).map_err(|_| Error::BadHeader)?;
        if self.plan_area.body.len() != header.body_len as usize {
            return Err(Error::Incomplete);
        }
        let steps = self.stored_steps()?;
        if steps.len() != header.steps as usize {
            return Err(Error::Incomplete);
        }
        for step in &steps {
            match step {
                Step::Copy { src, hash, .. } => {
                    let src = *src as usize;
                    if src >= CHUNKS {
                        return Err(Error::OutOfRange);
                    }
                    if plan::chunk_hash(&self.active()[src * CHUNK..(src + 1) * CHUNK]) != *hash {
                        return Err(Error::SourceMismatch);
                    }
                }
                Step::Write { hash, data, .. } if plan::chunk_hash(data) != *hash => {
                    return Err(Error::HashMismatch);
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn stored_steps(&self) -> Result<Vec<Step>, Error> {
        let body = &self.plan_area.body;
        let mut steps = vec![];
        let mut at = 0;
        while at < body.len() {
            match Step::decode(&body[at..]) {
                Ok(Some((step, len))) => {
                    steps.push(step);
                    at += len;
                }
                _ => return Err(Error::BadHeader),
            }
        }
        Ok(steps)
    }

    /// What the bootloader does at a reset. `Err(PowerCut)`: the power failed while it worked, run it again.
    pub fn boot(&mut self, rec: &Recorder) -> Result<Firmware, PowerCut> {
        match self.profile {
            Profile::AB(switch) => {
                match self.boot_state {
                    BootState::Staged => {
                        self.switch(switch, rec, "switches to the staged image");
                        self.boot_state = BootState::Trial;
                    }
                    BootState::Trial => {
                        self.switch(
                            switch,
                            rec,
                            "the trial image did not confirm: switches back",
                        );
                        self.boot_state = BootState::Reverted;
                        self.last_result = LastResult {
                            outcome: Outcome::Reverted,
                            version: self.pending_version,
                        };
                    }
                    _ => {}
                }
                Ok(Firmware::App)
            }
            Profile::InPlace => {
                if self.boot_state == BootState::PlanStored {
                    self.apply_stored(rec)?;
                }
                if self.enter_updater {
                    if !self.app_bootable() {
                        rec.note("bootloader: an update is under way, the app slot is not bootable: starts the updater");
                    }
                    return Ok(Firmware::Updater);
                }
                if !self.app_bootable() {
                    rec.note("bootloader: no valid app, starts the updater");
                    self.boot_state = BootState::Updating;
                    self.enter_updater = true;
                    return Ok(Firmware::Updater);
                }
                Ok(if self.plan_area_size > 0 {
                    Firmware::App
                } else {
                    Firmware::LeanApp
                })
            }
        }
    }

    fn switch(&mut self, switch: SlotSwitch, rec: &Recorder, why: &str) {
        match switch {
            SlotSwitch::Copy => {
                // page by page through a swap page, resumable after a power cut (embassy-boot)
                self.banks.swap(0, 1);
                rec.note(&format!(
                    "bootloader {why}: swaps the slots' contents page by page"
                ));
            }
            SlotSwitch::Remap => {
                self.run_bank = 1 - self.run_bank;
                rec.note(&format!(
                    "bootloader {why}: maps bank {} to the boot address, nothing is copied",
                    self.run_bank
                ));
            }
        }
        self.app_digest = plan::digest(self.active());
    }

    /// Apply the stored plan from where the journal says, offline.
    fn apply_stored(&mut self, rec: &Recorder) -> Result<(), PowerCut> {
        let steps = self.stored_steps().expect("checked by Stage::finish");
        let header = Header::parse(&self.plan_area.header).expect("checked by Stage::finish");
        let from = self.plan_area.applied;
        if from == 0 {
            rec.note(&format!(
                "bootloader applies the stored plan in place: {} steps",
                steps.len()
            ));
        } else {
            rec.note(&format!(
                "bootloader finds the apply journal at step {from}: redoes step {from}, continues"
            ));
        }
        for (i, step) in steps.iter().enumerate().skip(from as usize) {
            if self.cut == Some(Cut::Apply(i as u32)) {
                self.cut = None;
                // a real cut stops anywhere; half a step is the worst case the journal has to handle
                _ = self.apply(step, true);
                rec.note(&format!("power cut while the bootloader applies step {i}"));
                return Err(PowerCut);
            }
            if let Err(e) = self.apply(step, false) {
                rec.note(&format!("bootloader: step {i} failed ({e:?})"));
                self.boot_state = BootState::Updating;
                self.last_result = LastResult {
                    outcome: Outcome::Recovery,
                    version: header.version,
                };
                return Ok(());
            }
            self.plan_area.applied = i as u32 + 1;
        }
        if plan::digest(self.active()) == header.image_digest {
            rec.note("bootloader: image verified against the descriptor, plan area cleared");
            self.app_digest = header.image_digest;
            self.boot_state = BootState::Confirmed;
            self.last_result = LastResult {
                outcome: Outcome::Confirmed,
                version: header.version,
            };
        } else {
            rec.note("bootloader: image does not match the descriptor");
            self.last_result = LastResult {
                outcome: Outcome::Recovery,
                version: header.version,
            };
        }
        self.plan_area = PlanArea::default();
        Ok(())
    }
}
