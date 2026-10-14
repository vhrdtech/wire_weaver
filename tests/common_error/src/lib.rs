//! `#[common] Common(CommonError)` errors (STD-16) through a generated server, the generated client and the dynamic
//! client. `wire_weaver_py/tests/test_offline.py` decodes the same bytes.

#[cfg(test)]
mod tests {
    use common_error_api::SensorErrorOwned;
    use std::path::Path;
    use std::sync::Arc;
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use wire_weaver_client::ww_self::{ApiBundleOwned, FieldsValueOwned, TypeOwned, ValueOwned};
    use wire_weaver_client::{ClientConfig, Commander, DynClient, DynResource, WwClient};

    mod server {
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

        impl Server {
            fn read(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                fail: u8,
            ) -> RpcResult<Result<u16, SensorError<'_>>> {
                Ready(match fail {
                    0 => Ok(1234),
                    1 => claim().map(|_| 0).map_err(Into::into),
                    2 => Err(CommonError::Fault { code: 0x2a }.into()),
                    _ => Err(SensorError::Nack),
                })
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
    }

    pub struct Client {
        cmd: Commander,
    }

    impl WwClient for Client {
        fn default_config() -> ClientConfig {
            ClientConfig::default()
        }

        fn from_cmd(cmd: Commander) -> Self {
            Self { cmd }
        }
    }

    mod api_client {
        wire_weaver::ww_codegen!(
            common_error_api :: CommonErrors for super::Client,
            client = "std_client",
        );
    }

    fn start(path: &str) -> TestDevice {
        tests_common::start_device(
            path,
            server::Server,
            common_error_api::COMMON_ERRORS_FULL_GID,
            server::api_impl::api_hash(),
        )
    }

    fn in_use() -> CommonErrorOwned {
        CommonErrorOwned::InUse {
            by: MaxLen::new_unchecked("USART2".into()),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn generated_client() {
        let device = start("common_error/generated");
        let mut client = Client::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect");

        assert_eq!(client.read(0).call().await.unwrap(), Ok(1234));
        let e = client.read(1).call().await.unwrap().unwrap_err();
        assert_eq!(e, SensorErrorOwned::Common(in_use()));
        assert_eq!(e.common(), Some(&in_use()));
        assert_eq!(e.common().unwrap().to_string(), "In use by USART2");
        let e = client.read(2).call().await.unwrap().unwrap_err();
        assert_eq!(e.common(), Some(&CommonErrorOwned::Fault { code: 0x2a }));
        let e = client.read(3).call().await.unwrap().unwrap_err();
        assert_eq!(e, SensorErrorOwned::Nack);
        assert_eq!(e.common(), None);

        let e = client.calibrate().call().await.unwrap().unwrap_err();
        assert_eq!(e, CommonErrorOwned::Busy);
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

    /// `Err(Common(InUse { by: "USART2" }))` as the dynamic client sees it.
    fn in_use_value() -> ValueOwned {
        let in_use = variant(
            "InUse",
            FieldsValueOwned::Named(vec![("by".into(), ValueOwned::String("USART2".into()))]),
        );
        ValueOwned::Result(Err(Box::new(variant(
            "Common",
            FieldsValueOwned::Unnamed(vec![in_use]),
        ))))
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dynamic_client() {
        let device = start("common_error/dynamic");
        let client = DynClient::from_config(ClientConfig::new().in_process_path(device.path()))
            .connect()
            .await
            .expect("connect");
        let cmd = client.cmd();
        let root = DynResource::root(bundle());
        let read = root.child("read").unwrap();
        let fail = |n: u8| ValueOwned::Numeric(wire_weaver_client::ww_numeric::NumericValue::U8(n));

        let r = read.call(cmd, &[fail(1)]).await.unwrap();
        assert_eq!(r, Some(in_use_value()));
        let r = root
            .child("calibrate")
            .unwrap()
            .call(cmd, &[])
            .await
            .unwrap();
        assert_eq!(
            r,
            Some(ValueOwned::Result(Err(Box::new(variant(
                "Busy",
                FieldsValueOwned::Unit
            )))))
        );
    }

    /// The reply bytes `wire_weaver_py/tests/test_offline.py` decodes (`READ_IN_USE_HEX`, ..).
    #[test]
    fn reply_bytes_for_python() {
        let value: Result<u16, SensorErrorOwned> = Err(SensorErrorOwned::Common(in_use()));
        let bytes = value.to_ww_bytes_owned().unwrap();
        assert_eq!(hex(&bytes), "000020555341525432060929");
        // `NACK_HEX` and `BUSY_HEX` there
        let nack: Result<u16, SensorErrorOwned> = Err(SensorErrorOwned::Nack);
        assert_eq!(hex(&nack.to_ww_bytes_owned().unwrap()), "001001");
        let busy: Result<(), CommonErrorOwned> = Err(CommonErrorOwned::Busy);
        assert_eq!(hex(&busy.to_ww_bytes_owned().unwrap()), "003001");
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
        assert_eq!(value, in_use_value());

        // tools find the common error by its type, and its message from the variant's doc line
        let bundle = root.bundle();
        let TypeOwned::Result { err_ty, .. } = ty.get_in_line(bundle).unwrap() else {
            panic!("Result expected");
        };
        let ValueOwned::Result(Err(err)) = &value else {
            panic!("Err expected");
        };
        let (common, common_ty) = err.common_error(err_ty, bundle).unwrap();
        assert_eq!(
            common.doc_message(common_ty, bundle).as_deref(),
            Some("In use by USART2")
        );
        let nack = variant("Nack", FieldsValueOwned::Unit);
        assert_eq!(nack.common_error(err_ty, bundle), None);
        assert_eq!(
            nack.doc_message(err_ty, bundle).as_deref(),
            Some("The sensor did not answer")
        );
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}
