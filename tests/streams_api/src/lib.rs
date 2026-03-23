use wire_weaver::prelude::*;

#[ww_trait]
trait Streams {
    stream!(plain_stream: u8);
    sink!(plain_sink: u8);
    stream!(vec_stream: [u8]);
    stream!(array_of_streams[]: [u8]);
    fn finish();
}
