//! Self-referential user types in an API (`RefBox`, `RefVec` of the type itself), through generated server and
//! client code: method arguments and return values, a property and stream items; their definitions in
//! introspection data (each once, referring to its own index), and the dynamic client, which decodes and writes the
//! same bytes as the derived types. Values nested deeper than a reader allows are an error on both sides.

#[cfg(test)]
mod tests {
    use recursive_api::{ExprOwned, TreeOwned};
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use wire_weaver_client::ww_numeric::NumericValue;
    use wire_weaver_client::ww_self::{
        ApiBundleOwned, ApiItemKindOwned, FieldsValueOwned, TypeLocationOwned, TypeOwned,
        ValueOwned,
    };
    use wire_weaver_client::{ClientConfig, DynClient, DynResource};

    #[derive(Default)]
    struct SharedTestData {
        /// The tree property, serialized.
        tree: Option<Vec<u8>>,
    }

    mod server {
        use super::*;
        use recursive_api::{Expr, Tree};
        use tests_common::TestProcessEvents;
        use wire_weaver::{GetResult, SetResult};

        pub struct Server {
            pub data: Arc<Mutex<SharedTestData>>,
            /// Bytes of the last expression `unwrap_neg` got, the one it returns is read from them.
            pub last_expr: Vec<u8>,
            /// Bytes of the tree `get_tree` returns.
            pub tree: Vec<u8>,
        }

        /// `None` when a boxed expression can't be read (nested too deep).
        pub fn eval(expr: &Expr<'_>) -> Option<i32> {
            Some(match expr {
                Expr::Add(a, b) => eval(&a.read().ok()?)?.wrapping_add(eval(&b.read().ok()?)?),
                Expr::Mul { a, b } => eval(&a.read().ok()?)?.wrapping_mul(eval(&b.read().ok()?)?),
                Expr::Neg(e) => eval(&e.read().ok()?)?.wrapping_neg(),
                Expr::Lit(v) => *v,
                Expr::Unknown(_) => return None,
            })
        }

        impl Server {
            fn eval(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                expr: Expr<'_>,
            ) -> RpcResult<Option<i32>> {
                Ready(eval(&expr))
            }

            fn unwrap_neg(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                expr: Expr<'_>,
            ) -> RpcResult<Option<Expr<'_>>> {
                let mut buf = [0u8; 512];
                let Ok(bytes) = expr.to_ww_bytes(&mut buf) else {
                    return Ready(None);
                };
                self.last_expr = bytes.to_vec();
                Ready(match Expr::from_ww_bytes(&self.last_expr) {
                    Ok(Expr::Neg(inner)) => inner.read().ok(),
                    Ok(other) => Some(other),
                    Err(_) => None,
                })
            }

            fn set_tree(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: Tree<'_>,
            ) -> SetResult<()> {
                let mut buf = [0u8; 512];
                let bytes = value.to_ww_bytes(&mut buf).unwrap();
                self.data.lock().unwrap().tree = Some(bytes.to_vec());
                SetResult::Set
            }

            fn get_tree(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Tree<'_>, ()> {
                let Some(tree) = self.data.lock().unwrap().tree.clone() else {
                    return GetResult::GetError(());
                };
                self.tree = tree;
                match Tree::from_ww_bytes(&self.tree) {
                    Ok(tree) => GetResult::Value(tree),
                    Err(_) => GetResult::GetError(()),
                }
            }

            fn exprs(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
                Ready(())
            }

            fn close_exprs(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {}

            fn sideband_exprs(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _sideband: ww_client_server::StreamSideband,
            ) -> Option<ww_client_server::StreamSideband> {
                None
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                recursive_api :: Recursive for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                introspect = "no_docs",
                // debug_to_file = "../../target/tests_recursive_server.rs"
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
                recursive_api :: Recursive for super::StdClient,
                client = "std_client",
                // debug_to_file = "../../target/tests_recursive_client.rs"
            );
        }
    }

    use server::api_impl;
    use std_client::StdClient;

    fn start(path: &str) -> (TestDevice, Arc<Mutex<SharedTestData>>) {
        let _ = tracing_subscriber::fmt::try_init();
        let data = Arc::new(Mutex::new(SharedTestData::default()));
        let server = server::Server {
            data: data.clone(),
            last_expr: vec![],
            tree: vec![],
        };
        let device = tests_common::start_device(
            path,
            server,
            recursive_api::RECURSIVE_FULL_GID,
            api_impl::api_hash(),
        );
        (device, data)
    }

    async fn connect(device: &TestDevice) -> StdClient {
        StdClient::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect")
    }

    fn lit(v: i32) -> ExprOwned {
        ExprOwned::Lit(v)
    }

