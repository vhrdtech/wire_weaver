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
    /// Answers pending `deferred` and `deferred_unit` calls from the handler, with `value` for `deferred`
    fn answer_deferred(value: u8);
    /// Sequence number of this request, as seen by the handler
    fn request_seq() -> u32;
    /// Medium this request came from, as seen by the handler
    fn request_medium() -> u8;
}

#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Eq))]
struct UserDefined<'i> {
    a: u8,
    b: RefVec<'i, u8>,
}
