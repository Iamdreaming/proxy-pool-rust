//! HTTP proxy handler: CONNECT tunneling and forward proxying.

use crate::upstream::{UPSTREAM_CONNECT_TIMEOUT, connect_to_upstream_with_timeout};
use proxy_core::route_debug::{
    GatewayAttemptStatus, GatewayRouteProtocol, RouteExit, Upstream, UpstreamSelector,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Handle an HTTP CONNECT request.
pub async fn handle(
    mut stream: TcpStream,
    _client_addr: SocketAddr,
    selector: Arc<UpstreamSelector>,
) -> anyhow::Result<()> {
    // Read the CONNECT request line
    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await?;
    if n == 0 {
        return Ok(());
    }

    let request = String::from_utf8_lossy(&buf[..n]);

    // Parse the request line. A CONNECT request tunnels to host:port; any other
    // method carrying an absolute-form URI (e.g. `GET http://host/path`) is a
    // forward proxy request.
    let first_line = request.lines().next().unwrap_or("");
    if !first_line.starts_with("CONNECT ") {
        // Not a CONNECT request — treat it as a forward proxy request.
        return handle_forward(stream, &buf[..n], &request, selector).await;
    }

    let target = first_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("")
        .to_string();

    if target.is_empty() {
        let resp = "HTTP/1.1 400 Bad Request\r\n\r\n";
        stream.write_all(resp.as_bytes()).await?;
        return Ok(());
    }

    let host = target.split(':').next().unwrap_or(&target);
    let selection = selector.select_with_trace(host, "http").await;
    let metrics = selector.metrics();
    for candidate in &selection.upstream_candidates {
        if matches!(candidate.upstream, Upstream::NoProxy) {
            metrics.record(
                GatewayRouteProtocol::HttpConnect,
                RouteExit::NoProxy,
                GatewayAttemptStatus::Unavailable,
            );
            continue;
        }

        match connect_to_upstream_with_timeout(
            &candidate.upstream,
            &target,
            UPSTREAM_CONNECT_TIMEOUT,
        )
        .await
        {
            Ok(mut remote) => {
                metrics.record(
                    GatewayRouteProtocol::HttpConnect,
                    candidate.exit,
                    GatewayAttemptStatus::Success,
                );
                selector
                    .record_upstream_attempt(&candidate.upstream, GatewayAttemptStatus::Success)
                    .await;
                let resp = "HTTP/1.1 200 Connection Established\r\n\r\n";
                stream.write_all(resp.as_bytes()).await?;
                bidirectional_copy(stream, &mut remote).await;
                return Ok(());
            }
            Err(e) => {
                metrics.record(
                    GatewayRouteProtocol::HttpConnect,
                    candidate.exit,
                    GatewayAttemptStatus::Failure,
                );
                selector
                    .record_upstream_attempt(&candidate.upstream, GatewayAttemptStatus::Failure)
                    .await;
                tracing::warn!(
                    target = %target,
                    route_group = ?selection.decision.matched_group,
                    exit = ?candidate.exit,
                    detail = ?candidate.detail,
                    error = %e,
                    "HTTP CONNECT: upstream attempt failed"
                );
            }
        }
    }

    let resp = "HTTP/1.1 502 Bad Gateway\r\n\r\n";
    stream.write_all(resp.as_bytes()).await?;
    Ok(())
}

/// Handle an HTTP forward proxy request (absolute-form URI).
///
/// The client sent a request whose request-target is an absolute URI, e.g.
/// `GET http://example.com/path HTTP/1.1`. We rewrite it to origin-form
/// (`GET /path HTTP/1.1`), select an upstream proxy, connect through it, forward
/// the (possibly buffered) request, and relay the response back to the client.
/// If no upstream can be reached, a `502 Bad Gateway` is returned.
async fn handle_forward(
    mut client: TcpStream,
    raw: &[u8],
    request: &str,
    selector: Arc<UpstreamSelector>,
) -> anyhow::Result<()> {
    // Extract method, absolute URL, and HTTP version from the request line.
    let first_line = request.lines().next().unwrap_or("");
    let parts: Vec<&str> = first_line.splitn(3, ' ').collect();
    if parts.len() != 3 {
        let resp = "HTTP/1.1 400 Bad Request\r\n\r\n";
        client.write_all(resp.as_bytes()).await?;
        return Ok(());
    }

    let method = parts[0];
    let url = parts[1];
    let version = parts[2];

    // Only absolute-form URIs are valid for a forward proxy request.
    let (scheme, rest) = if let Some(rest) = url.strip_prefix("http://") {
        ("http", rest)
    } else if let Some(rest) = url.strip_prefix("https://") {
        ("https", rest)
    } else {
        let resp = "HTTP/1.1 400 Bad Request\r\n\r\n";
        client.write_all(resp.as_bytes()).await?;
        return Ok(());
    };

    // Split the authority from the path component.
    let (host_port, path) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, "/"),
    };
    let (host, port) = parse_host_port(host_port, scheme);
    let target = format!("{host}:{port}");

    // Rewrite the request to origin-form, fixing the Host header and dropping
    // Proxy-Connection (hop-by-hop, not meaningful upstream).
    let mut rewritten = format!("{method} {path} {version}\r\n");
    let mut has_host = false;
    for line in request.lines().skip(1).take_while(|l| !l.is_empty()) {
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("host:") {
            has_host = true;
            rewritten.push_str(&format!("Host: {host}\r\n"));
            continue;
        }
        if lower.starts_with("proxy-connection:") {
            continue;
        }
        rewritten.push_str(line);
        rewritten.push_str("\r\n");
    }
    if !has_host {
        rewritten.push_str(&format!("Host: {host}\r\n"));
    }
    rewritten.push_str("\r\n");

    // Any request body captured in the initial read must be forwarded too.
    let header_end = request.find("\r\n\r\n").map(|i| i + 4).unwrap_or(raw.len());
    let body_in_buf = &raw[header_end.min(raw.len())..];

    let selection = selector.select_with_trace(host, "http").await;
    let metrics = selector.metrics();

    for candidate in &selection.upstream_candidates {
        if matches!(candidate.upstream, Upstream::NoProxy) {
            metrics.record(
                GatewayRouteProtocol::HttpConnect,
                RouteExit::NoProxy,
                GatewayAttemptStatus::Unavailable,
            );
            continue;
        }

        match connect_to_upstream_with_timeout(
            &candidate.upstream,
            &target,
            UPSTREAM_CONNECT_TIMEOUT,
        )
        .await
        {
            Ok(mut remote) => {
                metrics.record(
                    GatewayRouteProtocol::HttpConnect,
                    candidate.exit,
                    GatewayAttemptStatus::Success,
                );
                selector
                    .record_upstream_attempt(&candidate.upstream, GatewayAttemptStatus::Success)
                    .await;

                if let Err(e) = remote.write_all(rewritten.as_bytes()).await {
                    tracing::warn!("forward proxy: failed to send request to upstream: {e}");
                    continue;
                }
                if !body_in_buf.is_empty()
                    && let Err(e) = remote.write_all(body_in_buf).await
                {
                    tracing::warn!("forward proxy: failed to forward body to upstream: {e}");
                    continue;
                }

                bidirectional_copy(client, &mut remote).await;
                return Ok(());
            }
            Err(e) => {
                metrics.record(
                    GatewayRouteProtocol::HttpConnect,
                    candidate.exit,
                    GatewayAttemptStatus::Failure,
                );
                selector
                    .record_upstream_attempt(&candidate.upstream, GatewayAttemptStatus::Failure)
                    .await;
                tracing::warn!(
                    target = %target,
                    route_group = ?selection.decision.matched_group,
                    exit = ?candidate.exit,
                    detail = ?candidate.detail,
                    error = %e,
                    "HTTP forward proxy: upstream attempt failed"
                );
            }
        }
    }

    let resp = "HTTP/1.1 502 Bad Gateway\r\n\r\n";
    client.write_all(resp.as_bytes()).await?;
    Ok(())
}

