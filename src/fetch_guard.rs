//! SSRF guard: URL validation, redirect-safe HTTP clients, and DNS-pinned
//! fetches.
//!
//! ADR 0056 moved this module out of `proof.rs`. The proof flow that used to
//! call these functions for a public-request RSS fetch is gone, but the
//! guard itself stays: sync registration uses it to reject a private or
//! reserved peer URL, and ADR 0054 uses it for the crawler's conditional
//! fetch. Every function here keeps the body it had in `proof.rs`.

use std::net::ToSocketAddrs;

// ── SSRF guard — 2026-03-13 ──────────────────────────────────────────────

/// Maximum number of redirects the SSRF-safe proof fetch client will follow.
// Issue-SSRF-REDIRECT — 2026-03-15
const MAX_SSRF_REDIRECTS: usize = 3;

// Issue-DNS-REBIND — 2026-03-16

/// Maximum total timeout for the entire redirect chain (seconds).
const REDIRECT_CHAIN_TIMEOUT_SECS: u64 = 30;

/// Validates that `feed_url` is safe to fetch (no SSRF).
///
/// Rejects:
/// - Non-HTTP(S) schemes (`file://`, `ftp://`, etc.)
/// - Hostnames that resolve to private/reserved IP ranges
/// - Literal private/reserved IP addresses in the hostname
///
/// Returns the list of resolved `SocketAddr`s on success, enabling the caller
/// to pin DNS (prevent rebinding between validation and fetch).
///
/// # Errors
///
/// Returns a human-readable error string if the URL is rejected.
// Issue-SSRF-REDIRECT — 2026-03-15: now returns resolved addresses for DNS pinning
pub fn validate_feed_url(feed_url: &str) -> Result<Vec<std::net::SocketAddr>, String> {
    let url = url::Url::parse(feed_url).map_err(|e| format!("invalid feed URL: {e}"))?;

    // Scheme check: only http and https are allowed.
    match url.scheme() {
        "http" | "https" => {}
        scheme => return Err(format!("disallowed URL scheme: {scheme}")),
    }

    let host = url
        .host_str()
        .ok_or_else(|| "feed URL has no host".to_string())?;

    // Check literal IP addresses against private/reserved ranges.
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        if is_private_ip(ip) {
            return Err(format!("feed URL targets a private/reserved IP: {ip}"));
        }
        let port = url.port_or_known_default().unwrap_or(443);
        return Ok(vec![std::net::SocketAddr::new(ip, port)]);
    }

    // Hostname: resolve and check all addresses.
    // Use std::net::ToSocketAddrs for synchronous DNS resolution (acceptable
    // because this runs before the async fetch and is fast for cached lookups).
    let socket_addr = format!("{host}:{}", url.port_or_known_default().unwrap_or(443));
    let resolved: Vec<std::net::SocketAddr> = socket_addr
        .to_socket_addrs()
        .map_err(|e| format!("DNS resolution failed for {host}: {e}"))?
        .collect();

    if resolved.is_empty() {
        return Err(format!("DNS resolution returned no addresses for {host}"));
    }

    for addr in &resolved {
        if is_private_ip(addr.ip()) {
            return Err(format!(
                "feed URL hostname resolves to private/reserved IP: {}",
                addr.ip()
            ));
        }
    }
    Ok(resolved)
}

// Issue-SSRF-REDIRECT — 2026-03-15

