use vhl_stdlib_nostd::q_numbers::UqC;
use vhl_stdlib_nostd::serdes::nibble_buf::Error as NibbleBufError;
use vhl_stdlib_nostd::serdes::{DeserializeVlu4, NibbleBuf};
use vhl_stdlib_nostd::units::UnitStatic;

// #[derive(Copy, Clone, Debug)]
// pub struct Vlu4RateArray<'i> {
//     pub data: &'i [u8],
//     pub len: usize,
//     pub pos: usize,
// }
//
// impl<'i> DeserializeVlu4<'i> for Vlu4RateArray<'i> {
//     type Error = crate::serdes::nibble_buf::Error;
//
//     fn des_vlu4<'di>(_rdr: &'di mut NibbleBuf<'i>) -> Result<Self, Self::Error> {
//         todo!()
//     }
// }
//

/// Observing or publishing rate in [Hz].
#[derive(Copy, Clone, Debug)]
pub struct Rate(UnitStatic<UqC<24, 8>, -1, 0, 0, 0, 0, 0, 0>);

impl<'i> DeserializeVlu4<'i> for Rate {
    type Error = NibbleBufError;

    fn des_vlu4<'di>(_rdr: &'di mut NibbleBuf<'i>) -> Result<Self, Self::Error> {
        todo!()
    }
}