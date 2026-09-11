//! A SOCKS5 front end that maps CONNECT onto TCP tunnels and UDP ASSOCIATE onto
//! CONNECT-UDP tunnels.
//!
//! SOCKS5 (RFC 1928) is what ordinary applications already speak, so putting it
//! in front of MASQUE is what makes the proxy usable by software that has never
//! heard of HTTP/3. A `CONNECT` becomes a classic-`CONNECT` TCP tunnel -- what
//! `curl`, `git` and database clients need -- and a `UDP ASSOCIATE` gives each
//! destination its own CONNECT-UDP tunnel on the shared HTTP/3 connection.

use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use skimasque::client::{Session, UdpTunnel};
use skimasque_core::connect_udp::{Target, TargetHost};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::mpsc;
use tokio::time::timeout;
use tracing::{debug, info, trace};

const VERSION: u8 = 0x05;

const METHOD_NO_AUTH: u8 = 0x00;
const METHOD_NONE_ACCEPTABLE: u8 = 0xff;

const CMD_CONNECT: u8 = 0x01;
const CMD_UDP_ASSOCIATE: u8 = 0x03;

const REPLY_SUCCEEDED: u8 = 0x00;
const REPLY_GENERAL_FAILURE: u8 = 0x01;
const REPLY_NOT_ALLOWED: u8 = 0x02;
const REPLY_HOST_UNREACHABLE: u8 = 0x04;
const REPLY_CONNECTION_REFUSED: u8 = 0x05;
const REPLY_COMMAND_NOT_SUPPORTED: u8 = 0x07;
const REPLY_ADDRESS_TYPE_NOT_SUPPORTED: u8 = 0x08;

const ATYP_IPV4: u8 = 0x01;
const ATYP_DOMAIN: u8 = 0x03;
const ATYP_IPV6: u8 = 0x04;

/// How long a destination's tunnel survives with no traffic.
///
/// RFC 9298, Section 3.1 asks proxies not to reap sockets faster than two
/// minutes; matching that here keeps a long-lived DNS resolver's tunnel alive
/// between queries instead of rebuilding it each time.
const IDLE_TIMEOUT: Duration = Duration::from_secs(120);

/// The largest datagram a SOCKS5 client can hand us.
const MAX_UDP_PACKET: usize = 65_535;

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("expected SOCKS version 5, got {0}")]
    WrongVersion(u8),
    #[error("unsupported address type {0:#04x}")]
    UnsupportedAddressType(u8),
    #[error("truncated SOCKS5 message")]
    Truncated,
    #[error("domain name is not valid UTF-8")]
    InvalidDomain,
}

/// A SOCKS5 address: a literal endpoint or a name the proxy will resolve.
///
/// Names are deliberately not resolved locally. Handing the name to the proxy
/// keeps the DNS lookup inside the tunnel, which is most of the point of using
/// one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Address {
    Ip(SocketAddr),
    Domain(String, u16),
}

impl Address {
    /// Decode an address from the front of `input`, advancing past it.
    pub fn decode(input: &mut &[u8]) -> Result<Self, ProtocolError> {
        let (&atyp, rest) = input.split_first().ok_or(ProtocolError::Truncated)?;
        *input = rest;
        let address = match atyp {
            ATYP_IPV4 => {
                let octets: [u8; 4] = take(input, 4)?.try_into().expect("length checked");
                Self::Ip(SocketAddr::new(
                    IpAddr::V4(Ipv4Addr::from(octets)),
                    take_port(input)?,
                ))
            }
            ATYP_IPV6 => {
                let octets: [u8; 16] = take(input, 16)?.try_into().expect("length checked");
                Self::Ip(SocketAddr::new(
                    IpAddr::V6(Ipv6Addr::from(octets)),
                    take_port(input)?,
                ))
            }
            ATYP_DOMAIN => {
                let len = usize::from(take(input, 1)?[0]);
                let name = std::str::from_utf8(take(input, len)?)
                    .map_err(|_| ProtocolError::InvalidDomain)?
                    .to_owned();
                Self::Domain(name, take_port(input)?)
            }
            other => return Err(ProtocolError::UnsupportedAddressType(other)),
        };
        Ok(address)
    }

