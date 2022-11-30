use crate::owned::convert_error::ConvertError;
use crate::owned::{MultiUriOwned, UriOwned};
use crate::resource_set::XpiGenericResourceSet;
use crate::xwfd;
use std::fmt::{Display, Formatter};
use vhl_stdlib::discrete::U3;
use vhl_stdlib::serdes::NibbleBufMut;
use crate::owned::serial_multi_uri::MultiUriFlatIter;
use crate::owned::serial_uri::URI_STACK_SEGMENTS;

pub type ResourceSet = XpiGenericResourceSet<UriOwned, MultiUriOwned>;

/// Special type used for converting from owned to xwfd.
/// Uses xwfd uri implementation (that can work without allocators) but with SmallVec, which can allocate.
/// Also remembers discriminant of the uri.
pub(crate) type ResourceSetConvertXwfd =
XpiGenericResourceSet<xwfd::SerialUri<smallvec::IntoIter<[u32; URI_STACK_SEGMENTS]>>, MultiUriOwned>;

impl ResourceSet {
    pub fn flat_iter(&self) -> MultiUriFlatIter {
        match self {
            ResourceSet::Uri(uri) => MultiUriFlatIter::OneUri(Some(uri.clone())),
            ResourceSet::MultiUri(multi_uri) => multi_uri.flat_iter()
        }
    }

    pub fn ser_header_xwfd(
        &self,
    ) -> Result<ResourceSetConvertXwfd, ConvertError> {
        match &self {
            XpiGenericResourceSet::Uri(uri) => {
                Ok(ResourceSetConvertXwfd::Uri(uri.ser_header_xwfd()))
            }
            XpiGenericResourceSet::MultiUri(_multi_uri) => todo!(),
        }
    }
}

impl ResourceSetConvertXwfd {
    pub fn ser_body_xwfd(&self, nwr: &mut NibbleBufMut) -> Result<(), ConvertError> {
        match self {
            ResourceSetConvertXwfd::Uri(uri) => {
                let mut uri_iter = uri.iter();
                nwr.unfold_as_vec(|| uri_iter.next())?;
            },
            ResourceSetConvertXwfd::MultiUri(_) => unimplemented!(),
        }
        Ok(())
    }

    pub fn discriminant(&self) -> U3 {
        unsafe {
            match self {
                ResourceSetConvertXwfd::Uri(uri) => U3::new_unchecked(uri.discriminant() as u8),
                ResourceSetConvertXwfd::MultiUri(_) => U3::new_unchecked(6),
            }
        }
    }
}

impl<'i> From<xwfd::ResourceSet<'i>> for ResourceSet {
    fn from(resource_set: xwfd::ResourceSet<'i>) -> Self {
        match resource_set {
            xwfd::ResourceSet::Uri(uri) => ResourceSet::Uri(uri.into()),
            xwfd::ResourceSet::MultiUri(_) => unimplemented!(),
        }
    }
}

impl Display for ResourceSet {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ResourceSet::Uri(uri) => write!(f, "{}", uri),
            ResourceSet::MultiUri(multi_uri) => write!(f, "{}", multi_uri),
        }
    }
}
