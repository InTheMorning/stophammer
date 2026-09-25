// Issue-SSRF-REDIRECT — 2026-03-15
// Issue-DNS-REBIND — 2026-03-16
// Tests for the SSRF guard in `stophammer::fetch_guard`: URL validation,
// the DNS-pinned redirect loop, and the fetch body-size limit.
//
// ADR 0056 removed the proof flow. `verify_podcast_txt` and
// `verify_podcast_txt_pinned` are gone with it, along with the tests that
// drove the guard through those two functions. The guard functions
// themselves stay, moved to `fetch_guard`, and keep the tests below that
// call them directly.

mod common;

use std::net::SocketAddr;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Extract the hostname and socket address from a wiremock `MockServer` URI.
///
/// Wiremock binds to `127.0.0.1:<port>`, so this returns `("127.0.0.1", [addr])`.
fn mock_server_host_and_addrs(server: &MockServer) -> (String, Vec<SocketAddr>) {
    let uri = server.uri();
    let url = url::Url::parse(&uri).expect("mock server URI is a valid URL");
    let host = url.host_str().expect("mock server has a host").to_string();
    let port = url.port().expect("mock server has a port");
    let addr: SocketAddr = format!("{host}:{port}").parse().expect("valid socket addr");
    (host, vec![addr])
}

// ---------------------------------------------------------------------------
// SSRF-REDIRECT-08: validate_feed_url returns resolved addresses
// ---------------------------------------------------------------------------

#[test]
fn validate_feed_url_returns_resolved_addrs() {
    // A URL with a literal IP should return that IP in the resolved list.
    let result = stophammer::fetch_guard::validate_feed_url("https://1.1.1.1/feed.xml");
    assert!(result.is_ok(), "public IP should pass validation");
    let addrs = result.unwrap();
    assert!(
        !addrs.is_empty(),
        "resolved addresses should not be empty for a literal IP"
    );
}

// ---------------------------------------------------------------------------
// SSRF-REDIRECT-09: validate_feed_url rejects private IPs (unchanged behavior)
// ---------------------------------------------------------------------------

#[test]
fn validate_feed_url_still_rejects_private_ips() {
    let result = stophammer::fetch_guard::validate_feed_url("https://127.0.0.1/feed.xml");
    assert!(result.is_err(), "loopback should be rejected");

    let result = stophammer::fetch_guard::validate_feed_url("https://10.0.0.1/feed.xml");
    assert!(result.is_err(), "private 10.x should be rejected");

    let result = stophammer::fetch_guard::validate_feed_url("https://192.168.1.1/feed.xml");
    assert!(result.is_err(), "private 192.168.x should be rejected");

    let result = stophammer::fetch_guard::validate_feed_url("https://169.254.169.254/feed.xml");
    assert!(result.is_err(), "link-local should be rejected");
}

// ---------------------------------------------------------------------------
// DNS-REBIND-01: a pinned redirect to loopback is blocked
// ---------------------------------------------------------------------------

