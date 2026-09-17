use tokio::sync::oneshot;
use wire_weaver::shrink_wrap::{
    BufReader, DeserializeShrinkWrap, DeserializeShrinkWrapOwned, SerializeShrinkWrapOwned, UNib32,
    either_any_vec::EitherAnyVec,
};
use ww_client_server::{MultiIndexOwned, RequestKindOwned};

use crate::{Error, event_loop::commander::TransportCommander};

pub(crate) trait PropertyPath {
    type Output;

    fn absolute_path(&self) -> Option<Vec<UNib32>>;
    fn commander(self) -> TransportCommander;
}

pub trait MultiRead<P, O> {
    fn multi_read(self) -> impl Future<Output = Result<O, Error>>;
}

impl<A, B, AO, BO>
    MultiRead<
        (A, B),
        (
            Result<AO, ww_client_server::ErrorOwned>,
            Result<BO, ww_client_server::ErrorOwned>,
        ),
    > for (A, B)
where
    A: PropertyPath<Output = AO>,
    AO: DeserializeShrinkWrapOwned,
    B: PropertyPath<Output = BO>,
    BO: DeserializeShrinkWrapOwned,
{
    async fn multi_read(
        self,
    ) -> Result<
        (
            Result<AO, ww_client_server::ErrorOwned>,
            Result<BO, ww_client_server::ErrorOwned>,
        ),
        Error,
    > {
        let Some(path_a) = self.0.absolute_path() else {
            return Err(Error::MultiReq("a path".into()));
        };
        let Some(path_b) = self.1.absolute_path() else {
            return Err(Error::MultiReq("b path".into()));
        };
        if path_a.len() != path_b.len() {
            return Err(Error::MultiReq(
                "Resources must be at the same level".into(),
            ));
        }
        if path_a.is_empty() {
            return Err(Error::MultiReq(
                "Cannot multi-request root level itself".into(),
            ));
        }
        let len = path_a.len();
        let req = if len == 1 {
            RequestKindOwned::MultiRead {
                multi_idx: MultiIndexOwned::List(vec![path_a[0], path_b[0]]),
                in_each_array_id: None,
            }
        } else {
            todo!()
        };

        let cmd = self.0.commander();
        let req = ww_client_server::RequestOwned {
            seq: 0,
            path_kind: ww_client_server::PathKindOwned::Absolute { path: vec![] },
            kind: req,
        };
        let req = req.to_ww_bytes_owned()?;
        let (done_tx, done_rx) = oneshot::channel();
        cmd.send_message_expect_response(req, done_tx, None).await?;
        let response = done_rx.await.map_err(|_| Error::RxDispatcherNotRunning)??;
        println!("{response:02x?}");

        let mut rd = BufReader::new(&response);
        let either_any_vec = EitherAnyVec::des_shrink_wrap(&mut rd).unwrap();
        let mut iter = either_any_vec.iter();
        let ao = iter.next_owned::<ww_client_server::ErrorOwned, AO>()?;
        let ao: Result<AO, ww_client_server::ErrorOwned> = ao.into();
        let bo = iter.next_owned::<ww_client_server::ErrorOwned, BO>()?;
        let bo: Result<BO, ww_client_server::ErrorOwned> = bo.into();

        Ok((ao, bo))
    }
}
