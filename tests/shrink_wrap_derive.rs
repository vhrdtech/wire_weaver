use wire_weaver::shrink_wrap;
use wire_weaver::shrink_wrap::SerializeShrinkWrap;
use wire_weaver::ShrinkWrap;

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
fn simple_struct() {
    #[derive(ShrinkWrap)]
    struct Abc {
        a: u8,
        b: u16,
        c: u32,
    }
    let abc = Abc { a: 1, b: 2, c: 3 };
    ser_and_cmp!(abc, &[1, 2, 0, 3, 0, 0, 0]);
}