    /// `(1 + 2) * -3`
    fn expr() -> ExprOwned {
        ExprOwned::Mul {
            a: Box::new(ExprOwned::Add(Box::new(lit(1)), Box::new(lit(2)))),
            b: Box::new(ExprOwned::Neg(Box::new(lit(3)))),
        }
    }

    /// `--..-7`, `depth` negations
    fn negations(depth: usize) -> ExprOwned {
        let mut e = lit(7);
        for _ in 0..depth {
            e = ExprOwned::Neg(Box::new(e));
        }
        e
    }

    fn tree() -> TreeOwned {
        let leaf = |label: &str| TreeOwned {
            label: label.into(),
            children: vec![],
        };
        TreeOwned {
            label: "root".into(),
            children: vec![
                leaf("a"),
                TreeOwned {
                    label: "b".into(),
                    children: vec![leaf("b1"), leaf("b2")],
                },
            ],
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn arguments_and_return_values() {
        let (device, _data) = start("recursive/methods");
        let client = connect(&device).await;
        assert_eq!(client.eval(expr()).call().await.unwrap(), Some(-9));
        assert_eq!(client.eval(negations(51)).call().await.unwrap(), Some(-7));
        let inner = ExprOwned::Add(Box::new(lit(1)), Box::new(lit(2)));
        let r = client
            .unwrap_neg(ExprOwned::Neg(Box::new(inner.clone())))
            .call()
            .await
            .unwrap();
        assert_eq!(r, Some(inner));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn too_deep_is_an_error_not_a_crash() {
        let (device, _data) = start("recursive/too_deep");
        let client = connect(&device).await;
        // deeper than the device's reader allows: reading the boxed expressions fails on the way down
        let depth = shrink_wrap::BufReader::DEFAULT_MAX_DEPTH as usize + 10;
        assert_eq!(client.eval(negations(depth)).call().await.unwrap(), None);
        // and it keeps working
        assert_eq!(client.eval(negations(2)).call().await.unwrap(), Some(7));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn property() {
        let (device, data) = start("recursive/property");
        let client = connect(&device).await;
        client.write_tree(tree()).write().await.unwrap();
        assert!(data.lock().unwrap().tree.is_some());
        assert_eq!(client.read_tree().read().await.unwrap(), tree());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stream() {
        let (device, _data) = start("recursive/stream");
        let client = connect(&device).await;
        let mut exprs = client.exprs().call().await.unwrap();
        let mut scratch = [0u8; 128];
        let bytes = expr().to_ww_bytes_owned().unwrap();
        let borrowed = recursive_api::Expr::from_ww_bytes(&bytes).unwrap();
        device.send(
            api_impl::stream_data_ser()
                .exprs(&borrowed, &mut scratch)
                .unwrap(),
        );
        assert_eq!(exprs.recv().await.unwrap(), expr());
    }

    /// The API model as codegen sees it.
    fn bundle() -> ApiBundleOwned {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../recursive_api");
        wire_weaver_core::load(&path, Some("Recursive".into()), false).unwrap()
    }

    /// Index of the definition named `name`, and how many there are.
    fn def_idx(bundle: &ApiBundleOwned, name: &str) -> (u32, usize) {
        let found: Vec<_> = bundle
            .types
            .iter()
            .enumerate()
            .filter(|(_, location)| match location {
                TypeLocationOwned::InLine {
                    ty: TypeOwned::Struct(s),
                    ..
                } => s.ident == name,
                TypeLocationOwned::InLine {
                    ty: TypeOwned::Enum(e),
                    ..
                } => e.ident == name,
                _ => false,
            })
            .map(|(idx, _)| idx as u32)
            .collect();
        (found[0], found.len())
    }

    #[test]
    fn definitions_refer_to_themselves() {
        let mut bundle = bundle();
        let (expr, count) = def_idx(&bundle, "Expr");
        assert_eq!(count, 1);
        let ty = TypeOwned::OutOfLine {
            type_idx: expr.into(),
        };
        assert_eq!(
            ty.human_definition(&bundle, true).unwrap(),
            "enum ub4 recursive_api::Expr {Add(Box<recursive_api::Expr>, Box<recursive_api::Expr>), \
             Mul{a: Box<recursive_api::Expr>, b: Box<recursive_api::Expr>}, Neg(Box<recursive_api::Expr>), Lit(i32)}"
        );
        let (tree, count) = def_idx(&bundle, "Tree");
        assert_eq!(count, 1);
        let ty = TypeOwned::OutOfLine {
            type_idx: tree.into(),
        };
        assert_eq!(
            ty.human_definition(&bundle, true).unwrap(),
            "struct recursive_api::Tree {label: String, children: Vec<recursive_api::Tree>}"
        );
        wire_weaver_client::ww_self::generics::prepare(&mut bundle).unwrap();
        // the first variant that doesn't contain the type itself
        assert_eq!(
            ValueOwned::default(
                &TypeOwned::OutOfLine {
                    type_idx: expr.into()
                },
                &bundle
            )
            .unwrap(),
            lit_value(0)
        );
    }

    fn num(v: i32) -> ValueOwned {
        ValueOwned::Numeric(NumericValue::I32(v))
    }

    fn lit_value(v: i32) -> ValueOwned {
        ValueOwned::Enum {
            variant: "Lit".into(),
            fields: FieldsValueOwned::Unnamed(vec![num(v)]),
        }
    }

    fn unnamed(variant: &str, fields: Vec<ValueOwned>) -> ValueOwned {
        ValueOwned::Enum {
            variant: variant.into(),
            fields: FieldsValueOwned::Unnamed(fields),
        }
    }

    /// [expr] as a dynamic value
    fn expr_value() -> ValueOwned {
        ValueOwned::Enum {
            variant: "Mul".into(),
            fields: FieldsValueOwned::Named(vec![
                ("a".into(), unnamed("Add", vec![lit_value(1), lit_value(2)])),
                ("b".into(), unnamed("Neg", vec![lit_value(3)])),
            ]),
        }
    }

    fn tree_value(label: &str, children: Vec<ValueOwned>) -> ValueOwned {
        ValueOwned::Struct {
            fields: FieldsValueOwned::Named(vec![
                ("label".into(), ValueOwned::String(label.into())),
                ("children".into(), ValueOwned::Vec(children)),
            ]),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dynamic_client() {
        let (device, data) = start("recursive/dynamic");
        let client = DynClient::from_config(ClientConfig::new().in_process_path(device.path()))
            .connect()
            .await
            .expect("connect");
        let cmd = client.cmd();
        let bundle = client.device_introspect().unwrap().api_bundle.clone();
        let root = DynResource::root(bundle.clone());

        let r = root
            .child("eval")
            .unwrap()
            .call(cmd, &[expr_value()])
            .await
            .unwrap();
        assert_eq!(r, Some(ValueOwned::Option(Some(Box::new(num(-9))))));
        let r = root
            .child("unwrap_neg")
            .unwrap()
            .call(cmd, &[unnamed("Neg", vec![expr_value()])])
            .await
            .unwrap();
        assert_eq!(r, Some(ValueOwned::Option(Some(Box::new(expr_value())))));

        // written dynamically, read by the server as the derived type
        let tree_res = root.child("tree").unwrap();
        let value = tree_value(
            "root",
            vec![
                tree_value("a", vec![]),
                tree_value(
                    "b",
                    vec![tree_value("b1", vec![]), tree_value("b2", vec![])],
                ),
            ],
        );
        tree_res.write(cmd, &value).await.unwrap();
        let stored = data.lock().unwrap().tree.clone().unwrap();
        assert_eq!(stored, tree().to_ww_bytes_owned().unwrap());
        assert_eq!(tree_res.read(cmd).await.unwrap(), value);
        let ApiItemKindOwned::Property { ty, .. } = &bundle.get_item(&[2]).unwrap().kind else {
            panic!("property");
        };
        assert_eq!(
            value.ser_shrink_wrap_dyn(ty, &bundle).unwrap(),
            tree().to_ww_bytes_owned().unwrap()
        );

        // stream items
        let mut exprs = root.child("exprs").unwrap().stream(cmd).await.unwrap();
        let mut scratch = [0u8; 128];
        let bytes = expr().to_ww_bytes_owned().unwrap();
        let borrowed = recursive_api::Expr::from_ww_bytes(&bytes).unwrap();
        device.send(
            api_impl::stream_data_ser()
                .exprs(&borrowed, &mut scratch)
                .unwrap(),
        );
        assert_eq!(exprs.recv().await.unwrap(), expr_value());

        // deeper than the dynamic decoder allows: an error, the client keeps working
        let deep = negations(wire_weaver_client::ww_self::MAX_DECODE_DEPTH);
        let bytes = deep.to_ww_bytes_owned().unwrap();
        let ApiItemKindOwned::Stream { ty, .. } = &bundle.get_item(&[3]).unwrap().kind else {
            panic!("stream");
        };
        assert!(ValueOwned::des_shrink_wrap_dyn(&bytes, ty, &bundle).is_err());
        let r = root
            .child("eval")
            .unwrap()
            .call(cmd, &[lit_value(5)])
            .await
            .unwrap();
        assert_eq!(r, Some(ValueOwned::Option(Some(Box::new(num(5))))));
    }
}
