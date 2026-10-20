//! Observing a trait level's `+observe` properties without generated code (TOOL-11): a level's `PropertyUpdate`
//! is decoded from introspection alone (the level's resource indices and types, from [ApiBundleOwned]), reusing
//! the CLT-28 event-loop routing and subscription machinery ([crate::observe]) through a schema-less
//! [DynUpdate] / [DynConfig] pair instead of a generated `<Trait>Update` / `<Trait>Observe`.
//!
//! [DynResource::observe](crate::DynResource::observe) builds a [DynObserver] for a trait level; it decodes each
//! update the same way the generated sparse `<Trait>Update` struct does
//! (`OptionAnyVecIter::read_slot_with`/`read_slot`), just from a runtime list of `(index, type)` instead of a
//! compile-time one.
//!
//! A `+batched` property's slot is a `Batched<T>` (API-39): one value, or a `Series<T>` of samples. Its layout is
//! the framework's, not in the model, so it is decoded here by hand: a nibble tag, `T` (`One`), or a byte-aligned
//! series (`Samples`). The samples' device times are the event's `sample_times`, shared by every `Samples` slot of
//! the update and unwrapped by the level, as for a generated observer.

use crate::observe::{Granted, LevelConfig, LevelUpdate, ObserveMode, Observed, Observer, Second};
use crate::{Error, StreamError};
use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::Arc;
use std::time::Instant;
use wire_weaver::shrink_wrap::option_any_vec::OptionAnyVec;
use wire_weaver::shrink_wrap::{
    BufReader, DeserializeShrinkWrapOwned, ElementSize, Error as SWError,
};
use ww_self::{ApiBundleOwned, TypeOwned, ValueOwned};

/// Raw bytes of a level's `PropertyUpdate`, kept undecoded until [LevelSchema::decode] runs against the level's
/// introspection (TOOL-11): what plugs into [crate::Commander::prepare_observe] in place of a generated
/// `<Trait>Update`.
#[derive(Debug, Clone, Default)]
pub(crate) struct DynUpdate {
    data: Vec<u8>,
}

impl DeserializeShrinkWrapOwned for DynUpdate {
    const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;

    fn des_shrink_wrap_owned(rd: &mut BufReader<'_>) -> Result<Self, SWError> {
        Ok(DynUpdate {
            data: rd.read_raw_slice(rd.bytes_left())?.to_vec(),
        })
    }
}

impl LevelUpdate for DynUpdate {
    /// No values kept: a dynamic observer decodes each update itself
    type Latest = ();
    type Config = DynConfig;

    fn update_latest(
        self,
        _latest: &mut (),
        _received: Instant,
        _t: Option<Second>,
        _sample_times: Option<&[Second]>,
    ) {
    }
}

/// Settings of a dynamically observed level, by resource index (TOOL-11): what a generated `<Trait>Observe` is
/// for a generated client.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DynConfig(pub BTreeMap<u32, ObserveMode>);

impl LevelConfig for DynConfig {
    fn to_entries(&self) -> BTreeMap<u32, ObserveMode> {
        self.0.clone()
    }

    fn from_entries(entries: &BTreeMap<u32, ObserveMode>) -> Self {
        DynConfig(entries.clone())
    }
}

/// A level's `+observe` properties, by resource index, from introspection (TOOL-11): what
/// [DynObserver] decodes a `PropertyUpdate` against, built by `DynResource::observe_schema`.
#[derive(Clone, Debug)]
pub(crate) struct LevelSchema {
    bundle: Arc<ApiBundleOwned>,
    /// `(resource index, property name, type)`, sorted by index: the level's `+observe` properties.
    props: Vec<(u32, String, TypeOwned)>,
    /// Per entry of `props`: how its `Batched<T>` slot decodes, `None` when it isn't `+batched`.
    batched: Vec<Option<BatchedSlot>>,
}

