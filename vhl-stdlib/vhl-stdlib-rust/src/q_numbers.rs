use crate::units::Unit;

pub struct Uq {
    pub m: usize,
    pub n: usize,
    pub unit: Unit,
}

#[derive(Copy, Clone, Debug)]
pub struct UqC<const M: usize, const N: usize> {

}