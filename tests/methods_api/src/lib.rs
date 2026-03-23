use wire_weaver::prelude::*;

#[ww_trait]
trait Methods {
    fn no_args();
    fn one_plain_arg(value: u8);
    fn plain_return() -> u8;
    fn user_arg(u: UserDefined<'i>);
    fn user_defined_return() -> UserDefined<'i>;

    // user-defined
    // ()
    // array of methods
    // evolve args
    // evolve return from plain to struct
}

#[derive_shrink_wrap]
#[owned = "std"]
struct UserDefined<'i> {
    a: u8,
    b: RefVec<'i, u8>,
}
