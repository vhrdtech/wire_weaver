//! Observing a trait level's `+observe` properties without generated code (TOOL-11): a level's `PropertyUpdate`
//! is decoded from introspection alone (the level's resource indices and types, from [ApiBundleOwned]), reusing
//! the CLT-28 event-loop routing and subscription machinery ([crate::observe]) through a schema-less
//! [DynUpdate] / [DynConfig] pair instead of a generated `<Trait>Update` / `<Trait>Observe`.
//!
//! [DynResource::observe](crate::DynResource::observe) builds a [DynObserver] for a trait level; it decodes each
//! update the same way the generated sparse `<Trait>Update` struct does
//! (`OptionAnyVecIter::read_slot_with`/`read_slot`), just from a runtime list of `(index, type)` instead of a
//! compile-time one.

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
pub(crate) struct DynUpdate(Vec<u8>);

impl DeserializeShrinkWrapOwned for DynUpdate {
    const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;

    fn des_shrink_wrap_owned(rd: &mut BufReader<'_>) -> Result<Self, SWError> {
        Ok(DynUpdate(rd.read_raw_slice(rd.bytes_left())?.to_vec()))
    }
}

impl LevelUpdate for DynUpdate {
    type Latest = ();
    type Config = DynConfig;

    fn update_latest(self, _latest: &mut Self::Latest, _received: Instant, _t: Option<Second>) {}
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
}

impl LevelSchema {
    pub(crate) fn new(bundle: Arc<ApiBundleOwned>, props: Vec<(u32, String, TypeOwned)>) -> Self {
        LevelSchema { bundle, props }
    }

    /// The level's observable properties, by resource index, as given to [LevelSchema::new].
    pub(crate) fn props(&self) -> &[(u32, String, TypeOwned)] {
        &self.props
    }

    /// Decode one `PropertyUpdate`'s data into its present properties, by name, in schema (resource index) order.
    /// A present slot past the ones in the schema (a newer device) is left unread: the enclosing `Unsized` bound
    /// skips it, same as a generated sparse struct with fewer fields than the data would.
    fn decode(&self, data: &[u8]) -> anyhow::Result<Vec<(String, ValueOwned)>> {
        let vec = OptionAnyVec::new(data)?;
        let mut iter = vec.iter();
        let mut out = Vec::with_capacity(self.props.len());
        for (index, name, ty) in &self.props {
            let value = iter.read_slot_with::<ValueOwned, anyhow::Error>(*index, |rd| {
                ValueOwned::des_shrink_wrap_field_dyn(rd, ty, &self.bundle)
            })?;
            if let Some(value) = value {
                out.push((name.clone(), value));
            }
        }
        Ok(out)
    }
}

/// One decoded update from a [DynObserver]: the level's present properties, by name, in schema order.
#[derive(Debug, Clone)]
pub struct DynObserved {
    /// Present properties, by name, in schema (resource index) order.
    pub values: Vec<(String, ValueOwned)>,
    /// When the device took the values, on its clock, if it sends timestamps.
    pub t: Option<Second>,
    /// Sends of this level that were due but did not fit the budget since the previous update.
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
    schema: LevelSchema,
}

impl DynObserver {
    pub(crate) fn new(inner: Observer<DynUpdate, DynUpdate>, schema: LevelSchema) -> Self {
        DynObserver { inner, schema }
    }

    fn decode(&self, observed: Observed<DynUpdate>) -> Result<DynObserved, StreamError> {
        let data_len = observed.value.0.len();
        let values = self.schema.decode(&observed.value.0).map_err(|e| {
            StreamError::Other(Error::User(format!("decoding PropertyUpdate: {e:#}")))
        })?;
        Ok(DynObserved {
            values,
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