/// Check whether a URL is safe from an SSRF perspective (synchronous).
///
/// Used by the custom redirect policy to re-validate each hop in a redirect
/// chain, and by node-URL registration to reject private/loopback targets.
/// Checks scheme and any literal IP in the hostname. For hostnames, performs
/// synchronous DNS resolution and rejects any address in a private range.
// Issue-SYNC-SSRF — 2026-03-16
#[must_use]
pub fn is_url_ssrf_safe(url: &url::Url) -> bool {
    // Only HTTP(S) schemes are allowed.
    match url.scheme() {
        "http" | "https" => {}
        _ => return false,
    }

    let Some(host) = url.host_str() else {
        return false;
    };

    // If the host is a literal IP address, check against private ranges.
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return !is_private_ip(ip);
    }

    // Hostname-based redirect: resolve synchronously and check all addresses.
    // This is acceptable because redirect targets are rare and DNS lookups for
    // cached entries are fast (~1ms). The blocking DNS is already used in
    // validate_feed_url which runs inside spawn_blocking.
    let port = url.port_or_known_default().unwrap_or(443);
    let socket_addr = format!("{host}:{port}");
    let Ok(addrs_iter) = socket_addr.to_socket_addrs() else {
        return false;
    };

    let addrs: Vec<_> = addrs_iter.collect();
    if addrs.is_empty() {
        return false;
    }

    for addr in addrs {
        if is_private_ip(addr.ip()) {
            return false;
        }
    }
    true
}

/// Build a `reqwest::Client` with SSRF-safe redirect policy.
///
/// This client:
/// - Follows at most `MAX_SSRF_REDIRECTS` redirects
/// - Re-validates each redirect target against the SSRF guard (scheme + IP check)
/// - Has a 10-second total timeout
///
/// Use this for proof RSS fetches instead of the shared `push_client`.
///
/// # Panics
///
/// Panics if the reqwest client builder fails, which cannot happen with the
/// options used here (no custom TLS roots, no invalid configuration).
#[must_use]
pub fn build_ssrf_safe_client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            // Enforce maximum redirect depth.
            if attempt.previous().len() >= MAX_SSRF_REDIRECTS {
                return attempt.error(SsrfRedirectError::TooManyRedirects);
            }
            let target = attempt.url().clone();
            // Re-run SSRF guard on the redirect target.
            if is_url_ssrf_safe(&target) {
                attempt.follow()
            } else {
                attempt.error(SsrfRedirectError::PrivateAddress(target.to_string()))
            }
        }))
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("SSRF-safe reqwest client builder uses only safe options")
}

/// Build a `reqwest::Client` with SSRF-safe redirect policy **and** DNS
/// pinning for a specific hostname.
///
/// The `resolved_addrs` from `validate_feed_url` are pinned to `hostname`,
/// preventing DNS rebinding between validation and fetch.
///
/// # Panics
///
/// Panics if the reqwest client builder fails, which cannot happen with the
/// options used here (no custom TLS roots, no invalid configuration).
#[must_use]
pub fn build_ssrf_safe_client_pinned(
    hostname: &str,
    resolved_addrs: &[std::net::SocketAddr],
) -> reqwest::Client {
    let mut builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= MAX_SSRF_REDIRECTS {
                return attempt.error(SsrfRedirectError::TooManyRedirects);
            }
            let target = attempt.url().clone();
            if is_url_ssrf_safe(&target) {
                attempt.follow()
            } else {
                attempt.error(SsrfRedirectError::PrivateAddress(target.to_string()))
            }
        }))
        .timeout(std::time::Duration::from_secs(10));

    // Pin DNS for the original hostname to prevent rebinding.
    for addr in resolved_addrs {
        builder = builder.resolve(hostname, *addr);
    }

    builder
        .build()
        .expect("SSRF-safe pinned reqwest client builder uses only safe options")
}

// Issue-DNS-REBIND — 2026-03-16

