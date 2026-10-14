//! The update logic of a node (app or updater), on top of [Flash]: what the `ww_dfu` traits do, with the checks the
//! docs promise. The generated servers in [crate::firmware] forward every handler here.

use std::sync::{Arc, Mutex, MutexGuard};

use ww_dfu::{
    BootState, BootTarget, BulkStatus, ChunkState, Error, LastResult, NodeInfo, Offers, Outcome,
    PlanAck, PlanSegmentOwned, Profile, Progress, Role, Session, Slot, SlotLayout, StageLimits,
    Target,
};

use crate::flash::{Cut, Flash, LiveRecord};
use crate::plan::{self, CHUNK, CHUNKS, ERASE, Header, HeaderError, SLOT, Step};

/// RAM state of an update session: lost at a reset.
struct Ram {
    header: Vec<u8>,
    target: Target,
    steps: u32,
    next_step: u32,
    /// Pieces of the WRITE in progress
    chunk: Vec<u8>,
    bulk: Option<Bulk>,
}

struct Bulk {
    total: u32,
    /// Bytes after the header received in order
    have: u32,
    ack_every: u32,
    last_ack: u32,
    /// Live: bytes of a step not complete yet
    pending: Vec<u8>,
    failed: bool,
}

pub struct Node {
    pub flash: Arc<Mutex<Flash>>,
    pub role: Role,
    pub offers: Offers,
    pub limits: StageLimits,
    ram: Option<Ram>,
    /// Set by `BootControl::reboot`, the firmware resets after the reply is out
    pub reboot: Option<BootTarget>,
    /// Power is gone (injected cut): the firmware stops without replying
    pub power_cut: bool,
    /// The step the power cut hit
    pub cut_step: Option<u32>,
    /// Acknowledgements to send on `plan_ack`
    pub acks: Vec<PlanAck>,
    /// Reply buffer of `Inventory::chunks`
    pub page: Vec<ChunkState>,
}

impl Node {
    pub fn new(flash: Arc<Mutex<Flash>>, role: Role, offers: Offers, limits: StageLimits) -> Node {
        Node {
            flash,
            role,
            offers,
            limits,
            ram: None,
            reboot: None,
            power_cut: false,
            cut_step: None,
            acks: vec![],
            page: vec![],
        }
    }

