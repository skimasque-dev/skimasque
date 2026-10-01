//! `skimasque-client` -- open MASQUE tunnels through a proxy.
//!
//! Modes: `connect` bridges one TCP tunnel to stdin/stdout, `socks5` puts a
//! SOCKS5 relay in front of the tunnel so ordinary applications can use it, and
//! `probe` sends a payload and prints what comes back, for checking a proxy by
//! hand.
//!
//! `skimasque connect <destination> <args>` is the front door for the `connect`
//! mode -- it execs this binary as `skimasque-client <args> connect --target
//! <destination>` -- but this binary can also be run directly.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use anyhow::Context;
use clap::{ArgAction, Args as ClapArgs, Parser, Subcommand};
use http::{HeaderMap, HeaderValue};
use skimasque::client::Session;
use skimasque_cli::probe::{self, DnsHeader, TYPE_A, TYPE_AAAA};
use skimasque_cli::session::{self, AuthArgs, Connected, TlsArgs};
use skimasque_cli::{init_tracing, proxy, socks5};
use skimasque_core::connect_udp::Target;
use tokio::net::TcpListener;
use tokio::time::timeout;

/// A MASQUE client for UDP over HTTP/3.
#[derive(Debug, Parser)]
#[command(
    name = "skimasque-client",
    version,
    about = "Open MASQUE tunnels through a gateway (the `connect` mode is also `skimasque connect`)",
    long_about = None
)]
struct Args {
    #[command(flatten)]
    connection: ConnectionArgs,

    /// Increase logging; repeat for more.
    #[arg(short, long, action = ArgAction::Count, global = true)]
    verbose: u8,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, ClapArgs)]
