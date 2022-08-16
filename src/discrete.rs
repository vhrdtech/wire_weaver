use crate::serdes::{BitBuf, DeserializeBits};

/// 3 bit unsigned integer
#[derive(Copy, Clone, Debug)]
pub struct U3(u8);
impl U3 {
    pub const fn new(from: u8) -> Option<Self> {
        if from <= 7 {
            Some(U3(from))
        } else {
            None
        }
    }
}

/// 4 bit unsigned integer
#[derive(Copy, Clone, Debug)]
pub struct U4(u8);
impl U4 {
    pub const fn new(from: u8) -> Option<Self> {
        if from <= 15 {
            Some(U4(from))
        } else {
            None
        }
    }
}

/// 6 bit unsigned integer
#[derive(Copy, Clone, Debug)]
pub struct U6(u8);
impl U6 {
    pub const fn new(from: u8) -> Option<Self> {
        if from <= 63 {
            Some(U6(from))
        } else {
            None
        }
    }
}

/// 7 bit unsigned integer shifted +1 == range 1..=128
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct U7Sp1(u8);
impl U7Sp1 {
    pub const fn new(from: u8) -> Option<Self> {
        if from >= 1 && from <= 128 {
            Some(U7Sp1(from - 1))
        } else {
            None
        }
    }

    pub fn to_u8(&self) -> u8 {
        self.0 + 1
    }
}

impl<'i> DeserializeBits<'i> for U7Sp1 {
    type Error = crate::serdes::bit_buf::Error;

    fn des_bits<'di>(rdr: &'di mut BitBuf<'i>) -> Result<Self, Self::Error> {
        let bits_6_0 = rdr.get_up_to_8(7)?;
        Ok(U7Sp1(bits_6_0))
    }
}

/// 2 bit unsigned integer shifted +1 == range 1..=4
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct U2Sp1(u8);
impl U2Sp1 {
    pub const fn new(from: u8) -> Option<Self> {
        if from >= 1 && from <= 4 {
            Some(U2Sp1(from - 1))
        } else {
            None
        }
    }

    pub fn to_u8(&self) -> u8 {
        self.0 + 1
    }
}

impl<'i> DeserializeBits<'i> for U2Sp1 {
    type Error = crate::serdes::bit_buf::Error;

    fn des_bits<'di>(rdr: &'di mut BitBuf<'i>) -> Result<Self, Self::Error> {
        let bits_1_0 = rdr.get_up_to_8(2)?;
        Ok(U2Sp1(bits_1_0))
    }
}