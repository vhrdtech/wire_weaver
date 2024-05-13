use wire_weaver::data_structures;

pub mod wfdb {
    #[derive(Debug)]
    pub enum Error {}

    pub struct WfdbBufMut {}

    impl WfdbBufMut {
        pub fn ser_f32(&mut self, v: f32) -> Result<(), Error> {
            Ok(())
        }
    }
}

data_structures!("./ww/blinker_v1.ww");

fn main() {
    let cmd = Command {
        blink_frequency: 0.25,
        blink_duty: 0.5,
    };

    let mut buf = wfdb::WfdbBufMut {};
    cmd.ser_wfdb(&mut buf).unwrap();

    println!("Ok");
}
