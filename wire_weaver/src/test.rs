#[cfg(test)]
mod tests {
    use crate::date_time::DateTime;
    use shrink_wrap::prelude::*;
    use wire_weaver_derive::derive_shrink_wrap;

    #[test]
    fn complex_types() {
        #[derive_shrink_wrap]
        struct Root {
            pub timestamp: DateTime,
        }
    }
}
