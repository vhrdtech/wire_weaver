use wire_weaver::prelude::*;

#[ww_trait]
trait Streams {
    stream!(plain_stream: u8);
    sink!(plain_sink: u8);
    stream!(vec_stream: [u8]);
    stream!(array_of_streams[]: [u8]);
    fn finish();
    stream!(user_stream: Point);
    sink!(user_sink: Point);
    sink!(bytes_sink: [u8]);
    /// Handler sends `count` user_stream updates and one array_of_streams[1] update, returns how many user_stream
    /// updates were sent before running out of space
    fn emit(count: u8) -> u8;
}

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Eq, Clone, Copy)
)]
pub struct Point {
    pub x: i16,
    pub y: i16,
}
