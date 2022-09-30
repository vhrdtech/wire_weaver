use crate::owned::convert_error::ConvertError;
use crate::owned::{SerialMultiUri, SerialUri};
use crate::resource_set::XpiGenericResourceSet;
use crate::xwfd;
use vhl_stdlib_nostd::serdes::{BitBufMut, NibbleBufMut};

pub type ResourceSet = XpiGenericResourceSet<SerialUri, SerialMultiUri>;

impl ResourceSet {
    pub(crate) fn ser_header_xwfd(
        &self,
        bwr: &mut BitBufMut,
    ) -> Result<Option<xwfd::SerialUriDiscriminant>, ConvertError> {
        match &self {
            XpiGenericResourceSet::Uri(uri) => {
                uri.ser_header_xwfd(bwr).map(|uri_kind| Some(uri_kind))
            }
            XpiGenericResourceSet::MultiUri(_multi_uri) => todo!(),
        }
    }

    pub(crate) fn ser_body_xwfd(
        &self,
        nwr: &mut NibbleBufMut,
        uri_kind: Option<xwfd::SerialUriDiscriminant>,
    ) -> Result<(), ConvertError> {
        match &self {
            ResourceSet::Uri(uri) => uri.ser_body_xwfd(nwr, uri_kind.expect("Internal error in ser_xwfd()"))?,
            ResourceSet::MultiUri(_) => unimplemented!()
        }
        Ok(())
    }
}