/// The `Series<T>` of a `+batched` property's `Batched::Samples` slot (API-39).
#[derive(Clone, Debug)]
pub(crate) struct BatchedSlot {
    series_ty: TypeOwned,
}

impl BatchedSlot {
    /// From a `+batched` property's type.
    pub(crate) fn new(ty: &TypeOwned) -> Self {
        BatchedSlot {
            series_ty: TypeOwned::Series(Box::new(ty.clone())),
        }
    }
}

/// One present slot of an update.
enum DynSlot {
    /// A property that isn't `+batched`, or `Batched::One`: one value, at the update's time.
    One(ValueOwned),
    /// `Batched::Samples`: samples at the times of the event's `sample_times`, oldest first.
    Samples(Vec<ValueOwned>),
}

/// A present slot of an update, by position in `props`, with the place it takes in the update's data.
struct Slot {
    k: usize,
    slot: DynSlot,
    front: Range<usize>,
    back: Range<usize>,
}

/// A present property of an update, see [LevelSchema::view].
pub(crate) struct SlotView {
    pub name: String,
    pub ty: TypeOwned,
    pub value: ValueOwned,
    /// Bits read from the front of the update's data, nibbles from its back.
    pub front: Range<usize>,
    pub back: Range<usize>,
}

const BATCHED_ONE: u8 = 0;
const BATCHED_SAMPLES: u8 = 1;

impl LevelSchema {
    pub(crate) fn new(
        bundle: Arc<ApiBundleOwned>,
        props: Vec<(u32, String, TypeOwned)>,
        batched: Vec<Option<BatchedSlot>>,
    ) -> Self {
        debug_assert_eq!(props.len(), batched.len());
        LevelSchema {
            bundle,
            props,
            batched,
        }
    }

    /// The level's observable properties, by resource index, as given to [LevelSchema::new].
    pub(crate) fn props(&self) -> &[(u32, String, TypeOwned)] {
        &self.props
    }

