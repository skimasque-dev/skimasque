use futures::StreamExt;
use netstack_smoltcp::TcpListener;
use skimasque::client::Session;
use skimasque_core::connect_udp::Target;
use std::{sync::Arc, time::Duration};
use tokio::task::JoinSet;
pub async fn serve(mut listener: TcpListener, session: Arc<Session>) -> anyhow::Result<()> {
    let mut tasks = JoinSet::new();
    loop {
        tokio::select! {
         item=listener.next()=> {
          let Some((stream,_,destination))=item else {anyhow::bail!("native TCP listener closed")};
          if tasks.len()>=1024 {drop(stream);continue;}
          let session=session.clone();
          tasks.spawn(async move {
           let result=async {
            let target=Target::parse(&destination.to_string())?;
            let tunnel=tokio::time::timeout(Duration::from_secs(30),session.connect_tcp(target)).await??;
            tunnel.relay(stream).await?; Ok::<_,anyhow::Error>(())
           }.await;
           if let Err(error)=result {tracing::debug!(%error,"native TCP flow ended");}
          });
         },
         result=tasks.join_next(),if !tasks.is_empty()=> {if let Some(Err(error))=result {return Err(error.into());}}
        }
    }
}
