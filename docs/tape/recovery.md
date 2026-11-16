---
level: advanced
---

# Damage and recovery

Recordings fail in ordinary ways. Power goes out in the middle of a write, a cable is pulled, an SD card is reused
without being erased, a sector goes bad on an old disk. A tape is built so that each of these costs the bytes it touched
and nothing else, and so that a reader says what was lost instead of guessing. This page explains how, then closes with
what the format does not protect against. It assumes the [overview](index.md) and refers to chunks from [Events and
series](events_and_series.md).

## Checksums at every level

Damage has to be found before it can be contained. A tape checks its bytes at four levels:

| Check | Covers | Damage costs |
|---|---|---|
| block CRC-32C | each whole block | that block |
| head CRC-32C | the head of a chunk or index block | the head alone; the block's place stays known |
| CRC segment, CRC-32C | each 64 KiB of a chunk's payload | one 64 KiB stretch of one chunk |
| content hash, BLAKE3 | every byte of a finished piece before its finish block | nothing; it proves the piece is unchanged |

The head CRC matters most. A chunk's head gives its length, its track, its item numbers and ticks, and the CRC of each
64 KiB stretch of its payload. When the payload is damaged but the head is good, a reader still trusts the length (so it
finds the next block) and still knows exactly which items were hit. A flipped bit in a 4 MiB series chunk costs one 64
KiB CRC segment of samples.

The checks detect damage; they do not correct it. Correcting is left to the medium: a disk's or flash controller's error
correction, or a file system. A block kind is reserved for parity blocks, but no writer writes them.

## Reading past damage

`FileBlocks` walks the blocks of a byte stream in order, in one of two modes:

- **Prefix** (the default) takes the longest valid prefix. It stops at the first block with a bad magic, length or CRC,
  at a block cut short, and at a sequence number that goes backwards, and says where it stopped. This is what a crash
  leaves: good blocks, then a torn one.
- **Resync** reads on. It reports a bad block and looks for the next block magic one byte after the bad block's start.
  Any byte may start a block, and a block is taken only when its CRCs match. A chunk with a good head and a damaged
  payload is handed out with its bad CRC segments marked, and the walk goes on after it by its length.

Inside a damaged events chunk, a reader can still **salvage** items. The time codec and the payload lengths are stored
per group of 128 items, and each group's record must agree with its neighbours: offsets that chain, ticks in order,
widths in range. A group that breaks the chain is marked suspect, and only its items are. A flipped bit in one residual
damages one item; in a group record, one group or two. A payload column coded with lz4 is the exception: one bad byte
makes every item of the chunk suspect, since the column decodes as a whole.

## Writing so a crash costs little

`TapeWriter` writes a file the way databases write their logs:

1. It creates the file as `NAME.partial` and syncs the file and its directory, so the name can't vanish in a crash.
2. It syncs the data by a policy: never, every N blocks, or at most an interval apart (1 s by default). What a crash can
   lose is bounded by the policy.
3. At the finish it writes the final index, the describe backup, the finish block and the tail, then renames the file to
   `NAME` and syncs the directory again.

A failed write or sync ends the file. The writer reports it and refuses every later write; it never retries, because
after a failed `fsync` Linux may drop the unwritten pages, and a second `fsync` then succeeds without them. A file that
still ends in `.partial` was never finished.

### Resuming after a crash

`TapeWriter::resume` reopens `NAME.partial` and continues the same piece. It reads the piece the way `TapeReader` does,
from the newest checkpoint and then by a scan that stops at the first bad block. It keeps every byte up to the end of the
last block the producer wrote, and cuts off the rest: a torn block, and the checkpoint, copy or end sequence the writer
had placed after that block. It then places those again by the same rules, so a resumed piece ends with the same bytes,
and the same content hash, as one written without the crash. The producer passes the same writer options it wrote the
piece with (`TapeWriter::options` gives them), and writes again from the sequence number `next_seq` reports.

