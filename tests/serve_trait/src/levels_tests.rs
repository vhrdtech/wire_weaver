use crate::levels::{LocalPlant, SLOTS, UNITS, channel_key, client, server, unit_tag};
use futures_lite::StreamExt;
use serve_trait_gauge_api::{Gauge, Reading};
use serve_trait_levels_api::{
    Channel, ChannelIdx, Desk, Level, Plant, PlantDyn, PlantError, Tag, Unit,
};
use std::sync::Arc;
use tokio::sync::oneshot;
use wire_weaver::call::{CallError, Error, ErrorKind};
use wire_weaver::shrink_wrap::UNib32;
use wire_weaver::{CallContext, ValidIndicesOwned};
use wire_weaver_client::in_process::TokioClock;
use wire_weaver_server::in_process::InProcessListener;
use wire_weaver_server::{Connection, Server};
use ww_keyed::KeyEvent;

/// A [LocalPlant] served on `path` until the sender is dropped, and a client connected to it.
async fn served(path: &str) -> (client::Client, oneshot::Sender<()>) {
    tests_common::api!(server::api_impl).seed();
    let plant = Arc::new(LocalPlant::default());
    let listener = InProcessListener::new(path, 4096);
    let (stop, stopped) = oneshot::channel::<()>();
    tokio::spawn(async move {
        let make = move |connection: &Connection| server::Server::new(plant.clone(), connection);
        Server::new(TokioClock::new())
            .serve(server::link_config(), listener, make, async {
                let _ = stopped.await;
            })
            .await
            .expect("serve");
    });
    let path = path.to_owned();
    let client = client::Client::config(|c| {
        c.in_process_path(path.clone())
            .resolver(tests_common::resolver())
    })
    .connect()
    .await
    .expect("connect");
    (client, stop)
}

/// The id of unit `index`, or the kind of the error on the way: the local plant refuses the index at the accessor,
/// the client at the call.
async fn unit_id(plant: &impl Plant, cx: &CallContext, index: u32) -> Result<Level, ErrorKind> {
    let unit = plant.unit(index).map_err(|e| e.kind())?;
    unit.id(cx).await.map_err(|e| e.kind())
}

/// The gain of channel `index` of unit 1, as [unit_id] does.
async fn channel_gain(
    plant: &impl Plant,
    cx: &CallContext,
    index: u8,
    by: u32,
) -> Result<Level, CallError<PlantError>> {
    let unit = plant.unit(1)?;
    let channel = unit.channel(ChannelIdx(index))?;
    channel.gain(cx, Level(by)).await
}

