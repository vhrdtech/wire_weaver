//! A line of a facade is async: `use_async = false` is refused before any code is generated.

mod v1 {
    wire_weaver_client::facade_line!(some_api::Thermo, use_async = false);
}

fn main() {}
