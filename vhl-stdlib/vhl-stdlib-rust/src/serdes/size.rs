use core::ops::Add;

/// Size of a type serialized into buffer of any kind,
/// in buffer elements (bits / nibbles / bytes / etc).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum SerDesSize {
    /// Size is known, no alignment requirements
    Sized(usize),

    /// Size is known (=.0), but there is an alignment requirement, requiring up to .1 padding elements.
    /// For example u8 slice in nibble buf requires up to 1 nibble of alignment. But whether padding
    /// will actually be used cannot be known in advance and depends on the runtime buffer state.
    SizedAligned(usize, usize),

    /// Size cannot be known in advance (vec, str, ..).
    /// Not knowing size before serialization is not an issue when not trying to conserve space.
    /// u16/u32 = 0 can be written into the buffer and updated afterwards for all reasonable lengths.
    /// When using variable integers, this becomes trickier.
    /// One way to deal with it is to use another buffer (or the same) and make a copy after serializing.
    /// Another approach is to speculatively assume all remaining space in a buffer to be the type size,
    /// updating it after serialization took place.
    ///
    /// For example if buffer space available is < 512 bytes and NibbleBuf is used, 3 nibbles must be
    /// written before attempting to serialize Unsized type (representing <511 in vlu4).
    /// Up to 2 of them might be left unused. When buffer fills up, lower and lower wasted elements are expected.
    /// Compared to just using 4/8 nibbles for size, it is still a big saving of at least 2/6 nibbles.
    Unsized,

    /// Same as Unsized, but maximum size is known in advance, for example for arrays with max bound.
    UnsizedBound(usize),
}

impl SerDesSize {
    pub fn upper_bound(&self, buffer_elements_left: usize) -> usize {
        match self {
            SerDesSize::Sized(len) => *len,
            SerDesSize::SizedAligned(len, padding) => *len + *padding,
            SerDesSize::Unsized => buffer_elements_left,
            SerDesSize::UnsizedBound(max_len) => *max_len,
        }
    }
}

impl Add<usize> for SerDesSize {
    type Output = SerDesSize;

    fn add(self, rhs: usize) -> Self::Output {
        match self {
            SerDesSize::Sized(len) => SerDesSize::Sized(len + rhs),
            SerDesSize::SizedAligned(len, padding) => SerDesSize::SizedAligned(len + rhs, padding),
            SerDesSize::Unsized => SerDesSize::Unsized,
            SerDesSize::UnsizedBound(max_len) => SerDesSize::UnsizedBound(max_len + rhs),
        }
    }
}
