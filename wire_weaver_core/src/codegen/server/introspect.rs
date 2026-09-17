use crate::codegen::util::ErrorSeq;
use proc_macro2::TokenStream;
use quote::quote;
use sha2::Digest;
use shrink_wrap::SerializeShrinkWrapOwned;
use ww_self::ApiBundleOwned;

pub(crate) fn introspect(
    api_bundle: &ApiBundleOwned,
    enabled: bool,
    include_docs: bool,
    use_async: bool,
    error_seq: &mut ErrorSeq,
) -> (TokenStream, TokenStream) {
    let IntrospectTs {
        introspect_bytes,
        no_docs_hash,
        with_docs_hash,
    } = introspect_prepare(api_bundle, include_docs);
    let api_hash = quote! {
        pub fn api_hash() -> wire_weaver::ww_version::ApiHashPair<'static> {
            pub const WW_API_HASH_NO_DOCS: #no_docs_hash;
            pub const WW_API_HASH_WITH_DOCS: #with_docs_hash;
            use wire_weaver::ww_version::ApiHash;
            wire_weaver::ww_version::ApiHashPair {
                no_docs: ApiHash::new(&WW_API_HASH_NO_DOCS),
                with_docs: ApiHash::new(&WW_API_HASH_WITH_DOCS),
            }
        }
    };
    if !use_async {
        // TODO: sync variant of MessageSink
        return (quote! {}, api_hash);
    }
    let es0 = error_seq.next();
    let es1 = error_seq.next();
    let handle_introspect = if enabled {
        quote! {
            RequestKind::Introspect => {
                pub const WW_SELF_BYTES: #introspect_bytes;
                for chunk in WW_SELF_BYTES.chunks(128) { // TODO: auto-determine better chunk size
                    let event = Event {
                        seq: request.seq,
                        result: Ok(EventKind::StreamData { path: RefVec::Slice { slice: &[] }, data: TailBytes(chunk) }),
                    };
                    wr.reset();
                    event.ser_shrink_wrap(wr).map_err(|_| Error::new(#es0, ErrorKind::ResponseSerFailed))?;
                    let event_bytes = wr.finish().map_err(|_| Error::new(#es0, ErrorKind::ResponseSerFailed))?;
                    msg_tx.send(event_bytes).await.map_err(|_| Error::new(#es1, ErrorKind::ResponseSerFailed))?;
                }
                let event = Event {
                    seq: request.seq,
                    result: Ok(EventKind::StreamSideband { path: RefVec::Slice { slice: &[] }, sideband: ww_client_server::StreamSideband::Close }),
                };
                wr.reset();
                event.ser_shrink_wrap(wr).map_err(|_| Error::new(#es0, ErrorKind::ResponseSerFailed))?;
                let event_bytes = wr.finish().map_err(|_| Error::new(#es0, ErrorKind::ResponseSerFailed))?;
                msg_tx.send(event_bytes).await.map_err(|_| Error::new(#es1, ErrorKind::ResponseSerFailed))?;
                Ok(WrAction::Deferred)
            }
        }
    } else {
        quote! {
            RequestKind::Introspect => {
                let event = Event {
                    seq: request.seq,
                    result: Ok(EventKind::StreamSideband { path: RefVec::Slice { slice: &[] }, sideband: ww_client_server::StreamSideband::Close }),
                };
                wr.reset();
                event.ser_shrink_wrap(wr).map_err(|_| Error::new(#es0, ErrorKind::ResponseSerFailed))?;
                let event_bytes = wr.finish().map_err(|_| Error::new(#es0, ErrorKind::ResponseSerFailed))?;
                msg_tx.send(event_bytes).await.map_err(|_| Error::new(#es1, ErrorKind::ResponseSerFailed))?;
                Ok(WrAction::Deferred)
            }
        }
    };
    (handle_introspect, api_hash)
}

pub(crate) struct IntrospectTs {
    pub(crate) introspect_bytes: TokenStream,
    pub(crate) no_docs_hash: TokenStream,
    pub(crate) with_docs_hash: TokenStream,
}

pub(crate) fn introspect_prepare(api_bundle: &ApiBundleOwned, include_docs: bool) -> IntrospectTs {
    let mut api_bundle_cloned = api_bundle.clone();
    let mut contains_docs = ContainsDocs::default();
    ww_self::visitor::visit_api_bundle_mut(&mut api_bundle_cloned, &mut contains_docs);

    if contains_docs.contains {
        ww_self::visitor::visit_api_bundle_mut(&mut api_bundle_cloned, &mut DropDocs {});
        let api_no_docs = api_bundle_cloned;
        let api_with_docs = api_bundle;

        let (no_docs_bytes, no_docs_hash) = ser_hash_and_cache(&api_no_docs, false);
        let (with_docs_bytes, with_docs_hash) = ser_hash_and_cache(api_with_docs, true);

        let introspect_bytes = if include_docs {
            bytes_to_ts(&with_docs_bytes)
        } else {
            bytes_to_ts(&no_docs_bytes)
        };
        IntrospectTs {
            introspect_bytes,
            no_docs_hash,
            with_docs_hash,
        }
    } else {
        let api_no_docs = api_bundle;
        let (no_docs_bytes, no_docs_hash) = ser_hash_and_cache(api_no_docs, false);
        IntrospectTs {
            introspect_bytes: bytes_to_ts(&no_docs_bytes),
            no_docs_hash,
            with_docs_hash: bytes_to_ts(&[]),
        }
    }
}

fn ser_hash_and_cache(api_bundle: &ApiBundleOwned, contains_docs: bool) -> (Vec<u8>, TokenStream) {
    let api_bytes = api_bundle.to_ww_bytes_owned().unwrap();
    // TODO: calculate api signature properly?
    let hash = sha2::Sha256::digest(&api_bytes);
    let hash = &hash[..8];
    crate::local_registry::cache_api_bundle(api_bundle, contains_docs, hash);
    (api_bytes, bytes_to_ts(hash))
}

fn bytes_to_ts(bytes: &[u8]) -> TokenStream {
    let bytes_len = bytes.len();
    quote! {
        [u8; #bytes_len] = [ #(#bytes),* ]
    }
}

struct DropDocs {}

impl ww_self::visitor::VisitMut for DropDocs {
    fn visit_doc(&mut self, docs: &mut Vec<String>) {
        docs.clear();
    }
}

#[derive(Default)]
struct ContainsDocs {
    contains: bool,
}

impl ww_self::visitor::VisitMut for ContainsDocs {
    fn visit_doc(&mut self, docs: &mut Vec<String>) {
        if !docs.is_empty() {
            self.contains = true;
        }
    }
}