#[tokio::test]
async fn pinned_redirect_to_loopback_blocked() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(302).insert_header("Location", "http://127.0.0.1:9999/secret"),
        )
        .mount(&mock_server)
        .await;

    let (host, addrs) = mock_server_host_and_addrs(&mock_server);
    let result = stophammer::fetch_guard::fetch_with_pinned_redirects(
        &mock_server.uri(),
        &host,
        &addrs,
        3,
        5 * 1024 * 1024,
    )
    .await;

    assert!(
        result.is_err(),
        "a pinned redirect to 127.0.0.1 must be blocked, got: {result:?}"
    );
    let err = result.unwrap_err();
    assert!(
        err.contains("private") || err.contains("blocked") || err.contains("redirect"),
        "error should mention private/blocked/redirect, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// DNS-REBIND-02: a pinned redirect to link-local is blocked
// ---------------------------------------------------------------------------

#[tokio::test]
async fn pinned_redirect_to_link_local_blocked() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("Location", "http://169.254.169.254/latest/meta-data/"),
        )
        .mount(&mock_server)
        .await;

    let (host, addrs) = mock_server_host_and_addrs(&mock_server);
    let result = stophammer::fetch_guard::fetch_with_pinned_redirects(
        &mock_server.uri(),
        &host,
        &addrs,
        3,
        5 * 1024 * 1024,
    )
    .await;

    assert!(
        result.is_err(),
        "a pinned redirect to 169.254.169.254 must be blocked, got: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// DNS-REBIND-03: a pinned redirect to private 10.x is blocked
// ---------------------------------------------------------------------------

#[tokio::test]
async fn pinned_redirect_to_private_10_blocked() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(302).insert_header("Location", "http://10.0.0.1:8080/internal"),
        )
        .mount(&mock_server)
        .await;

    let (host, addrs) = mock_server_host_and_addrs(&mock_server);
    let result = stophammer::fetch_guard::fetch_with_pinned_redirects(
        &mock_server.uri(),
        &host,
        &addrs,
        3,
        5 * 1024 * 1024,
    )
    .await;

    assert!(
        result.is_err(),
        "a pinned redirect to 10.0.0.1 must be blocked, got: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// DNS-REBIND-04: a pinned redirect to private 192.168.x is blocked
// ---------------------------------------------------------------------------

#[tokio::test]
async fn pinned_redirect_to_private_192_blocked() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(302).insert_header("Location", "http://192.168.1.1/admin"),
        )
        .mount(&mock_server)
        .await;

    let (host, addrs) = mock_server_host_and_addrs(&mock_server);
    let result = stophammer::fetch_guard::fetch_with_pinned_redirects(
        &mock_server.uri(),
        &host,
        &addrs,
        3,
        5 * 1024 * 1024,
    )
    .await;

    assert!(
        result.is_err(),
        "a pinned redirect to 192.168.1.1 must be blocked, got: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// DNS-REBIND-04b: a pinned redirect to private 172.16.x is blocked
// ---------------------------------------------------------------------------

#[tokio::test]
async fn pinned_redirect_to_private_172_blocked() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(302).insert_header("Location", "http://172.16.0.1/internal"),
        )
        .mount(&mock_server)
        .await;

    let (host, addrs) = mock_server_host_and_addrs(&mock_server);
    let result = stophammer::fetch_guard::fetch_with_pinned_redirects(
        &mock_server.uri(),
        &host,
        &addrs,
        3,
        5 * 1024 * 1024,
    )
    .await;

    assert!(
        result.is_err(),
        "a pinned redirect to 172.16.0.1 must be blocked, got: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// DNS-REBIND-05: a pinned redirect to a file:// scheme is blocked
// ---------------------------------------------------------------------------

#[tokio::test]
async fn pinned_redirect_to_file_scheme_blocked() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(302).insert_header("Location", "file:///etc/passwd"))
        .mount(&mock_server)
        .await;

    let (host, addrs) = mock_server_host_and_addrs(&mock_server);
    let result = stophammer::fetch_guard::fetch_with_pinned_redirects(
        &mock_server.uri(),
        &host,
        &addrs,
        3,
        5 * 1024 * 1024,
    )
    .await;

    // The redirect to file:// is blocked because the scheme is not http/https.
    assert!(
        result.is_err(),
        "a pinned redirect to file:// must be blocked, got: {result:?}"
    );
    let err = result.unwrap_err();
    assert!(
        err.contains("scheme") || err.contains("blocked"),
        "error should mention disallowed scheme, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// DNS-REBIND-06: a pinned fetch with no redirect returns the RSS body
// ---------------------------------------------------------------------------

#[tokio::test]
async fn pinned_direct_fetch_works() {
    let mock_server = MockServer::start().await;

    let token_binding = "pinned-direct.hash";
    let rss = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:podcast="https://podcastindex.org/namespace/1.0">
  <channel>
    <title>Test Podcast</title>
    <podcast:txt>stophammer-proof {token_binding}</podcast:txt>
  </channel>
</rss>"#
    );

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string(rss.clone()))
        .mount(&mock_server)
        .await;

    let (host, addrs) = mock_server_host_and_addrs(&mock_server);
    let result = stophammer::fetch_guard::fetch_with_pinned_redirects(
        &mock_server.uri(),
        &host,
        &addrs,
        3,
        5 * 1024 * 1024,
    )
    .await;

    assert_eq!(
        result,
        Ok(rss),
        "a direct pinned fetch of a safe URL should succeed and return the body"
    );
}