    pub fn encode_into(&self, out: &mut Vec<u8>) {
        match self {
            Self::Ip(SocketAddr::V4(addr)) => {
                out.push(ATYP_IPV4);
                out.extend_from_slice(&addr.ip().octets());
                out.extend_from_slice(&addr.port().to_be_bytes());
            }
            Self::Ip(SocketAddr::V6(addr)) => {
                out.push(ATYP_IPV6);
                out.extend_from_slice(&addr.ip().octets());
                out.extend_from_slice(&addr.port().to_be_bytes());
            }
            Self::Domain(name, port) => {
                out.push(ATYP_DOMAIN);
                // A name longer than 255 bytes cannot be expressed; truncating
                // would produce a different name, so clamp at the boundary.
                let bytes = name.as_bytes();
                let len = bytes.len().min(u8::MAX as usize);
                out.push(len as u8);
                out.extend_from_slice(&bytes[..len]);
                out.extend_from_slice(&port.to_be_bytes());
            }
        }
    }

    pub fn port(&self) -> u16 {
        match self {
            Self::Ip(addr) => addr.port(),
            Self::Domain(_, port) => *port,
        }
    }

    /// Convert to the MASQUE target this address names.
    pub fn to_target(&self) -> Result<Target, skimasque_core::connect_udp::Error> {
        match self {
            Self::Ip(addr) => Target::new(TargetHost::Ip(addr.ip()), addr.port()),
            Self::Domain(name, port) => Target::new(TargetHost::parse(name)?, *port),
        }
    }
}

impl std::fmt::Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ip(addr) => write!(f, "{addr}"),
            Self::Domain(name, port) => write!(f, "{name}:{port}"),
        }
    }
}

fn take<'a>(input: &mut &'a [u8], len: usize) -> Result<&'a [u8], ProtocolError> {
    let taken = input.get(..len).ok_or(ProtocolError::Truncated)?;
    *input = &input[len..];
    Ok(taken)
}

fn take_port(input: &mut &[u8]) -> Result<u16, ProtocolError> {
    let bytes = take(input, 2)?;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
}

/// Parse a SOCKS5 UDP request header, returning the fragment number, the
/// destination, and the payload that follows.
///
/// Layout (RFC 1928, Section 7): `RSV(2) FRAG(1) ATYP+ADDR+PORT DATA`.
pub fn decode_udp_packet(packet: &[u8]) -> Result<(u8, Address, &[u8]), ProtocolError> {
    let mut input = packet;
    let header = take(&mut input, 3)?;
    let frag = header[2];
    let destination = Address::decode(&mut input)?;
    Ok((frag, destination, input))
}

/// Wrap a payload in a SOCKS5 UDP header addressed from `source`.
pub fn encode_udp_packet(source: &Address, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 22);
    out.extend_from_slice(&[0x00, 0x00, 0x00]); // RSV, RSV, FRAG
    source.encode_into(&mut out);
    out.extend_from_slice(payload);
    out
}

/// Accept SOCKS5 clients until the listener is closed.
pub async fn serve(listener: TcpListener, session: Arc<Session>) -> io::Result<()> {
    info!(addr = %listener.local_addr()?, "SOCKS5 listener ready");
    loop {
        let (stream, client) = listener.accept().await?;
        let session = session.clone();
        tokio::spawn(async move {
            if let Err(error) = serve_client(stream, client, session).await {
                debug!(%client, %error, "SOCKS5 client ended");
            }
        });
    }
}

async fn serve_client(
    mut stream: TcpStream,
    client: SocketAddr,
    session: Arc<Session>,
) -> anyhow::Result<()> {
    negotiate_method(&mut stream).await?;

    // Request: VER, CMD, RSV, then the address the client claims it will send
    // UDP from.
    let mut head = [0u8; 3];
    stream.read_exact(&mut head).await?;
    if head[0] != VERSION {
        anyhow::bail!(ProtocolError::WrongVersion(head[0]));
    }
    let requested = read_address(&mut stream).await?;

    match head[1] {
        CMD_CONNECT => return serve_connect(stream, client, session, requested).await,
        CMD_UDP_ASSOCIATE => {}
        other => {
            debug!(%client, command = other, "refusing: unsupported SOCKS5 command");
            send_reply(&mut stream, REPLY_COMMAND_NOT_SUPPORTED, &unspecified_address()).await?;
            return Ok(());
        }
    }

    // Bind the relay on the same interface the control connection arrived on,
    // so a client that reached us can reach the relay too.
    let relay = UdpSocket::bind(SocketAddr::new(local_bind_ip(&stream)?, 0)).await?;
    let relay_addr = relay.local_addr()?;
    send_reply(&mut stream, REPLY_SUCCEEDED, &Address::Ip(relay_addr)).await?;

    // RFC 1928 lets the client name the address it will send from, or give
    // zeroes to mean "you will find out". Either way, only that one address is
    // served: an open UDP relay would forward for anyone who found the port.
    let expected = match requested {
        Address::Ip(addr) if !addr.ip().is_unspecified() && addr.port() != 0 => Some(addr),
        _ => None,
    };

    info!(%client, %relay_addr, "UDP association established");
    let outcome = run_association(Arc::new(relay), stream, session, expected).await;
    info!(%client, "UDP association closed");
    outcome
}

