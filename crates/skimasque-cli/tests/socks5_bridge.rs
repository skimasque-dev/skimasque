//! End-to-end tests of the SOCKS5 bridge: a raw SOCKS5 client, the bridge, a
//! real MASQUE proxy, and real UDP targets.
//!
//! The SOCKS5 side is driven with hand-built bytes rather than a client library,
//! so the test fails if the wire format drifts.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use skimasque::client::{Client, Session};
use skimasque::policy::AddressPolicy;
use skimasque::server::{ProxyConfig, Server};
use skimasque::service::{Dispatch, TcpProxy, UdpProxy};
use skimasque::tls;
use skimasque_cli::socks5::{self, Address};
use skimasque_core::UriTemplate;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::time::timeout;

const REPLY_TIMEOUT: Duration = Duration::from_secs(10);

async fn spawn_echo(tag: &'static [u8]) -> SocketAddr {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = socket.local_addr().unwrap();
    tokio::spawn(async move {
        let mut buf = vec![0u8; 2048];
        loop {
            let Ok((len, from)) = socket.recv_from(&mut buf).await else {
                return;
            };
            let mut reply = tag.to_vec();
            reply.extend_from_slice(&buf[..len]);
            if socket.send_to(&reply, from).await.is_err() {
                return;
            }
        }
    });
    addr
}

/// A MASQUE proxy and a SOCKS5 bridge in front of it. Returns the SOCKS5 address.
async fn spawn_bridge() -> SocketAddr {
    let generated = tls::generate_self_signed(vec!["localhost".to_owned()]).unwrap();
    let server_tls = tls::server_config_from_pem(
        generated.certificate_pem.as_bytes(),
        generated.key_pem.as_bytes(),
    )
    .unwrap();
    let server = Server::bind(
        "127.0.0.1:0".parse().unwrap(),
        server_tls,
        // Every target in these tests is on loopback.
        Dispatch::new()
            .with_udp(UdpProxy::new(AddressPolicy::permissive()))
            .with_tcp(TcpProxy::new(AddressPolicy::permissive())),
        ProxyConfig::new("localhost").unwrap(),
    )
    .unwrap();
    let proxy_addr = server.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = server.run().await;
    });

    let client_tls = tls::client_config_with_ca(generated.certificate_pem.as_bytes()).unwrap();
    let template =
        UriTemplate::default_connect_udp(&format!("localhost:{}", proxy_addr.port())).unwrap();
    let session: Session = Client::new(client_tls)
        .unwrap()
        .connect(proxy_addr, template)
        .await
        .unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let socks_addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = socks5::serve(listener, Arc::new(session)).await;
    });
    socks_addr
}

/// A raw SOCKS5 client holding an open UDP association.
struct Socks5Client {
    /// The control connection. Dropping it tears the association down, so the
    /// test has to keep it alive.
    _control: TcpStream,
    relay: UdpSocket,
    relay_addr: SocketAddr,
}