/// Resolve a URL's hostname and validate all IPs against SSRF rules.
///
/// Returns the hostname and resolved socket addresses. For literal IP hosts,
/// returns the IP wrapped in a `SocketAddr`. Rejects private/reserved IPs.
///
/// # Errors
///
/// Returns a human-readable error string if the URL has no host, uses a
/// disallowed scheme, or resolves to a private/reserved IP.
pub fn resolve_and_validate_url(
    url: &url::Url,
) -> Result<(String, Vec<std::net::SocketAddr>), String> {
    match url.scheme() {
        "http" | "https" => {}
        scheme => return Err(format!("disallowed URL scheme: {scheme}")),
    }

    let host = url
        .host_str()
        .ok_or_else(|| "URL has no host".to_string())?
        .to_string();

    let port = url.port_or_known_default().unwrap_or(443);

    // Literal IP address: validate directly.
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        if is_private_ip(ip) {
            return Err(format!("URL targets a private/reserved IP: {ip}"));
        }
        return Ok((host, vec![std::net::SocketAddr::new(ip, port)]));
    }

    // Hostname: resolve and validate all addresses.
    let socket_addr = format!("{host}:{port}");
    let addrs: Vec<std::net::SocketAddr> = socket_addr
        .to_socket_addrs()
        .map_err(|e| format!("DNS resolution failed for {host}: {e}"))?
        .collect();

    if addrs.is_empty() {
        return Err(format!("DNS resolution returned no addresses for {host}"));
    }

    for addr in &addrs {
        if is_private_ip(addr.ip()) {
            return Err(format!(
                "URL hostname resolves to private/reserved IP: {}",
                addr.ip()
            ));
        }
    }

    Ok((host, addrs))
}

/// Build a `reqwest::Client` with redirects disabled and DNS pinned for a hostname.
///
/// Used by `fetch_with_pinned_redirects` to make a single-hop request where
/// the DNS resolution is locked to the validated addresses.
fn build_no_redirect_pinned_client(
    hostname: &str,
    resolved_addrs: &[std::net::SocketAddr],
) -> reqwest::Client {
    let mut builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(10));

    for addr in resolved_addrs {
        builder = builder.resolve(hostname, *addr);
    }

    builder
        .build()
        .expect("no-redirect pinned reqwest client builder uses only safe options")
}

/// Fetches a URL following redirects manually, pinning DNS at each hop.
///
/// Eliminates the DNS-rebinding TOCTOU hole: the DNS resolution used for SSRF
/// validation is the **same** resolution used for the actual TCP connection,
/// because we pin each hop's addresses via `reqwest::ClientBuilder::resolve`.
///
/// The caller provides the pre-validated initial addresses (from `validate_feed_url`
/// or `resolve_and_validate_url`), so the first hop is already pinned. Each
/// subsequent redirect target is resolved, validated, and pinned before the
/// request is sent.
///
/// # Errors
///
/// Returns a human-readable error string if:
/// - A redirect target fails SSRF validation
/// - The redirect chain exceeds `max_redirects`
/// - Any individual request fails
/// - The response body exceeds `max_body_bytes`
pub async fn fetch_with_pinned_redirects(
    initial_url: &str,
    initial_hostname: &str,
    initial_addrs: &[std::net::SocketAddr],
    max_redirects: usize,
    max_body_bytes: usize,
) -> Result<String, String> {
    use futures_util::StreamExt;
    use std::time::{Duration, Instant};

    let deadline = Instant::now() + Duration::from_secs(REDIRECT_CHAIN_TIMEOUT_SECS);
    let mut current_url = initial_url.to_string();
    let mut current_hostname = initial_hostname.to_string();
    let mut current_addrs = initial_addrs.to_vec();

    for hop in 0..=max_redirects {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .unwrap_or(Duration::ZERO);
        if remaining.is_zero() {
            return Err("redirect chain timed out".to_string());
        }

        let client = build_no_redirect_pinned_client(&current_hostname, &current_addrs);

        let resp = client
            .get(&current_url)
            .timeout(remaining.min(Duration::from_secs(10)))
            .send()
            .await
            .map_err(|e| format!("RSS fetch failed: {e}"))?;

        let status = resp.status();

        // Not a redirect -- read the body and return.
        if !status.is_redirection() {
            if !status.is_success() {
                return Err(format!("RSS fetch returned HTTP {status}"));
            }

            // Check Content-Length header before reading.
            if let Some(cl) = resp.content_length()
                && cl > max_body_bytes as u64
            {
                return Err(format!(
                    "RSS response too large: {cl} bytes (limit: {max_body_bytes})"
                ));
            }

            let mut body_bytes = Vec::new();
            let mut stream = resp.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|e| format!("RSS fetch error: {e}"))?;
                body_bytes.extend_from_slice(&chunk);
                if body_bytes.len() > max_body_bytes {
                    return Err(format!("RSS response exceeds {max_body_bytes} bytes"));
                }
            }

            return String::from_utf8(body_bytes)
                .map_err(|e| format!("RSS response is not valid UTF-8: {e}"));
        }

        // 3xx redirect: extract Location, resolve, validate, and pin.
        if hop == max_redirects {
            return Err(format!("too many redirects (max {max_redirects})"));
        }

        let location = resp
            .headers()
            .get(reqwest::header::LOCATION)
            .ok_or_else(|| "redirect response missing Location header".to_string())?
            .to_str()
            .map_err(|e| format!("invalid Location header: {e}"))?;

        // Resolve relative URLs against the current URL.
        let base =
            url::Url::parse(&current_url).map_err(|e| format!("invalid current URL: {e}"))?;
        let next_url = base
            .join(location)
            .map_err(|e| format!("invalid redirect Location: {e}"))?;

        // Validate and resolve DNS for the redirect target, pinning the result.
        let (next_hostname, next_addrs) = resolve_and_validate_url(&next_url)
            .map_err(|e| format!("redirect to {next_url} blocked: {e}"))?;

        current_url = next_url.to_string();
        current_hostname = next_hostname;
        current_addrs = next_addrs;
    }

    Err(format!("too many redirects (max {max_redirects})"))
}