    fn flash(&self) -> MutexGuard<'_, Flash> {
        self.flash.lock().unwrap()
    }

    // ---- UpdateNode

    pub fn node(&self) -> NodeInfo<'static> {
        let f = self.flash();
        let bootable = f.app_bootable();
        NodeInfo {
            role: self.role,
            profile: f.profile,
            product: wire_weaver::prelude::MaxLen::new("sim-dfu").unwrap(),
            geometry_id: plan::GEOMETRY_ID,
            app_version: if bootable {
                plan::image_version(f.active())
            } else {
                None
            },
            version_floor: f.version_floor,
            manifest_digest: bootable.then(|| manifest_digest(f.active())),
            boot_state: f.boot_state,
            last_result: f.last_result,
            offers: self.offers,
        }
    }

    // ---- BootControl

    pub fn boot_state(&self) -> BootState {
        self.flash().boot_state
    }

    pub fn confirm(&self) -> Result<(), Error> {
        let mut f = self.flash();
        if f.boot_state == BootState::Trial {
            f.boot_state = BootState::Confirmed;
            f.last_result = LastResult {
                outcome: Outcome::Confirmed,
                version: f.pending_version,
            };
        }
        Ok(())
    }

    pub fn revert(&self) -> Result<(), Error> {
        let mut f = self.flash();
        if !f.is_ab() {
            return Err(Error::Unsupported);
        }
        match f.boot_state {
            // the next reset finds an unconfirmed trial and switches back
            BootState::Trial => Ok(()),
            BootState::Staged => {
                f.boot_state = BootState::Reverted;
                Ok(())
            }
            state => Err(Error::WrongState(state)),
        }
    }

    pub fn reboot(&mut self, into: BootTarget) -> Result<(), Error> {
        let mut f = self.flash();
        match into {
            BootTarget::Updater if f.is_ab() => return Err(Error::Unsupported),
            BootTarget::Updater => f.enter_updater = true,
            BootTarget::App => {}
        }
        drop(f);
        self.reboot = Some(into);
        Ok(())
    }

    // ---- Inventory

    fn slot(&self, slot: Slot) -> Result<Vec<u8>, Error> {
        let f = self.flash();
        match slot {
            Slot::Active => Ok(f.active().to_vec()),
            Slot::Inactive if f.is_ab() => Ok(f.banks[1 - f.run_bank].clone()),
            Slot::Inactive => Err(Error::Unsupported),
        }
    }

    pub fn layout(&self, slot: Slot) -> Result<SlotLayout, Error> {
        self.slot(slot)?;
        Ok(SlotLayout {
            chunk_size: CHUNK as u32,
            chunk_count: CHUNKS as u16,
            erase_size: ERASE as u32,
            erase_count: (SLOT / ERASE) as u16,
        })
    }

    pub fn manifest_digest(&self, slot: Slot) -> Result<[u8; 8], Error> {
        Ok(manifest_digest(&self.slot(slot)?))
    }

    pub fn chunks(&mut self, slot: Slot, first: u16, count: u16) -> Result<&[ChunkState], Error> {
        let bytes = self.slot(slot)?;
        if first as usize >= CHUNKS {
            return Err(Error::OutOfRange);
        }
        let end = (first as usize + (count as usize).min(ww_dfu::MAX_CHUNKS_PAGE)).min(CHUNKS);
        self.page = bytes[first as usize * CHUNK..end * CHUNK]
            .chunks(CHUNK)
            .map(chunk_state)
            .collect();
        Ok(&self.page)
    }

    // ---- Stage

    /// The checks of `Stage::begin` and `StageBulk::begin_bulk`, then a new or resumed session. Returns the session
    /// and, for a bulk one, the byte to resume from.
    fn start(&mut self, target: Target, header: &[u8]) -> Result<(Session, u32), Error> {
        let hdr = match Header::parse(header) {
            Ok(h) => h,
            Err(HeaderError::Malformed) => return Err(Error::BadHeader),
            Err(HeaderError::BadSignature) => return Err(Error::BadSignature),
        };
        let accepted = match target {
            Target::Live => self.limits.live,
            Target::Stored => self.limits.stored,
        };
        if !accepted {
            return Err(Error::Unsupported);
        }
        if hdr.geometry_id != plan::GEOMETRY_ID {
            return Err(Error::WrongGeometry);
        }
        let mut f = self.flash();
        if hdr.version < f.version_floor {
            return Err(Error::BelowFloor);
        }
        if target == Target::Stored && plan::HEADER_LEN as u32 + hdr.body_len > f.plan_area_size {
            return Err(Error::PlanTooBig);
        }
        let steps = hdr.steps as u32;
        // resume: the session in RAM (a lost link), or what the flash says (a host restart, a reset)
        if let Some(ram) = &self.ram
            && ram.header == header
            && ram.target == target
        {
            let have = ram.bulk.as_ref().map(|b| b.have).unwrap_or(0);
            return Ok((
                Session {
                    steps,
                    resumed: true,
                    resume_from: ram.next_step,
                },
                have,
            ));
        }
        if !f.boot_state.may_stage() {
            return Err(Error::WrongState(f.boot_state));
        }
        let (resumed, next_step, have) = match target {
            Target::Stored if f.plan_area.header == header && !f.plan_area.complete => {
                (true, f.plan_area.steps, f.plan_area.body.len() as u32)
            }
            Target::Stored => {
                f.plan_area = Default::default();
                f.plan_area.header = header.to_vec();
                (false, 0, 0)
            }
            Target::Live if f.is_ab() => (false, 0, 0),
            Target::Live => match &f.live_record {
                Some(r) if r.header == header => (true, r.next_step, r.plan_bytes),
                _ => {
                    // from here on the app slot is not bootable until the session finishes
                    f.live_record = Some(LiveRecord {
                        header: header.to_vec(),
                        next_step: 0,
                        plan_bytes: 0,
                    });
                    f.boot_state = BootState::Updating;
                    (false, 0, 0)
                }
            },
        };
        drop(f);
        self.ram = Some(Ram {
            header: header.to_vec(),
            target,
            steps,
            next_step,
            chunk: vec![],
            bulk: None,
        });
        Ok((
            Session {
                steps,
                resumed,
                resume_from: next_step,
            },
            have,
        ))
    }

    pub fn begin(&mut self, target: Target, header: &[u8]) -> Result<Session, Error> {
        let (session, _) = self.start(target, header)?;
        if let Some(ram) = &mut self.ram {
            ram.bulk = None;
        }
        Ok(session)
    }

    /// The step-order rule: `Ok(false)` to apply, `Ok(true)` for a resend of a step that is done.
    fn check_order(&self, index: u32) -> Result<bool, Error> {
        let ram = self.ram.as_ref().ok_or(Error::NoSession)?;
        if index >= ram.steps {
            return Err(Error::OutOfRange);
        }
        if index > ram.next_step {
            return Err(Error::OutOfOrder(ram.next_step));
        }
        Ok(index < ram.next_step)
    }

    /// Apply (live) or store (stored) the next step.
    fn take_step(&mut self, step: Step) -> Result<(), Error> {
        let ram = self.ram.as_mut().ok_or(Error::NoSession)?;
        let index = ram.next_step;
        let mut f = self.flash.lock().unwrap();
        match ram.target {
            Target::Stored => {
                let mut bytes = vec![];
                step.encode(&mut bytes);
                let area = &mut f.plan_area;
                if (plan::HEADER_LEN + area.body.len() + bytes.len()) as u32 > f.plan_area_size {
                    return Err(Error::PlanTooBig);
                }
                let area = &mut f.plan_area;
                area.body.extend_from_slice(&bytes);
                area.steps += 1;
            }
            Target::Live => {
                if f.cut == Some(Cut::Download(index)) {
                    f.cut = None;
                    _ = f.apply(&step, true);
                    self.power_cut = true;
                    self.cut_step = Some(index);
                    return Ok(());
                }
                f.apply(&step, false)?;
                if let Some(r) = &mut f.live_record {
                    let mut bytes = vec![];
                    step.encode(&mut bytes);
                    r.next_step = index + 1;
                    r.plan_bytes += bytes.len() as u32;
                }
            }
        }
        ram.next_step += 1;
        Ok(())
    }

    pub fn erase(&mut self, index: u32, block: u16) -> Result<(), Error> {
        if self.check_order(index)? {
            return Ok(());
        }
        self.take_step(Step::Erase { block })
    }

    pub fn copy(&mut self, index: u32, dst: u16, src: u16, hash: [u8; 8]) -> Result<(), Error> {
        if self.check_order(index)? {
            return Ok(());
        }
        let source_ok = {
            let f = self.flash();
            let src = src as usize;
            if src >= CHUNKS || dst as usize >= CHUNKS {
                return Err(Error::OutOfRange);
            }
            plan::chunk_hash(&f.active()[src * CHUNK..(src + 1) * CHUNK]) == hash
        };
        if !source_ok {
            return Err(Error::SourceMismatch);
        }
        self.take_step(Step::Copy { dst, src, hash })
    }

    pub fn write(
        &mut self,
        index: u32,
        dst: u16,
        hash: [u8; 8],
        offset: u32,
        data: &[u8],
    ) -> Result<(), Error> {
        if self.check_order(index)? {
            return Ok(());
        }
        if data.len() > self.limits.max_piece as usize {
            return Err(Error::PieceTooBig);
        }
        let ram = self.ram.as_mut().ok_or(Error::NoSession)?;
        if offset == 0 {
            ram.chunk.clear();
        }
        if offset as usize != ram.chunk.len()
            || dst as usize >= CHUNKS
            || ram.chunk.len() + data.len() > CHUNK
        {
            return Err(Error::OutOfRange);
        }
        ram.chunk.extend_from_slice(data);
        if ram.chunk.len() < CHUNK {
            return Ok(());
        }
        let data = core::mem::take(&mut ram.chunk);
        if plan::chunk_hash(&data) != hash {
            return Err(Error::HashMismatch);
        }
        self.take_step(Step::Write { dst, hash, data })
    }

    pub fn progress(&self) -> Result<Progress, Error> {
        let ram = self.ram.as_ref().ok_or(Error::NoSession)?;
        Ok(Progress {
            steps: ram.steps,
            next_step: ram.next_step,
            chunk_offset: ram.chunk.len() as u32,
            plan_bytes: ram.bulk.as_ref().map(|b| b.have).unwrap_or(0),
        })
    }

    pub fn finish(&mut self) -> Result<BootState, Error> {
        let ram = self.ram.as_ref().ok_or(Error::NoSession)?;
        let complete = match (&ram.bulk, ram.target) {
            // stored as it came: the steps are counted by the plan check below
            (Some(bulk), Target::Stored) => bulk.have == bulk.total,
            _ => ram.next_step == ram.steps,
        };
        if !complete {
            return Err(Error::Incomplete);
        }
        let hdr = Header::parse(&ram.header).expect("checked by begin");
        let mut f = self.flash();
        let state = match ram.target {
            Target::Stored => {
                f.check_stored_plan()?;
                f.plan_area.complete = true;
                BootState::PlanStored
            }
            Target::Live => {
                let bank = f.target_bank();
                if plan::digest(&f.banks[bank]) != hdr.image_digest {
                    return Err(Error::VerifyFailed);
                }
                f.pending_version = hdr.version;
                if f.is_ab() {
                    BootState::Staged
                } else {
                    f.app_digest = hdr.image_digest;
                    f.enter_updater = false;
                    f.live_record = None;
                    f.last_result = LastResult {
                        outcome: Outcome::Confirmed,
                        version: hdr.version,
                    };
                    BootState::Confirmed
                }
            }
        };
        f.boot_state = state;
        drop(f);
        self.ram = None;
        Ok(state)
    }

    pub fn abort(&mut self) {
        self.ram = None;
    }

    // ---- StageBulk

    pub fn begin_bulk(
        &mut self,
        target: Target,
        header: &[u8],
        total_len: u32,
        ack_every: u32,
    ) -> Result<Session, Error> {
        let hdr = Header::parse(header).map_err(|_| Error::BadHeader)?;
        if hdr.body_len != total_len {
            return Err(Error::BadHeader);
        }
        let (mut session, have) = self.start(target, header)?;
        let ram = self.ram.as_mut().expect("started");
        ram.bulk = Some(Bulk {
            total: total_len,
            have,
            ack_every,
            last_ack: have,
            pending: vec![],
            failed: false,
        });
        session.resume_from = have;
        Ok(session)
    }

    /// One segment from the `plan` sink.
    pub fn segment(&mut self, segment: PlanSegmentOwned) {
        let Some(ram) = self.ram.as_mut() else {
            return;
        };
        let Some(bulk) = ram.bulk.as_mut() else {
            return;
        };
        if bulk.failed {
            return;
        }
        if segment.offset != bulk.have {
            self.acks.push(PlanAck {
                have: bulk.have,
                status: BulkStatus::Gap,
            });
            return;
        }
        let data: &[u8] = &segment.data;
        if data.len() > self.limits.max_piece as usize {
            return self.fail(Error::PieceTooBig);
        }
        bulk.have += data.len() as u32;
        match ram.target {
            Target::Stored => {
                let mut f = self.flash.lock().unwrap();
                if (plan::HEADER_LEN + f.plan_area.body.len() + data.len()) as u32
                    > f.plan_area_size
                {
                    drop(f);
                    return self.fail(Error::PlanTooBig);
                }
                f.plan_area.body.extend_from_slice(data);
            }
            Target::Live => {
                bulk.pending.extend_from_slice(data);
                loop {
                    let bulk = self.ram.as_mut().unwrap().bulk.as_mut().unwrap();
                    let (step, len) = match Step::decode(&bulk.pending) {
                        Ok(Some(s)) => s,
                        Ok(None) => break,
                        Err(_) => return self.fail(Error::BadHeader),
                    };
                    bulk.pending.drain(..len);
                    if let Err(e) = self.take_step(step) {
                        return self.fail(e);
                    }
                    if self.power_cut {
                        return;
                    }
                }
            }
        }
        let bulk = self.ram.as_mut().unwrap().bulk.as_mut().unwrap();
        if bulk.have == bulk.total {
            self.acks.push(PlanAck {
                have: bulk.have,
                status: BulkStatus::Complete,
            });
        } else if bulk.ack_every > 0 && bulk.have - bulk.last_ack >= bulk.ack_every {
            bulk.last_ack = bulk.have;
            self.acks.push(PlanAck {
                have: bulk.have,
                status: BulkStatus::Receiving,
            });
        }
    }

    fn fail(&mut self, e: Error) {
        let bulk = self.ram.as_mut().unwrap().bulk.as_mut().unwrap();
        bulk.failed = true;
        self.acks.push(PlanAck {
            have: bulk.have,
            status: BulkStatus::Failed(e),
        });
    }
}

fn chunk_state(chunk: &[u8]) -> ChunkState {
    if plan::is_blank(chunk) {
        ChunkState::Blank
    } else {
        ChunkState::Hash(plan::chunk_hash(chunk))
    }
}

/// Digest of a slot's manifest: here, of its chunk hashes (blank chunks as zeros).
pub fn manifest_digest(slot: &[u8]) -> [u8; 8] {
    let mut m = vec![];
    for c in slot.chunks(CHUNK) {
        match chunk_state(c) {
            ChunkState::Hash(h) => m.extend_from_slice(&h),
            _ => m.extend_from_slice(&[0; 8]),
        }
    }
    plan::digest(&m)
}

/// What a node with `profile` accepts, for its role.
pub fn limits(profile: Profile, role: Role, plan_area: u32) -> StageLimits {
    let in_place_live = role != Role::App;
    StageLimits {
        // an updater has a small receive buffer: chunks come in two pieces
        max_piece: if in_place_live { 32 } else { CHUNK as u32 },
        in_flight: 1,
        live: matches!(profile, Profile::AB(_)) || in_place_live,
        stored: plan_area > 0 && !in_place_live,
        plan_area,
        writes_in_background: !in_place_live,
    }
}