/// Handle a SOCKS5 `CONNECT`: open a TCP tunnel through the gateway and splice
/// it to the client's connection.
async fn serve_connect(
    mut stream: TcpStream,
    client: SocketAddr,
    session: Arc<Session>,
    requested: Address,
) -> anyhow::Result<()> {
    let target = match requested.to_target() {
        Ok(target) => target,
        Err(error) => {
            debug!(%client, %requested, %error, "invalid CONNECT target");
            send_reply(&mut stream, REPLY_GENERAL_FAILURE, &unspecified_address()).await?;
            return Ok(());
        }
    };

    let tunnel = match session.connect_tcp(target).await {
        Ok(tunnel) => tunnel,
        Err(error) => {
            let reply = connect_reply_for(&error);
            debug!(%client, %requested, %error, reply, "CONNECT refused by the gateway");
            send_reply(&mut stream, reply, &unspecified_address()).await?;
            return Ok(());
        }
    };

    // RFC 1928 BND.ADDR/BND.PORT is the proxy's own egress address; skimasque
    // does not expose the gateway's socket, so report the unspecified address.
    send_reply(&mut stream, REPLY_SUCCEEDED, &unspecified_address()).await?;

    info!(%client, %requested, "CONNECT tunnel established");
    let outcome = tunnel.relay(stream).await;
    info!(%client, %requested, "CONNECT tunnel closed");
    outcome.map_err(anyhow::Error::from)
}

/// Map a gateway rejection to the closest SOCKS5 reply code (RFC 1928, 6).
fn connect_reply_for(error: &skimasque::Error) -> u8 {
    let skimasque::Error::Rejected {
        status,
        proxy_status,
    } = error
    else {
        return REPLY_GENERAL_FAILURE;
    };
    if *status == http::StatusCode::FORBIDDEN {
        return REPLY_NOT_ALLOWED;
    }
    match proxy_status.as_deref() {
        Some(s) if s.contains("connection_refused") => REPLY_CONNECTION_REFUSED,
        Some(s) if s.contains("dns_error") => REPLY_HOST_UNREACHABLE,
        _ => REPLY_GENERAL_FAILURE,
    }
}

async fn negotiate_method(stream: &mut TcpStream) -> anyhow::Result<()> {
    let mut head = [0u8; 2];
    stream.read_exact(&mut head).await?;
    if head[0] != VERSION {
        anyhow::bail!(ProtocolError::WrongVersion(head[0]));
    }
    let mut methods = vec![0u8; usize::from(head[1])];
    stream.read_exact(&mut methods).await?;

    if methods.contains(&METHOD_NO_AUTH) {
        stream.write_all(&[VERSION, METHOD_NO_AUTH]).await?;
        Ok(())
    } else {
        stream.write_all(&[VERSION, METHOD_NONE_ACCEPTABLE]).await?;
        anyhow::bail!("client offered no acceptable authentication method")
    }
}

async fn read_address(stream: &mut TcpStream) -> anyhow::Result<Address> {
    // Read the address type first, since it determines how much follows.
    let mut atyp = [0u8; 1];
    stream.read_exact(&mut atyp).await?;
    let body_len = match atyp[0] {
        ATYP_IPV4 => 4 + 2,
        ATYP_IPV6 => 16 + 2,
        ATYP_DOMAIN => {
            let mut len = [0u8; 1];
            stream.read_exact(&mut len).await?;
            let mut rest = vec![0u8; usize::from(len[0]) + 2];
            stream.read_exact(&mut rest).await?;
            let mut buf = vec![atyp[0], len[0]];
            buf.extend_from_slice(&rest);
            return Ok(Address::decode(&mut &buf[..])?);
        }
        other => {
            send_reply(stream, REPLY_ADDRESS_TYPE_NOT_SUPPORTED, &unspecified_address()).await?;
            anyhow::bail!(ProtocolError::UnsupportedAddressType(other))
        }
    };
    let mut rest = vec![0u8; body_len];
    stream.read_exact(&mut rest).await?;
    let mut buf = vec![atyp[0]];
    buf.extend_from_slice(&rest);
    Ok(Address::decode(&mut &buf[..])?)
}

