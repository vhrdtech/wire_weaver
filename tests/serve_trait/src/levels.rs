//! Nested levels and resource arrays through the local traits (CLT-58): [LocalPlant] implements [Plant] and hands
//! out its levels by reference, [server::Server] forwards every item of every level to it (`serve_trait`), and
//! [client::Client] implements the same traits on its level structs.

use futures_lite::stream;
use serve_trait_gauge_api::{Gauge, Reading};
use serve_trait_levels_api::{Channel, ChannelIdx, Desk, Level, Plant, PlantError, Tag, Unit};
use std::sync::Mutex;
use wire_weaver::call::{CallError, Error, ErrorKind, ItemStream, item_stream};
use wire_weaver::shrink_wrap::UNib32;
use wire_weaver::{CallContext, ValidIndicesOwned};
use ww_keyed::KeyEvent;

use super::locked;

/// Units, slots and channels per unit.
pub const UNITS: u32 = 2;
pub const SLOTS: u32 = 3;
pub const CHANNELS: u8 = 2;

/// The tag of unit `index`.
pub fn unit_tag(index: u32) -> Tag {
    Tag(10 + index)
}

/// The key of channel `index`.
pub fn channel_key(index: u8) -> Level {
    Level(100 + u32::from(index))
}

/// The plant: its levels are fields, handed out as `&` references.
pub struct LocalPlant {
    desk: LocalDesk,
    units: Vec<LocalUnit>,
    gauge: LocalGauge,
    slots: Mutex<Vec<Level>>,
}

impl Default for LocalPlant {
    fn default() -> Self {
        LocalPlant {
            desk: LocalDesk {
                lamp: Mutex::new(Level(0)),
            },
            units: (0..UNITS)
                .map(|id| LocalUnit {
                    id,
                    level: Mutex::new(Level(0)),
                    channels: (0..CHANNELS).map(LocalChannel).collect(),
                })
                .collect(),
            gauge: LocalGauge {
                zero: Mutex::new(Reading(0)),
            },
            slots: Mutex::new(vec![Level(0); SLOTS as usize]),
        }
    }
}

pub struct LocalDesk {
    lamp: Mutex<Level>,
}

pub struct LocalUnit {
    id: u32,
    level: Mutex<Level>,
    channels: Vec<LocalChannel>,
}

pub struct LocalChannel(u8);

pub struct LocalGauge {
    zero: Mutex<Reading>,
}

/// `0..n` as valid indices.
fn first(n: u32) -> ValidIndicesOwned {
    ValidIndicesOwned::Range(UNib32(0)..UNib32(n))
}

/// The element at `index` of `elements`, `BadIndex` past the end.
fn element<T>(elements: &[T], index: usize) -> Result<&T, Error> {
    elements
        .get(index)
        .ok_or_else(|| Error::from_kind(ErrorKind::BadIndex))
}

impl Plant for LocalPlant {
    type Desk<'a> = &'a LocalDesk;

    fn desk(&self) -> &LocalDesk {
        &self.desk
    }

    type Unit<'a> = &'a LocalUnit;

    fn unit(&self, index: u32) -> Result<&LocalUnit, Error> {
        element(&self.units, index as usize)
    }

    type Gauge<'a> = &'a LocalGauge;

    fn gauge(&self) -> &LocalGauge {
        &self.gauge
    }

    async fn unit_valid_indices(&self, _cx: &CallContext) -> Result<ValidIndicesOwned, Error> {
        Ok(first(UNITS))
    }

    async fn unit_index_of(&self, _cx: &CallContext, key: Tag) -> Result<Option<u32>, Error> {
        Ok((0..UNITS).find(|&index| unit_tag(index) == key))
    }

    async fn unit_key(&self, _cx: &CallContext, index: u32) -> Result<Tag, Error> {
        element(&self.units, index as usize).map(|unit| unit_tag(unit.id))
    }

