//! Events written in place, for servers.
//!
//! The writers below each write one kind of [Event] with [EventBuilder] and [EventKindBuilder], the same bytes as
//! serializing the [Event], without linking [Event]'s serializer. That one covers every [EventKind], so a firmware
//! that calls it also gets the `Series<Second>` codec of [EventKind::PropertyUpdate] and the [StreamHead] writer of
//! [EventKind::StreamItem], whether it observes and streams or not (SRV-54, about 2 KB on a Cortex-M0+). Server
//! code (generated or in `ww_device`) uses only these. [ser_ok_event] takes any [EventKind] and so does link it: it
//! is for hosts and tests.
//!
//! [StreamHead]: crate::stream::StreamHead

use super::{Event, EventKind, EventKindDiscriminants, ResourceIndex, StreamSideband};
use crate::builder::{EventBuilder, EventKindBuilder};
use wire_weaver::shrink_wrap::{BufWriter, Error, SerializeShrinkWrap, UVlq32, ref_vec::RefVec};

/// Serialize an `Ok` event of any kind. Links the serializer of every [EventKind], see the [module docs](self):
/// device code uses the writer of its kind instead.
pub fn ser_ok_event<'a>(
    scratch: &'a mut [u8],
    seq: u32,
    kind: EventKind<'_>,
) -> Result<&'a [u8], Error> {
    let mut wr = BufWriter::new(scratch);
    let event = Event {
        seq: UVlq32(seq),
        result: Ok(kind),
    };
    event.ser_shrink_wrap(&mut wr)?;
    wr.finish_and_take()
}

/// Serialize an event carrying `error`.
pub fn ser_err_event<'i>(
    scratch: &'i mut [u8],
    seq: u32,
    error: super::Error<'_>,
) -> Result<&'i [u8], Error> {
    let mut wr = BufWriter::new(scratch);
    let event_builder = EventBuilder::new(seq, &mut wr)?;
    error.ser_shrink_wrap(&mut wr)?;
    event_builder.finish(false, &mut wr);
    wr.finish_and_take()
}

/// Serialize the [EventKind::Value] of a method that returns nothing, nothing at all when `seq` is 0.
pub fn ser_unit_return_event(scratch: &mut [u8], seq: u32) -> Result<&[u8], Error> {
    if seq == 0 {
        return Ok(&[]);
    }
    ser_value_event(scratch, seq, &[])
}

/// Serialize an [EventKind::Value] with `data`, a value serialized before (with `to_ww_bytes`).
pub fn ser_value_event<'a>(
    scratch: &'a mut [u8],
    seq: u32,
    data: &[u8],
) -> Result<&'a [u8], Error> {
    let mut wr = BufWriter::new(scratch);
    let event_builder = EventBuilder::new(seq, &mut wr)?;
    let kind_builder = EventKindBuilder::new(&mut wr)?;
    wr.write_raw_slice(data)?;
    kind_builder.finish_with_kind(EventKindDiscriminants::Value, &mut wr);
    event_builder.finish(true, &mut wr);
    wr.finish_and_take()
}

/// Serialize an [EventKind::StreamData] of the stream at `path` with `data` as it is: a chunk of a byte stream,
/// or a value serialized before. [ser_stream_data_event](crate::stream::ser_stream_data_event) serializes the value
/// in place instead.
pub fn ser_stream_data_bytes_event<'a>(
    scratch: &'a mut [u8],
    seq: u32,
    path: &[ResourceIndex],
    data: &[u8],
) -> Result<&'a [u8], Error> {
    let mut wr = BufWriter::new(scratch);
    let event_builder = EventBuilder::new(seq, &mut wr)?;
    let kind_builder = EventKindBuilder::new(&mut wr)?;
    wr.write(&RefVec::Slice { slice: path })?;
    wr.write_raw_slice(data)?;
    kind_builder.finish_with_kind(EventKindDiscriminants::StreamData, &mut wr);
    event_builder.finish(true, &mut wr);
    wr.finish_and_take()
}