    /// Resource indices of the `+batched` properties.
    pub(crate) fn batched_indices(&self) -> impl Iterator<Item = u32> + '_ {
        self.props
            .iter()
            .zip(&self.batched)
            .filter(|(_, b)| b.is_some())
            .map(|((index, ..), _)| *index)
    }

    /// The present slots of one `PropertyUpdate`'s data, by position in `props`. A present slot past the ones in
    /// the schema (a newer device) is left unread: the enclosing `Unsized` bound skips it, same as a generated
    /// sparse struct with fewer fields than the data would.
    fn decode_slots(&self, data: &[u8]) -> anyhow::Result<Vec<Slot>> {
        let vec = OptionAnyVec::new(data)?;
        let mut iter = vec.iter();
        let mut out = Vec::with_capacity(self.props.len());
        for (k, ((index, _, ty), batched)) in self.props.iter().zip(&self.batched).enumerate() {
            let mut at = None;
            let slot = iter.read_slot_with::<DynSlot, anyhow::Error>(*index, |rd| {
                let from = rd.cursors_in(data);
                let slot = match batched {
                    None => {
                        DynSlot::One(ValueOwned::des_shrink_wrap_field_dyn(rd, ty, &self.bundle)?)
                    }
                    Some(batched) => self.decode_batched(rd, ty, batched)?,
                };
                at = from.zip(rd.cursors_in(data));
                Ok(slot)
            })?;
            if let Some(slot) = slot {
                let (front, back) = at.map_or((0..0, 0..0), |(from, to)| {
                    (
                        from.front_bit..to.front_bit,
                        to.back_nibble.min(from.back_nibble)..from.back_nibble,
                    )
                });
                out.push(Slot {
                    k,
                    slot,
                    front,
                    back,
                });
            }
        }
        Ok(out)
    }

    /// The present slots of one `PropertyUpdate`'s data as they are, for a tap to show what an update carries:
    /// name, type and value of each, and the bits and nibbles it takes in `data`. A `+batched` property's samples
    /// are all there (a `Series`), without the times, which belong to the event.
    pub(crate) fn view(&self, data: &[u8]) -> anyhow::Result<Vec<SlotView>> {
        Ok(self
            .decode_slots(data)?
            .into_iter()
            .map(
                |Slot {
                     k,
                     slot,
                     front,
                     back,
                 }| {
                    let (_, name, ty) = &self.props[k];
                    let (ty, value) = match (slot, &self.batched[k]) {
                        (DynSlot::One(value), _) => (ty.clone(), value),
                        (DynSlot::Samples(items), Some(b)) => {
                            (b.series_ty.clone(), ValueOwned::Vec(items))
                        }
                        (DynSlot::Samples(items), None) => (ty.clone(), ValueOwned::Vec(items)),
                    };
                    SlotView {
                        name: name.clone(),
                        ty,
                        value,
                        front,
                        back,
                    }
                },
            )
            .collect())
    }

    /// A `Batched<T>` slot (API-39), `Unsized`: its length, a nibble tag, then `T` or a byte-aligned series.
    fn decode_batched(
        &self,
        rd: &mut BufReader<'_>,
        ty: &TypeOwned,
        batched: &BatchedSlot,
    ) -> anyhow::Result<DynSlot> {
        let len = rd.read_rev_len()?;
        let mut rd = rd.split(len)?;
        match rd.read_nib()?.value() {
            BATCHED_ONE => Ok(DynSlot::One(ValueOwned::des_shrink_wrap_field_dyn(
                &mut rd,
                ty,
                &self.bundle,
            )?)),
            BATCHED_SAMPLES => {
                rd.align_byte();
                let bytes = rd.read_raw_slice(rd.bytes_left())?;
                let ValueOwned::Vec(items) =
                    ValueOwned::des_shrink_wrap_dyn(bytes, &batched.series_ty, &self.bundle)?
                else {
                    anyhow::bail!("expected a Series value");
                };
                Ok(DynSlot::Samples(items))
            }
            tag => anyhow::bail!("Batched tag {tag}: from a newer device or malformed"),
        }
    }

    /// Decode one `PropertyUpdate`'s data into its present properties, by name, in schema (resource index) order:
    /// the latest value of each, and every sample of the `+batched` ones. `sample_times` are the samples' device
    /// times (the event's, unwrapped by the level), `t` the update's. Every `Batched::Samples` slot has as many
    /// samples as there are times, else the update is malformed.
    fn decode(
        &self,
        data: &[u8],
        sample_times: Option<&[Second]>,
        t: Option<Second>,
    ) -> anyhow::Result<(Values, Vec<DynSamples>)> {
        let mut values = Vec::with_capacity(self.props.len());
        let mut samples = vec![];
        let times = sample_times.unwrap_or_default();
        for Slot { k, slot, .. } in self.decode_slots(data)? {
            let name = &self.props[k].1;
            let batched = self.batched[k].is_some();
            let items = match slot {
                DynSlot::One(value) if !batched => {
                    values.push((name.clone(), value));
                    continue;
                }
                DynSlot::One(value) => vec![(value, t)],
                DynSlot::Samples(items) => {
                    if items.len() != times.len() {
                        anyhow::bail!(
                            "{name}: {} samples, {} sample times",
                            items.len(),
                            times.len()
                        );
                    }
                    items
                        .into_iter()
                        .zip(times)
                        .map(|(value, t)| (value, Some(*t)))
                        .collect()
                }
            };
            if let Some((last, _)) = items.last() {
                values.push((name.clone(), last.clone()));
            }
            samples.push(DynSamples {
                name: name.clone(),
                items,
            });
        }
        Ok((values, samples))
    }
}

/// Properties by name, see [DynObserved::values].
type Values = Vec<(String, ValueOwned)>;