/// Every item of every level, the same whatever implements the traits.
async fn exercise(plant: &impl Plant) {
    let cx = &CallContext::local();

    // a nested level
    let desk = plant.desk();
    assert_eq!(desk.ping(cx, Level(3)).await.unwrap(), Level(4));
    desk.write_lamp(cx, Level(7)).await.unwrap();
    assert_eq!(desk.read_lamp(cx).await.unwrap(), Level(7));

    // an array of levels
    let all = ValidIndicesOwned::Range(UNib32(0)..UNib32(UNITS));
    assert_eq!(plant.unit_valid_indices(cx).await.unwrap(), all);
    assert_eq!(unit_id(plant, cx, 1).await, Ok(Level(1)));
    assert_eq!(unit_id(plant, cx, UNITS).await, Err(ErrorKind::BadIndex));
    let unit = plant.unit(1).unwrap();
    unit.write_level(cx, Level(5)).await.unwrap();
    assert_eq!(unit.read_level(cx).await.unwrap(), Level(5));
    assert_eq!(
        plant.unit(0).unwrap().read_level(cx).await.unwrap(),
        Level(0)
    );
    let (count, ticks) = unit.ticks(cx, Level(3)).await.unwrap().unwrap();
    assert_eq!(count, Level(3));
    let ticks: Vec<_> = ticks.map(Result::unwrap).collect().await;
    assert_eq!(ticks, [Level(1), Level(2), Level(3)]);
    assert!(matches!(
        unit.ticks(cx, Level(0)).await,
        Ok(Err(PlantError::Refused))
    ));

    // a nested array under a typed index
    assert_eq!(channel_gain(plant, cx, 1, 3).await.unwrap(), Level(6));
    assert!(matches!(
        channel_gain(plant, cx, 0, 0).await,
        Err(CallError::Api(PlantError::Refused))
    ));
    assert_eq!(
        channel_gain(plant, cx, 9, 1).await.unwrap_err().kind(),
        Some(ErrorKind::BadIndex)
    );
    assert_eq!(
        unit.channel_valid_indices(cx).await.unwrap(),
        ValidIndicesOwned::Range(UNib32(0)..UNib32(2))
    );

    // keyed arrays: of levels, and nested under a typed index
    assert_eq!(plant.unit_index_of(cx, unit_tag(1)).await.unwrap(), Some(1));
    assert_eq!(plant.unit_index_of(cx, Tag(99)).await.unwrap(), None);
    assert_eq!(plant.unit_key(cx, 0).await.unwrap(), unit_tag(0));
    assert_eq!(
        plant.unit_key(cx, UNITS).await.unwrap_err().kind(),
        ErrorKind::BadIndex
    );
    let by_tag = plant.unit_by(cx, unit_tag(1)).await.unwrap().unwrap();
    assert_eq!(by_tag.id(cx).await.unwrap(), Level(1));
    assert!(plant.unit_by(cx, Tag(99)).await.unwrap().is_none());
    let keys: Vec<_> = plant
        .unit_keys(cx)
        .await
        .unwrap()
        .map(Result::unwrap)
        .collect()
        .await;
    let added = |index| KeyEvent::Added {
        index,
        key: unit_tag(index),
    };
    assert_eq!(keys, [added(0), added(1)]);
    assert_eq!(
        unit.channel_index_of(cx, channel_key(1)).await.unwrap(),
        Some(ChannelIdx(1))
    );
    assert_eq!(
        unit.channel_key(cx, ChannelIdx(0)).await.unwrap(),
        channel_key(0)
    );
    let channel = unit.channel_by(cx, channel_key(1)).await.unwrap().unwrap();
    assert_eq!(channel.gain(cx, Level(3)).await.unwrap(), Level(6));
    let keys: Vec<_> = unit
        .channel_keys(cx)
        .await
        .unwrap()
        .map(Result::unwrap)
        .collect()
        .await;
    assert_eq!(
        keys,
        [
            KeyEvent::Added {
                index: ChannelIdx(1),
                key: channel_key(1)
            },
            KeyEvent::Removed {
                index: ChannelIdx(1)
            }
        ]
    );

    // a level from another crate
    let gauge = plant.gauge();
    gauge.write_zero(cx, Reading(40)).await.unwrap();
    assert_eq!(gauge.read_zero(cx).await.unwrap(), Reading(40));
    assert_eq!(gauge.reading(cx).await.unwrap(), Reading(60));
    let readings: Vec<_> = gauge
        .readings(cx)
        .await
        .unwrap()
        .map(Result::unwrap)
        .collect()
        .await;
    assert_eq!(readings, [Reading(0), Reading(1), Reading(2)]);

    // arrays of properties and streams
    let slots = ValidIndicesOwned::Range(UNib32(0)..UNib32(SLOTS));
    assert_eq!(plant.slot_valid_indices(cx).await.unwrap(), slots);
    assert_eq!(plant.feed_valid_indices(cx).await.unwrap(), slots);
    plant.write_slot(cx, 2, Level(21)).await.unwrap();
    assert_eq!(plant.read_slot(cx, 2).await.unwrap(), Level(21));
    assert_eq!(plant.read_slot(cx, 0).await.unwrap(), Level(0));
    let e: Error = plant.read_slot(cx, SLOTS).await.unwrap_err();
    assert_eq!(e.kind(), ErrorKind::BadIndex);
    let feed: Vec<_> = plant
        .feed(cx, 2)
        .await
        .unwrap()
        .map(Result::unwrap)
        .collect()
        .await;
    assert_eq!(feed, [Level(21), Level(42)]);
}

#[tokio::test]
async fn local_plant_directly() {
    exercise(&LocalPlant::default()).await;
}

#[tokio::test]
async fn local_plant_as_trait_object() {
    let plant: Box<dyn PlantDyn> = Box::new(LocalPlant::default());
    exercise(&plant).await;
    let plant: Arc<dyn PlantDyn> = Arc::new(LocalPlant::default());
    exercise(&plant).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_of_a_served_plant() {
    let (client, _stop) = served("serve_trait/levels").await;
    exercise(&client).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_as_trait_object() {
    let (client, _stop) = served("serve_trait/levels_dyn").await;
    let plant: Arc<dyn PlantDyn> = Arc::new(client);
    exercise(&plant).await;
}

/// The index of a stream of an array names its pump: two elements' streams run side by side, each with its own items.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pumps_of_array_elements() {
    let (client, _stop) = served("serve_trait/levels_pumps").await;
    let cx = &CallContext::local();
    // through the trait: the client's own `feed(index)` prepares the open
    Plant::write_slot(&client, cx, 0, Level(1)).await.unwrap();
    Plant::write_slot(&client, cx, 1, Level(10)).await.unwrap();
    let first = Plant::feed(&client, cx, 0).await.unwrap();
    let second = Plant::feed(&client, cx, 1).await.unwrap();
    let second: Vec<_> = second.map(Result::unwrap).collect().await;
    let first: Vec<_> = first.map(Result::unwrap).collect().await;
    assert_eq!(first, [Level(1), Level(2)]);
    assert_eq!(second, [Level(10), Level(20)]);
}
