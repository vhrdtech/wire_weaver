use crate::ast::api::ApiLevel;
use crate::codegen::util::ErrorSeq;
use proc_macro2::TokenStream;
use quote::quote;

pub(crate) fn introspect(
    api_level: &ApiLevel,
    enabled: bool,
    use_async: bool,
    error_seq: &mut ErrorSeq,
) -> (TokenStream, TokenStream) {
    let (ww_self_bytes_const, api_signature) = crate::codegen::introspect::introspect(api_level);
    let api_signature = quote! { pub const WW_API_SIGNATURE: #api_signature; };
    if !use_async {
        // TODO: sync variant of MessageSink
        return (quote! {}, api_signature);
    }
    let es0 = error_seq.next_err();
    let es1 = error_seq.next_err();
    let handle_introspect = if enabled {
        quote! {
            RequestKind::Introspect => {
                pub const WW_SELF_BYTES: #ww_self_bytes_const;
                for chunk in WW_SELF_BYTES.chunks(128).chain([&[][..]]) { // TODO: auto-determine better chunk size
                    let event = Event {
                        seq: request.seq,
                        result: Ok(EventKind::Introspect { ww_self_bytes_chunk: RefVec::new_bytes(chunk) }),
                    };
                    let event_bytes = event.to_ww_bytes(scratch_event).map_err(|_| Error::new(#es0, ErrorKind::ResponseSerFailed))?;
                    msg_tx.send(event_bytes).await.map_err(|_| Error::new(#es1, ErrorKind::ResponseSerFailed))?;
                }
                Ok(&[])
            }
        }
    } else {
        quote! {
            RequestKind::Introspect => {
                let event = Event {
                    seq: request.seq,
                    result: Ok(EventKind::Introspect { ww_self_bytes_chunk: RefVec::new_bytes(&[]) }),
                };
                let event_bytes = event.to_ww_bytes(scratch_event).map_err(|_| Error::new(#es0, ErrorKind::ResponseSerFailed))?;
                msg_tx.send(event_bytes).await.map_err(|_| Error::new(#es1, ErrorKind::ResponseSerFailed))?;
                Ok(&[])
            }
        }
    };
    (handle_introspect, api_signature)
}