async fn send_reply(stream: &mut TcpStream, reply: u8, bound: &Address) -> io::Result<()> {
    let mut out = vec![VERSION, reply, 0x00];
    bound.encode_into(&mut out);
    stream.write_all(&out).await
}

fn unspecified_address() -> Address {
    Address::Ip(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0))
}

/// The address to bind the UDP relay on, matching the control connection.
fn local_bind_ip(stream: &TcpStream) -> io::Result<IpAddr> {
    Ok(stream.local_addr()?.ip())
}

/// Relay datagrams until the control connection closes.
///
/// RFC 1928 ties the association's lifetime to the TCP connection, so reading
/// it to EOF is how the client says it is finished; that also tears down every
/// tunnel opened on its behalf.
async fn run_association(
    relay: Arc<UdpSocket>,
    mut control: TcpStream,
    session: Arc<Session>,
    mut expected_client: Option<SocketAddr>,
) -> anyhow::Result<()> {
    let mut tunnels: HashMap<Address, mpsc::Sender<Bytes>> = HashMap::new();
    let mut buf = vec![0u8; MAX_UDP_PACKET];
    let mut control_buf = [0u8; 1];

    loop {
        tokio::select! {
            received = relay.recv_from(&mut buf) => {
                let (len, from) = received?;

                // Pin the association to the first sender, then ignore everyone
                // else, so the relay port is not a service for third parties.
                match expected_client {
                    Some(client) if client != from => {
                        trace!(%from, %client, "ignoring datagram from an unexpected source");
                        continue;
                    }
                    Some(_) => {}
                    None => {
                        debug!(%from, "pinning association to its client");
                        expected_client = Some(from);
                    }
                }

                if let Err(error) = forward(
                    &buf[..len],
                    from,
                    &relay,
                    &session,
                    &mut tunnels,
                ).await {
                    debug!(%error, "dropping datagram");
                }
            }

            // A read of the control connection completes only at EOF or error,
            // since a SOCKS5 client sends nothing more on it.
            result = control.read(&mut control_buf) => {
                match result {
                    Ok(0) => return Ok(()),
                    Ok(_) => trace!("ignoring unexpected data on the control connection"),
                    Err(error) => return Err(error.into()),
                }
            }
        }
    }
}

async fn forward(
    packet: &[u8],
    client: SocketAddr,
    relay: &Arc<UdpSocket>,
    session: &Arc<Session>,
    tunnels: &mut HashMap<Address, mpsc::Sender<Bytes>>,
) -> anyhow::Result<()> {
    let (frag, destination, payload) = decode_udp_packet(packet)?;
    if frag != 0 {
        // Reassembly is optional in RFC 1928 and essentially unused; accepting
        // fragments we cannot reassemble would corrupt the stream.
        anyhow::bail!("fragmented SOCKS5 datagrams are not supported");
    }

    if let Some(sender) = tunnels.get(&destination) {
        if sender.try_send(Bytes::copy_from_slice(payload)).is_ok() {
            return Ok(());
        }
        // The tunnel's task has gone; fall through and build a new one.
        tunnels.remove(&destination);
    }

    let target = destination.to_target()?;
    let tunnel = session.connect_udp(target).await?;
    debug!(%destination, stream_id = tunnel.stream_id(), "opened a tunnel");

    let (sender, receiver) = mpsc::channel(256);
    sender
        .try_send(Bytes::copy_from_slice(payload))
        .map_err(|_| anyhow::anyhow!("new tunnel queue was full"))?;
    tunnels.insert(destination.clone(), sender);

    tokio::spawn(pump(tunnel, receiver, relay.clone(), client, destination));
    Ok(())
}

