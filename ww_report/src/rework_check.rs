//! The rework check every flasher makes before writing an image (TOOL-29, `ww_board_info` STD-101).
//!
//! A firmware never runs a board with a rework it doesn't know: it boots guarded. So before writing, a flasher
//! compares the board's applied reworks with the reworks the new image knows, and asks the user when the image lacks
//! one. Only if they agree is an allowance stored for the new image's [ImageId]; otherwise a stored allowance is
//! cleared, so it can't outlive the image it was for.
//!
//! 1. [ImageReworks::from_flash] reads the image's rework record and build id from the bytes about to be written.
//! 2. [read_board] reads the board's name, revision, applied reworks and guard from the running device.
//! 3. [verdict] compares the two ([ww_board_info::image::check]), [Verdict::needs_consent] says when to ask.
//! 4. [store_allowance] after the user agreed to [Verdict::Lacking], [clear_allowance] otherwise; then write.
//!
//! ```ignore
//! let image = ImageReworks::from_flash(&flash_bytes);
//! let BoardRead::Board(board) = read_board(&BoardInfoClient::new(&device)).await? else { /* can't check */ };
//! match verdict(&image, &board) {
//!     Verdict::Ok => { clear_allowance(&client).await?; }
//!     Verdict::Lacking { lacking, image: Some(id) } if user_agrees() => { store_allowance(&client, id, lacking).await?; }
//!     other => { /* warn, ask or refuse */ }
//! }
//! ```

use std::fmt;

use anyhow::{Result, anyhow};
use wire_weaver::prelude::MaxLen;
use wire_weaver_client::{CallError, Error};
use ww_board_info::image::{self, Check, ImageTable, ImageTableError};
use ww_board_info::table::{IdRanges, ReworkSet};
use ww_board_info::{GuardState, GuardSummaryOwned, ImageId, ReworkChange, ReworkErrorOwned};

use crate::clients::BoardInfoClient;
use crate::info::Part;

/// What a new image says about reworks: its record (`ww_board_info::image`) and its [ImageId].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageReworks {
    /// The record's bytes, `Ok(None)` for an image without one (from before the guard).
    record: Result<Option<Vec<u8>>, ImageTableError>,
    /// The first 8 bytes of the image's GNU build id; `None` without a build-id note: no allowance can be its own.
    pub id: Option<ImageId>,
}

impl ImageReworks {
    /// From the image's flash contents, as the flasher is about to write them (segments of an ELF, UF2 or raw binary
    /// put one after the other).
    pub fn from_flash(bytes: &[u8]) -> Self {
        let record = ImageTable::find(bytes).map(|t| t.map(|t| t.as_bytes().to_vec()));
        let id = ww_firmware_info::find_build_id(bytes).and_then(ImageId::from_build_id);
        ImageReworks { record, id }
    }

    /// The image's record, `Ok(None)` when it has none.
    pub fn table(&self) -> Result<Option<ImageTable<'_>>, ImageTableError> {
        match &self.record {
            Ok(Some(bytes)) => ImageTable::parse(bytes).map(Some),
            Ok(None) => Ok(None),
            Err(e) => Err(*e),
        }
    }

    /// The board revisions the image lists, `B200 A, B200 B`.
    pub fn boards(&self) -> String {
        let Ok(Some(table)) = self.table() else {
            return String::new();
        };
        table
            .entries()
            .map(|e| format!("{} {}", e.name, e.revision))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// The image guards: it has a usable record.
    pub fn guards(&self) -> bool {
        matches!(self.table(), Ok(Some(_)))
    }
}

/// The board as the running device reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoardReworks {
    pub name: String,
    pub revision: String,
    /// Applied reworks, known to the running firmware or not.
    pub applied: ReworkSet,
    /// `None`: the running firmware doesn't serve `GUARD` (from before the guard).
    pub guard: Option<GuardSummaryOwned>,
}

impl BoardReworks {
    /// An allowance is stored on the device (for the running image or another one).
    pub fn has_allowance(&self) -> bool {
        self.guard.as_ref().is_some_and(|g| g.allowance.is_some())
    }
}

/// What [read_board] found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BoardRead {
    Board(BoardReworks),
    /// The device doesn't serve `ww_board_info::BoardInfo`: nothing to compare.
    NoBoardInfo,
    /// It serves `BOARD` but not `REWORKS` (firmware from before STD-98): its reworks can't be read.
    NoReworks {
        name: String,
        revision: String,
    },
}

/// Read the board's name, revision, applied reworks and guard.
pub async fn read_board(client: &BoardInfoClient) -> Result<BoardRead> {
    let board = match Part::from_result(client.read_BOARD().read().await) {
        Part::Value(b) => b,
        Part::NotImplemented => return Ok(BoardRead::NoBoardInfo),
        Part::Error(e) => return Err(anyhow!("reading the board: {e}")),
    };
    let (name, revision) = (board.name.to_string(), board.revision.to_string());
    let list = match Part::from_result(client.read_REWORKS().read().await) {
        Part::Value(list) => list,
        Part::NotImplemented => return Ok(BoardRead::NoReworks { name, revision }),
        Part::Error(e) => return Err(anyhow!("reading the board's reworks: {e}")),
    };
    let applied = list
        .iter()
        .filter(|r| r.applied)
        .fold(ReworkSet::EMPTY, |s, r| s.with(r.id, true));
    let guard = match Part::from_result(client.read_GUARD().read().await) {
        Part::Value(g) => Some(g),
        Part::NotImplemented => None,
        Part::Error(e) => return Err(anyhow!("reading the board's guard: {e}")),
    };
    Ok(BoardRead::Board(BoardReworks {
        name,
        revision,
        applied,
        guard,
    }))
}

