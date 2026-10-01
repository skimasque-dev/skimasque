//! `skimasque exec --forward [LOCAL_PORT:]HOST:PORT`: a loopback TCP listener
//! whose every connection becomes a TCP tunnel to one destination — for tools
//! such as `psql` that ignore proxy settings.

use std::net::Ipv4Addr;
use std::sync::Arc;

use skimasque::client::Session;
use skimasque_core::target::{Target, TargetHost};
use tokio::net::TcpListener;
use tracing::debug;

use crate::session::refusal_line;

/// One `--forward`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardSpec {
    /// The loopback port to listen on; `0` picks a free one.
    pub local_port: u16,
    /// Where each connection is tunnelled to.
    pub target: Target,
}

impl ForwardSpec {
    /// Parse `HOST:PORT` or `LOCAL_PORT:HOST:PORT` (IPv6 hosts bracketed).
    pub fn parse(spec: &str) -> Result<Self, String> {
        let (local_port, destination) = match spec.split_once(':') {
            Some((head, rest))
                if !head.is_empty()
                    && head.bytes().all(|b| b.is_ascii_digit())
                    && rest.contains(':') =>
            {
                let port = head
                    .parse::<u16>()
                    .map_err(|_| format!("{head:?} is not a local port (1-65535)"))?;
                (port, rest)
            }
            _ => (0, spec),
        };
        let target = Target::parse(destination)
            .map_err(|e| format!("{destination:?} is not HOST:PORT ({e})"))?;
        Ok(Self { local_port, target })
    }

    /// The environment variable exec sets to this forward's local address:
    /// `SKIMASQUE_FORWARD_<HOST>_<PORT>`, upper-cased, every other byte `_`.
    pub fn env_name(&self) -> String {
        let host = match &self.target.host {
            TargetHost::Name(name) => name.clone(),
            TargetHost::Ip(ip) => ip.to_string(),
        };
        let host: String = host
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_uppercase()
                } else {
                    '_'
                }
            })
            .collect();
        format!("SKIMASQUE_FORWARD_{host}_{}", self.target.port)
    }
}

/// Listen on `127.0.0.1:<local_port>`.
pub async fn bind(spec: &ForwardSpec) -> std::io::Result<TcpListener> {
    TcpListener::bind((Ipv4Addr::LOCALHOST, spec.local_port)).await
}

/// Tunnel every connection on `listener` to `target` until the listener fails.
pub async fn serve(
    listener: TcpListener,
    target: Target,
    session: Arc<Session>,
) -> std::io::Result<()> {
    loop {
        let (stream, client) = listener.accept().await?;
        let session = session.clone();
        let target = target.clone();
        tokio::spawn(async move {
            match session.connect_tcp(target.clone()).await {
                Ok(tunnel) => {
                    if let Err(error) = tunnel.relay(stream).await {
                        debug!(%client, %target, %error, "forwarded connection ended with an error");
                    }
                }
                Err(error) => eprintln!("skimasque: {}", refusal_line(&target.to_string(), &error)),
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_destination_gets_an_ephemeral_port() {
        let f = ForwardSpec::parse("db.prod:5432").unwrap();
        assert_eq!(f.local_port, 0);
        assert_eq!(f.target.to_string(), "db.prod:5432");
    }

    #[test]
    fn a_leading_port_is_the_local_one() {
        let f = ForwardSpec::parse("15432:db.prod:5432").unwrap();
        assert_eq!(f.local_port, 15432);
        assert_eq!(f.target.to_string(), "db.prod:5432");
    }

    #[test]
    fn ipv6_destinations_are_bracketed() {
        let f = ForwardSpec::parse("15432:[fd00::5]:5432").unwrap();
        assert_eq!(f.local_port, 15432);
        assert_eq!(f.target.to_string(), "[fd00::5]:5432");
        assert_eq!(ForwardSpec::parse("[fd00::5]:5432").unwrap().local_port, 0);
    }

    #[test]
    fn bad_specs_are_refused_with_a_reason() {
        for bad in [
            "db.prod",
            "70000:db.prod:5432",
            "db.prod:notaport",
            "",
            "15432:",
        ] {
            assert!(
                ForwardSpec::parse(bad).is_err(),
                "{bad:?} should be refused"
            );
        }
    }

    #[test]
    fn env_names_are_upper_snake_case() {
        assert_eq!(
            ForwardSpec::parse("db.prod:5432").unwrap().env_name(),
            "SKIMASQUE_FORWARD_DB_PROD_5432"
        );
        assert_eq!(
            ForwardSpec::parse("[fd00::5]:5432").unwrap().env_name(),
            "SKIMASQUE_FORWARD_FD00__5_5432"
        );
        assert_eq!(
            ForwardSpec::parse("my-db.internal:6379")
                .unwrap()
                .env_name(),
            "SKIMASQUE_FORWARD_MY_DB_INTERNAL_6379"
        );
    }
}
