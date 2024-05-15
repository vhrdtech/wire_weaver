use shrink_wrap::SerializeShrinkWrap;
use wire_weaver::data_structures;

data_structures!("./ww/blinker_v1.ww");

fn main() {
    let cmd = Command {
        blink_frequency: 0.25,
        blink_duty: 0.5,
    };

    let mut buf = [0u8; 256];
    let mut wr = shrink_wrap::BufWriter::new(&mut buf);
    cmd.ser_shrink_wrap(&mut wr).unwrap();
    let buf = wr.finish();
    println!("{:02x?}", buf);
}
