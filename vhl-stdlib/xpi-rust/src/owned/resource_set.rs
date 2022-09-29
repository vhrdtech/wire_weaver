use vhl_stdlib_nostd::serdes::BitBufMut;
use crate::owned::{SerialMultiUri, SerialUri};
use crate::owned::convert_error::ConvertError;
use crate::resource_set::XpiGenericResourceSet;

pub type ResourceSet = XpiGenericResourceSet<SerialUri, SerialMultiUri>;

impl ResourceSet {
    pub(crate) fn ser_header_xwfd(&self, bwr: &mut BitBufMut) -> Result<(), ConvertError> {
        match &self {
            XpiGenericResourceSet::Uri(uri) => {}
            XpiGenericResourceSet::MultiUri(multi_uri) => {}
        }
        Ok(())
    }
}
