//! Host decoder of a [Store] flash dump (TOOL-12): given the raw bytes of the store's flash range (read off a
//! chip, from a `.bin`/debug probe dump), [analyze] reports the same current record per key a device would read,
//! by running the real [Store] over the bytes through [FakeFlash], plus a raw, word-by-word walk of every page and
//! item (valid, cut or garbage) that [Store] itself doesn't expose, for forensics.
//!
//! `word_size` and `page_size` aren't in the image: pass the values the device's flash uses (see the datasheet
//! figures in the `ww_settings` docs page). [analyze] checks both against the sizes [Store] supports before
//! decoding anything.
//!
//! ```no_run
//! let image = std::fs::read("settings.bin").unwrap();
//! let dump = ww_settings::dump::analyze(&image, 0, 8, 2048).unwrap();
//! for page in &dump.pages {
//!     println!("{page:?}");
//! }
//! for rec in &dump.current {
//!     println!("key {} @ {:#x}, seq {}: {} bytes", rec.key, rec.addr, rec.seq, rec.bytes.len());
//! }
//! ```

use crate::fake_flash::FakeFlash;
use crate::group::SettingsGroup;
use crate::record::{self, DecodeStats, Record, RecordError};
use crate::store::{Key, Store};
use crc::{CRC_32_ISCSI, Crc};
use std::collections::{BTreeMap, BTreeSet};

const CRC: Crc<u32> = Crc::<u32>::new(&CRC_32_ISCSI);
const PAGE_MAGIC: [u8; 4] = [b'W', b'S', 1, 0xff];
const PAGE_HEADER_LEN: usize = 12;
const ITEM_TAG: u8 = 0xa5;
const ITEM_HEADER_LEN: usize = 12;

/// Word sizes [Store] can be configured for (the flash's `WRITE_SIZE`/`READ_SIZE`, whichever is larger).
pub const WORD_SIZES: &[usize] = &[1, 4, 8, 16, 32];
/// Page (erase unit) sizes [Store] can be configured for.
pub const PAGE_SIZES: &[usize] = &[2 * 1024, 4 * 1024, 8 * 1024, 16 * 1024, 128 * 1024];

/// Why [analyze] couldn't decode an image.
#[derive(Debug)]
pub enum DumpError {
    /// Not one of [WORD_SIZES].
    UnsupportedWordSize(usize),
    /// Not one of [PAGE_SIZES].
    UnsupportedPageSize(usize),
    /// The image length isn't a whole number of pages, or fewer than 2.
    BadImageLen { len: usize, page_size: usize },
    /// [Store::new] or a [Store] operation failed on the re-created flash (should not happen: the same bytes a
    /// real [Store] wrote or would refuse to write).
    Store(String),
}

impl core::fmt::Display for DumpError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DumpError::UnsupportedWordSize(w) => {
                write!(f, "word size {w} not one of {WORD_SIZES:?}")
            }
            DumpError::UnsupportedPageSize(p) => {
                write!(f, "page size {p} not one of {PAGE_SIZES:?}")
            }
            DumpError::BadImageLen { len, page_size } => write!(
                f,
                "image is {len} bytes, not a whole number of {page_size}-byte pages (or fewer than 2)"
            ),
            DumpError::Store(e) => write!(f, "store: {e}"),
        }
    }
}

impl std::error::Error for DumpError {}

/// Whether the word at `page_bytes[offset..offset + word]` reads erased (all `0xff`).
fn word_erased(page_bytes: &[u8], offset: usize, word: usize) -> bool {
    page_bytes[offset..offset + word].iter().all(|b| *b == 0xff)
}

fn header_span(word: usize) -> usize {
    PAGE_HEADER_LEN.div_ceil(word) * word
}

fn item_footprint(word: usize, len: usize) -> usize {
    (ITEM_HEADER_LEN + len).div_ceil(word) * word
}

/// A page's header, as parsed by the raw walk.
#[derive(Copy, Clone, Debug)]
pub struct RawPage {
    /// Index of the page in the range (0-based).
    pub index: usize,
    /// Absolute address of the page's first byte.
    pub addr: u32,
    /// The header's magic and crc32 checked out.
    pub valid_header: bool,
    /// The page's sequence number, `None` when the header isn't valid.
    pub page_seq: Option<u32>,
    /// This is the page with the highest valid `page_seq`: what [Store] would append to next.
    pub is_current: bool,
}

