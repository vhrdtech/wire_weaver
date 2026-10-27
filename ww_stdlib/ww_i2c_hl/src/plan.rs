//! An embedded-hal transaction as one `ww_i2c` call: contiguous operations of the same direction merged, checked
//! against the device's limits, and the read bytes handed back to the caller's buffers.

use crate::{Error, I2cLimits};
use embedded_hal::i2c::Operation;
use wire_weaver::prelude::*;
use ww_i2c::{I2cOpOwned, MAX_OPS};

/// The one `ww_i2c` call an embedded-hal transaction becomes.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Plan {
    /// No bytes to move: an empty transaction, or reads of zero bytes only.
    Nothing,
    /// `I2c::i2c_write`.
    Write(Vec<u8>),
    /// `I2c::i2c_read`, `I2cReadKind::Plain`.
    Read(u16),
    /// `I2c::i2c_read`, `I2cReadKind::RepeatedStart`: a write of at most `max_write_read_len` bytes, then a read.
    WriteRead { write: Vec<u8>, read: u16 },
    /// `I2c::i2c_transaction`.
    Transaction(Vec<I2cOpOwned>),
}

/// One merged operation, before it is checked.
enum Merged {
    Write(Vec<u8>),
    Read(usize),
}

/// Merges contiguous operations of the same direction into one (embedded-hal runs them without a repeated start,
/// `ww_i2c` puts one before every operation) and picks the call. Refuses, without sending anything, what the
/// device doesn't take in one call: splitting it would put a stop and a start where the driver expects none.
pub(crate) fn plan(operations: &[Operation<'_>], limits: &I2cLimits) -> Result<Plan, Error> {
    let mut merged: Vec<Merged> = Vec::new();
    for op in operations {
        match (op, merged.last_mut()) {
            (Operation::Write(bytes), Some(Merged::Write(w))) => w.extend_from_slice(bytes),
            (Operation::Write(bytes), _) => merged.push(Merged::Write(bytes.to_vec())),
            // a read of nothing puts nothing on the bus
            (Operation::Read(buf), _) if buf.is_empty() => {}
            (Operation::Read(buf), Some(Merged::Read(n))) => *n += buf.len(),
            (Operation::Read(buf), _) => merged.push(Merged::Read(buf.len())),
        }
    }

    let read_total: usize = merged
        .iter()
        .map(|m| match m {
            Merged::Read(n) => *n,
            Merged::Write(_) => 0,
        })
        .sum();
    if read_total > limits.max_read_len as usize {
        return Err(Error::ReadTooLong {
            len: read_total,
            max: limits.max_read_len,
        });
    }
    for m in &merged {
        if let Merged::Write(w) = m
            && w.len() > limits.max_write_len as usize
        {
            return Err(Error::WriteTooLong {
                len: w.len(),
                max: limits.max_write_len,
            });
        }
    }

    // read_total fits max_read_len, a u16
    Ok(match merged.as_slice() {
        [] => Plan::Nothing,
        [Merged::Write(_)] => {
            let Some(Merged::Write(w)) = merged.pop() else {
                unreachable!()
            };
            Plan::Write(w)
        }
        [Merged::Read(n)] => Plan::Read(*n as u16),
        [Merged::Write(w), Merged::Read(n)] if w.len() <= limits.max_write_read_len as usize => {
            Plan::WriteRead {
                write: w.clone(),
                read: *n as u16,
            }
        }
        _ => {
            let max = limits.max_ops.min(MAX_OPS as u16);
            if merged.len() > max as usize {
                return Err(Error::TooManyOps {
                    ops: merged.len(),
                    max,
                });
            }
            Plan::Transaction(
                merged
                    .into_iter()
                    .map(|m| match m {
                        Merged::Write(w) => I2cOpOwned::Write(MaxLen::new_unchecked(w)),
                        Merged::Read(n) => I2cOpOwned::Read(n as u16),
                    })
                    .collect(),
            )
        }
    })
}

impl Plan {
    /// Bytes the call reads in all.
    pub(crate) fn read_len(&self) -> usize {
        match self {
            Plan::Nothing | Plan::Write(_) => 0,
            Plan::Read(n) | Plan::WriteRead { read: n, .. } => *n as usize,
            Plan::Transaction(ops) => ops
                .iter()
                .map(|op| match op {
                    I2cOpOwned::Read(n) => *n as usize,
                    _ => 0,
                })
                .sum(),
        }
    }
}

/// Hands the bytes the call read back to the read buffers of `operations`, in order.
pub(crate) fn scatter(read: &[u8], operations: &mut [Operation<'_>]) {
    let mut rest = read;
    for op in operations {
        if let Operation::Read(buf) = op {
            let (head, tail) = rest.split_at(buf.len());
            buf.copy_from_slice(head);
            rest = tail;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> I2cLimits {
        I2cLimits {
            max_write_len: 8,
            max_read_len: 8,
            max_write_read_len: 2,
            max_ops: 3,
        }
    }

    fn write(bytes: &[u8]) -> I2cOpOwned {
        I2cOpOwned::Write(MaxLen::new_unchecked(bytes.to_vec()))
    }

    #[test]
    fn single_operations_use_the_plain_calls() {
        let l = limits();
        assert_eq!(plan(&[], &l).unwrap(), Plan::Nothing);
        assert_eq!(
            plan(&[Operation::Write(&[1, 2])], &l).unwrap(),
            Plan::Write(vec![1, 2])
        );
        assert_eq!(
            plan(&[Operation::Write(&[])], &l).unwrap(),
            Plan::Write(vec![])
        );
        let mut buf = [0u8; 3];
        assert_eq!(
            plan(&[Operation::Read(&mut buf)], &l).unwrap(),
            Plan::Read(3)
        );
        let mut buf = [0u8; 0];
        assert_eq!(
            plan(&[Operation::Read(&mut buf)], &l).unwrap(),
            Plan::Nothing
        );
    }

    #[test]
    fn write_read_is_a_register_read_while_the_write_fits() {
        let l = limits();
        let mut buf = [0u8; 2];
        assert_eq!(
            plan(&[Operation::Write(&[0x10]), Operation::Read(&mut buf)], &l).unwrap(),
            Plan::WriteRead {
                write: vec![0x10],
                read: 2
            }
        );
        // past max_write_read_len: still one call, as a transaction
        assert_eq!(
            plan(
                &[Operation::Write(&[1, 2, 3]), Operation::Read(&mut buf)],
                &l
            )
            .unwrap(),
            Plan::Transaction(vec![write(&[1, 2, 3]), I2cOpOwned::Read(2)])
        );
    }

    #[test]
    fn same_direction_neighbours_merge() {
        let l = limits();
        let (mut a, mut b, mut c) = ([0u8; 1], [0u8; 2], [0u8; 0]);
        let ops = [
            Operation::Write(&[1]),
            Operation::Write(&[2, 3]),
            Operation::Read(&mut a),
            Operation::Read(&mut c),
            Operation::Read(&mut b),
            Operation::Write(&[4]),
        ];
        assert_eq!(
            plan(&ops, &l).unwrap(),
            Plan::Transaction(vec![write(&[1, 2, 3]), I2cOpOwned::Read(3), write(&[4])])
        );
        // two writes become one, so a register write split in the driver is still one write
        assert_eq!(
            plan(&[Operation::Write(&[1]), Operation::Write(&[2])], &l).unwrap(),
            Plan::Write(vec![1, 2])
        );
    }

    #[test]
    fn past_the_limits_is_refused_not_split() {
        let l = limits();
        let mut buf = [0u8; 9];
        assert!(matches!(
            plan(&[Operation::Read(&mut buf)], &l),
            Err(Error::ReadTooLong { len: 9, max: 8 })
        ));
        assert!(matches!(
            plan(&[Operation::Write(&[0; 9])], &l),
            Err(Error::WriteTooLong { len: 9, max: 8 })
        ));
        let (mut a, mut b) = ([0u8; 1], [0u8; 1]);
        let ops = [
            Operation::Write(&[1]),
            Operation::Read(&mut a),
            Operation::Write(&[2]),
            Operation::Read(&mut b),
        ];
        assert!(matches!(
            plan(&ops, &l),
            Err(Error::TooManyOps { ops: 4, max: 3 })
        ));
    }

    #[test]
    fn read_bytes_go_back_in_order() {
        let (mut a, mut b, mut c) = ([0u8; 1], [0u8; 0], [0u8; 2]);
        let mut ops = [
            Operation::Read(&mut a),
            Operation::Write(&[9]),
            Operation::Read(&mut b),
            Operation::Read(&mut c),
        ];
        scatter(&[1, 2, 3], &mut ops);
        assert_eq!((a, c), ([1], [2, 3]));
    }
}
