//! Common errors (STD-16) through a generated server, the generated client and the dynamic client: a method with
//! an error type of its own returns `Result<T, wire_weaver::Error<'_, E>>`, one without returns
//! `Result<T, CommonError>`, and the client sees `CallError::Common` or `Error::common()`. The same server is
//! `inproc:errors-device` of `tests/hub_pytest_device`, for `wire_weaver_py`'s `test_errors.py`.

/// The server: `read(fail)` picks its error, `calibrate()` is always `Busy`.
pub mod server {
    use common_error_api::SensorError;
    use tests_common::TestProcessEvents;
    use wire_weaver::prelude::*;

    pub struct Server;

    /// A helper shared by handlers: returns `CommonError`, `?` turns it into any API's error.
    fn claim() -> Result<(), CommonError<'static>> {
        Err(CommonError::InUse {
            by: MaxLen::new_unchecked("USART2"),
        })
    }

    /// `?` takes a common error and the API's own alike.
    fn read(fail: u8) -> Result<u16, wire_weaver::Error<'static, SensorError>> {
        match fail {
            0 => Ok(1234),
            1 => {
                claim()?;
                Ok(0)
            }
            2 => Err(CommonError::Fault { code: 0x2a })?,
            _ => Err(SensorError::Nack)?,
        }
    }

    impl Server {
        fn read(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
            fail: u8,
        ) -> RpcResult<Result<u16, wire_weaver::Error<'_, SensorError>>> {
            Ready(read(fail))
        }

        fn calibrate(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
        ) -> RpcResult<Result<(), CommonError<'_>>> {
            Ready(Err(CommonError::Busy))
        }
    }

    pub mod api_impl {
        wire_weaver::ww_codegen!(
            common_error_api :: CommonErrors for super::Server,
            server = true, no_alloc = true, use_async = false,
            method_model = "_=immediate",
            property_model = "_=get_set",
        );
    }

    impl TestProcessEvents for Server {
        type Medium = ();

        fn process_request_bytes<'a>(
            &mut self,
            bytes: &[u8],
            scratch: &'a mut [u8],
            out: &mut impl BlockingEventOut,
            medium: (),
        ) -> Result<&'a [u8], ShrinkWrapError> {
            self.process_request_bytes(bytes, scratch, out, medium)
        }
    }

    /// The server on `path` through the real host event loop (`tests_common::start_device`).
    pub fn start(path: &str) -> tests_common::TestDevice {
        tests_common::start_device(
            path,
            Server,
            common_error_api::COMMON_ERRORS_FULL_GID,
            common_error_api::COMMON_ERRORS_WIRE_VERSION,
            tests_common::api!(api_impl),
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::server::start;
    use common_error_api::SensorError;
    use std::path::Path;
    use std::sync::Arc;
    use wire_weaver::prelude::*;
    use wire_weaver_client::ww_self::{ApiBundleOwned, FieldsValueOwned, TypeOwned, ValueOwned};
    use wire_weaver_client::{
        CallError, ClientConfig, Commander, DynClient, DynResource, WwClient,
    };

    pub struct Client {
        cmd: Commander,
    }

    impl WwClient for Client {
        fn default_config() -> ClientConfig {
            ClientConfig::default()
        }

        fn try_from_cmd(cmd: Commander) -> Result<Self, wire_weaver_client::Error> {
            Ok(Self { cmd })
        }
    }

    mod api_client {
        wire_weaver::ww_codegen!(
            common_error_api :: CommonErrors for super::Client,
            client = "std_client",
        );
    }

    fn in_use() -> CommonErrorOwned {
        CommonErrorOwned::InUse {
            by: MaxLen::new_unchecked("USART2".into()),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn generated_client() {
        let device = start("common_error/generated");
        let client = Client::config(|c| {
            c.in_process_path(device.path())
                .resolver(tests_common::resolver())
        })
        .connect()
        .await
        .expect("connect");

        assert_eq!(client.read(0).call().await.unwrap(), 1234);
        // a common error of a method with an error type of its own: `CallError::Common`, not `Device`
        let e = client.read(1).call().await.unwrap_err();
        assert!(matches!(e, _), "{e:?}");
        assert_eq!(e.common(), Some(&in_use()));
        assert_eq!(e.common().unwrap().to_string(), "In use by USART2");
        let e = client.read(2).call().await.unwrap_err();
        assert_eq!(e.common(), Some(&CommonErrorOwned::Fault { code: 0x2a }));
        // the API's own error, as before
        let e = client.read(3).call().await.unwrap_err();
        assert!(matches!(e, CallError::Device(SensorError::Nack)), "{e:?}");
        assert_eq!(e.common(), None);

        // a method without an error type: the common error is the call's error
        let e = client.calibrate().call().await.unwrap_err();
        assert_eq!(e.common(), Some(&CommonErrorOwned::Busy));
        assert_eq!(
            e.common().unwrap().to_string(),
            "Busy with another operation, try again later"
        );
    }

    fn bundle() -> Arc<ApiBundleOwned> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../common_error_api");
        Arc::new(wire_weaver_core::load(&path, Some("CommonErrors".into()), false).unwrap())
    }

    fn variant(name: &str, fields: FieldsValueOwned) -> ValueOwned {
        ValueOwned::Enum {
            variant: name.into(),
            fields,
        }
    }

    /// `Err(Nack)` as the dynamic client sees it.
    fn nack_value() -> ValueOwned {
        ValueOwned::Result(Err(Box::new(variant("Nack", FieldsValueOwned::Unit))))
    }

    /// The dynamic client reads a common error from the protocol, whatever the item's own error type (STD-16).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dynamic_client() {
        let device = start("common_error/dynamic");
        let client = DynClient::from_config(
            ClientConfig::new()
                .in_process_path(device.path())
                .resolver(tests_common::resolver()),
        )
        .connect()
        .await
        .expect("connect");
        let cmd = client.cmd();
        let root = DynResource::root(bundle());
        let read = root.child("read").unwrap();
        let fail = |n: u8| ValueOwned::Numeric(wire_weaver_client::ww_numeric::NumericValue::U8(n));

        let e = read.call(cmd, &[fail(1)]).await.unwrap_err();
        assert_eq!(e.common(), Some(&in_use()));
        let r = read.call(cmd, &[fail(3)]).await.unwrap();
        assert_eq!(r, Some(nack_value()));
        let e = root
            .child("calibrate")
            .unwrap()
            .call(cmd, &[])
            .await
            .unwrap_err();
        assert_eq!(e.common(), Some(&CommonErrorOwned::Busy));
    }

    /// The reply bytes `wire_weaver_py/tests/test_offline.py` decodes (`NACK_HEX`): an error of the API's own is
    /// still the method's `Result`; a common error is an error event of the protocol (the conformance case
    /// `client_server.event.common_error`), with its message from `CommonError::DOCS`.
    #[test]
    fn reply_bytes_for_python() {
        let nack: Result<u16, SensorError> = Err(SensorError::Nack);
        let bytes = nack.to_ww_bytes_owned().unwrap();
        assert_eq!(hex(&bytes), "000001");
        // the dynamic client reads them the same way
        let root = DynResource::root(bundle());
        let read = root.child("read").unwrap();
        let wire_weaver_client::DynResourceKind::Method {
            return_ty: Some(ty),
            ..
        } = read.kind().unwrap()
        else {
            panic!("method with a return type expected");
        };
        let value = ValueOwned::des_shrink_wrap_dyn(&bytes, ty, root.bundle()).unwrap();
        assert_eq!(value, nack_value());

        // tools take an error's message from its variant's doc line, a common error's from `CommonError::DOCS`
        let bundle = root.bundle();
        let TypeOwned::Result { err_ty, .. } = ty.get_in_line(bundle).unwrap() else {
            panic!("Result expected");
        };
        let ValueOwned::Result(Err(err)) = &value else {
            panic!("Err expected");
        };
        assert_eq!(
            err.doc_message(err_ty, bundle).as_deref(),
            Some("The sensor did not answer")
        );
        assert_eq!(
            wire_weaver_client::ww_self::errors::common_error_doc("InUse"),
            Some("In use by {by}")
        );
        assert_eq!(in_use().to_string(), "In use by USART2");
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}
