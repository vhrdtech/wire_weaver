use wire_weaver::shrink_wrap::prelude::*;

fn simple_wr() {
    let mut buf = [0u8; 256];
    let mut wr = BufWriter::new(&mut buf);
    wr.write_bool(true).unwrap();
    wr.write_u8(0xaa).unwrap();
    let bytes = wr.finish().unwrap();
    assert_eq!(bytes, &[0x80, 0xaa]);
}

fn simple_rd() {
    let buf = [0x80, 0xaa];
    let mut rd = BufReader::new(&buf[..]);
    assert!(rd.read_bool().unwrap());
    assert_eq!(rd.read_u8().unwrap(), 0xaa);
}

fn main() {
    simple_wr();
    simple_rd();
}