/// What the raw walk found at one item slot.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ItemState {
    /// Header and crc32 check out.
    Valid,
    /// Looked like an item (the tag byte matched) but the crc32, or the length, didn't check out: a cut write.
    Cut,
    /// Doesn't look like an item at all: neither a valid header nor an erased word.
    Garbage,
    /// Reads erased: free space, or a cut that left this word erased.
    Erased,
}

/// One item slot found by the raw walk, valid or not.
#[derive(Copy, Clone, Debug)]
pub struct RawItem {
    /// Index of the page it is on.
    pub page: usize,
    /// Absolute address of the slot's first byte.
    pub addr: u32,
    /// How many bytes this slot takes (its footprint if [ItemState::Valid], one word otherwise, or the page's
    /// remaining bytes for a trailing [ItemState::Erased] run).
    pub size: usize,
    pub state: ItemState,
    /// Key byte, only for [ItemState::Valid].
    pub key: Option<u8>,
    /// Sequence number, only for [ItemState::Valid].
    pub seq: Option<u32>,
    /// Data length, only for [ItemState::Valid].
    pub len: Option<u16>,
    /// This is the item [Store] would currently read back for its key (highest `seq`, ties broken by the page's
    /// `page_seq`), among items on a page with a valid header.
    pub is_current: bool,
}

/// A key's current record, as the raw walk selected it (and [analyze] cross-checked against a real [Store]).
pub struct CurrentRecord<'i> {
    pub key: u8,
    pub addr: u32,
    pub page: usize,
    pub seq: u32,
    /// The item's data: the [record] bytes ([Record::parse] reads them).
    pub bytes: &'i [u8],
}

/// What [analyze] found in an image.
pub struct Dump<'i> {
    pub word_size: usize,
    pub page_size: usize,
    pub base: u32,
    /// One entry per page, in order.
    pub pages: Vec<RawPage>,
    /// Every item slot on every page, in order (including pages without a valid header, which [Store] itself
    /// never looks at).
    pub items: Vec<RawItem>,
    /// The current record of every key the raw walk found valid, in increasing key order.
    pub current: Vec<CurrentRecord<'i>>,
    /// Keys where a real [Store] over the same bytes disagreed with the raw walk's selection: should always be
    /// empty, a non-empty list means this module's raw walk has a bug.
    pub store_mismatch: Vec<u8>,
}

