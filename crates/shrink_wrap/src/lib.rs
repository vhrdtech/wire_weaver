#![no_std]

pub mod buf_reader;
pub mod buf_writer;
pub mod traits;

pub use buf_reader::BufReader;
pub use buf_writer::BufWriter;
pub use traits::SerializeShrinkWrap;

#[derive(Debug)]
pub enum Error {
    OutOfBounds,
    MalformedVlu,
    MalformedLeb,
}
