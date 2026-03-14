// SP-06 startup guard — 2026-03-12
//
// Document the expected behavior: community mode must panic (refuse to start)
// if the primary pubkey cannot be determined.
//
// We cannot easily integration-test a process panic in community mode without
// spawning a child process, so this test verifies the contract at the function
// level: fetch_primary_pubkey returns None when the primary is unreachable,
// and the caller is expected to .expect() on it rather than falling back to
// the node's own pubkey.

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

// The main.rs code should call .expect() on the None, producing a panic with:
//   "FATAL: cannot determine primary node public key. Set PRIMARY_PUBKEY env var
//    with the hex pubkey of the primary node, or ensure the primary is reachable
//    at TRACKER_URL."
//
// This is a startup guard and cannot be tested without spawning a child process.
// The implementation is verified by code review.
