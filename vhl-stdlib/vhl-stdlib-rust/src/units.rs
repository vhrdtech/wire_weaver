use core::marker::PhantomData;

pub struct Unit {

}

#[derive(Copy, Clone, Debug)]
pub struct UnitStatic<
    X,
    //SCALE,
    const T: isize,
    const L: isize,
    const M: isize,
    const I: isize,
    const O: isize,
    const N: isize,
    const J: isize
> {
    _phantom: PhantomData<X>,
}