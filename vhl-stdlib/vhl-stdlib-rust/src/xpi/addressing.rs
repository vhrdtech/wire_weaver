#[derive(Copy, Clone, Debug)]
pub enum XpiGenericNodeSet<DST, TS> {
    /// Request is targeted at only one specific node.
    /// Any resources can be used from the node's vhL description.
    Unicast(DST),

    /// Request is targeted at only one node, but through traits interface.
    /// More expensive in terms of size and processing, but gives other benefits.
    UnicastTraits {
        destination: DST,
        traits: TS,
    },

    /// Request is targeted at many nodes at once. Only nodes implementing a set of common traits can
    /// be addressed that way.
    ///
    /// Trait in this context is an xPI block defined and published to the Registry with a particular version.
    /// Might be thought of as an abstract class as well.
    ///
    /// Examples of xpi traits:
    /// * log - to e.g. subscribe to all node's logs at once
    /// * bootloader - to e.g. request all firmware versions
    /// * power_mgmt - to e.g. put all nodes to sleep
    /// Other more specific traits that only some nodes would implement:
    /// * led_feedback - to e.g. enable or disable led on devices
    /// * canbus_counters - to monitor CANBus status across the whole network
    Multicast {
        /// List of traits a node have to implement.
        /// Uri structure is arranged differently for this kind of requests.
        /// For example if 3 traits were provided, then there are /0, /1, /2 resources,
        /// each corresponding to the trait specified, in order.
        /// So e.g. it is possible to call 3 different functions from 3 different traits in one request.
        traits: TS,
    },
    // Broadcast,
}

/// It is possible to perform operations on a set of resources at once for reducing requests and
/// responses amount. Any operations can be grouped into one request or response.
/// For example several method calls (each with it's own arguments), property writes, reads, etc.
/// Mixed requests and response is also allowed.
///
/// If operation is only targeted at one resource, there are more efficient ways to select it than
/// using [MultiUri].
/// It is possible to select one resource in several different ways for efficiency reasons.
/// If there are several choices on how to construct the same uri, select the smallest one in size.
/// If both choices are the same size, choose [Uri].
///
/// [MultiUri] is the only way to select several resources at once within one request.
#[derive(Clone, Debug)]
pub enum XpiGenericResourceSet<
    U,
    MU
> {
    /// Select any one resource at any depth.
    /// Or root resource by providing 0 length Uri.
    Uri(U),

    /// Selects any set of resources at any depths at once.
    MultiUri(MU),
}