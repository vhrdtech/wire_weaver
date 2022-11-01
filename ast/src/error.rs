#[derive(Debug)]
pub enum Error {
    /// File does not contain the specified line index.
    LineTooLarge { given: usize, max: usize },
    _Temp,
}