    async fn unit_keys(&self, _cx: &CallContext) -> Result<ItemStream<KeyEvent<u32, Tag>>, Error> {
        let added = (0..UNITS).map(|index| {
            Ok(KeyEvent::Added {
                index,
                key: unit_tag(index),
            })
        });
        Ok(item_stream(stream::iter(added)))
    }

    async fn read_slot(&self, _cx: &CallContext, index: u32) -> Result<Level, Error> {
        element(&locked(&self.slots), index as usize).copied()
    }

    async fn write_slot(&self, _cx: &CallContext, index: u32, slot: Level) -> Result<(), Error> {
        let mut slots = locked(&self.slots);
        let Some(s) = slots.get_mut(index as usize) else {
            return Err(ErrorKind::BadIndex.into());
        };
        *s = slot;
        Ok(())
    }

    async fn slot_valid_indices(&self, _cx: &CallContext) -> Result<ValidIndicesOwned, Error> {
        Ok(first(SLOTS))
    }

    async fn feed(&self, _cx: &CallContext, index: u32) -> Result<ItemStream<Level>, Error> {
        let level = element(&locked(&self.slots), index as usize)?.0;
        Ok(item_stream(stream::iter([
            Ok(Level(level)),
            Ok(Level(level * 2)),
        ])))
    }

    async fn feed_valid_indices(&self, _cx: &CallContext) -> Result<ValidIndicesOwned, Error> {
        Ok(first(SLOTS))
    }
}

impl Desk for LocalDesk {
    async fn ping(&self, _cx: &CallContext, n: Level) -> Result<Level, Error> {
        Ok(Level(n.0 + 1))
    }

    async fn read_lamp(&self, _cx: &CallContext) -> Result<Level, Error> {
        Ok(*locked(&self.lamp))
    }

    async fn write_lamp(&self, _cx: &CallContext, lamp: Level) -> Result<(), Error> {
        *locked(&self.lamp) = lamp;
        Ok(())
    }
}

impl Unit for LocalUnit {
    type Channel<'a> = &'a LocalChannel;

    fn channel(&self, index: ChannelIdx) -> Result<&LocalChannel, Error> {
        element(&self.channels, usize::from(index.0))
    }

    async fn channel_valid_indices(&self, _cx: &CallContext) -> Result<ValidIndicesOwned, Error> {
        Ok(first(u32::from(CHANNELS)))
    }

    async fn channel_index_of(
        &self,
        _cx: &CallContext,
        key: Level,
    ) -> Result<Option<ChannelIdx>, Error> {
        Ok((0..CHANNELS)
            .find(|&index| channel_key(index) == key)
            .map(ChannelIdx))
    }

    async fn channel_key(&self, _cx: &CallContext, index: ChannelIdx) -> Result<Level, Error> {
        element(&self.channels, usize::from(index.0)).map(|channel| channel_key(channel.0))
    }

    async fn channel_keys(
        &self,
        _cx: &CallContext,
    ) -> Result<ItemStream<KeyEvent<ChannelIdx, Level>>, Error> {
        // the last channel only: its element is gone again
        let last = CHANNELS - 1;
        Ok(item_stream(stream::iter([
            Ok(KeyEvent::Added {
                index: ChannelIdx(last),
                key: channel_key(last),
            }),
            Ok(KeyEvent::Removed {
                index: ChannelIdx(last),
            }),
        ])))
    }

    async fn id(&self, _cx: &CallContext) -> Result<Level, Error> {
        Ok(Level(self.id))
    }

    async fn read_level(&self, _cx: &CallContext) -> Result<Level, Error> {
        Ok(*locked(&self.level))
    }

    async fn write_level(&self, _cx: &CallContext, level: Level) -> Result<(), Error> {
        *locked(&self.level) = level;
        Ok(())
    }

    async fn ticks(
        &self,
        _cx: &CallContext,
        count: Level,
    ) -> Result<Result<(Level, ItemStream<Level>), PlantError>, Error> {
        if count.0 == 0 {
            return Ok(Err(PlantError::Refused));
        }
        let from = self.id;
        let items = stream::iter((from..from + count.0).map(|n| Ok(Level(n))));
        Ok(Ok((count, item_stream(items))))
    }
}