/// How a new image fits the board.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// The image knows every applied rework (or has no record and the board none applied): write it, clear a
    /// stored allowance.
    Ok,
    /// The image doesn't know these applied reworks: it boots guarded unless the user agrees and an allowance is
    /// stored for `image` (`None`: the image has no build id, no allowance can be its own).
    Lacking {
        lacking: ReworkSet,
        image: Option<ImageId>,
    },
    /// The image lists other boards only: probably the wrong image.
    OtherBoard { boards: String },
    /// The image has no record (built before the guard): it would run the board as built, ignoring these applied
    /// reworks.
    NoRecord { applied: ReworkSet },
    /// The image's record is unusable: the image would boot guarded on any board with a rework applied.
    BadRecord(ImageTableError),
}

impl Verdict {
    /// The user must agree before the image is written.
    pub fn needs_consent(&self) -> bool {
        !matches!(self, Verdict::Ok)
    }
}

/// Compare the board with the image: [ww_board_info::image::check], with what the flasher needs to say.
pub fn verdict(image: &ImageReworks, board: &BoardReworks) -> Verdict {
    let table = match image.table() {
        Ok(t) => t,
        // nothing applied: no rework the record could lack
        Err(_) if board.applied.is_empty() => return Verdict::Ok,
        Err(e) => return Verdict::BadRecord(e),
    };
    match image::check(table.as_ref(), &board.name, &board.revision, board.applied) {
        Check::Ok => Verdict::Ok,
        Check::Lacking(lacking) => Verdict::Lacking {
            lacking,
            image: image.id,
        },
        Check::OtherBoard { .. } => Verdict::OtherBoard {
            boards: image.boards(),
        },
        Check::NoRecord { applied } => Verdict::NoRecord { applied },
    }
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Verdict::Ok => f.write_str("the image knows every rework applied on the board"),
            Verdict::Lacking {
                lacking,
                image: Some(id),
            } => write!(
                f,
                "the image doesn't know rework(s) {} applied on the board: it boots guarded unless they are allowed for image {id}",
                IdRanges(*lacking)
            ),
            Verdict::Lacking {
                lacking,
                image: None,
            } => write!(
                f,
                "the image doesn't know rework(s) {} applied on the board, and has no build id to allow them for: it boots guarded",
                IdRanges(*lacking)
            ),
            Verdict::OtherBoard { boards } => {
                write!(f, "the image is for {boards}, not this board")
            }
            Verdict::NoRecord { applied } => write!(
                f,
                "the image has no rework record (built before the rework guard): it runs the board as built, ignoring rework(s) {}",
                IdRanges(*applied)
            ),
            Verdict::BadRecord(e) => write!(f, "{e}: it boots guarded"),
        }
    }
}

/// The board's guard, one line: `guarded: rework(s) 7 unknown`, `allowance stored for image 3fa2..`.
pub fn guard_line(guard: &GuardSummaryOwned) -> String {
    let unknown = guard
        .unknown
        .iter()
        .fold(ReworkSet::EMPTY, |s, id| s.with(*id, true));
    let mut line = match guard.state {
        GuardState::Normal => "normal".to_string(),
        GuardState::Guarded => format!("guarded: rework(s) {} unknown", IdRanges(unknown)),
        GuardState::Allowed => format!("allowed: rework(s) {} unknown", IdRanges(unknown)),
        other => format!("{other:?}"),
    };
    if let Some(a) = &guard.allowance {
        line.push_str(&format!(
            ", allowance stored for {}image {}",
            if a.for_running_image {
                "the running "
            } else {
                ""
            },
            a.image
        ));
    }
    line
}

/// Store the allowance for `image` to run with `reworks` (the [Verdict::Lacking] set), after the user agreed.
pub async fn store_allowance(
    client: &BoardInfoClient,
    image: ImageId,
    reworks: ReworkSet,
) -> Result<ReworkChange> {
    let ids = reworks.iter().collect();
    client
        .allow_unknown_reworks(image, MaxLen::new_unchecked(ids), true)
        .call()
        .await
        .map_err(|e| match e {
            CallError::Unimplemented { .. } => anyhow!(
                "the running firmware can't store an allowance (from before the rework guard)"
            ),
            e => call_error("storing the allowance", e),
        })
}

