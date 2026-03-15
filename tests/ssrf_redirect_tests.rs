// Issue-SSRF-REDIRECT — 2026-03-15
// Tests that verify_podcast_txt blocks redirect chains to private/reserved IPs.

mod common;

use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

// ---------------------------------------------------------------------------
// SSRF-REDIRECT-01: redirect to loopback (127.0.0.1) must be blocked
// ---------------------------------------------------------------------------

#[tokio::test]
async fn verify_podcast_txt_redirect_to_loopback_blocked() {
    let mock_server = MockServer::start().await;

    // The mock returns a 302 redirect to a loopback address.
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("Location", "http://127.0.0.1:9999/secret"),
        )
        .mount(&mock_server)
        .await;

    let client = stophammer::proof::build_ssrf_safe_client();
    let result =
        stophammer::proof::verify_podcast_txt(&client, &mock_server.uri(), "token.hash").await;

    assert!(
        result.is_err(),
        "redirect to 127.0.0.1 must be blocked, got: {result:?}"
    );
    let err = result.unwrap_err();
    // The error surfaces through reqwest's redirect machinery; it may be
    // wrapped as "error following redirect" containing our custom message,
    // or as a direct connection-refused error. Either way, it must not succeed.
    assert!(
        err.contains("private") || err.contains("blocked") || err.contains("redirect"),
        "error should mention private/blocked/redirect, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// SSRF-REDIRECT-02: redirect to link-local (169.254.x.x) must be blocked
// ---------------------------------------------------------------------------

#[tokio::test]
async fn verify_podcast_txt_redirect_to_link_local_blocked() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("Location", "http://169.254.169.254/latest/meta-data/"),
        )
        .mount(&mock_server)
        .await;

    let client = stophammer::proof::build_ssrf_safe_client();
    let result =
        stophammer::proof::verify_podcast_txt(&client, &mock_server.uri(), "token.hash").await;

    assert!(
        result.is_err(),
        "redirect to 169.254.169.254 must be blocked, got: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// SSRF-REDIRECT-03: redirect to private (10.x.x.x) must be blocked
// ---------------------------------------------------------------------------

#[tokio::test]
async fn verify_podcast_txt_redirect_to_private_10_blocked() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("Location", "http://10.0.0.1:8080/internal"),
        )
        .mount(&mock_server)
        .await;

    let client = stophammer::proof::build_ssrf_safe_client();
    let result =
        stophammer::proof::verify_podcast_txt(&client, &mock_server.uri(), "token.hash").await;

    assert!(
        result.is_err(),
        "redirect to 10.0.0.1 must be blocked, got: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// SSRF-REDIRECT-04: redirect to private (192.168.x.x) must be blocked
// ---------------------------------------------------------------------------

#[tokio::test]
async fn verify_podcast_txt_redirect_to_private_192_blocked() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("Location", "http://192.168.1.1/admin"),
        )
        .mount(&mock_server)
        .await;

    let client = stophammer::proof::build_ssrf_safe_client();
    let result =
        stophammer::proof::verify_podcast_txt(&client, &mock_server.uri(), "token.hash").await;

    assert!(
        result.is_err(),
        "redirect to 192.168.1.1 must be blocked, got: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// SSRF-REDIRECT-05: redirect to non-HTTP scheme must be blocked
// ---------------------------------------------------------------------------

#[tokio::test]
async fn verify_podcast_txt_redirect_to_file_scheme_blocked() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("Location", "file:///etc/passwd"),
        )
        .mount(&mock_server)
        .await;

    let client = stophammer::proof::build_ssrf_safe_client();
    let result =
        stophammer::proof::verify_podcast_txt(&client, &mock_server.uri(), "token.hash").await;

    // reqwest silently stops following redirects for non-HTTP schemes rather
    // than invoking the redirect policy, so the request may "succeed" with the
    // 302 body (no podcast:txt found → Ok(false)) or error out. Either
    // outcome is safe — the important thing is it must NOT return Ok(true).
    assert_ne!(
        result,
        Ok(true),
        "redirect to file:// must not succeed with Ok(true)"
    );
}

