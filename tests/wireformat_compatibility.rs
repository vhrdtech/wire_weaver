/// This examples demonstrates backwards and forwards compatibility between a LED controller application
/// and a LED controller device

// Usually only one set of data structures and API dispatchers should be code generated for a particular version.
// But in this example we generate two versions and keep them in separate modules to test interoperability.
mod wire_v0_1_0 {
    use crate::wire_weaver_data_structures;
    wire_weaver_data_structures!(blinker, "0.1.0");
}

mod wire_v0_1_1 {
    use crate::wire_weaver_data_structures;
    wire_weaver_data_structures!(blinker, "0.1.1");
}

/// Application with wire format v0.1.0 and device with v0.1.0
#[test]
fn same_version() {}

// Application with wire format v0.1.1 and device with v0.1.0
#[test]
fn backwards_compatibility() {}

// Application with wire format v0.1.0 and device with v0.1.1
#[test]
fn forward_compatibility() {}