struct ConnectionArgs {
    /// The gateway to reach, as `host[:port]` (an IP is fine too). The host is
    /// resolved for the QUIC socket and used as the TLS server name; the port
    /// defaults to 443.
    #[arg(
        long,
        value_name = "HOST[:PORT]",
        default_value = "gateway.skimasque.com"
    )]
    proxy: String,

    #[command(flatten)]
    tls: TlsArgs,

    #[command(flatten)]
    auth: AuthArgs,

    /// The application name to declare, sent as `X-Masque-Application`.
    ///
    /// A policy-enforcing proxy matches on this. It is session context, not an
    /// authenticated fact -- the proxy trusts it exactly as far as it trusts
    /// this client.
    #[arg(long, value_name = "NAME")]
    app: Option<String>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Report supported integration features without opening a gateway session.
    Capabilities {
        #[arg(long)]
        json: bool,
    },
    /// Serve HTTP/HTTPS and SOCKS5 through one authenticated gateway session.
    Proxy {
        #[arg(long, default_value = "127.0.0.1:8080", value_name = "ADDR")]
        http_listen: SocketAddr,
        #[arg(long, default_value = "127.0.0.1:1080", value_name = "ADDR")]
        socks_listen: SocketAddr,
        /// Atomically publish bound addresses after authentication and startup.
        #[arg(long, value_name = "PATH")]
        ready_file: Option<PathBuf>,
        /// Attach an existing user-owned Linux TUN (MTU 1280).
        #[arg(long, value_name = "NAME")]
        tun_interface: Option<String>,
    },
    /// Run a SOCKS5 server whose UDP associations go through the proxy.
    Socks5 {
        /// Address to serve SOCKS5 on.
        #[arg(long, default_value = "127.0.0.1:1080", value_name = "ADDR")]
        listen: SocketAddr,
    },
    /// Open a raw TCP tunnel and bridge it to stdin/stdout.
    ///
    /// Useful as an SSH `ProxyCommand` (`ProxyCommand skimasque-client ...
    /// connect --target %h:%p`) and for interop testing.
    Connect {
        /// The destination, as `host:port`.
        #[arg(long, value_name = "HOST:PORT")]
        target: String,
    },
    /// Send a payload through a tunnel and print the replies.
    Probe {
        /// The destination, as `host:port`.
        #[arg(long, value_name = "HOST:PORT")]
        target: String,

        /// Send a DNS query for this name.
        #[arg(long, value_name = "NAME", group = "payload")]
        dns: Option<String>,

        /// Ask for AAAA instead of A. Only meaningful with `--dns`.
        #[arg(long, requires = "dns")]
        aaaa: bool,

        /// Send this text.
        #[arg(long, value_name = "TEXT", group = "payload")]
        text: Option<String>,

        /// Send these hex bytes. Spaces and colons are ignored.
        #[arg(long, value_name = "HEX", group = "payload")]
        hex: Option<String>,

        /// How many times to send.
        #[arg(long, default_value_t = 1, value_name = "N")]
        count: u32,

        /// How long to wait for each reply.
        #[arg(long, default_value_t = 3000, value_name = "MS")]
        timeout: u64,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    init_tracing(args.verbose);

    if let Command::Capabilities { json } = args.command {
        if json {
            println!(
                "{}",
                serde_json::json!({"schema": 1, "features": proxy::FEATURES})
            );
        } else {
            println!("{}", proxy::FEATURES.join("\n"));
        }
        return Ok(());
    }

    if let Command::Proxy {
        tun_interface: Some(name),
        ..
    } = &args.command
    {
        #[cfg(not(target_os = "linux"))]
        {
            let _ = name;
            anyhow::bail!("native TUN requires Linux");
        }
        #[cfg(target_os = "linux")]
        skimasque_cli::native::validate_name(name)?;
    }
    let mut headers = HeaderMap::new();
    if let Some(app) = &args.connection.app {
        let value = HeaderValue::from_str(app)
            .context("the application name contains characters a header cannot carry")?;
        headers.insert(skimasque::APPLICATION_HEADER, value);
    }
    let Connected {
        session,
        refresh,
        note,
        ..
    } = session::open_session(
        &args.connection.proxy,
        &args.connection.tls,
        &args.connection.auth,
        headers,
    )
    .await?;
    if let Some(note) = note {
        eprintln!("{note}");
    }
    let session = Arc::new(session);
    eprintln!(
        "connected to {} serving {}",
        session.remote_address(),
        session.template().as_str()
    );

    // In OIDC mode the platform credential outlives neither a long deploy nor
    // the gateway's `--credential-ttl`, so keep it fresh for as long as we run.
    if let Some((exchange, ttl)) = refresh {
        tokio::spawn(session::refresh_credential(
            session.clone(),
            exchange,
            ttl,
            true,
        ));
    }

    match args.command {
        Command::Capabilities { .. } => unreachable!("handled before connecting"),
        Command::Proxy {
            http_listen,
            socks_listen,
            ready_file,
            tun_interface,
        } => {
            #[cfg(target_os = "linux")]
            let native = match tun_interface.as_deref() {
                Some(name) => Some(skimasque_cli::native::NativeTun::attach(name).await?),
                None => None,
            };
            let listeners = proxy::Listeners::bind(http_listen, socks_listen).await?;
            let (http, socks) = listeners.addresses()?;
            if let Some(path) = &ready_file {
                listeners
                    .write_ready_with_tun(path, session.remote_address(), tun_interface.as_deref())
                    .await?;
            }
            eprintln!("HTTP proxy on {http}; SOCKS5 on {socks}");
            #[cfg(target_os = "linux")]
            let native_session = session.clone();
            let result = tokio::select! {
                result = listeners.serve(session) => result,
                result = async {
                    #[cfg(target_os = "linux")]
                    if let Some(native) = native { return native.serve(native_session).await; }
                    std::future::pending::<anyhow::Result<()>>().await
                } => result,
                _ = shutdown_signal() => Ok(()),
            };
            if let Some(path) = ready_file {
                let _ = tokio::fs::remove_file(path).await;
            }
            result
        }
        Command::Socks5 { listen } => run_socks5(listen, session).await,
        Command::Connect { target } => run_connect(session, &target).await,
        Command::Probe {
            target,
            dns,
            aaaa,
            text,
            hex,
            count,
            timeout,
        } => {
            run_probe(
                session,
                &target,
                build_payload(dns.as_deref(), aaaa, text.as_deref(), hex.as_deref())?,
                count,
                Duration::from_millis(timeout),
            )
            .await
        }
    }
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        if let Ok(mut term) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! { _ = term.recv() => {}, _ = tokio::signal::ctrl_c() => {} }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}

async fn run_connect(session: Arc<Session>, target: &str) -> anyhow::Result<()> {
    let target = Target::parse(target).context("parsing --target")?;
    let tunnel = session
        .connect_tcp(target.clone())
        .await
        .with_context(|| format!("opening a TCP tunnel to {target}"))?;
    eprintln!("tunnel open to {target}; bridging stdin/stdout");
    let stdio = tokio::io::join(tokio::io::stdin(), tokio::io::stdout());
    tunnel.relay(stdio).await?;
    Ok(())
}

async fn run_socks5(listen: SocketAddr, session: Arc<Session>) -> anyhow::Result<()> {
    let listener = TcpListener::bind(listen)
        .await
        .with_context(|| format!("binding {listen}"))?;
    let addr = listener.local_addr()?;
    eprintln!("SOCKS5 listening on {addr} (CONNECT and UDP ASSOCIATE)");
    eprintln!("point a client at socks5h://{addr} so names resolve at the proxy");

    tokio::select! {
        result = socks5::serve(listener, session) => Ok(result?),
        _ = tokio::signal::ctrl_c() => {
            eprintln!("\nshutting down");
            Ok(())
        }
    }
}

