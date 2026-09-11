//! The gateway's operational endpoints: `/healthz`, `/readyz`, `/metrics`.
//!
//! A small `hyper` HTTP/1.1 listener, separate from the QUIC data plane, for a
//! load balancer's health checks and a Prometheus scrape. `skimasque-server`
//! starts it when `--metrics-listen` is given.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::Context;
use http_body_util::Full;
use hyper::body::Bytes;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use tokio::net::TcpListener;

/// Install the process-wide Prometheus recorder and register the gateway's
/// series. Call once, before the first `metrics::*` call that should be
/// captured.
pub fn install_metrics() -> anyhow::Result<PrometheusHandle> {
    let handle = PrometheusBuilder::new()
        .install_recorder()
        .context("installing the Prometheus metrics recorder")?;
    skimasque::metrics::describe();
    Ok(handle)
}

/// A readiness latch. `/readyz` answers `200` once [`mark_ready`](Self::mark_ready)
/// has been called, `503` before.
#[derive(Clone, Default)]
pub struct Ready(Arc<AtomicBool>);

impl Ready {
    pub fn new() -> Self {
        Self::default()
    }

    /// The gateway is bound and its policy (if any) is loaded.
    pub fn mark_ready(&self) {
        self.0.store(true, Ordering::Release);
    }

    fn is_ready(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Serve the ops endpoints on `addr` until the process exits.
pub async fn serve(addr: SocketAddr, metrics: PrometheusHandle, ready: Ready) -> anyhow::Result<()> {
    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("binding the ops listener on {addr}"))?;

    loop {
        let (stream, _) = match listener.accept().await {
            Ok(pair) => pair,
            Err(error) => {
                tracing::debug!(%error, "ops listener accept failed");
                continue;
            }
        };
        let metrics = metrics.clone();
        let ready = ready.clone();
        tokio::spawn(async move {
            let io = TokioIo::new(stream);
            let handler = service_fn(move |request| {
                let response = route(request, &metrics, &ready);
                async move { Ok::<_, Infallible>(response) }
            });
            if let Err(error) = hyper::server::conn::http1::Builder::new()
                .serve_connection(io, handler)
                .await
            {
                tracing::debug!(%error, "ops connection ended");
            }
        });
    }
}

fn route<B>(request: Request<B>, metrics: &PrometheusHandle, ready: &Ready) -> Response<Full<Bytes>> {
    if request.method() != Method::GET {
        return text(StatusCode::METHOD_NOT_ALLOWED, "GET only\n");
    }
    match request.uri().path() {
        "/healthz" => text(StatusCode::OK, "ok\n"),
        "/readyz" if ready.is_ready() => text(StatusCode::OK, "ready\n"),
        "/readyz" => text(StatusCode::SERVICE_UNAVAILABLE, "not ready\n"),
        "/metrics" => Response::builder()
            .status(StatusCode::OK)
            .header(hyper::header::CONTENT_TYPE, "text/plain; version=0.0.4")
            .body(Full::new(Bytes::from(metrics.render())))
            .expect("a well-formed metrics response"),
        _ => text(StatusCode::NOT_FOUND, "not found\n"),
    }
}

fn text(status: StatusCode, body: &'static str) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header(hyper::header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Full::new(Bytes::from_static(body.as_bytes())))
        .expect("a well-formed text response")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readyz_flips_once_marked() {
        let ready = Ready::new();
        assert!(!ready.is_ready());
        ready.mark_ready();
        assert!(ready.is_ready());
    }

    fn get(path: &str, metrics: &PrometheusHandle, ready: &Ready) -> Response<Full<Bytes>> {
        route(
            Request::builder().uri(path).body(()).unwrap(),
            metrics,
            ready,
        )
    }

    #[test]
    fn the_routes_answer_the_expected_status_codes() {
        // A local recorder just for the test; `install_recorder` is process-wide
        // and would clash with other tests, so build a handle directly.
        let handle = PrometheusBuilder::new().build_recorder().handle();
        let ready = Ready::new();

        assert_eq!(get("/healthz", &handle, &ready).status(), StatusCode::OK);
        assert_eq!(
            get("/readyz", &handle, &ready).status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        ready.mark_ready();
        assert_eq!(get("/readyz", &handle, &ready).status(), StatusCode::OK);
        assert_eq!(get("/metrics", &handle, &ready).status(), StatusCode::OK);
        assert_eq!(get("/nope", &handle, &ready).status(), StatusCode::NOT_FOUND);
    }
}
