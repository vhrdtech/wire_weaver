use vhl_stdlib_nostd::serdes::BitBufMut;
use crate::owned::{SerialMultiUri, SerialUri};
use crate::owned::convert_error::ConvertError;
use crate::resource_set::XpiGenericResourceSet;
use crate::xwfd;

pub type ResourceSet = XpiGenericResourceSet<SerialUri, SerialMultiUri>;

impl ResourceSet {
    pub(crate) fn ser_header_xwfd(&self, bwr: &mut BitBufMut) -> Result<Option<xwfd::SerialUriDiscriminant>, ConvertError> {
        match &self {
            XpiGenericResourceSet::Uri(uri) => uri.ser_header_xwfd(bwr).map(|uri_kind| Some(uri_kind)),
            XpiGenericResourceSet::MultiUri(_multi_uri) => todo!(),
        }
    }
}
