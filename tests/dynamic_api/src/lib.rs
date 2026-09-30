use wire_weaver::prelude::*;

/// API used by the dynamic client tests, covers every kind of type the dynamic serializer supports.
#[ww_trait]
trait Dynamic {
    fn no_args();
    fn add(a: u32, b: i16) -> i64;
    fn echo(value: Everything<'i>) -> Everything<'i>;
    fn check(x: Option<u8>) -> Result<u8, CheckError>;
    property!(rw speed: u16);
    property!(rw everything: Everything<'i>);
    ww_impl!(channel[]: Channel);
}

#[ww_trait]
trait Channel {
    property!(rw gain: f32);
    fn id() -> u32;
}

#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Clone))]
pub struct Everything<'i> {
    pub flag: bool,
    pub small: U4,
    pub signed: I5,
    pub nib: Nibble,
    pub v: UNib32,
    pub a: u8,
    pub b: u16,
    pub c: u32,
    pub d: u64,
    pub e: i8,
    pub f: i16,
    pub g: i32,
    pub h: i64,
    pub x: f32,
    pub y: f64,
    pub name: &'i str,
    pub bytes: RefVec<'i, u8>,
    pub arr: [u16; 3],
    pub pair: (u8, u32),
    pub opt: Option<Inner<'i>>,
    pub res: Result<u8, CheckError>,
    pub shapes: RefVec<'i, Shape<'i>>,
    pub range: Range<u16>,
    pub fixed: Fixed,
    pub last: U3,
}

#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Clone))]
pub struct Inner<'i> {
    pub id: u8,
    pub note: &'i str,
}

#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Clone), ww_repr = u4)]
pub enum Shape<'i> {
    Empty,
    Circle(u16),
    Rect { w: u8, h: u8, label: &'i str },
    Far = 9,
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Clone, Copy), ww_repr = u2)]
pub enum CheckError {
    TooBig,
    Missing,
}

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Clone, Copy),
    final_structure
)]
pub struct Fixed {
    pub a: u8,
    pub b: bool,
}
