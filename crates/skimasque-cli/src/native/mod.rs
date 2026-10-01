//! Native Linux TUN packet forwarding through the authenticated MASQUE session.
mod device;
mod packet;
mod tcp;
mod udp;
use anyhow::{bail, Context};
pub use device::validate_name;
use futures::{SinkExt, StreamExt};
use netstack_smoltcp::{Runner, Stack, StackBuilder, TcpListener, UdpSocket};
use skimasque::client::Session;
use std::sync::Arc;
use tokio::task::JoinSet;
pub struct NativeTun {
    device: Arc<device::Device>,
    stack: Stack,
    runner: Runner,
    tcp: TcpListener,
    udp: UdpSocket,
}
impl NativeTun {
    pub async fn attach(interface: &str) -> anyhow::Result<Self> {
        let device = Arc::new(device::Device::attach(interface).await?);
        let (stack, runner, udp, tcp) = StackBuilder::default()
            .enable_tcp(true)
            .enable_udp(true)
            .enable_icmp(false)
            .mtu(1280)
            .stack_buffer_size(512)
            .tcp_buffer_size(512)
            .udp_buffer_size(512)
            .tcp_recv_buffer_size(65536)
            .tcp_send_buffer_size(65536)
            .build()?;
        Ok(Self {
            device,
            stack,
            runner: runner.context("TCP runner missing")?,
            tcp: tcp.context("TCP missing")?,
            udp: udp.context("UDP missing")?,
        })
    }
    pub fn interface(&self) -> &str {
        &self.device.name
    }
    pub async fn serve(self, session: Arc<Session>) -> anyhow::Result<()> {
        let mut tasks = JoinSet::<anyhow::Result<()>>::new();
        let (mut sink, mut stream) = self.stack.split();
        let read = self.device.clone();
        let write = self.device;
        tasks.spawn(async move {
            let mut buffer = vec![0; 65536];
            loop {
                let n = read.read(&mut buffer).await?;
                if n == 0 {
                    bail!("TUN input closed");
                }
                if packet::valid(&buffer[..n]) {
                    sink.send(buffer[..n].to_vec()).await?;
                }
            }
        });
        tasks.spawn(async move {
            while let Some(packet) = stream.next().await {
                write.write(&packet?).await?;
            }
            bail!("native packet output closed")
        });
        tasks.spawn(async move {
            self.runner.await?;
            bail!("native stack runner stopped")
        });
        tasks.spawn(tcp::serve(self.tcp, session.clone()));
        tasks.spawn(udp::serve(self.udp, session));
        let result = tasks.join_next().await.context("native tasks missing")?;
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
        result?
    }
}