/// Error type for SSRF redirect policy violations.
// Issue-SSRF-REDIRECT — 2026-03-15
#[derive(Debug)]
enum SsrfRedirectError {
    /// Redirect target resolves to or is a private/reserved IP address.
    PrivateAddress(String),
    /// Too many redirects (exceeds `MAX_SSRF_REDIRECTS`).
    TooManyRedirects,
}

impl std::fmt::Display for SsrfRedirectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PrivateAddress(url) => {
                write!(f, "redirect to private/reserved address blocked: {url}")
            }
            Self::TooManyRedirects => {
                write!(f, "too many redirects (max {MAX_SSRF_REDIRECTS})")
            }
        }
    }
}

impl std::error::Error for SsrfRedirectError {}

// Issue-SYNC-SSRF — 2026-03-16

/// Validates that `node_url` is safe for peer push registration.
///
/// Rejects:
/// - Non-HTTP(S) schemes (`file://`, `ftp://`, etc.)
/// - Hostnames that resolve to private/reserved IP ranges
/// - Literal private/reserved IP addresses in the hostname
///
/// # Errors
///
/// Returns a human-readable error string if the URL is rejected.
pub fn validate_node_url(node_url: &str) -> Result<(), String> {
    let url = url::Url::parse(node_url).map_err(|e| format!("invalid node URL: {e}"))?;

    if !is_url_ssrf_safe(&url) {
        return Err(format!(
            "node URL rejected: targets a private/reserved address or uses a disallowed scheme: {node_url}"
        ));
    }

    Ok(())
}

/// Returns `true` if the IP address is in a private or reserved range
/// that should not be reachable via SSRF.
const fn is_private_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            v4.is_loopback()           // 127.0.0.0/8
                || v4.is_private()     // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
                || v4.is_link_local()  // 169.254.0.0/16
                || v4.is_broadcast()   // 255.255.255.255
                || v4.is_unspecified() // 0.0.0.0
                || v4.octets()[0] == 100 && (v4.octets()[1] & 0xC0) == 64 // 100.64.0.0/10 (CGNAT)
        }
        std::net::IpAddr::V6(v6) => {
            v6.is_loopback()           // ::1
                || v6.is_unspecified() // ::
                // fc00::/7 (unique local addresses)
                || (v6.segments()[0] & 0xFE00) == 0xFC00
                // fe80::/10 (link-local)
                || (v6.segments()[0] & 0xFFC0) == 0xFE80
        }
    }
}

// fetch_guard.rs security compliant — 2026-03-13