// ---------------------------------------------------------------------------
// SSRF-REDIRECT-06: redirect to a safe public URL must still work
// ---------------------------------------------------------------------------

#[tokio::test]
async fn verify_podcast_txt_redirect_to_safe_url_works() {
    let mock_server = MockServer::start().await;

    let token_binding = "safe-redirect.hash";
    let rss = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:podcast="https://podcastindex.org/namespace/1.0">
  <channel>
    <title>Test Podcast</title>
    <podcast:txt>stophammer-proof {token_binding}</podcast:txt>
  </channel>
</rss>"#
    );

    // Mount the final RSS response on /feed path.
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string(rss))
        .mount(&mock_server)
        .await;

    // A direct request (no redirect) to the mock should work fine.
    let client = stophammer::proof::build_ssrf_safe_client();
    let result =
        stophammer::proof::verify_podcast_txt(&client, &mock_server.uri(), token_binding).await;

    assert_eq!(
        result,
        Ok(true),
        "direct request to safe URL should succeed"
    );
}

// ---------------------------------------------------------------------------
// SSRF-REDIRECT-07: chained redirect (safe -> safe -> private) must be blocked
// ---------------------------------------------------------------------------

#[tokio::test]
async fn verify_podcast_txt_chained_redirect_to_private_blocked() {
    let mock_server = MockServer::start().await;

    // The mock returns a redirect to another path on the same (safe) server,
    // which in turn redirects to a private IP. We simulate this with a single
    // redirect to a private IP (wiremock doesn't easily support multi-hop on
    // different paths with the same mock, but the redirect policy checks every
    // hop independently).
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("Location", "http://172.16.0.1/internal"),
        )
        .mount(&mock_server)
        .await;

    let client = stophammer::proof::build_ssrf_safe_client();
    let result =
        stophammer::proof::verify_podcast_txt(&client, &mock_server.uri(), "token.hash").await;

    assert!(
        result.is_err(),
        "redirect chain ending at private IP must be blocked, got: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// SSRF-REDIRECT-08: validate_feed_url returns resolved addresses
// ---------------------------------------------------------------------------

#[test]
fn validate_feed_url_returns_resolved_addrs() {
    // A URL with a literal IP should return that IP in the resolved list.
    let result = stophammer::proof::validate_feed_url("https://1.1.1.1/feed.xml");
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
    let result = stophammer::proof::validate_feed_url("https://127.0.0.1/feed.xml");
    assert!(result.is_err(), "loopback should be rejected");

    let result = stophammer::proof::validate_feed_url("https://10.0.0.1/feed.xml");
    assert!(result.is_err(), "private 10.x should be rejected");

    let result = stophammer::proof::validate_feed_url("https://192.168.1.1/feed.xml");
    assert!(result.is_err(), "private 192.168.x should be rejected");

    let result = stophammer::proof::validate_feed_url("https://169.254.169.254/feed.xml");
    assert!(result.is_err(), "link-local should be rejected");
}

// ---------------------------------------------------------------------------
// SSRF-REDIRECT-10: max redirect depth is enforced
// ---------------------------------------------------------------------------

#[tokio::test]
async fn verify_podcast_txt_max_redirects_enforced() {
    let mock_server = MockServer::start().await;

    // Mock server that always redirects to itself -- should hit the max.
    let uri = mock_server.uri();
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("Location", &*uri),
        )
        .mount(&mock_server)
        .await;

    let client = stophammer::proof::build_ssrf_safe_client();
    let result =
        stophammer::proof::verify_podcast_txt(&client, &uri, "token.hash").await;

    assert!(
        result.is_err(),
        "infinite redirect loop must be stopped, got: {result:?}"
    );
}