The resume checks those options against the layout the piece's describe set states, before it cuts or writes anything.
Options with another chunk alignment would place blocks the way the file says it doesn't, and a reader trusts the
layout when it looks for the end of the data, so the resume refuses them with an error naming both alignments. The
layout's gap settings need no check, because the writer never leaves a gap. A piece cut off inside its first describe
set, with no copy after it, states no layout; it resumes unchecked, and the result reports no layout.

For example, a recorder loses power while writing piece 3 of a chain. On restart it calls `PieceChain::resume` with the
directory and the chain options. The chain reads pieces 0 to 2 back from their tails, reopens `00000003.wwt.partial`,
and the recorder goes on from the first block the file lacks. A crash between finishing one piece and creating the next
resumes too: the chain creates the next piece.

A writer writes through a **sink**: a file (`FileSink`) or memory (`MemSink`). A memory sink reads back as a byte source,
so `TapeReader` reads a piece while it is being written, and tests drive crashes by cutting the bytes anywhere.
`PieceChain` takes a factory that makes the sink for each piece, `Files` or `MemTapes`.

### Example: a recorder that lost power

A recorder on a PC writes a tape of several tracks, 50 MiB so far, with the default sync interval of 1 s. The power
fails. On disk is `capture.wwt.partial`: every block up to the last sync, and perhaps part of the block being written.

A reader opens it. The file has no tail, so the reader looks backwards from the end for the newest checkpoint index,
which is at most about 4 MiB back. From that checkpoint it follows the chain of checkpoints back and so lists every
block before it, without reading them. It then scans the blocks after the newest checkpoint, one by one, and stops at
the torn block, which fails its CRC. The reader reports the piece as unfinished, cut at that offset.

The recording loses the torn block and whatever the writer had not yet synced: at most about one second of data.
Everything else reads, including the describe set and the summary blocks written so far. The content hash can't be
checked, because the finish block was never written; that is how a reader knows the file was not closed.

## Sessions and stale data

On a file, the end of the data is the end of the file. On a raw medium, such as an SD card or raw NAND flash written
without a file system, it is not: the card may hold an older, longer recording behind the current one, and that
recording's blocks are as valid as the new ones.

A **session** is one writer's unbroken append: a producer start, a device boot. Every block of a session carries the same 32-bit
**session tag** in its header, and its sequence numbers count from 0. A writer picks the tag at random, or derives it from
the piece id when the same input must give the same bytes; never 0 nor FFFFFFFFh, so a blank header never names a session. A
reader keeps to one session at a time. A good block of another session is **stale**, unless a new session may start there.

Where a new session may start depends on the **medium**:

- In a file, any describe block (another producer appended its blocks) or any piece head (files concatenated) may start
  a session.
- On a raw medium, a new session starts only at a piece head that names the session just ended as its previous session. Stale data
  from an earlier recording names some other session and is never taken, even when it starts with a piece head of its own.

A raw medium is therefore read forward, from its start or from a hint the medium layer gives, never from its end, where
a stale tail could pass for the newest piece.

### Blank space and gaps

**Blank** bytes are all 0x00 or all 0xFF: what an erased or preallocated medium reads, depending on its kind. No block
starts with either, so a blank stretch running to the end of the file or medium is the end of the data.

A blank or damaged stretch in the middle is not always the end. A NAND writer whose page program failed moves on to the
next block; a writer padding a NAND page at a sync leaves 0xFF behind. The describe set's **layout** says how far a
writer may skip and where it resumes:

| Medium | Longest gap | Resumes at a multiple of |
|---|---|---|
| file | 0: the first bad stretch is the end | 1 byte |
| SD card or eMMC | 4 MiB | 512 bytes, a sector |
| raw NAND | two erase blocks | one page |

Past a blank or bad stretch, a reader reads one block header at the first non-blank byte and at each resume place within
the longest gap. A good block of the current session that goes on with the data there makes the stretch a **gap**: reported
with its length, and the walk goes on. When there is none, the stretch is the end of the data.

The format bounds what a reader pays per gap. A valid layout allows at most 64 MiB of gap with at most 8192 resume
places, and a reader checks at most 4 whole blocks per gap. On a raw medium, blank space longer than the gap is the end,
and nothing after it is read: an erased 64 GB card costs one gap's worth of header reads.