/// Split an authority (`host[:port]`) into host and port, defaulting the port
/// from the URI scheme when omitted. Supports bracketed IPv6 literals.
fn parse_host_port<'a>(host_port: &'a str, scheme: &str) -> (&'a str, u16) {
    let default_port: u16 = if scheme == "https" { 443 } else { 80 };
    if host_port.starts_with('[') {
        // IPv6 literal: [addr]:port
        if let Some(bracket_end) = host_port.find(']') {
            let host = &host_port[1..bracket_end];
            let port = host_port
                .get(bracket_end + 2..)
                .and_then(|s| s.parse::<u16>().ok())
                .unwrap_or(default_port);
            return (host, port);
        }
    }
    match host_port.rsplit_once(':') {
        Some((h, port_str)) => {
            if let Ok(p) = port_str.parse::<u16>() {
                (h, p)
            } else {
                (host_port, default_port)
            }
        }
        None => (host_port, default_port),
    }
}

/// Bidirectional copy between client stream and remote stream.
///
/// Uses `copy_bidirectional`, which propagates half-close correctly: when one
/// side reaches EOF it shuts down the corresponding write half and keeps
/// relaying the other direction until it also finishes, instead of tearing the
/// whole tunnel down on the first EOF (which truncated responses when a client
/// closed its write side early).
async fn bidirectional_copy(mut stream: TcpStream, remote: &mut TcpStream) {
    match tokio::io::copy_bidirectional(&mut stream, remote).await {
        Ok((c2s, s2c)) => {
            tracing::debug!("tunnel closed: client→server {c2s}B, server→client {s2c}B");
        }
        Err(e) => tracing::debug!("tunnel copy error: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_host_port_default_http() {
        let (host, port) = parse_host_port("example.com", "http");
        assert_eq!(host, "example.com");
        assert_eq!(port, 80);
    }

    #[test]
    fn test_parse_host_port_default_https() {
        let (host, port) = parse_host_port("example.com", "https");
        assert_eq!(host, "example.com");
        assert_eq!(port, 443);
    }

    #[test]
    fn test_parse_host_port_explicit_port() {
        let (host, port) = parse_host_port("example.com:8080", "http");
        assert_eq!(host, "example.com");
        assert_eq!(port, 8080);
    }

    #[test]
    fn test_parse_host_port_ipv6_with_port() {
        let (host, port) = parse_host_port("[::1]:9000", "http");
        assert_eq!(host, "::1");
        assert_eq!(port, 9000);
    }

    #[test]
    fn test_parse_host_port_ipv6_default_port() {
        let (host, port) = parse_host_port("[::1]", "https");
        assert_eq!(host, "::1");
        assert_eq!(port, 443);
    }
}
