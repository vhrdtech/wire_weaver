use shrink_wrap::SerializeShrinkWrap;
use wire_weaver::wire_weaver;

macro_rules! ser_and_cmp {
    ($item:expr, $expected:expr) => {
        let mut buf = [0u8; 256];
        let mut wr = shrink_wrap::BufWriter::new(&mut buf);
        $item.ser_shrink_wrap(&mut wr).unwrap();
        let buf = wr.finish().unwrap();
        assert_eq!(buf, $expected)
    };
}

#[test]
fn empty_non_final_struct() {
    wire_weaver!(r#" struct X {} "#);
    let x = X {};
    ser_and_cmp!(x, &[]);
}

#[test]
fn empty_final_struct() {
    wire_weaver!(r#" #[final_ev] struct X {} "#);
    let x = X {};
    ser_and_cmp!(x, &[]);
}

#[test]
fn empty_non_final_struct_in_struct() {
    wire_weaver!(r#" struct X { y: Y } struct Y { } "#);
    let x = X { y: Y {} };
    ser_and_cmp!(x, &[0x00]);
}

#[test]
fn empty_final_struct_in_struct() {
    wire_weaver!(r#" struct X { y: Y } #[final_ev] struct Y { } "#);
    let x = X { y: Y {} };
    ser_and_cmp!(x, &[]);
}

#[test]
fn enum_vlu16n_standalone() {
    wire_weaver!(r#" #[repr(vlu16n)] enum E { A, B, C = 9 } "#);
    let e = E::A;
    ser_and_cmp!(e, &[0x10]);
    let e = E::C;
    ser_and_cmp!(e, &[0x91]);
}

#[test]
fn enum_vlu16n_in_struct() {
    wire_weaver!(r#" #[repr(vlu16n)] enum E { A, B, C = 9 } struct X { e: E } "#);
    let x = X { e: E::A };
    ser_and_cmp!(x, &[0x10]);
}

/// Unsized enum, but since it's the root object, size is not written
#[test]
fn enum_vlu16n_data_standalone() {
    wire_weaver!(r#" #[repr(vlu16n)] enum E { A { a: u8 }, B { b: u8, c: u16 } = 9 } "#);
    let e = E::A { a: 0xAA };
    ser_and_cmp!(e, &[0x10, 0xAA]);
    let e = E::B { b: 0xAA, c: 0xBBCC };
    ser_and_cmp!(e, &[0x91, 0xAA, 0xCC, 0xBB]);
}

/// Unsized enum in a struct, size is written = 4
#[test]
fn enum_vlu16n_data_in_struct() {
    wire_weaver!(
        r#"
        #[repr(vlu16n)]
        enum E {
            A { a: u8 },
            B { b: u8, c: u16 } = 9
        }
        struct X { e: E }
    "#
    );
    let x = X {
        e: E::B { b: 0xAA, c: 0xBBCC },
    };
    ser_and_cmp!(x, &[0x91, 0xAA, 0xCC, 0xBB, 0x04]);
}

// #[test]
// fn enum_vlu16n_final() {
//     wire_weaver!(r#"
//         #[final_ev]
//         #[repr(vlu16n)]
//         enum E { A, B, C }
//         struct X { e1: E, e2: E, e3: E }
//     "#);
//     let x = X { e1: E::C, e2: E::B, e3: E::C };
//     ser_and_cmp!(x, &[0x21, 0x00]);
// }

//
// #[test]
// fn empty_non_final_struct() {
//     wire_weaver!(r#" struct X {} "#);
//     let x = X {};
//     ser_and_cmp!(x, &[]);
// }
//
// #[test]
// fn empty_non_final_struct() {
//     wire_weaver!(r#" struct X {} "#);
//     let x = X {};
//     ser_and_cmp!(x, &[]);
// }
//
// #[test]
// fn empty_non_final_struct() {
//     wire_weaver!(r#" struct X {} "#);
//     let x = X {};
//     ser_and_cmp!(x, &[]);
// }
//
// #[test]
// fn empty_non_final_struct() {
//     wire_weaver!(r#" struct X {} "#);
//     let x = X {};
//     ser_and_cmp!(x, &[]);
// }
//
// #[test]
// fn empty_non_final_struct() {
//     wire_weaver!(r#" struct X {} "#);
//     let x = X {};
//     ser_and_cmp!(x, &[]);
// }
//
// #[test]
// fn empty_non_final_struct() {
//     wire_weaver!(r#" struct X {} "#);
//     let x = X {};
//     ser_and_cmp!(x, &[]);
// }
//
// #[test]
// fn empty_non_final_struct() {
//     wire_weaver!(r#" struct X {} "#);
//     let x = X {};
//     ser_and_cmp!(x, &[]);
// }
//
// #[test]
// fn empty_non_final_struct() {
//     wire_weaver!(r#" struct X {} "#);
//     let x = X {};
//     ser_and_cmp!(x, &[]);
// }
//
