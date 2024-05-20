use shrink_wrap::{DeserializeShrinkWrap, SerializeShrinkWrap};
use wire_weaver::data_structures;

data_structures!("./ww/blinker_dev_v1.ww");

fn main() {
    let x = RequestKind::Read;

    let mut buf = [0u8; 256];
    let mut wr = shrink_wrap::BufWriter::new(&mut buf);
    x.ser_shrink_wrap(&mut wr).unwrap();
    let buf = wr.finish().unwrap();
    println!("{:02x?} {}", buf, buf.len());

    let mut rd = shrink_wrap::BufReader::new(buf);
    let cmd_des = RequestKind::des_shrink_wrap(&mut rd).unwrap();
    dbg!(rd.bytes_left());
    println!("{:?}", cmd_des);
    //
    // // Deserialize v1.0 message
    // let mut rd = shrink_wrap::BufReader::new(&buf[0..4]);
    // let cmd_des = Command::des_shrink_wrap(&mut rd).unwrap();
    // println!("{:?}", cmd_des);
}
