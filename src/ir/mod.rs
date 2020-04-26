use std::fmt;
use std::fmt::{Error, Formatter};

use crate::types::Numeric;

pub enum BaseAddress {
    EightBit(u8),
    SixteenBit(u16),
    ThirtyTwoBit(u32),
    SixtyFour(u64),
}

impl fmt::Display for BaseAddress {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            BaseAddress::EightBit(addr) => write!(f, "8b:{:#04x}", addr),
            BaseAddress::SixteenBit(addr) => write!(f, "16b:{:#06x}", addr),
            BaseAddress::ThirtyTwoBit(addr) => write!(f, "32b:{:#10x}", addr),
            BaseAddress::SixtyFour(addr) => write!(f, "64b:{:#18x}", addr),
        }
    }
}

impl fmt::Debug for BaseAddress {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

#[derive(Debug)]
pub enum Endianness {
    Big,
    Little,
}

#[derive(Debug)]
pub struct SIUnit {}

#[derive(Debug)]
pub enum Access {
    ReadOnly,
    ReadWrite,
    WriteOnly,
}

#[derive(Debug)]
pub struct Register {
    pub base_address: BaseAddress,
    pub r#type: Numeric,
    pub endianness: Endianness,
    pub unit: SIUnit,
    pub default: Vec<u8>,
    pub access: Access,
    pub description: u32,
    //examples
    //allowed
}

impl Register {
    pub fn new() -> Self {
        Register {
            base_address: BaseAddress::SixteenBit(0xaa),
            r#type: Numeric::Q(1, 2),
            endianness: Endianness::Little,
            unit: SIUnit {},
            default: Vec::new(),
            access: Access::ReadOnly,
            description: 0,
        }
    }
}
