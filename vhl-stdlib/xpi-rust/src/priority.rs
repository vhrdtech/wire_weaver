/// Priority selection: lossy or lossless (to an extent).
/// Truly lossless mode is not achievable, for example if connection is physically lost mid-transfer,
/// or memory is exceeded.
///
/// Higher priority in either mode means higher chance of successfully transferring a message.
/// If channels is wide enough, all messages will go through unaffected.
///
/// Some form of fair queueing must be implemented not to starve lossy channels by lossless ones.
/// Or several underlying channels may be used to separate the two. Up to the Link to decide on
/// implementation.
///
/// Some form of rate shaping should be implemented to be able to work with different channel speeds.
/// Rates can be changed in real time, limiting property observing or streams bandwidth.
/// TCP algorithms for congestion control may be applied here?
/// Alternatively discrete event simulation may be attempted to prove lossless properties.
/// Knowing streaming rates and precise size of various messages can help with that.
///
/// If loss occurs in lossy mode, it is silently ignored.
/// If loss occurs in lossless mode, it is flagged as an error.
///
/// Priority may be mapped into fewer levels by the underlying Link? (needed for constrained channels)
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum XpiGenericPriority<T> {
    Lossy(T),
    Lossless(T),
}