//! Arguments that evolve (EVO-22): trailing arguments with a default are read as their default when a client of an
//! older version leaves them out, and a `#[flatten]` argument's struct is the args struct itself, so `configure(a, b)`
//! and `configure_flat(Config { a, b })` are the same bytes. Served with `no_alloc = true` and `false` (the same
//! answers), called through the generated client and the dynamic one.

#[cfg(test)]
mod tests {
    use args_evolution_api::{Config, OpenError, Seen};
    use std::path::Path;
    use std::sync::Arc;
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use wire_weaver_client::ww_numeric::NumericValue as N;
    use wire_weaver_client::ww_self::{ApiBundleOwned, FieldsValueOwned, ValueOwned};
    use wire_weaver_client::{ClientConfig, DynClient, DynResource};

    fn sum(cfg: Config) -> u32 {
        cfg.a as u32 + cfg.b as u32
    }

    mod no_alloc_server {
        use super::*;
        use tests_common::TestProcessEvents;

        pub struct Server;

        impl Server {
            fn configure(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                a: u8,
                b: u16,
            ) -> RpcResult<u32> {
                Ready(sum(Config { a, b }))
            }

            fn configure_flat(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                cfg: Config,
            ) -> RpcResult<u32> {
                Ready(sum(cfg))
            }

            fn with_defaults(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                a: u8,
                b: u8,
                c: Option<u8>,
                tag: &'_ str,
            ) -> RpcResult<Seen> {
                Ready(Seen {
                    a,
                    b,
                    c,
                    tag_len: tag.len() as u8,
                })
            }

            fn sums(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                cfg: Config,
            ) -> RpcResult<Result<(), OpenError>> {
                Ready(if sum(cfg) == 0 {
                    Err(OpenError::Zero)
                } else {
                    Ok(())
                })
            }