/// Parse the image into pages and item slots, word size `word`, page size `page_size`; `base..base + image.len()`
/// are the absolute addresses. Doesn't look at [Store] at all: pages without a valid header are still walked, and
/// cut/garbage slots are reported, not skipped.
fn raw_walk(
    image: &[u8],
    base: u32,
    word: usize,
    page_size: usize,
) -> (Vec<RawPage>, Vec<RawItem>) {
    let header_len = header_span(word);
    let min_item = ITEM_HEADER_LEN.div_ceil(word) * word;
    let mut pages = Vec::new();
    let mut items = Vec::new();
    let page_count = image.len() / page_size;
    for index in 0..page_count {
        let page_bytes = &image[index * page_size..(index + 1) * page_size];
        let addr = base + (index * page_size) as u32;
        let valid_header = page_bytes[..4] == PAGE_MAGIC
            && CRC.checksum(&page_bytes[..8]).to_le_bytes() == page_bytes[8..12];
        let page_seq =
            valid_header.then(|| u32::from_le_bytes(page_bytes[4..8].try_into().unwrap()));
        pages.push(RawPage {
            index,
            addr,
            valid_header,
            page_seq,
            is_current: false, // filled in below
        });

        let mut offset = header_len;
        while offset < page_size {
            if offset + min_item > page_size {
                // too little left for an item header: whatever is there, one slot to the end of the page
                let state = if word_erased(page_bytes, offset, page_size - offset) {
                    ItemState::Erased
                } else {
                    ItemState::Garbage
                };
                items.push(raw_item(
                    index,
                    addr,
                    offset,
                    page_size - offset,
                    state,
                    None,
                ));
                break;
            }
            let h = &page_bytes[offset..offset + ITEM_HEADER_LEN];
            if h[3] != ITEM_TAG {
                if word_erased(page_bytes, offset, word) {
                    let start = offset;
                    // page_size is always a multiple of word, so this always lands exactly on page_size
                    while offset < page_size && word_erased(page_bytes, offset, word) {
                        offset += word;
                    }
                    items.push(raw_item(
                        index,
                        addr,
                        start,
                        offset - start,
                        ItemState::Erased,
                        None,
                    ));
                } else {
                    items.push(raw_item(
                        index,
                        addr,
                        offset,
                        word,
                        ItemState::Garbage,
                        None,
                    ));
                    offset += word;
                }
                continue;
            }
            let len = u16::from_le_bytes([h[0], h[1]]) as usize;
            let size = item_footprint(word, len);
            if offset + size > page_size {
                items.push(raw_item(index, addr, offset, word, ItemState::Cut, None));
                offset += word;
                continue;
            }
            let data = &page_bytes[offset + ITEM_HEADER_LEN..offset + ITEM_HEADER_LEN + len];
            let mut digest = CRC.digest();
            digest.update(&h[..8]);
            digest.update(data);
            if digest.finalize().to_le_bytes() != h[8..12] {
                items.push(raw_item(index, addr, offset, word, ItemState::Cut, None));
                offset += word;
                continue;
            }
            let key = h[2];
            let seq = u32::from_le_bytes(h[4..8].try_into().unwrap());
            items.push(RawItem {
                page: index,
                addr: addr + offset as u32,
                size,
                state: ItemState::Valid,
                key: Some(key),
                seq: Some(seq),
                len: Some(len as u16),
                is_current: false, // filled in below
            });
            offset += size;
        }
    }

    // the current page: highest valid page_seq
    let current_page = pages
        .iter()
        .filter(|p| p.valid_header)
        .map(|p| (p.page_seq.unwrap(), p.index))
        .max()
        .map(|(_, index)| index);
    if let Some(index) = current_page {
        pages[index].is_current = true;
    }

    // the current record per key: highest (seq, page_seq) among valid items on a page with a valid header
    // (a page without one is never looked at by Store, even if an item on it happens to be intact)
    let mut best: BTreeMap<u8, (u32, u32, usize)> = BTreeMap::new();
    for (i, item) in items.iter().enumerate() {
        if item.state != ItemState::Valid || !pages[item.page].valid_header {
            continue;
        }
        let key = item.key.unwrap();
        let seq = item.seq.unwrap();
        let page_seq = pages[item.page].page_seq.unwrap();
        let better = match best.get(&key) {
            Some((bseq, bpage_seq, _)) => (seq, page_seq) >= (*bseq, *bpage_seq),
            None => true,
        };
        if better {
            best.insert(key, (seq, page_seq, i));
        }
    }
    for (_, _, i) in best.values() {
        items[*i].is_current = true;
    }

    (pages, items)
}

fn raw_item(
    page: usize,
    page_addr: u32,
    offset: usize,
    size: usize,
    state: ItemState,
    key: Option<u8>,
) -> RawItem {
    RawItem {
        page,
        addr: page_addr + offset as u32,
        size,
        state,
        key,
        seq: None,
        len: None,
        is_current: false,
    }
}

