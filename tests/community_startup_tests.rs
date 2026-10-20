// Community startup helper tests.

#[tokio::test]
async fn fetch_primary_pubkey_returns_none_when_unreachable() {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(100))
        .build()
        .expect("build client");

    // Use a definitely-unreachable address (RFC 5737 TEST-NET)
    let result = stophammer::community::fetch_primary_pubkey(
        &client,
        "http://192.0.2.1:9999",
        1, // single attempt, no retries
    )
    .await;

    assert!(
        result.is_none(),
        "fetch_primary_pubkey must return None when the primary is unreachable"
    );
}

// main.rs turns the None into a startup error:
//   "cannot determine primary node public key. set PRIMARY_PUBKEY or ensure
//    the primary is reachable at {primary_url}"
//
// This is a startup guard and cannot be tested without spawning a child process.
// The implementation is verified by code review.