fn build_payload(
    dns: Option<&str>,
    aaaa: bool,
    text: Option<&str>,
    hex: Option<&str>,
) -> anyhow::Result<Payload> {
    match (dns, text, hex) {
        (Some(name), _, _) => {
            // Any unpredictable id will do; the point is to notice a reply that
            // does not belong to this query.
            let id = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.subsec_nanos() as u16)
                .unwrap_or(0x1234);
            let qtype = if aaaa { TYPE_AAAA } else { TYPE_A };
            let bytes = probe::dns_query(id, name, qtype).map_err(anyhow::Error::msg)?;
            Ok(Payload {
                bytes,
                dns_id: Some(id),
            })
        }
        (None, Some(text), _) => Ok(Payload {
            bytes: text.as_bytes().to_vec(),
            dns_id: None,
        }),
        (None, None, Some(hex)) => Ok(Payload {
            bytes: probe::parse_hex(hex).map_err(anyhow::Error::msg)?,
            dns_id: None,
        }),
        (None, None, None) => {
            anyhow::bail!("give the probe something to send: --dns, --text or --hex")
        }
    }
}

struct Payload {
    bytes: Vec<u8>,
    /// Set when the payload is a DNS query, so the reply can be checked.
    dns_id: Option<u16>,
}

async fn run_probe(
    session: Arc<Session>,
    target: &str,
    payload: Payload,
    count: u32,
    reply_timeout: Duration,
) -> anyhow::Result<()> {
    let target = Target::parse(target).context("parsing --target")?;
    let mut tunnel = session
        .connect_udp(target.clone())
        .await
        .with_context(|| format!("opening a tunnel to {target}"))?;

    println!(
        "tunnel to {target} open on stream {} (max payload {} bytes)",
        tunnel.stream_id(),
        tunnel
            .max_payload_size()
            .map_or_else(|| "unknown".to_owned(), |n| n.to_string())
    );
    println!(
        "sending {} bytes:\n{}",
        payload.bytes.len(),
        probe::hexdump(&payload.bytes)
    );

    let mut received = 0u32;
    for attempt in 1..=count {
        let sent_at = Instant::now();
        tunnel.send(&payload.bytes)?;

        match timeout(reply_timeout, tunnel.recv()).await {
            Ok(Some(reply)) => {
                received += 1;
                println!(
                    "reply {attempt}: {} bytes in {:.1?}",
                    reply.len(),
                    sent_at.elapsed()
                );
                if let Some(query_id) = payload.dns_id {
                    print_dns_summary(query_id, &reply);
                }
                print!("{}", probe::hexdump(&reply));
            }
            Ok(None) => {
                println!("reply {attempt}: the proxy closed the tunnel");
                break;
            }
            Err(_) => println!("reply {attempt}: timed out after {reply_timeout:.1?}"),
        }
    }

    println!("{received}/{count} replies received");
    tunnel.close().await?;

    // A tunnel that sent but never heard back is a failure worth an exit code.
    if received == 0 {
        anyhow::bail!("no replies");
    }
    Ok(())
}

fn print_dns_summary(query_id: u16, reply: &[u8]) {
    let Some(header) = DnsHeader::parse(reply) else {
        println!("  (too short to be a DNS reply)");
        return;
    };
    if header.id != query_id {
        println!(
            "  warning: transaction id {:#06x} does not match the query's {:#06x}",
            header.id, query_id
        );
    }
    println!(
        "  DNS: {} rcode={} questions={} answers={}",
        if header.is_response {
            "response"
        } else {
            "query"
        },
        header.response_code_name(),
        header.questions,
        header.answers
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    /// clap's own consistency checks. Without this, a contradiction like
    /// "global and required" only surfaces the first time someone runs the
    /// binary.
    #[test]
    fn the_command_definition_is_consistent() {
        Args::command().debug_assert();
    }

    #[test]
    fn a_probe_needs_something_to_send() {
        let payload = build_payload(None, false, None, None);
        assert!(payload.is_err());
    }

    #[test]
    fn probe_payloads_come_from_whichever_source_was_given() {
        assert_eq!(
            build_payload(None, false, Some("hi"), None).unwrap().bytes,
            b"hi"
        );
        assert_eq!(
            build_payload(None, false, None, Some("de ad"))
                .unwrap()
                .bytes,
            vec![0xde, 0xad]
        );
        let dns = build_payload(Some("example.com"), false, None, None).unwrap();
        assert!(dns.dns_id.is_some(), "a DNS probe must track its query id");
        assert_eq!(
            &dns.bytes[dns.bytes.len() - 4..],
            &[0, 1, 0, 1],
            "QTYPE=A QCLASS=IN"
        );

        let aaaa = build_payload(Some("example.com"), true, None, None).unwrap();
        assert_eq!(
            &aaaa.bytes[aaaa.bytes.len() - 4..],
            &[0, 28, 0, 1],
            "QTYPE=AAAA"
        );
    }
}