/// Fetch the current record of every key in `keys` from a real [Store] over `image`, for the cross-check. `word`
/// and `page` are checked against [WORD_SIZES]/[PAGE_SIZES] by the caller.
fn store_fetch_all(
    word: usize,
    page: usize,
    base: u32,
    image: &[u8],
    keys: &BTreeSet<u8>,
) -> Result<BTreeMap<u8, Option<Vec<u8>>>, DumpError> {
    macro_rules! run {
        ($w:literal, $p:literal) => {{
            let flash = FakeFlash::<$w, $p>::from_image(base, image);
            let mut store: Store<FakeFlash<$w, $p>, 512, 256> =
                Store::new(flash, base..base + image.len() as u32)
                    .map_err(|e| DumpError::Store(format!("{e:?}")))?;
            let mut out = BTreeMap::new();
            for &key in keys {
                let mut buf = std::vec![0u8; $p];
                let found = embassy_futures::block_on(store.fetch(Key(key), &mut buf))
                    .map_err(|e| DumpError::Store(format!("{e:?}")))?;
                out.insert(key, found.map(|len| buf[..len].to_vec()));
            }
            out
        }};
    }
    Ok(match (word, page) {
        (1, 2048) => run!(1, 2048),
        (1, 4096) => run!(1, 4096),
        (1, 8192) => run!(1, 8192),
        (1, 16384) => run!(1, 16384),
        (1, 131072) => run!(1, 131072),
        (4, 2048) => run!(4, 2048),
        (4, 4096) => run!(4, 4096),
        (4, 8192) => run!(4, 8192),
        (4, 16384) => run!(4, 16384),
        (4, 131072) => run!(4, 131072),
        (8, 2048) => run!(8, 2048),
        (8, 4096) => run!(8, 4096),
        (8, 8192) => run!(8, 8192),
        (8, 16384) => run!(8, 16384),
        (8, 131072) => run!(8, 131072),
        (16, 2048) => run!(16, 2048),
        (16, 4096) => run!(16, 4096),
        (16, 8192) => run!(16, 8192),
        (16, 16384) => run!(16, 16384),
        (16, 131072) => run!(16, 131072),
        (32, 2048) => run!(32, 2048),
        (32, 4096) => run!(32, 4096),
        (32, 8192) => run!(32, 8192),
        (32, 16384) => run!(32, 16384),
        (32, 131072) => run!(32, 131072),
        _ => unreachable!("word/page already validated by analyze()"),
    })
}

/// Decode a [Store] flash dump: the raw bytes of its flash range, `base` the absolute address of `image[0]`
/// (matching what the firmware passed to [Store::new](crate::store::Store::new)), `word_size`/`page_size` the
/// flash's word and page (erase unit) size.
///
/// Reports the same current record per key a device would read (checked by actually running a [Store] over the
/// bytes, through [FakeFlash]), plus a raw, word-by-word walk of every page and item for forensics: see [Dump].
pub fn analyze(
    image: &[u8],
    base: u32,
    word_size: usize,
    page_size: usize,
) -> Result<Dump<'_>, DumpError> {
    if !WORD_SIZES.contains(&word_size) {
        return Err(DumpError::UnsupportedWordSize(word_size));
    }
    if !PAGE_SIZES.contains(&page_size) {
        return Err(DumpError::UnsupportedPageSize(page_size));
    }
    if image.is_empty() || !image.len().is_multiple_of(page_size) || image.len() / page_size < 2 {
        return Err(DumpError::BadImageLen {
            len: image.len(),
            page_size,
        });
    }

    let (pages, items) = raw_walk(image, base, word_size, page_size);

    let keys: BTreeSet<u8> = items
        .iter()
        .filter(|i| i.state == ItemState::Valid)
        .map(|i| i.key.unwrap())
        .collect();
    let store_results = store_fetch_all(word_size, page_size, base, image, &keys)?;

    let mut current = Vec::new();
    let mut store_mismatch = Vec::new();
    for item in items.iter().filter(|i| i.is_current) {
        let key = item.key.unwrap();
        let offset = (item.addr - base) as usize;
        let bytes =
            &image[offset + ITEM_HEADER_LEN..offset + ITEM_HEADER_LEN + item.len.unwrap() as usize];
        match store_results.get(&key) {
            Some(Some(store_bytes)) if store_bytes.as_slice() == bytes => {}
            _ => store_mismatch.push(key),
        }
        current.push(CurrentRecord {
            key,
            addr: item.addr,
            page: item.page,
            seq: item.seq.unwrap(),
            bytes,
        });
    }

    Ok(Dump {
        word_size,
        page_size,
        base,
        pages,
        items,
        current,
        store_mismatch,
    })
}

/// Parse a record's bytes (as found in [CurrentRecord::bytes] or [RawItem]) and decode it into a concrete
/// [SettingsGroup], for a caller that knows the group's type (e.g. the device's own settings struct, from a test
/// or a bespoke dump tool). The generic `ww` CLI shows slots raw instead: it doesn't know user types.
pub fn decode_group<G: SettingsGroup>(bytes: &[u8]) -> Result<(G, DecodeStats), RecordError> {
    let rec = Record::parse(bytes)?;
    let mut value = G::defaults();
    let stats = record::decode(&rec, &mut value).map_err(RecordError::Malformed)?;
    Ok((value, stats))
}