impl Socks5Client {
    async fn associate(socks: SocketAddr) -> Self {
        let mut control = TcpStream::connect(socks).await.unwrap();

        // Greeting: version 5, one method, "no authentication".
        control.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
        let mut chosen = [0u8; 2];
        control.read_exact(&mut chosen).await.unwrap();
        assert_eq!(chosen, [0x05, 0x00], "expected the no-auth method");

        // UDP ASSOCIATE, declaring 0.0.0.0:0 so the bridge learns our address
        // from the first datagram.
        control
            .write_all(&[0x05, 0x03, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
            .await
            .unwrap();

        let mut head = [0u8; 4];
        control.read_exact(&mut head).await.unwrap();
        assert_eq!(head[0], 0x05);
        assert_eq!(head[1], 0x00, "association was refused");
        assert_eq!(head[3], 0x01, "expected an IPv4 bound address");
        let mut bound = [0u8; 6];
        control.read_exact(&mut bound).await.unwrap();
        let relay_addr = SocketAddr::from((
            [bound[0], bound[1], bound[2], bound[3]],
            u16::from_be_bytes([bound[4], bound[5]]),
        ));

        let relay = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        Self {
            _control: control,
            relay,
            relay_addr,
        }
    }

    async fn send_to(&self, destination: &Address, payload: &[u8]) {
        let packet = socks5::encode_udp_packet(destination, payload);
        self.relay.send_to(&packet, self.relay_addr).await.unwrap();
    }

    /// Receive one relayed datagram, returning its source and payload.
    async fn recv(&self) -> (Address, Vec<u8>) {
        let mut buf = vec![0u8; 65535];
        let (len, _) = timeout(REPLY_TIMEOUT, self.relay.recv_from(&mut buf))
            .await
            .expect("timed out waiting for a relayed reply")
            .unwrap();
        let (frag, source, payload) = socks5::decode_udp_packet(&buf[..len]).unwrap();
        assert_eq!(frag, 0, "the bridge must not fragment");
        (source, payload.to_vec())
    }
}

#[tokio::test]
async fn a_socks5_datagram_reaches_its_target_through_the_tunnel() {
    let echo = spawn_echo(b"echo:").await;
    let socks = spawn_bridge().await;
    let client = Socks5Client::associate(socks).await;

    let destination = Address::Ip(echo);
    client.send_to(&destination, b"hello").await;

    let (source, payload) = client.recv().await;
    assert_eq!(source, destination, "reply named the wrong source");
    assert_eq!(payload, b"echo:hello");
}

/// One association, several destinations, one HTTP/3 connection underneath.
/// Each destination gets its own CONNECT-UDP tunnel, so this is the SOCKS5-level
/// version of the datagram demultiplexing test.
#[tokio::test]
async fn one_association_keeps_several_destinations_apart() {
    let tags: [&'static [u8]; 3] = [b"alpha:", b"beta:", b"gamma:"];
    let mut destinations = Vec::new();
    for tag in tags {
        destinations.push(Address::Ip(spawn_echo(tag).await));
    }

    let socks = spawn_bridge().await;
    let client = Socks5Client::associate(socks).await;

    for destination in &destinations {
        client.send_to(destination, b"ping").await;
    }

    // Replies can arrive in any order, so match each one to its source.
    let mut seen = std::collections::HashMap::new();
    for _ in 0..destinations.len() {
        let (source, payload) = client.recv().await;
        seen.insert(source, payload);
    }

    for (destination, tag) in destinations.iter().zip(tags) {
        let payload = seen
            .get(destination)
            .unwrap_or_else(|| panic!("no reply from {destination}"));
        let mut expected = tag.to_vec();
        expected.extend_from_slice(b"ping");
        assert_eq!(payload, &expected, "reply from {destination} was mislabelled");
    }
}

/// A destination reused across datagrams must reuse its tunnel rather than
/// opening a new one each time.
#[tokio::test]
async fn repeated_datagrams_to_one_destination_reuse_the_tunnel() {
    let echo = spawn_echo(b"n:").await;
    let socks = spawn_bridge().await;
    let client = Socks5Client::associate(socks).await;
    let destination = Address::Ip(echo);

    for i in 0..5u8 {
        client.send_to(&destination, &[i]).await;
        let (source, payload) = client.recv().await;
        assert_eq!(source, destination);
        assert_eq!(payload, vec![b'n', b':', i]);
    }
}

/// A TCP server that echoes `tag` before every chunk it reads.
async fn spawn_tcp_echo(tag: &'static [u8]) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                loop {
                    let n = match stream.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => n,
                    };
                    let mut reply = tag.to_vec();
                    reply.extend_from_slice(&buf[..n]);
                    if stream.write_all(&reply).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    addr
}

#[tokio::test]
async fn connect_opens_a_tcp_tunnel_through_the_gateway() {
    let echo = spawn_tcp_echo(b"echo:").await;
    let socks = spawn_bridge().await;
    let mut control = TcpStream::connect(socks).await.unwrap();

    control.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
    let mut chosen = [0u8; 2];
    control.read_exact(&mut chosen).await.unwrap();
    assert_eq!(chosen, [0x05, 0x00]);

    // CMD = 1 (CONNECT) to the echo server.
    let mut request = vec![0x05, 0x01, 0x00];
    Address::Ip(echo).encode_into(&mut request);
    control.write_all(&request).await.unwrap();

    let mut reply = [0u8; 4];
    control.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply[0], 0x05);
    assert_eq!(reply[1], 0x00, "CONNECT should succeed");
    // Consume the bound address (IPv4 unspecified + port).
    let mut bound = [0u8; 6];
    control.read_exact(&mut bound).await.unwrap();

    control.write_all(b"ping").await.unwrap();
    let mut buf = [0u8; 9];
    timeout(REPLY_TIMEOUT, control.read_exact(&mut buf))
        .await
        .expect("timed out")
        .unwrap();
    assert_eq!(&buf, b"echo:ping");
}

#[tokio::test]
async fn connect_to_a_dead_port_reports_a_refusal() {
    let dead = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dead_addr = dead.local_addr().unwrap();
    drop(dead);

    let socks = spawn_bridge().await;
    let mut control = TcpStream::connect(socks).await.unwrap();
    control.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
    let mut chosen = [0u8; 2];
    control.read_exact(&mut chosen).await.unwrap();

    let mut request = vec![0x05, 0x01, 0x00];
    Address::Ip(dead_addr).encode_into(&mut request);
    control.write_all(&request).await.unwrap();

    let mut reply = [0u8; 4];
    control.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply[0], 0x05);
    assert_eq!(
        reply[1], 0x05,
        "expected REP = connection refused, got {:#04x}",
        reply[1]
    );
}

/// The relay port must not forward for whoever finds it. Only the address that
/// sent the first datagram is served.
#[tokio::test]
async fn the_relay_ignores_datagrams_from_other_sources() {
    let echo = spawn_echo(b"echo:").await;
    let socks = spawn_bridge().await;
    let client = Socks5Client::associate(socks).await;
    let destination = Address::Ip(echo);

    // The association pins itself to this client.
    client.send_to(&destination, b"first").await;
    let (_, payload) = client.recv().await;
    assert_eq!(payload, b"echo:first");

    // A different socket now tries to use the same relay.
    let intruder = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let packet = socks5::encode_udp_packet(&destination, b"intruder");
    intruder.send_to(&packet, client.relay_addr).await.unwrap();

    let mut buf = vec![0u8; 2048];
    assert!(
        timeout(Duration::from_millis(500), intruder.recv_from(&mut buf))
            .await
            .is_err(),
        "the relay answered an unrelated source"
    );
}