// ---------------------------------------------------------------------------
// DNS-REBIND-07b: max redirects enforced with public redirect target
// This uses fetch_with_pinned_redirects directly to test the counter by
// redirecting to another path on the same server (which we pre-pin).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn pinned_max_redirects_enforced_non_private() {
    // Use fetch_with_pinned_redirects directly with max_redirects=0 to verify
    // that even a single redirect is rejected when the limit is 0.
    let mock_server = MockServer::start().await;
    let uri = mock_server.uri();

    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(302).insert_header("Location", &*format!("{uri}/other")),
        )
        .mount(&mock_server)
        .await;

    let (host, addrs) = mock_server_host_and_addrs(&mock_server);
    let result = stophammer::fetch_guard::fetch_with_pinned_redirects(
        &uri,
        &host,
        &addrs,
        0, // max_redirects = 0
        5 * 1024 * 1024,
    )
    .await;

    assert!(
        result.is_err(),
        "should fail with max_redirects=0 and a redirect, got: {result:?}"
    );
    let err = result.unwrap_err();
    assert!(
        err.contains("too many redirects"),
        "error should mention too many redirects, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// DNS-REBIND-09: resolve_and_validate_url rejects private IPs
// ---------------------------------------------------------------------------

#[test]
fn resolve_and_validate_url_rejects_private_ips() {
    let cases = [
        "http://127.0.0.1/feed.xml",
        "http://10.0.0.1/feed.xml",
        "http://192.168.1.1/feed.xml",
        "http://169.254.169.254/latest/meta-data/",
        "http://172.16.0.1/feed.xml",
    ];
    for url_str in &cases {
        let url = url::Url::parse(url_str).expect("valid URL");
        let result = stophammer::fetch_guard::resolve_and_validate_url(&url);
        assert!(
            result.is_err(),
            "resolve_and_validate_url should reject {url_str}, got: {result:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// DNS-REBIND-10: resolve_and_validate_url rejects non-http schemes
// ---------------------------------------------------------------------------

#[test]
fn resolve_and_validate_url_rejects_bad_schemes() {
    let cases = ["file:///etc/passwd", "ftp://example.com/feed.xml"];
    for url_str in &cases {
        let url = url::Url::parse(url_str).expect("valid URL");
        let result = stophammer::fetch_guard::resolve_and_validate_url(&url);
        assert!(
            result.is_err(),
            "resolve_and_validate_url should reject {url_str}, got: {result:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// DNS-REBIND-11: resolve_and_validate_url accepts public IPs
// ---------------------------------------------------------------------------

#[test]
fn resolve_and_validate_url_accepts_public_ips() {
    let url = url::Url::parse("https://1.1.1.1/feed.xml").expect("valid URL");
    let result = stophammer::fetch_guard::resolve_and_validate_url(&url);
    assert!(
        result.is_ok(),
        "resolve_and_validate_url should accept 1.1.1.1, got: {result:?}"
    );
    let (host, addrs) = result.unwrap();
    assert_eq!(host, "1.1.1.1");
    assert!(!addrs.is_empty());
}

// ---------------------------------------------------------------------------
// DNS-REBIND-12: fetch_with_pinned_redirects respects body size limit
// ---------------------------------------------------------------------------

#[tokio::test]
async fn pinned_fetch_rejects_oversized_body() {
    let mock_server = MockServer::start().await;

    // 1 KiB body, but we set max_body_bytes to 100.
    let big_body = "x".repeat(1024);
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string(big_body))
        .mount(&mock_server)
        .await;

    let (host, addrs) = mock_server_host_and_addrs(&mock_server);
    let result = stophammer::fetch_guard::fetch_with_pinned_redirects(
        &mock_server.uri(),
        &host,
        &addrs,
        3,
        100, // very small limit
    )
    .await;

    assert!(
        result.is_err(),
        "oversized body should be rejected, got: {result:?}"
    );
    let err = result.unwrap_err();
    assert!(
        err.contains("exceeds") || err.contains("too large"),
        "error should mention size limit, got: {err}"
    );
}
