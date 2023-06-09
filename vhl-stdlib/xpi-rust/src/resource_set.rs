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
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum XpiGenericResourceSet<U, MU> {
    /// Select any one resource at any depth.
    /// Or root resource by providing 0 length Uri.
    Uri(U),

    /// Selects any set of resources at any depths at once.
    MultiUri(MU),
}