/// Clear a stored allowance: `None` when the firmware has no allowance to clear (from before the guard).
pub async fn clear_allowance(client: &BoardInfoClient) -> Result<Option<ReworkChange>> {
    match client.clear_unknown_reworks_allowance(true).call().await {
        Ok(change) => Ok(Some(change)),
        Err(CallError::Unimplemented { .. }) => Ok(None),
        Err(CallError::Client(e)) if not_implemented(&e) => Ok(None),
        Err(e) => Err(call_error("clearing the allowance", e)),
    }
}

fn call_error(what: &str, e: CallError<ReworkErrorOwned>) -> anyhow::Error {
    match e {
        CallError::Device(e) => anyhow!("{what}: {e}"),
        CallError::Unimplemented { resource } => {
            anyhow!("{what}: the device doesn't implement {resource}")
        }
        CallError::Client(e) => anyhow!("{what}: {e}"),
    }
}

fn not_implemented(e: &Error) -> bool {
    crate::info::not_implemented(e)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ww_board_info::ReworkId;
    use ww_board_info::table::{ReworkDef, ReworkTable};

    const B200B: ReworkTable =
        ReworkTable::new(&[ReworkDef::new(1, "a"), ReworkDef::new(3, "b")], &[]);
    ww_board_info::image_reworks!(static RECORD = [("B200", "B", B200B)]);

    fn set(ids: &[u16]) -> ReworkSet {
        ids.iter()
            .fold(ReworkSet::EMPTY, |s, &id| s.with(ReworkId(id), true))
    }

    /// An image's flash contents: code, the build-id note, the record.
    fn flash(record: bool, build_id: bool) -> Vec<u8> {
        let mut v = vec![0xff; 64];
        if build_id {
            v.extend([4, 0, 0, 0, 20, 0, 0, 0, 3, 0, 0, 0]);
            v.extend(b"GNU\0");
            v.extend(0x10..0x24u8);
        }
        if record {
            v.extend(RECORD.as_bytes());
        }
        v.extend([0; 32]);
        v
    }

    fn board(name: &str, revision: &str, applied: &[u16]) -> BoardReworks {
        BoardReworks {
            name: name.into(),
            revision: revision.into(),
            applied: set(applied),
            guard: None,
        }
    }

    #[test]
    fn image_id_and_record_from_flash() {
        let image = ImageReworks::from_flash(&flash(true, true));
        assert_eq!(
            image.id,
            Some(ImageId([0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17]))
        );
        assert!(image.guards());
        assert_eq!(image.boards(), "B200 B");
        let old = ImageReworks::from_flash(&flash(false, false));
        assert_eq!(old.id, None);
        assert!(!old.guards());
    }

    #[test]
    fn verdicts() {
        let image = ImageReworks::from_flash(&flash(true, true));
        assert_eq!(verdict(&image, &board("B200", "B", &[1, 3])), Verdict::Ok);
        assert_eq!(
            verdict(&image, &board("B200", "B", &[1, 7, 9])),
            Verdict::Lacking {
                lacking: set(&[7, 9]),
                image: image.id
            }
        );
        // a revision the image doesn't list: it knows none of its reworks
        assert_eq!(
            verdict(&image, &board("B200", "A", &[1])),
            Verdict::Lacking {
                lacking: set(&[1]),
                image: image.id
            }
        );
        assert_eq!(verdict(&image, &board("B200", "A", &[])), Verdict::Ok);
        assert_eq!(
            verdict(&image, &board("B135", "A", &[])),
            Verdict::OtherBoard {
                boards: "B200 B".into()
            }
        );

        let old = ImageReworks::from_flash(&flash(false, true));
        assert_eq!(verdict(&old, &board("B200", "B", &[])), Verdict::Ok);
        assert_eq!(
            verdict(&old, &board("B200", "B", &[3])),
            Verdict::NoRecord { applied: set(&[3]) }
        );

        let no_id = ImageReworks::from_flash(&flash(true, false));
        assert_eq!(
            verdict(&no_id, &board("B200", "B", &[5])),
            Verdict::Lacking {
                lacking: set(&[5]),
                image: None
            }
        );
    }

    #[test]
    fn corrupt_record() {
        let mut bytes = flash(true, true);
        let at = bytes.len() - 32 - 8;
        bytes[at] ^= 1; // in the known reworks of the entry: the CRC no longer matches
        let image = ImageReworks::from_flash(&bytes);
        assert!(!image.guards());
        assert_eq!(
            verdict(&image, &board("B200", "B", &[1])),
            Verdict::BadRecord(ImageTableError::Corrupt)
        );
        assert_eq!(verdict(&image, &board("B200", "B", &[])), Verdict::Ok);
    }

    #[test]
    fn messages() {
        let id = ImageId([0x3f, 0xa2, 1, 2, 3, 4, 5, 6]);
        assert_eq!(
            Verdict::Lacking {
                lacking: set(&[7, 9]),
                image: Some(id)
            }
            .to_string(),
            "the image doesn't know rework(s) 7, 9 applied on the board: it boots guarded unless they are allowed for image 3fa2010203040506"
        );
        assert_eq!(
            Verdict::NoRecord { applied: set(&[3]) }.to_string(),
            "the image has no rework record (built before the rework guard): it runs the board as built, ignoring rework(s) 3"
        );
    }
}
