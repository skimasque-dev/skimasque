use futures::{SinkExt, StreamExt};
use netstack_smoltcp::{udp::UdpMsg, UdpSocket};
use skimasque::client::Session;
use skimasque_core::connect_udp::Target;
use std::{collections::HashMap, net::SocketAddr, sync::Arc, time::Duration};
use tokio::{sync::mpsc, task::JoinSet};
#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
pub struct FlowKey {
    pub source: SocketAddr,
    pub destination: SocketAddr,
}
pub async fn serve(socket: UdpSocket, session: Arc<Session>) -> anyhow::Result<()> {
    let (mut input, mut output) = socket.split();
    let (reply_tx, mut replies) = mpsc::channel::<UdpMsg>(512);
    let mut flows: HashMap<FlowKey, (u64, mpsc::Sender<Vec<u8>>)> = HashMap::new();
    let mut generation = 0u64;
    let mut tasks = JoinSet::new();
    loop {
        tokio::select! {
         item=input.next()=> {
          let Some((bytes,source,destination))=item else {anyhow::bail!("native UDP input closed")};
          let key=FlowKey {source,destination};
          if let Some((_,tx))=flows.get(&key) {let _=tx.try_send(bytes);continue;}
          if flows.len()>=1024 {continue;}
          let (tx,rx)=mpsc::channel(64); tx.try_send(bytes)?;
          generation=generation.wrapping_add(1);let id=generation;
          flows.insert(key,(id,tx));let session=session.clone();let reply_tx=reply_tx.clone();
          tasks.spawn(async move { let result=flow(key,rx,reply_tx,session,Duration::from_secs(120)).await;
           if let Err(error)=result {tracing::debug!(%error,"native UDP association ended");}
           (key,id)
          });
         },
         Some(reply)=replies.recv()=> {output.send(reply).await?;},
         result=tasks.join_next(),if !tasks.is_empty()=> {
          let (key,id)=result.context("native UDP task missing")??;
          if flows.get(&key).is_some_and(|(current,_)|*current==id) {flows.remove(&key);}
         }
        }
    }
}
use anyhow::Context;
async fn flow(
    key: FlowKey,
    mut rx: mpsc::Receiver<Vec<u8>>,
    reply: mpsc::Sender<UdpMsg>,
    session: Arc<Session>,
    idle: Duration,
) -> anyhow::Result<()> {
    let target = Target::parse(&key.destination.to_string())?;
    let mut tunnel =
        tokio::time::timeout(Duration::from_secs(30), session.connect_udp(target)).await??;
    loop {
        tokio::select! {
         packet=rx.recv()=> {let Some(packet)=packet else {break};
          if tunnel.max_payload_size().is_some_and(|max|packet.len()>max) {continue;}
          tunnel.send(&packet)?;
         },
         packet=tunnel.recv()=> {let Some(packet)=packet else {break};
          let header=if key.source.is_ipv4(){28}else{48};
          if packet.len()+header<=1280 {reply.send((packet.to_vec(),key.destination,key.source)).await?;}
         },
         _=tokio::time::sleep(idle)=>break,
        }
    }
    tunnel.close().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use skimasque::{
        client::Client,
        policy::AddressPolicy,
        server::{ProxyConfig, Server},
        service::UdpProxy,
        tls,
    };
    use skimasque_core::UriTemplate;

    #[tokio::test]
    async fn idle_association_expires_and_same_tuple_can_reopen_on_live_session() {
        let certificate = tls::generate_self_signed(vec!["localhost".into()]).unwrap();
        let server = Server::bind(
            "127.0.0.1:0".parse().unwrap(),
            tls::server_config_from_pem(
                certificate.certificate_pem.as_bytes(),
                certificate.key_pem.as_bytes(),
            )
            .unwrap(),
            UdpProxy::new(AddressPolicy::permissive()),
            ProxyConfig::new("localhost").unwrap(),
        )
        .unwrap();
        let gateway = server.local_addr().unwrap();
        let mut owned = JoinSet::new();
        owned.spawn(async move {
            let _ = server.run().await;
        });
        let client = Client::new(
            tls::client_config_with_ca(certificate.certificate_pem.as_bytes()).unwrap(),
        )
        .unwrap();
        let session = Arc::new(
            client
                .connect(
                    gateway,
                    UriTemplate::default_connect_udp("localhost").unwrap(),
                )
                .await
                .unwrap(),
        );
        let destination = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let key = FlowKey {
            source: "127.0.0.1:12345".parse().unwrap(),
            destination: destination.local_addr().unwrap(),
        };
        for payload in [b"first".as_slice(), b"reused"] {
            let (tx, rx) = mpsc::channel(1);
            let (reply, _replies) = mpsc::channel(1);
            tx.send(payload.to_vec()).await.unwrap();
            let session = session.clone();
            owned.spawn(async move {
                flow(key, rx, reply, session, Duration::from_millis(100))
                    .await
                    .unwrap();
            });
            let mut packet = [0; 32];
            let (length, _) =
                tokio::time::timeout(Duration::from_secs(2), destination.recv_from(&mut packet))
                    .await
                    .unwrap()
                    .unwrap();
            assert_eq!(&packet[..length], payload);
            tokio::time::timeout(Duration::from_secs(1), owned.join_next())
                .await
                .expect("idle association did not release its tunnel")
                .unwrap()
                .unwrap();
            drop(tx);
        }
    }
}