/// Carry one destination's traffic between the SOCKS5 client and its tunnel.
async fn pump(
    mut tunnel: UdpTunnel,
    mut outbound: mpsc::Receiver<Bytes>,
    relay: Arc<UdpSocket>,
    client: SocketAddr,
    destination: Address,
) {
    loop {
        let step = timeout(IDLE_TIMEOUT, async {
            tokio::select! {
                payload = outbound.recv() => payload.map(Step::ToTarget),
                reply = tunnel.recv() => reply.map(Step::ToClient),
            }
        })
        .await;

        match step {
            Ok(Some(Step::ToTarget(payload))) => {
                if let Err(error) = tunnel.send(&payload) {
                    debug!(%destination, %error, "dropping outbound datagram");
                }
            }
            Ok(Some(Step::ToClient(reply))) => {
                let packet = encode_udp_packet(&destination, &reply);
                if let Err(error) = relay.send_to(&packet, client).await {
                    debug!(%destination, %error, "could not reach the SOCKS5 client");
                    break;
                }
            }
            // Either side closed.
            Ok(None) => break,
            Err(_) => {
                debug!(%destination, "tunnel idle; closing");
                break;
            }
        }
    }
    if let Err(error) = tunnel.close().await {
        trace!(%destination, %error, "closing the tunnel");
    }
}

enum Step {
    ToTarget(Bytes),
    ToClient(Bytes),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_round_trip_through_the_wire_form() {
        for address in [
            Address::Ip("192.0.2.6:443".parse().unwrap()),
            Address::Ip("[2001:db8::42]:53".parse().unwrap()),
            Address::Domain("dns.example.com".to_owned(), 853),
        ] {
            let mut encoded = Vec::new();
            address.encode_into(&mut encoded);
            let mut cursor = &encoded[..];
            assert_eq!(Address::decode(&mut cursor).unwrap(), address);
            assert!(cursor.is_empty(), "decoder left {} bytes", cursor.len());
        }
    }

    /// RFC 1928, Section 7 layout, checked byte for byte.
    #[test]
    fn udp_packets_match_the_rfc1928_layout() {
        let packet = encode_udp_packet(&Address::Ip("192.0.2.6:53".parse().unwrap()), b"hi");
        assert_eq!(
            packet,
            vec![
                0x00, 0x00, // RSV
                0x00, // FRAG
                0x01, // ATYP = IPv4
                192, 0, 2, 6, // DST.ADDR
                0x00, 0x35, // DST.PORT = 53
                b'h', b'i',
            ]
        );

        let (frag, destination, payload) = decode_udp_packet(&packet).unwrap();
        assert_eq!(frag, 0);
        assert_eq!(destination, Address::Ip("192.0.2.6:53".parse().unwrap()));
        assert_eq!(payload, b"hi");
    }

    #[test]
    fn an_empty_udp_payload_is_still_a_valid_packet() {
        let address = Address::Domain("example.com".to_owned(), 53);
        let packet = encode_udp_packet(&address, b"");
        let (_, destination, payload) = decode_udp_packet(&packet).unwrap();
        assert_eq!(destination, address);
        assert!(payload.is_empty());
    }

    #[test]
    fn truncated_packets_are_rejected_rather_than_misread() {
        let packet = encode_udp_packet(&Address::Ip("192.0.2.6:53".parse().unwrap()), b"hi");
        for len in 0..packet.len() - 2 {
            assert!(
                decode_udp_packet(&packet[..len]).is_err(),
                "{len}-byte prefix should not decode"
            );
        }
    }

    #[test]
    fn unknown_address_types_are_reported() {
        let mut cursor = &[0x09u8, 1, 2, 3][..];
        assert!(matches!(
            Address::decode(&mut cursor),
            Err(ProtocolError::UnsupportedAddressType(0x09))
        ));
    }

    #[test]
    fn addresses_convert_to_the_targets_they_name() {
        assert_eq!(
            Address::Ip("192.0.2.6:443".parse().unwrap())
                .to_target()
                .unwrap()
                .to_string(),
            "192.0.2.6:443"
        );
        // A name is passed through untouched, so the proxy resolves it.
        assert_eq!(
            Address::Domain("dns.example.com".to_owned(), 853)
                .to_target()
                .unwrap()
                .host,
            TargetHost::Name("dns.example.com".to_owned())
        );
        // Port 0 is not a destination.
        assert!(Address::Domain("example.com".to_owned(), 0)
            .to_target()
            .is_err());
    }
}