/// Every sample of one `+batched` property in an update (API-39), see [DynObserved::samples].
#[derive(Debug, Clone)]
pub struct DynSamples {
    /// The property's name.
    pub name: String,
    /// The samples, oldest first, each with its device time (unwrapped as [DynObserved::t]); one value sent on
    /// its own has the update's time.
    pub items: Vec<(ValueOwned, Option<Second>)>,
}

/// One decoded update from a [DynObserver]: the level's present properties, by name, in schema order.
#[derive(Debug, Clone)]
pub struct DynObserved {
    /// Present properties, by name, in schema (resource index) order. A `+batched` property gives its newest
    /// sample here, all of them in [Self::samples].
    pub values: Vec<(String, ValueOwned)>,
    /// Every sample of the present `+batched` properties (API-39), in schema order.
    pub samples: Vec<DynSamples>,
    /// When the device took the values, on its clock, if it sends timestamps. An update carrying only batches
    /// has none: each sample has its own.
    pub t: Option<Second>,
    /// Sends of this level that were due but did not fit the budget since the previous update, and samples
    /// lost before they could be sent.
    pub missed: u32,
    /// When the update arrived.
    pub received: Instant,
    /// Bytes of the `PropertyUpdate`'s data (before decoding), e.g. for `ww observe`'s link bytes/s.
    pub data_len: usize,
}

/// Observes a trait level's `+observe` properties without generated code (TOOL-11), see
/// [DynResource::observe](crate::DynResource::observe).
pub struct DynObserver {
    inner: Observer<DynUpdate, DynUpdate>,
    schema: Arc<LevelSchema>,
}

impl DynObserver {
    pub(crate) fn new(inner: Observer<DynUpdate, DynUpdate>, schema: Arc<LevelSchema>) -> Self {
        DynObserver { inner, schema }
    }

    fn decode(&self, observed: Observed<DynUpdate>) -> Result<DynObserved, StreamError> {
        let update = observed.value;
        let data_len = update.data.len();
        let (values, samples) = self
            .schema
            .decode(&update.data, observed.sample_times.as_deref(), observed.t)
            .map_err(|e| {
                StreamError::Other(Error::User(format!("decoding PropertyUpdate: {e:#}")))
            })?;
        Ok(DynObserved {
            values,
            samples,
            t: observed.t,
            missed: observed.missed,
            received: observed.received,
            data_len,
        })
    }

    /// Next update, skipping link state events ([StreamError::Closed] once nothing more can come).
    pub async fn recv(&mut self) -> Result<DynObserved, StreamError> {
        let observed = self.inner.recv().await?;
        self.decode(observed)
    }

    /// Blocking variant of [Self::recv].
    pub fn recv_blocking(&mut self) -> Result<DynObserved, StreamError> {
        let observed = self.inner.recv_blocking()?;
        self.decode(observed)
    }

    /// An update if one is queued, skipping link state events.
    pub fn try_recv(&mut self) -> Result<Option<DynObserved>, StreamError> {
        match self.inner.try_recv()? {
            Some(observed) => Ok(Some(self.decode(observed)?)),
            None => Ok(None),
        }
    }

    /// What the device applies to the level (for all its observers), `None` before it answered.
    pub fn granted(&self) -> Option<Granted<DynConfig>> {
        self.inner.granted()
    }

    /// Change what this observer asks for, its budget stays.
    pub async fn configure(
        &self,
        entries: BTreeMap<u32, ObserveMode>,
    ) -> Result<Granted<DynConfig>, Error> {
        self.inner.configure(DynConfig(entries)).await
    }

    /// Blocking variant of [Self::configure].
    pub fn configure_blocking(
        &self,
        entries: BTreeMap<u32, ObserveMode>,
    ) -> Result<Granted<DynConfig>, Error> {
        self.inner.configure_blocking(DynConfig(entries))
    }

    /// Updates dropped because this observer's queue on the host was full.
    pub fn dropped(&self) -> u64 {
        self.inner.dropped()
    }
}
