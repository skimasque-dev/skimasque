use skimasque_cli::proxy::Listeners;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;

#[test]
fn capabilities_require_no_gateway_or_credentials() {
    let output = Command::new(env!("CARGO_BIN_EXE_skimasque-client"))
        .args(["capabilities", "--json"])
        .env_remove("SKIMASQUE_TOKEN")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema"], 1);
    assert_eq!(
        value["features"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "proxy-tun-v1"),
        cfg!(target_os = "linux")
    );
    for required in [
        "proxy-http",
        "proxy-connect",
        "proxy-socks5",
        "proxy-socks5-udp",
        "proxy-ready-v1",
    ] {
        assert!(value["features"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == required));
    }
}

#[tokio::test]
async fn binding_either_occupied_port_releases_the_other_listener() {
    let occupied = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = occupied.local_addr().unwrap();
    assert!(Listeners::bind(addr, "127.0.0.1:0".parse().unwrap())
        .await
        .is_err());
    let free = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let free_addr = free.local_addr().unwrap();
    drop(free);
    assert!(Listeners::bind(free_addr, addr).await.is_err());
    assert!(TcpListener::bind(free_addr).await.is_ok());
}

#[tokio::test]
async fn proxy_listeners_report_actual_ports_and_refuse_non_loopback_addresses() {
    let listeners = Listeners::bind(
        "127.0.0.1:0".parse().unwrap(),
        "127.0.0.1:0".parse().unwrap(),
    )
    .await
    .unwrap();
    let (http, socks) = listeners.addresses().unwrap();
    assert_ne!(http.port(), 0);
    assert_ne!(socks.port(), 0);
    assert_ne!(http, socks);
    assert!(Listeners::bind("0.0.0.0:0".parse().unwrap(), socks)
        .await
        .is_err());
}

#[tokio::test]
async fn readiness_is_atomic_and_does_not_replace_an_existing_file() {
    let dir = std::env::temp_dir().join(format!("skimasque-ready-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("ready.json");
    let _ = std::fs::remove_file(&path);
    let listeners = Listeners::bind(
        "127.0.0.1:0".parse().unwrap(),
        "127.0.0.1:0".parse().unwrap(),
    )
    .await
    .unwrap();
    listeners
        .write_ready(&path, "192.0.2.10:443".parse().unwrap())
        .await
        .unwrap();
    let record: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(record["gateway"], "192.0.2.10:443");
    assert_eq!(record["pid"], std::process::id());
    assert!(listeners
        .write_ready(&path, "192.0.2.11:443".parse().unwrap())
        .await
        .is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn gateway_closure_terminates_both_frontends() {
    use skimasque::{
        client::Client,
        policy::AddressPolicy,
        server::{ProxyConfig, Server},
        service::{Dispatch, TcpProxy},
        tls,
    };
    let generated = tls::generate_self_signed(vec!["localhost".into()]).unwrap();
    let server = Server::bind(
        "127.0.0.1:0".parse().unwrap(),
        tls::server_config_from_pem(
            generated.certificate_pem.as_bytes(),
            generated.key_pem.as_bytes(),
        )
        .unwrap(),
        Dispatch::new().with_tcp(TcpProxy::new(AddressPolicy::permissive())),
        ProxyConfig::new("localhost").unwrap(),
    )
    .unwrap();
    let addr = server.local_addr().unwrap();
    let task = tokio::spawn(server.run());
    let client =
        Client::new(tls::client_config_with_ca(generated.certificate_pem.as_bytes()).unwrap())
            .unwrap();
    let session = client
        .connect(
            addr,
            skimasque_core::UriTemplate::default_connect_udp(&format!("localhost:{}", addr.port()))
                .unwrap(),
        )
        .await
        .unwrap();
    let listeners = Listeners::bind(
        "127.0.0.1:0".parse().unwrap(),
        "127.0.0.1:0".parse().unwrap(),
    )
    .await
    .unwrap();
    let relay = tokio::spawn(listeners.serve(Arc::new(session)));
    task.abort();
    assert!(tokio::time::timeout(Duration::from_secs(10), relay)
        .await
        .unwrap()
        .unwrap()
        .is_err());
}

#[test]
fn native_option_has_explicit_platform_contract() {
    let output = Command::new(env!("CARGO_BIN_EXE_skimasque-client"))
        .args(["proxy", "--tun-interface", "../bad"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(if cfg!(target_os = "linux") {
            "invalid TUN interface"
        } else {
            "native TUN requires Linux"
        })
    );
}
