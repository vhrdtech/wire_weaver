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
//! series (`Samples`). The samples' device times are the level's `sample_times` slot (a `Series` of the
//! properties' `sample_time_ty`, past the level's last resource index), shared by every `Samples` slot of the
//! update. The level unwraps them (it keeps the [LevelSchema] in its latest-value slot for that, [DynLatest]), so
//! they count on the same way as a generated observer's.

use crate::observe::{Granted, LevelConfig, LevelUpdate, ObserveMode, Observed, Observer, Second};
use crate::{Error, StreamError};
use std::collections::BTreeMap;
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
    /// Device times of the samples in `Batched::Samples` slots (the level's `sample_times`), unwrapped by the level
    /// (API-39)
    times: Vec<Second>,
}

impl DeserializeShrinkWrapOwned for DynUpdate {
    const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;

    fn des_shrink_wrap_owned(rd: &mut BufReader<'_>) -> Result<Self, SWError> {
        Ok(DynUpdate {
            data: rd.read_raw_slice(rd.bytes_left())?.to_vec(),
            times: vec![],
        })
    }
}

/// The level's latest-value slot for dynamic observers: no values, the schema the level finds the samples' times
/// with (API-39).
#[derive(Debug, Default)]
pub(crate) struct DynLatest {
    pub(crate) schema: Option<Arc<LevelSchema>>,
}

impl LevelUpdate for DynUpdate {
    type Latest = DynLatest;
    type Config = DynConfig;

    fn update_latest(self, _latest: &mut Self::Latest, _received: Instant, _t: Option<Second>) {}

    fn level_times_mut(&mut self, latest: &Self::Latest, f: &mut dyn FnMut(&mut Second)) {
        let Some(schema) = &latest.schema else {
            return;
        };
        // an update that doesn't decode is reported by the observers, which decode it again
        if let Ok((_, times)) = schema.decode_slots(&self.data) {
            for mut t in times {
                f(&mut t);
            }
        }
    }

    fn set_times(&mut self, times: &[Second]) {
        self.times = times.to_vec();
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
    /// The level's `sample_times` slot (API-39, shared clock), with `+batched` properties.
    sample_times: Option<SampleTimes>,
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

/// A level's `sample_times` slot (API-39): a `Series<Second>` past the level's last resource index, the device
/// times of the samples in the update's `Batched::Samples` slots.
#[derive(Clone, Debug)]
pub(crate) struct SampleTimes {
    /// Resource index of the slot: the level's largest + 1
    pub(crate) index: u32,
    /// `ww_si::Second`, from the properties' `sample_time_ty`
    pub(crate) second_ty: TypeOwned,
}

/// One present slot of an update.
enum DynSlot {
    /// A property that isn't `+batched`, or `Batched::One`: one value, at the update's time.
    One(ValueOwned),
    /// `Batched::Samples`: samples at the times of the update's `sample_times`, oldest first.
    Samples(Vec<ValueOwned>),
}

const BATCHED_ONE: u8 = 0;
const BATCHED_SAMPLES: u8 = 1;

impl LevelSchema {
    pub(crate) fn new(
        bundle: Arc<ApiBundleOwned>,
        props: Vec<(u32, String, TypeOwned)>,
        batched: Vec<Option<BatchedSlot>>,
        sample_times: Option<SampleTimes>,
    ) -> Self {
        debug_assert_eq!(props.len(), batched.len());
        LevelSchema {
            bundle,
            props,
            batched,
            sample_times,
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

    /// The present slots of one `PropertyUpdate`'s data, by position in `props`, and the times in its
    /// `sample_times` (as sent, wrapping). A present slot past the ones in the schema (a newer device) is left
    /// unread: the enclosing `Unsized` bound skips it, same as a generated sparse struct with fewer fields than the
    /// data would.
    fn decode_slots(&self, data: &[u8]) -> anyhow::Result<(Vec<(usize, DynSlot)>, Vec<Second>)> {
        let vec = OptionAnyVec::new(data)?;
        let mut iter = vec.iter();
        let mut out = Vec::with_capacity(self.props.len());
        for (k, ((index, _, ty), batched)) in self.props.iter().zip(&self.batched).enumerate() {
            let slot =
                iter.read_slot_with::<DynSlot, anyhow::Error>(*index, |rd| match batched {
                    None => Ok(DynSlot::One(ValueOwned::des_shrink_wrap_field_dyn(
                        rd,
                        ty,
                        &self.bundle,
                    )?)),
                    Some(batched) => self.decode_batched(rd, ty, batched),
                })?;
            if let Some(slot) = slot {
                out.push((k, slot));
            }
        }
        let mut times = vec![];
        if let Some(sample_times) = &self.sample_times {
            let series_ty = TypeOwned::Series(Box::new(sample_times.second_ty.clone()));
            let values = iter.read_slot_with::<ValueOwned, anyhow::Error>(sample_times.index, |rd| {
                Ok(ValueOwned::des_shrink_wrap_field_dyn(
                    rd,
                    &series_ty,
                    &self.bundle,
                )?)
            })?;
            if let Some(values) = values {
                let ValueOwned::Vec(values) = values else {
                    anyhow::bail!("sample_times: expected a Series value");
                };
                for t in values {
                    let t = crate::quantity::DynQuantity::from_value(
                        &t,
                        &sample_times.second_ty,
                        &self.bundle,
                    )
                    .ok_or_else(|| anyhow::anyhow!("sample_times: not a quantity"))?;
                    times.push(Second::from_parts(t.prefix, t.value));
                }
            }
        }
        Ok((out, times))
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
    /// the latest value of each, and every sample of the `+batched` ones. `times` are the samples' device times
    /// the level unwrapped (its `sample_times`); `t` the update's. Every `Batched::Samples` slot has as many
    /// samples as there are times, else the update is malformed.
    fn decode(
        &self,
        data: &[u8],
        times: &[Second],
        t: Option<Second>,
    ) -> anyhow::Result<(Values, Vec<DynSamples>)> {
        let mut values = Vec::with_capacity(self.props.len());
        let mut samples = vec![];
        let (slots, sent_times) = self.decode_slots(data)?;
        if times.len() != sent_times.len() {
            anyhow::bail!(
                "{} sample times unwrapped, {} sent",
                times.len(),
                sent_times.len()
            );
        }
        for (k, slot) in slots {
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
            .decode(&update.data, &update.times, observed.t)
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
