use std::vec::IntoIter;
use crate::owned::convert_error::ConvertError;
use crate::owned::{SerialMultiUri, SerialUri};
use crate::resource_set::XpiGenericResourceSet;
use crate::xwfd;
use vhl_stdlib_nostd::serdes::{BitBufMut, NibbleBufMut};
use vhl_stdlib_nostd::serdes::vlu4::Vlu32;

pub type ResourceSet = XpiGenericResourceSet<SerialUri, SerialMultiUri>;

pub(crate) type ResourceSetConvertXwfd = XpiGenericResourceSet<xwfd::SerialUri<IntoIter<Vlu32>>, SerialMultiUri>;

impl ResourceSet {
    pub(crate) fn ser_header_xwfd(
        &self,
        bwr: &mut BitBufMut,
    ) -> Result<ResourceSetConvertXwfd, ConvertError> {
        match &self {
            XpiGenericResourceSet::Uri(uri) => {
                Ok(ResourceSetConvertXwfd::Uri(uri.ser_header_xwfd(bwr)?))
            }
            XpiGenericResourceSet::MultiUri(_multi_uri) => todo!(),
        }
    }
}

impl ResourceSetConvertXwfd {
    pub(crate) fn ser_body_xwfd(
        &self,
        nwr: &mut NibbleBufMut,
    ) -> Result<(), ConvertError> {
        match self {
            ResourceSetConvertXwfd::Uri(uri) => nwr.put(uri)?,
            ResourceSetConvertXwfd::MultiUri(_) => unimplemented!()
        }
        Ok(())
    }
}