impl Channel for LocalChannel {
    async fn gain(&self, _cx: &CallContext, by: Level) -> Result<Level, CallError<PlantError>> {
        if by.0 == 0 {
            return Err(CallError::Api(PlantError::Refused));
        }
        Ok(Level(by.0 * (u32::from(self.0) + 1)))
    }
}

impl Gauge for LocalGauge {
    async fn reading(&self, _cx: &CallContext) -> Result<Reading, Error> {
        Ok(Reading(100 - locked(&self.zero).0))
    }

    async fn read_zero(&self, _cx: &CallContext) -> Result<Reading, Error> {
        Ok(*locked(&self.zero))
    }

    async fn write_zero(&self, _cx: &CallContext, zero: Reading) -> Result<(), Error> {
        *locked(&self.zero) = zero;
        Ok(())
    }

    async fn readings(&self, _cx: &CallContext) -> Result<ItemStream<Reading>, Error> {
        Ok(item_stream(stream::iter((0..3).map(|n| Ok(Reading(n))))))
    }
}

/// The server: every handler of every level generated, forwarding to the shared [LocalPlant].
pub mod server {
    use super::LocalPlant;
    use std::sync::Arc;
    use wire_weaver_server::{Backend, Connection, Pumps, Serves};

    /// One connection's server.
    pub struct Server {
        plant: Arc<LocalPlant>,
        pumps: Pumps,
    }

    impl Server {
        /// The server of `connection`, forwarding to `plant`.
        pub fn new(plant: Arc<LocalPlant>, connection: &Connection) -> Self {
            Server {
                plant,
                pumps: connection.pumps.clone(),
            }
        }
    }

    impl Serves for Server {
        type Service = LocalPlant;
        type Pumps = Pumps;

        fn service(&self) -> &LocalPlant {
            &self.plant
        }

        fn pumps(&self) -> &Pumps {
            &self.pumps
        }
    }

    impl Backend for Server {}

    /// What a client checks in link setup.
    pub fn link_config() -> ww_device::LinkConfig<'static> {
        ww_device::LinkConfig::new(
            serve_trait_levels_api::PLANT_API_FULL_GID,
            serve_trait_levels_api::PLANT_API_WIRE_VERSION,
            api_impl::api_hash(),
            api_impl::image_facts(),
            ww_client_server::COMPACT_VERSION,
        )
    }

    pub mod api_impl {
        wire_weaver::ww_codegen!(
            serve_trait_levels_api :: PlantApi for super::Server,
            server = true,
            serve_trait = "serve_trait_levels_api::Plant",
        );
    }
}

/// The generated client, implementing [Plant] and, on its level structs, the levels' traits.
pub mod client {
    use wire_weaver_client::{ClientConfig, Commander, WwClient};

    pub struct Client {
        pub cmd: Commander,
    }

    impl WwClient for Client {
        fn default_config() -> ClientConfig {
            ClientConfig::default()
        }

        fn try_from_cmd(cmd: Commander) -> Result<Self, wire_weaver_client::Error> {
            Ok(Self { cmd })
        }
    }

    mod api_client {
        wire_weaver::ww_codegen!(
            serve_trait_levels_api :: PlantApi for super::Client,
            client = "std_client",
            impl_local_trait = "serve_trait_levels_api::Plant",
        );
    }
}

/// The traits of the API and its levels without `Send` (SRV-73), made here: `Plant`, `Desk`, `Unit` and `Channel`
/// with `LocalBoxFuture`s, `Rc` trait objects and `LocalItemStream`s; the gauge's level stays its crate's.
pub mod not_send {
    wire_weaver::ww_codegen!(
        serve_trait_levels_api::PlantApi,
        local_trait = "Plant",
        local_trait_send = false,
    );
}
