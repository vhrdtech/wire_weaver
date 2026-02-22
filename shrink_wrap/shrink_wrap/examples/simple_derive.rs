use shrink_wrap::prelude::*;

#[derive_shrink_wrap]
#[derive(Debug, PartialEq)]
struct CoordV1 {
    x: u8,
    y: u8,
}

fn main() {
    let mut buf = [0u8; 64];
    let coord = CoordV1 { x: 0xAA, y: 0xCC };
    let bytes = coord.to_ww_bytes(&mut buf).unwrap();
    assert_eq!(bytes, &[0xAA, 0xCC]);

    let des = CoordV1::from_ww_bytes(bytes).unwrap();
    println!("{:02x?}", des);
}