### Duplicates

A writer that retried a write may leave the same block twice. The same session and sequence number twice is a **duplicate**,
not a cut. With the same bytes, the second copy is dropped. With other bytes, the later copy is taken and the
replacement is reported.

### Example: a logger card recorded twice

A data logger records to an SD card as a raw medium. Its first recording, session A, filled 100 MiB. The card is reused: session
B writes 30 MiB from the start, then the power fails during a write.

A reader walks forward from the start of the card. It takes B's blocks up to the torn one, then probes for B's next
block within 4 MiB. It finds only A's old blocks, which carry A's session tag and don't start with a piece head naming B.
They are stale, so the data ends at the torn block, and none of A's 70 remaining MiB is mistaken for B's.

When the logger boots again, it finds the same end of data and writes its next session after it, starting with a piece head
that names B as the previous session. That session, C, is taken by the next reader; A's leftovers beyond C stay stale.

## What a damaged file still gives

The three cases below cover where damage lands in a file. "Lost" means unreadable: overwritten, zeroed or bad sectors.

**The first 4 KiB are lost.** These hold the piece head, the describe set and perhaps the first data blocks.

- A finished file opens from its tail as usual. The final index lists every block, and every block outside the lost
  range reads. The describe set comes from the newest copy that reads whole: the one after the first data block, the
  one after the first checkpoint, a 128 MiB repeat, or the backup before the finish. The records and chunks inside the lost range are lost, and the content
  hash no longer matches. The link back to the previous piece in a chain can't be checked, since the head named it; the
  link forward still can.
- An unfinished file with at least one checkpoint opens from its newest checkpoint, and the same holds, without the
  backup.
- An unfinished file shorter than one checkpoint interval (about 4 MiB) is scanned from its start, and the scan stops at
  the damage: `TapeReader` lists nothing after it. A walk in Resync mode reads every good block after the damage.

**The last 4 KiB are lost.** These hold the tail and the finish, and often the describe backup and part of the final
index. The reader finds no tail and reads the file as unfinished: it finds the newest checkpoint and scans up to the
damage. Every data block before the damage reads. The content hash and the link forward to the next piece are lost, and
the file reports as unfinished instead of complete.

**One block in the middle is lost.** The cost depends on its kind:

| Lost block | Cost |
|---|---|
| events chunk, payload damaged | the items of the groups it hits are marked suspect; the rest read |
| events chunk, head damaged | the chunk's items; the index still says which track, items and ticks they were |
| series chunk | one 64 KiB CRC segment per hit with a good head; the whole chunk with a bad head |
| record block | its records; the index entry says how many, the gap in sequence numbers how many blocks |
| summary | nothing; it is rebuilt from the data |
| describe | nothing while a copy reads whole |
| checkpoint index | the stretch it covered is scanned instead |

In a finished file a reader reads blocks by the final index, so a lost block in the middle costs that block alone. In an
unfinished file the same holds before the newest checkpoint. After it, the reader's scan stops at the lost block, and a
Resync walk reads what follows.

## What tape and flash media taught the format

Tape drives and flash memory solved these problems long before; the format takes their answers where they fit. Each
point below names its source.

- **A tag per write session.** LTO marks every data set with a write pass number that grows whenever data is
  overwritten, so a reader tells current data from residue (ECMA-319, §13.5.16). UBI stamps an image sequence number on
  every eraseblock for the same reason (Linux `ubi-media.h`). The session tag does this job. It is random instead of a
  counter, because a wiped card or a device without stored state has no counter to continue.
- **Nothing after the end is data.** LTO declares data recorded after the end-of-data set inaccessible and invalid
  (ECMA-319, §13.4.3). The format has no end-of-data block, since a crash leaves none; blank space and the session tag mark
  the end instead.
- **Erased is not always zero.** NAND erases to 0xFF (Micron TN-29-19). An SD card reads 0 or 1 after an erase, by
  vendor (SD Physical Layer Simplified Specification 9.10, §4.3.5.1), and eMMC reports which (JESD84-A441). So blank is
  0x00 or 0xFF.