            fn close_sums(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {}

            fn sideband_sums(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _sideband: ww_client_server::StreamSideband,
            ) -> Option<ww_client_server::StreamSideband> {
                None
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                args_evolution_api :: ArgsEvolution for super::Server,
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

    /// The same API with `no_alloc = false`: owned arguments, the flattened struct and the defaults all the same.
    mod alloc_server {
        use super::*;
        use tests_common::TestProcessEvents;

        pub struct Server;

        impl Server {
            fn configure(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                a: u8,
                b: u16,
            ) -> RpcResult<u32> {
                Ready(sum(Config { a, b }))
            }

            fn configure_flat(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                cfg: Config,
            ) -> RpcResult<u32> {
                Ready(sum(cfg))
            }

            fn with_defaults(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                a: u8,
                b: u8,
                c: Option<u8>,
                tag: String,
            ) -> RpcResult<Seen> {
                Ready(Seen {
                    a,
                    b,
                    c,
                    tag_len: tag.len() as u8,
                })
            }

            fn sums(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                cfg: Config,
            ) -> RpcResult<Result<(), OpenError>> {
                Ready(if sum(cfg) == 0 {
                    Err(OpenError::Zero)
                } else {
                    Ok(())
                })
            }

            fn close_sums(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {}

            fn sideband_sums(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _sideband: ww_client_server::StreamSideband,
            ) -> Option<ww_client_server::StreamSideband> {
                None
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                args_evolution_api :: ArgsEvolution for super::Server,
                server = true, no_alloc = false, use_async = false,
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

    mod std_client {
        use wire_weaver_client::{ClientConfig, Commander, WwClient};

        pub struct StdClient {
            pub cmd: Commander,
        }

        impl WwClient for StdClient {
            fn default_config() -> ClientConfig {
                ClientConfig::default()
            }

            fn from_cmd(cmd: Commander) -> Self {
                Self { cmd }
            }
        }

        mod api_client {
            wire_weaver::ww_codegen!(
                args_evolution_api :: ArgsEvolution for super::StdClient,
                client = "std_client",
            );
        }
    }

    use std_client::StdClient;

    fn start(path: &str) -> TestDevice {
        assert_eq!(
            format!("{:?}", alloc_server::api_impl::api_hash()),
            format!("{:?}", no_alloc_server::api_impl::api_hash())
        );
        // both servers answer every request, their replies must be the same bytes
        let server = tests_common::Twin {
            a: alloc_server::Server,
            b: no_alloc_server::Server,
        };
        tests_common::start_device(
            path,
            server,
            args_evolution_api::ARGS_EVOLUTION_FULL_GID,
            args_evolution_api::ARGS_EVOLUTION_WIRE_VERSION,
            alloc_server::api_impl::api_hash(),
        )
    }

    fn bundle() -> Arc<ApiBundleOwned> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../args_evolution_api");
        Arc::new(wire_weaver_core::load(&path, Some("ArgsEvolution".into()), false).unwrap())
    }

    fn u8v(v: u8) -> ValueOwned {
        ValueOwned::Numeric(N::U8(v))
    }

    fn seen(value: Option<ValueOwned>) -> Vec<(String, ValueOwned)> {
        let Some(ValueOwned::Struct {
            fields: FieldsValueOwned::Named(fields),
        }) = value
        else {
            panic!("expected Seen, got {value:?}");
        };
        fields
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn flattened_struct_and_defaults_through_the_generated_client() {
        let device = start("args_evolution/generated");
        let client = StdClient::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect");

        assert_eq!(client.configure(1, 300).call().await.unwrap(), 301);
        let cfg = Config { a: 2, b: 400 };
        assert_eq!(client.configure_flat(cfg).call().await.unwrap(), 402);

        let r = client
            .with_defaults(1, 2, Some(3), "four".into())
            .call()
            .await
            .unwrap();
        assert_eq!(
            r,
            Seen {
                a: 1,
                b: 2,
                c: Some(3),
                tag_len: 4
            }
        );

        assert!(client.sums(cfg).call().await.unwrap().is_ok());
        let zero = Config { a: 0, b: 0 };
        assert_eq!(
            client.sums(zero).call().await.unwrap().err(),
            Some(OpenError::Zero)
        );
    }

    /// A client of an older version sends fewer arguments: the device reads the defaults of the missing ones.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn left_out_arguments_are_read_as_their_defaults() {
        let device = start("args_evolution/defaults");
        let client = DynClient::from_config(ClientConfig::new().in_process_path(device.path()))
            .connect()
            .await
            .expect("connect");
        let cmd = client.cmd();
        let root = DynResource::root(bundle());
        let with_defaults = root.child("with_defaults").unwrap();

        let r = with_defaults.call(cmd, &[u8v(5)]).await.unwrap();
        let expected = |b: u8, c: Option<u8>| {
            vec![
                ("a".to_string(), u8v(5)),
                ("b".to_string(), u8v(b)),
                (
                    "c".to_string(),
                    ValueOwned::Option(c.map(|c| Box::new(u8v(c)))),
                ),
                ("tag_len".to_string(), u8v(0)),
            ]
        };
        assert_eq!(seen(r), expected(7, None));
        let r = with_defaults.call(cmd, &[u8v(5), u8v(9)]).await.unwrap();
        assert_eq!(seen(r), expected(9, None));
        let r = with_defaults
            .call(
                cmd,
                &[u8v(5), u8v(9), ValueOwned::Option(Some(Box::new(u8v(1))))],
            )
            .await
            .unwrap();
        assert_eq!(seen(r), expected(9, Some(1)));

        // `a` has no default
        let e = with_defaults.call(cmd, &[]).await.unwrap_err().to_string();
        assert!(
            e.contains("takes 1 to 4 arguments, got 0: `a` has no default"),
            "{e}"
        );
    }

    /// A dynamic client sees a `#[flatten]` argument as its struct's fields: the same call as before the arguments
    /// became a struct.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dynamic_client_calls_a_flattened_method_with_the_fields() {
        let device = start("args_evolution/dynamic_flatten");
        let client = DynClient::from_config(ClientConfig::new().in_process_path(device.path()))
            .connect()
            .await
            .expect("connect");
        let cmd = client.cmd();
        let root = DynResource::root(bundle());
        let args = [u8v(3), ValueOwned::Numeric(N::U16(500))];
        for method in ["configure", "configure_flat"] {
            let r = root.child(method).unwrap().call(cmd, &args).await.unwrap();
            assert_eq!(r, Some(ValueOwned::Numeric(N::U32(503))), "{method}");
        }
        let bytes = |method: &str| {
            let root = DynResource::root(bundle());
            let res = root.child(method).unwrap();
            let Ok(wire_weaver_client::DynResourceKind::Method { args: defs, .. }) = res.kind()
            else {
                panic!("method");
            };
            ValueOwned::ser_args_dyn(&args, defs, res.bundle()).unwrap()
        };
        assert_eq!(bytes("configure"), bytes("configure_flat"));
        assert_eq!(bytes("configure"), [3, 0xF4, 0x01]);
    }
}
