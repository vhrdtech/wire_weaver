//! API of the argument evolution tests (EVO-22): trailing arguments with defaults, and a struct whose fields are the
//! arguments (`#[flatten]`).
use wire_weaver::prelude::*;

#[ww_trait]
trait ArgsEvolution {
    /// `a + b`, the arguments one by one
    fn configure(a: u8, b: u16) -> u32;
    /// `a + b`: the same arguments as one struct, the same bytes as `configure`
    fn configure_flat(#[flatten] cfg: Config) -> u32;
    /// What the handler got: `b` has a written default, `c` and `tag` the natural ones (`None`, empty)
    fn with_defaults(a: u8, #[default = 7] b: u8, c: Option<u8>, tag: &'i str) -> Seen;
    /// A stream function whose arguments are a struct; refused with `Zero` when `a + b` is 0
    #[stream(u32)]
    fn sums(#[flatten] cfg: Config) -> Result<(), OpenError>;
}

/// The arguments of `configure` as a struct
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone, Copy))]
pub struct Config {
    pub a: u8,
    pub b: u16,
}

/// The arguments `with_defaults` got, `tag` by its length
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone, Copy))]
pub struct Seen {
    pub a: u8,
    pub b: u8,
    pub c: Option<u8>,
    pub tag_len: u8,
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone, Copy), ww_repr = u4)]
pub enum OpenError {
    Zero,
}