- **A blank unit isn't the end.** A NAND writer that hits a failed program moves on and leaves erased pages behind;
  UBIFS looks past such units when it scans for the end of written data (Linux `fs/ubifs/scan.c`, `recovery.c`). The
  layout's gap and resume alignment say how far to look. Sizes follow the media: SD allocation units, up to 64 MB (SD
  9.10, §4.13.1), and NAND erase blocks and pages (ONFI 6.0, §3.1).
- **Program a NAND page once.** Programming the same part of a page twice without an erase gives undefined contents
  (ONFI 6.0, §6.7.1.25), and Micron's MLC example allows one program per page (Micron TN-29-19). A writer on raw NAND
  pads the rest of the page at each sync, and readers take the padding as a gap.
- **Rewrites: the later copy wins.** LTO rewrites an interrupted data set in its entirety, and the later write counts
  (ECMA-319, §15.4). The same block twice in a tape is a duplicate, and the later copy is taken.
- **Indexes that point at themselves and back.** An LTFS index carries a self pointer and a pointer to the index before
  it, and an index whose self pointer doesn't match is taken as data (LTFS Format Specification 2.5.1, §5.4.2, §5.4.3).
  A tape's index blocks carry the same two offsets, and a reader checks the self offset against where it found the
  block.
- **Never grow incompressible data.** SLDC switches to a scheme that copies bytes, so incompressible data grows by about
  0.05 % instead of 12.5 % (ECMA-321, §7.2). A writer that codes a block's body keeps the plain body when coding doesn't
  shrink it.
- **Correction belongs to the medium.** ONFI leaves error correction to the host controller, per page (ONFI 6.0,
  §6.7.1.26). A tape only detects what that correction missed.

## Limits

The format does not do everything. These limits follow from the design:

- **No correction.** Checksums detect damage; they don't repair it. The parity block kind is reserved, and nothing
  writes or reads it.
- **Unfinished small files.** A file without a checkpoint yet, under about 4 MiB, is read by a scan from its start,
  which stops at the first bad block. Only a Resync walk reads past it.
- **Describe set near the start.** The first describe copy follows the first data block. In an event file of small
  blocks the head's set and that copy can both sit in the first 4 KiB, so one bad sector can take both. The next copy
  follows the first checkpoint, about 4 MiB in; an unfinished file shorter than that has no other. Chunks then still
  place themselves by track id, item number and tick, but without clock rates and item types a reader can't turn ticks
  into seconds or bytes into values.
- **Lost additions in a scanned stretch.** `describe_set` names a damaged addition only when an index entry or a
  checkpoint lists it. A damaged block where a scan stopped has no known kind; the piece reports it as its cut.
- **A damaged chunk head costs the chunk.** The head CRC protects the payload's CRC segments, not itself.
- **lz4 payload columns.** One bad byte makes every item of the chunk suspect.
- **32-bit session tags.** Stale data matches the current session by chance with probability 2⁻³² per stale session.
- **MLC paired pages.** On MLC NAND, a power cut can corrupt an earlier, already synced page paired with the one being
  programmed. Keeping unsynced writes away from synced pages is the medium layer's task, not the format's.
- **The content hash needs a finished piece.** An unfinished or cut-off piece has no hash to check.
- **Bursty nanosecond times cost more than varints.** Random access costs about 1.4 bytes per event on such data ([What
  it costs](events_and_series.md#what-it-costs)).
- **Fine ticks need epochs.** A 64-bit tick at 1 ps spans about 106 days; longer captures are cut into clock epochs.
- **Reserved, not built.** A payload column holding one shrink_wrap `Series<T>`, run-coded discrete samples, struct
  samples with a column per field, outer codecs on chunk columns, and preallocated space. Today's readers refuse the
  reserved numbers, so files that use them later won't be misread.

## See also

- [The link tap](../transport/tap.md#sessions-media-and-damage): sessions, media, gaps and describe copies as a reference.
