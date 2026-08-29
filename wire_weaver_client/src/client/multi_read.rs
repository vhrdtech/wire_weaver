use wire_weaver::shrink_wrap::{DeserializeShrinkWrapOwned, UNib32};

use crate::event_loop::commander::TransportCommander;

pub(crate) trait PropertyPath {
    type Output;

    fn absolute_path(&self) -> Option<Vec<UNib32>>;
    fn commander(self) -> TransportCommander;
}

pub trait MultiRead {
    fn multi_read(self);
}

impl<A, B, AO, BO> MultiRead for (A, B)
where
    A: PropertyPath<Output = AO>,
    AO: DeserializeShrinkWrapOwned,
    B: PropertyPath<Output = BO>,
    BO: DeserializeShrinkWrapOwned,
{
    fn multi_read(self) {
        todo!()
    }
}
