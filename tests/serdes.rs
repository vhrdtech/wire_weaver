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
    wire_weaver!(r#" struct X { y: Y } struct Y { } "#);
    let x = X { y: Y {} };
    ser_and_cmp!(x, &[0x00]);
}

#[test]
fn empty_final_struct() {
    wire_weaver!(r#" struct X { y: Y } #[final_ev] struct Y { } "#, dbg_gen);
    let x = X { y: Y {} };
    ser_and_cmp!(x, &[]);
}
