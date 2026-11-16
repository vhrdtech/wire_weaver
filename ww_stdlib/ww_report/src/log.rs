//! The device's stored `defmt` log through `ww_log::LogDefmt`, decoded with its ELF.

use anyhow::{Result, bail};
use defmt_decoder::{DecodeError, Table};
use serde::Serialize;
use std::time::Duration;
use wire_weaver::prelude::UVlq64;
use wire_weaver_client::{StreamCursor, StreamEvent};

use crate::clients::LogDefmtClient;

/// What `ww report` shows of the log.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LogReport {
    /// One line per message, `--- N log messages lost ---` where the device dropped some
    pub lines: Vec<String>,
    pub lost: u64,
}

/// Longest wait for the next item once the stored log has started arriving (and for the first one).
const IDLE: Duration = Duration::from_millis(1500);

/// Resume the log session the device started at boot from its start, collect what the store holds and decode it.
/// Opening it this way leaves the store as it is.
pub async fn read(client: &LogDefmtClient, table: &Table) -> Result<LogReport> {
    let status = client.frames_status().read().await?;
    let Some(session) = status.session else {
        bail!("the device has no log session");
    };
    let end = status.next.0;
    let mut lines = vec![];
    let mut lost = 0;
    if end == 0 {
        return Ok(LogReport { lines, lost });
    }
    let cursor = StreamCursor {
        session,
        pos: UVlq64(0),
    };
    let mut frames = client.frames_resume(cursor).call().await?;
    let mut decoder = table.new_stream_decoder();
    let mut covered = 0u64;
    while covered < end {
        let event = match tokio::time::timeout(IDLE, frames.recv_event()).await {
            Ok(Ok(event)) => event,
            Ok(Err(e)) => {
                lines.push(format!("--- log stream ended: {e} ---"));
                break;
            }
            Err(_) => {
                lines.push("--- the device stopped sending its log ---".to_string());
                break;
            }
        };
        match event {
            StreamEvent::Item { value, units, .. } => {
                covered += units;
                decoder.received(&value.bytes.into_inner());
                loop {
                    match decoder.decode() {
                        Ok(frame) => lines.push(frame.display(false).to_string()),
                        Err(DecodeError::UnexpectedEof) => break,
                        Err(DecodeError::Malformed) => {
                            lines.push("--- malformed frame (wrong ELF?) ---".to_string());
                            break;
                        }
                    }
                }
            }
            StreamEvent::Gap { units, cause, .. } => {
                covered += units;
                lost += units;
                lines.push(format!("--- {units} log messages lost ({cause:?}) ---"));
            }
            StreamEvent::Closed | StreamEvent::Disconnected => break,
            _ => {}
        }
    }
    Ok(LogReport { lines, lost })
}
