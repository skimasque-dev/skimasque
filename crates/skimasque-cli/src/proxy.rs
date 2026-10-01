//! One authenticated session serving both local proxy protocols.
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use skimasque::client::Session;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

pub const FEATURES: &[&str] = &[
    "proxy-http",
    "proxy-connect",
    "proxy-socks5",
    "proxy-socks5-udp",
    "proxy-ready-v1",
];

pub struct Listeners {
    http: TcpListener,
    socks: TcpListener,
}

impl Listeners {
    pub async fn bind(http: SocketAddr, socks: SocketAddr) -> std::io::Result<Self> {
        if !http.ip().is_loopback() || !socks.ip().is_loopback() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "proxy listeners must use loopback addresses",
            ));
        }
        let http = TcpListener::bind(http).await?;
        let socks = TcpListener::bind(socks).await?;
        Ok(Self { http, socks })
    }

    pub fn addresses(&self) -> std::io::Result<(SocketAddr, SocketAddr)> {
        Ok((self.http.local_addr()?, self.socks.local_addr()?))
    }

    /// Publish without replacing another instance's record. A hard link makes
    /// the already-written file visible atomically and fails if PATH exists.
    pub async fn write_ready(&self, path: &Path, gateway: SocketAddr) -> anyhow::Result<()> {
        let (http, socks) = self.addresses()?;
        let record = serde_json::json!({"schema": 1, "pid": std::process::id(),
            "http": http.to_string(), "socks": socks.to_string(), "gateway": gateway.to_string()});
        let mut name = path.as_os_str().to_os_string();
        name.push(format!(".{}.tmp", std::process::id()));
        let temp = std::path::PathBuf::from(name);
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .await?;
        let result = async {
            file.write_all(&serde_json::to_vec(&record)?).await?;
            file.sync_all().await?;
            drop(file);
            tokio::fs::hard_link(&temp, path).await?;
            Ok::<_, anyhow::Error>(())
        }
        .await;
        let _ = tokio::fs::remove_file(&temp).await;
        result
    }

    pub async fn serve(self, session: Arc<Session>) -> anyhow::Result<()> {
        tokio::select! {
            result = crate::http_proxy::serve(self.http, session.clone()) => result?,
            result = crate::socks5::serve(self.socks, session.clone()) => result?,
            error = session.closed() => anyhow::bail!("gateway session closed: {error}"),
        }
        Ok(())
    }
}
