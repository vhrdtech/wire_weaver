//! Messages over the device's scratch buffers and over the link's limit, and results too long for one message
//! (CLT-55). The device's scratch buffers are `MAX_MESSAGE_LEN` long, the replies here are far longer.
#![cfg(test)]

use client::Client;
use exact_units::Bytes;
use futures_lite::StreamExt;
use large_messages_api::{
    ARCHIVE_FULL_GID, ARCHIVE_WIRE_VERSION, Cursor, PageOwned, ReadingId, ReadingOwned, RowCount,
};
use local::Results;
use tests_common::{
    MAX_MESSAGE_LEN, TestDevice, TestProcessEventsAsync, start_device_async_with_limit,
};
use wire_weaver::CallContext;
use wire_weaver::call::{self, ItemStream, MessageKind, item_stream};
use wire_weaver::prelude::*;
use wire_weaver_client::Error;
use ww_client_server::StreamSideband;

const LIMIT: usize = 64 * 1024;

fn row(id: u32) -> ReadingOwned {
    ReadingOwned {
        id: ReadingId(id),
        name: format!("reading {id}"),
    }
}

fn table(rows: RowCount) -> Vec<ReadingOwned> {
    (0..rows.get()).map(row).collect()
}

fn page(rows: RowCount, cursor: Option<Cursor>, limit: RowCount) -> PageOwned {
    let from = cursor.map_or(0, |c| c.0);
    let to = from.saturating_add(limit.get()).min(rows.get());
    PageOwned {
        rows: (from..to).map(row).collect(),
        next: (to < rows.get()).then_some(Cursor(to)),
    }
}

#[derive(Default)]
pub struct Server {
    last_table: Vec<ReadingOwned>,
    /// The next row and the end of the open `scan`
    scan: Option<(u32, u32)>,
}

impl Server {
    async fn table(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        rows: RowCount,
    ) -> RpcResult<Vec<ReadingOwned>> {
        self.last_table = table(rows);
        RpcResult::Ready(self.last_table.clone())
    }

    async fn table_later(
        &mut self,
        cx: &mut Context<'_, impl EventOut>,
        rows: RowCount,
    ) -> RpcResult<Vec<ReadingOwned>> {
        let reply_to = cx.reply_to().expect("a call");
        Server::table_later_send_return(cx, reply_to, table(rows))
            .await
            .ok();
        RpcResult::Deferred
    }

    async fn get_last_table(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> GetResult<Vec<ReadingOwned>, ()> {
        GetResult::Value(self.last_table.clone())
    }

    async fn scan(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        rows: RowCount,
    ) -> RpcResult<()> {
        self.scan = Some((0, rows.get()));
        RpcResult::Ready(())
    }

    async fn close_scan(&mut self, _cx: &mut Context<'_, impl EventOut>) {
        self.scan = None;
    }

    async fn sideband_scan(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        _sideband: StreamSideband,
    ) -> Option<StreamSideband> {
        None
    }

    async fn page(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        rows: RowCount,
        cursor: Option<Cursor>,
        limit: RowCount,
    ) -> RpcResult<PageOwned> {
        RpcResult::Ready(page(rows, cursor, limit))
    }
}

impl TestProcessEventsAsync for Server {
    type Medium = ();

    async fn process_request_bytes<'a>(
        &mut self,
        bytes: &[u8],
        scratch: &'a mut [u8],
        out: &mut impl EventOut,
        medium: (),
    ) -> Result<&'a [u8], ShrinkWrapError> {
        self.process_request_bytes(bytes, scratch, out, medium)
            .await
    }

    /// Rows go out here, after the reply to the open: events before it don't reach the handle.
    async fn poll(&mut self, out: &mut impl EventOut, _now_ms: u64) {
        let Some((from, to)) = self.scan.take() else {
            return;
        };
        let send = api_impl::stream_data_ser();
        for id in from..to {
            send.scan_send(&row(id), out).await.expect("row sent");
        }
        send.scan_sideband_send(StreamSideband::Close, out)
            .await
            .expect("close sent");
    }
}

mod server {
    pub mod api_impl {
        wire_weaver::ww_codegen!(
            large_messages_api::Archive for super::super::Server,
            server = true, no_alloc = false, use_async = true,
            method_model = "table_later=deferred, _=immediate",
            property_model = "_=get_set",
        );
    }
}
use server::api_impl;

mod client {
    use wire_weaver_client::{ClientConfig, Commander, WwClient};

    pub struct Client {
        pub cmd: Commander,
    }

    impl WwClient for Client {
        fn default_config() -> ClientConfig {
            ClientConfig::default()
        }

        fn from_cmd(cmd: Commander) -> Self {
            Self { cmd }
        }
    }

    mod api {
        wire_weaver::ww_codegen!(
            large_messages_api::Archive for super::Client,
            client = "std_client",
            impl_local_trait = "crate::local::Results",
        );
    }
}

/// `Results`, the API as an async trait.
mod local {
    wire_weaver::ww_codegen!(large_messages_api::Archive, local_trait = "Results");
}

fn start(name: &str, device_limit: usize) -> TestDevice {
    start_device_async_with_limit(
        &format!("large_messages/{name}"),
        device_limit,
        Server::default(),
        ARCHIVE_FULL_GID,
        ARCHIVE_WIRE_VERSION,
        api_impl::api_hash(),
    )
}

async fn connect(device: &TestDevice, host_limit: usize) -> Client {
    Client::config(|c| c.in_process_path(device.path()).max_message_len(host_limit))
        .connect()
        .await
        .expect("connect")
}