/// Serialize an [EventKind::StreamSideband] of the stream (or sink) at `path`.
pub fn ser_stream_sideband_event<'a>(
    scratch: &'a mut [u8],
    seq: u32,
    path: &[ResourceIndex],
    sideband: StreamSideband,
) -> Result<&'a [u8], Error> {
    let mut wr = BufWriter::new(scratch);
    let event_builder = EventBuilder::new(seq, &mut wr)?;
    let kind_builder = EventKindBuilder::new(&mut wr)?;
    wr.write(&RefVec::Slice { slice: path })?;
    wr.write(&sideband)?;
    kind_builder.finish_with_kind(EventKindDiscriminants::StreamSideband, &mut wr);
    event_builder.finish(true, &mut wr);
    wr.finish_and_take()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Error as WwError, ErrorKind};
    use wire_weaver::shrink_wrap::{UNib32, UVlq64, tail_bytes::TailBytes};

    fn whole<'a, 'i>(
        buf: &'a mut [u8],
        seq: u32,
        result: Result<EventKind<'i>, WwError<'i>>,
    ) -> &'a [u8] {
        Event {
            seq: UVlq32(seq),
            result,
        }
        .to_ww_bytes(buf)
        .unwrap()
    }

    const SEQS: [u32; 4] = [0, 1, 127, 0x1234_5678];
    const DATA: [&[u8]; 3] = [
        &[],
        &[0xaa],
        &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17],
    ];
    const PATHS: [&[ResourceIndex]; 3] = [
        &[],
        &crate::resource_path([3]),
        &crate::resource_path([0, 7, 1000]),
    ];

    #[test]
    fn value_event_as_whole() {
        for seq in SEQS {
            for data in DATA {
                let (mut a, mut b) = ([0u8; 64], [0u8; 64]);
                let expected = whole(
                    &mut a,
                    seq,
                    Ok(EventKind::Value {
                        data: TailBytes(data),
                    }),
                );
                assert_eq!(ser_value_event(&mut b, seq, data).unwrap(), expected);
            }
            let (mut a, mut b) = ([0u8; 64], [0u8; 64]);
            let expected = whole(
                &mut a,
                seq,
                Ok(EventKind::Value {
                    data: TailBytes(&[]),
                }),
            );
            let unit = ser_unit_return_event(&mut b, seq).unwrap();
            if seq == 0 {
                assert!(unit.is_empty());
            } else {
                assert_eq!(unit, expected);
            }
        }
    }

    #[test]
    fn err_event_as_whole() {
        let errors = || {
            [
                WwError::response_ser_failed(5),
                WwError::not_supported(123),
                WwError::new(
                    0xff,
                    ErrorKind::UserBytes(RefVec::Slice {
                        slice: &[0xaa, 0xbb, 0xcc],
                    }),
                ),
            ]
        };
        for seq in SEQS {
            for (error, again) in errors().into_iter().zip(errors()) {
                let (mut a, mut b) = ([0u8; 64], [0u8; 64]);
                let expected = whole(&mut a, seq, Err(error));
                assert_eq!(ser_err_event(&mut b, seq, again).unwrap(), expected);
            }
        }
    }

    #[test]
    fn stream_data_bytes_event_as_whole() {
        for seq in SEQS {
            for path in PATHS {
                for data in DATA {
                    let (mut a, mut b) = ([0u8; 64], [0u8; 64]);
                    let expected = whole(
                        &mut a,
                        seq,
                        Ok(EventKind::StreamData {
                            path: RefVec::Slice { slice: path },
                            data: TailBytes(data),
                        }),
                    );
                    let built = ser_stream_data_bytes_event(&mut b, seq, path, data).unwrap();
                    assert_eq!(built, expected);
                }
            }
        }
    }

    #[test]
    fn stream_sideband_event_as_whole() {
        let sidebands = [
            StreamSideband::Open,
            StreamSideband::Close,
            StreamSideband::SizeHint(UNib32(1000)),
            StreamSideband::Ack {
                backfill: Some(UVlq64(5)),
                have: UVlq64(70_000),
                grant: ww_si::Bytes::new(UVlq32(512)),
            },
        ];
        for seq in SEQS {
            for path in PATHS {
                for sideband in sidebands {
                    let (mut a, mut b) = ([0u8; 64], [0u8; 64]);
                    let expected = whole(
                        &mut a,
                        seq,
                        Ok(EventKind::StreamSideband {
                            path: RefVec::Slice { slice: path },
                            sideband,
                        }),
                    );
                    let built = ser_stream_sideband_event(&mut b, seq, path, sideband).unwrap();
                    assert_eq!(built, expected);
                    assert_eq!(
                        ser_ok_event(
                            &mut a,
                            seq,
                            EventKind::StreamSideband {
                                path: RefVec::Slice { slice: path },
                                sideband,
                            }
                        )
                        .unwrap(),
                        built
                    );
                }
            }
        }
    }

    #[test]
    fn too_small_scratch_is_an_error() {
        let mut b = [0u8; 4];
        assert!(ser_value_event(&mut b, 1, &[0; 8]).is_err());
        assert!(
            ser_stream_data_bytes_event(&mut b, 1, &crate::resource_path([1]), &[0; 8]).is_err()
        );
        assert!(ser_err_event(&mut b[..1], 1000, WwError::not_supported(123)).is_err());
    }
}
