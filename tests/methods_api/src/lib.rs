use wire_weaver::prelude::*;

#[ww_trait]
trait Methods {
    fn no_args();
    fn one_plain_arg(value: u8);
    fn plain_return() -> u8;
    fn user_arg(u: UserDefined<'i>);
    fn user_defined_return() -> UserDefined<'i>;
    /// Server returns Unimplemented
    fn absent();
    /// Answered later from outside of the handler, or never
    fn deferred() -> u8;
    fn deferred_unit();
}

#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Eq))]
struct UserDefined<'i> {
    a: u8,
    b: RefVec<'i, u8>,
}