fn rows(n: u32) -> RowCount {
    RowCount::new(n)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn thousand_rows_in_one_reply() {
    let device = start("thousand_rows", LIMIT);
    let client = connect(&device, LIMIT).await;
    let reply = client.table(rows(1000)).call().await.unwrap();
    assert_eq!(reply, table(rows(1000)));
    assert!(reply.iter().map(|r| r.name.len()).sum::<usize>() > 4 * MAX_MESSAGE_LEN);
    // the same long reply through the other two paths
    assert_eq!(
        client.table_later(rows(1000)).call().await.unwrap(),
        table(rows(1000))
    );
    assert_eq!(
        client.read_last_table().read().await.unwrap(),
        table(rows(1000))
    );
    // a reply that fits the scratch buffer still works after a long one
    assert_eq!(client.table(rows(2)).call().await.unwrap(), table(rows(2)));
}

/// The device's limit is the smaller one: the error names both and says so.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reply_over_the_device_limit() {
    let device = start("over_device", 2 * MAX_MESSAGE_LEN);
    let client = connect(&device, LIMIT).await;
    for error in [
        client.table(rows(1000)).call().await.unwrap_err(),
        client.table_later(rows(1000)).call().await.unwrap_err(),
    ] {
        assert_over_limit(
            error,
            MessageKind::Reply,
            "device",
            2 * MAX_MESSAGE_LEN,
            LIMIT,
        );
    }
    // the link is fine after it
    assert_eq!(client.table(rows(2)).call().await.unwrap(), table(rows(2)));
}

/// The host's limit is the smaller one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reply_over_the_host_limit() {
    let device = start("over_host", LIMIT);
    let client = connect(&device, 2 * MAX_MESSAGE_LEN).await;
    let error = client.table(rows(1000)).call().await.unwrap_err();
    assert_over_limit(
        error,
        MessageKind::Reply,
        "host",
        LIMIT,
        2 * MAX_MESSAGE_LEN,
    );
}

#[track_caller]
fn assert_over_limit(error: Error, side: MessageKind, set_by: &str, device: usize, host: usize) {
    let text = error.to_string();
    // a local trait's caller sees the same: which message and the link's limit
    let wire_weaver::call::ErrorKind::MessageTooBig(too_big) = error.call_kind() else {
        panic!("not MessageTooBig: {:?}", error.call_kind());
    };
    assert_eq!(too_big.message, side);
    assert_eq!(too_big.limit, Bytes::new(device.min(host) as u32));
    let Error::MessageTooBig { what, limit, .. } = error else {
        panic!("not over the limit: {error:?}");
    };
    assert_eq!(what, side);
    assert_eq!(limit.set_by(), set_by);
    assert_eq!((limit.device, limit.host), (device, host));
    assert!(text.contains(&device.to_string()), "{text}");
    assert!(text.contains(&host.to_string()), "{text}");
    assert!(text.contains(set_by), "{text}");
}

/// A local implementation of the result-set forms: the call site below works with it and with the client.
struct LocalArchive;

impl Results for LocalArchive {
    async fn table(
        &self,
        _cx: &CallContext,
        rows: RowCount,
    ) -> Result<Vec<ReadingOwned>, call::Error> {
        Ok(table(rows))
    }

    async fn table_later(
        &self,
        _cx: &CallContext,
        rows: RowCount,
    ) -> Result<Vec<ReadingOwned>, call::Error> {
        Ok(table(rows))
    }

    async fn read_last_table(&self, _cx: &CallContext) -> Result<Vec<ReadingOwned>, call::Error> {
        Ok(vec![])
    }

    async fn scan(
        &self,
        _cx: &CallContext,
        rows: RowCount,
    ) -> Result<ItemStream<ReadingOwned>, call::Error> {
        let rows = table(rows).into_iter().map(Ok::<_, call::Error>);
        Ok(item_stream(futures_lite::stream::iter(rows)))
    }

    async fn page(
        &self,
        _cx: &CallContext,
        rows: RowCount,
        cursor: Option<Cursor>,
        limit: RowCount,
    ) -> Result<PageOwned, call::Error> {
        Ok(page(rows, cursor, limit))
    }
}

/// How many rows a scan gives, in order, ending by itself when the stream closes.
async fn scan_all(results: &impl Results, n: u32) -> u32 {
    let mut stream = results.scan(&CallContext::local(), rows(n)).await.unwrap();
    let mut seen = 0;
    while let Some(item) = stream.next().await {
        assert_eq!(item.unwrap(), row(seen));
        seen += 1;
    }
    seen
}

/// All rows through pages of 50, each fits the smallest limit here.
async fn page_all(results: &impl Results, n: u32) -> Vec<ReadingOwned> {
    let (mut all, mut cursor) = (vec![], None);
    loop {
        let page = results
            .page(&CallContext::local(), rows(n), cursor, rows(50))
            .await
            .unwrap();
        all.extend(page.rows);
        cursor = page.next;
        if cursor.is_none() {
            return all;
        }
    }
}

/// 10 000 rows don't fit a message at any sensible limit, they go as a stream, at the default limit.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ten_thousand_rows_as_a_stream() {
    let device = start("stream", MAX_MESSAGE_LEN);
    let client = connect(&device, wire_weaver_client::DEFAULT_MAX_MESSAGE_LEN).await;
    assert_eq!(scan_all(&client, 10_000).await, 10_000);
    assert_eq!(scan_all(&LocalArchive, 10_000).await, 10_000);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn paging_with_a_cursor() {
    let device = start("paging", MAX_MESSAGE_LEN);
    let client = connect(&device, wire_weaver_client::DEFAULT_MAX_MESSAGE_LEN).await;
    assert_eq!(page_all(&client, 1000).await, table(rows(1000)));
    assert_eq!(page_all(&LocalArchive, 1000).await, table(rows(1000)));
}
