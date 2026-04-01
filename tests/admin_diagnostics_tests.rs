mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use rusqlite::params;
use tower::ServiceExt;

fn test_app_state(
    db: Arc<Mutex<rusqlite::Connection>>,
    signer_path: &std::path::Path,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(
        stophammer::signing::NodeSigner::load_or_create(signer_path).expect("create signer"),
    );
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec {
        names: vec!["crawl_token".to_string()],
    };
    let chain = stophammer::verify::build_chain(&spec, "test-crawl-token".to_string());

    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-admin-token".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        skip_ssrf_validation: true,
    })
}

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("parse json")
}

#[tokio::test]
async fn admin_feed_diagnostics_exposes_artist_reviews_and_wallet_links() {
    let db = common::test_db_arc();
    let now = common::now();

    {
        let conn = db.lock().expect("lock db");

        let canonical =
            stophammer::db::resolve_artist(&conn, "HeyCitizen", Some("feed-wallet-variant"))
                .expect("canonical artist");
        let canonical_credit = stophammer::db::get_or_create_artist_credit(
            &conn,
            &canonical.name,
            &[(
                canonical.artist_id.clone(),
                canonical.name.clone(),
                String::new(),
            )],
            Some("feed-wallet-variant"),
        )
        .expect("canonical credit");
        conn.execute(
            "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
             VALUES ('feed-wallet-variant', 'https://example.com/wallet-variant.xml', 'Wallet Variant', 'wallet variant', ?1, ?2, ?2)",
            params![canonical_credit.id, now],
        )
        .expect("insert feed");

        let variant =
            stophammer::db::resolve_artist(&conn, "Hey Citizen", Some("feed-wallet-variant"))
                .expect("variant artist");
        let variant_credit = stophammer::db::get_or_create_artist_credit(
            &conn,
            &variant.name,
            &[(
                variant.artist_id.clone(),
                variant.name.clone(),
                String::new(),
            )],
            Some("feed-wallet-variant"),
        )
        .expect("variant credit");
        conn.execute(
            "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, explicit, created_at, updated_at) \
             VALUES ('track-wallet-variant', 'feed-wallet-variant', ?1, 'Autistic Girl', 'autistic girl', 0, ?2, ?2)",
            params![variant_credit.id, now],
        )
        .expect("insert track");

        conn.execute(
            "INSERT INTO feed_payment_routes \
             (feed_guid, recipient_name, route_type, address, custom_key, custom_value, split, fee) \
             VALUES ('feed-wallet-variant', 'HeyCitizen', 'lnaddress', 'heycitizen@example.com', NULL, NULL, 100, 0)",
            [],
        )
        .expect("insert feed route");
    }

    {
        let mut conn = db.lock().expect("lock db");
        stophammer::db::resolve_wallet_identity_for_feed(&conn, "feed-wallet-variant")
            .expect("resolve wallet identity");
        stophammer::db::backfill_wallet_pass3(&conn).expect("wallet pass3");
        stophammer::db::resolve_artist_identity_for_feed(&mut conn, "feed-wallet-variant")
            .expect("resolve artist identity");
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    let signer_path = tmp.path().join("admin-diagnostics.key");
    let state = test_app_state(Arc::clone(&db), &signer_path);
    let app = stophammer::api::build_router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/diagnostics/feeds/feed-wallet-variant")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), 200);

    let json = body_json(resp).await;
    assert_eq!(json["feed_guid"], "feed-wallet-variant");
    assert_eq!(json["tracks"][0]["title"], "Autistic Girl");
    assert_eq!(
        json["tracks"][0]["artist_credit"]["display_name"],
        "Hey Citizen"
    );
    let candidate_sources = json["artist_identity_plan"]["candidate_groups"]
        .as_array()
        .expect("candidate_groups array")
        .iter()
        .filter_map(|group| group["source"].as_str())
        .collect::<Vec<_>>();
    assert!(
        candidate_sources.contains(&"wallet_name_variant"),
        "wallet_name_variant should appear among candidate groups"
    );
    let review_sources = json["artist_identity_reviews"]
        .as_array()
        .expect("artist_identity_reviews array")
        .iter()
        .filter_map(|review| review["source"].as_str())
        .collect::<Vec<_>>();
    assert!(
        review_sources.contains(&"wallet_name_variant"),
        "wallet_name_variant should appear among stored review items"
    );
    assert_eq!(
        json["wallets"][0]["wallet"]["artist_links"][0]["confidence"],
        "high_confidence"
    );
    assert_eq!(
        json["wallets"][0]["claim_feed"]["routes"][0]["route_scope"],
        "feed"
    );
}

#[tokio::test]
async fn admin_artist_diagnostics_exposes_redirects_wallets_and_reviews() {
    let db = common::test_db_arc();
    let now = common::now();
    let canonical_artist_id = {
        let conn = db.lock().expect("lock db");

        let canonical =
            stophammer::db::resolve_artist(&conn, "HeyCitizen", Some("feed-artist-diagnostics"))
                .expect("canonical artist");
        let canonical_credit = stophammer::db::get_or_create_artist_credit(
            &conn,
            &canonical.name,
            &[(
                canonical.artist_id.clone(),
                canonical.name.clone(),
                String::new(),
            )],
            Some("feed-artist-diagnostics"),
        )
        .expect("canonical credit");
        conn.execute(
            "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
             VALUES ('feed-artist-diagnostics', 'https://example.com/artist-diagnostics.xml', 'Artist Diagnostics', 'artist diagnostics', ?1, ?2, ?2)",
            params![canonical_credit.id, now],
        )
        .expect("insert feed");

        let variant =
            stophammer::db::resolve_artist(&conn, "Hey Citizen", Some("feed-artist-diagnostics"))
                .expect("variant artist");
        let variant_credit = stophammer::db::get_or_create_artist_credit(
            &conn,
            &variant.name,
            &[(
                variant.artist_id.clone(),
                variant.name.clone(),
                String::new(),
            )],
            Some("feed-artist-diagnostics"),
        )
        .expect("variant credit");
        conn.execute(
            "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, explicit, created_at, updated_at) \
             VALUES ('track-artist-diagnostics', 'feed-artist-diagnostics', ?1, 'Autistic Girl', 'autistic girl', 0, ?2, ?2)",
            params![variant_credit.id, now],
        )
        .expect("insert track");

        conn.execute(
            "INSERT INTO feed_payment_routes \
             (feed_guid, recipient_name, route_type, address, custom_key, custom_value, split, fee) \
             VALUES ('feed-artist-diagnostics', 'HeyCitizen', 'lnaddress', 'heycitizen@example.com', NULL, NULL, 100, 0)",
            [],
        )
        .expect("insert feed route");

        conn.execute(
            "INSERT INTO artist_id_redirect (old_artist_id, new_artist_id, merged_at) VALUES (?1, ?2, ?3)",
            params!["artist-old-heycitizen", canonical.artist_id, now],
        )
        .expect("insert redirect");

        canonical.artist_id
    };

    {
        let mut conn = db.lock().expect("lock db");
        stophammer::db::resolve_wallet_identity_for_feed(&conn, "feed-artist-diagnostics")
            .expect("resolve wallet identity");
        stophammer::db::backfill_wallet_pass3(&conn).expect("wallet pass3");
        stophammer::db::resolve_artist_identity_for_feed(&mut conn, "feed-artist-diagnostics")
            .expect("resolve artist identity");
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    let signer_path = tmp.path().join("admin-artist-diagnostics.key");
    let state = test_app_state(Arc::clone(&db), &signer_path);
    let app = stophammer::api::build_router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/diagnostics/artists/{canonical_artist_id}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), 200);

    let json = body_json(resp).await;
    assert_eq!(json["artist"]["artist_id"], canonical_artist_id);
    assert_eq!(json["redirected_from"][0], "artist-old-heycitizen");
    assert_eq!(json["feeds"][0]["feed_guid"], "feed-artist-diagnostics");
    assert_eq!(
        json["wallets"][0]["artist_links"][0]["confidence"],
        "high_confidence"
    );
    assert_eq!(
        json["reviews"][0]["review"]["source"],
        "wallet_name_variant"
    );
    let review_names = json["reviews"][0]["review"]["artist_names"]
        .as_array()
        .expect("artist_names array");
    assert!(
        review_names.iter().any(|value| value == "Hey Citizen"),
        "review should mention the split variant artist name"
    );
}

#[tokio::test]
async fn admin_artist_diagnostics_exposes_unlinked_feed_wallets() {
    let db = common::test_db_arc();
    let now = common::now();
    let artist_id = {
        let conn = db.lock().expect("lock db");

        let artist = stophammer::db::resolve_artist(
            &conn,
            "Artist With Feed Wallet",
            Some("feed-artist-unlinked-wallet"),
        )
        .expect("artist");
        let credit = stophammer::db::get_or_create_artist_credit(
            &conn,
            &artist.name,
            &[(artist.artist_id.clone(), artist.name.clone(), String::new())],
            Some("feed-artist-unlinked-wallet"),
        )
        .expect("credit");
        conn.execute(
            "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
             VALUES ('feed-artist-unlinked-wallet', 'https://example.com/artist-unlinked-wallet.xml', 'Artist Unlinked Wallet', 'artist unlinked wallet', ?1, ?2, ?2)",
            params![credit.id, now],
        )
        .expect("insert feed");
        conn.execute(
            "INSERT INTO feed_payment_routes \
             (feed_guid, recipient_name, route_type, address, custom_key, custom_value, split, fee) \
             VALUES ('feed-artist-unlinked-wallet', 'Platform Wallet', 'lnaddress', 'platform@example.com', NULL, NULL, 100, 0)",
            [],
        )
        .expect("insert feed route");
        artist.artist_id
    };

    {
        let conn = db.lock().expect("lock db");
        stophammer::db::resolve_wallet_identity_for_feed(&conn, "feed-artist-unlinked-wallet")
            .expect("resolve wallet identity");
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    let signer_path = tmp.path().join("admin-artist-unlinked-wallet.key");
    let state = test_app_state(Arc::clone(&db), &signer_path);
    let app = stophammer::api::build_router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/diagnostics/artists/{artist_id}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), 200);

    let json = body_json(resp).await;
    assert_eq!(json["wallets"], serde_json::json!([]));
    assert_eq!(
        json["unlinked_feed_wallets"][0]["wallet"]["display_name"],
        "Platform Wallet"
    );
    assert_eq!(
        json["unlinked_feed_wallets"][0]["claim_feed"]["feed_guid"],
        "feed-artist-unlinked-wallet"
    );
}

#[tokio::test]
async fn admin_artist_review_resolution_endpoint_applies_merge_override() {
    let db = common::test_db_arc();
    let now = common::now();
    let (review_id, canonical_artist_id) = {
        let mut conn = db.lock().expect("lock db");

        let canonical =
            stophammer::db::resolve_artist(&conn, "HeyCitizen", Some("feed-admin-review-action"))
                .expect("canonical artist");
        let canonical_credit = stophammer::db::get_or_create_artist_credit(
            &conn,
            &canonical.name,
            &[(
                canonical.artist_id.clone(),
                canonical.name.clone(),
                String::new(),
            )],
            Some("feed-admin-review-action"),
        )
        .expect("canonical credit");
        conn.execute(
            "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
             VALUES ('feed-admin-review-action', 'https://example.com/admin-review-action.xml', 'Admin Review Action', 'admin review action', ?1, ?2, ?2)",
            params![canonical_credit.id, now],
        )
        .expect("insert feed");

        let variant =
            stophammer::db::resolve_artist(&conn, "Hey Citizen", Some("feed-admin-review-action"))
                .expect("variant artist");
        let variant_credit = stophammer::db::get_or_create_artist_credit(
            &conn,
            &variant.name,
            &[(
                variant.artist_id.clone(),
                variant.name.clone(),
                String::new(),
            )],
            Some("feed-admin-review-action"),
        )
        .expect("variant credit");
        conn.execute(
            "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, explicit, created_at, updated_at) \
             VALUES ('track-admin-review-action', 'feed-admin-review-action', ?1, 'Autistic Girl', 'autistic girl', 0, ?2, ?2)",
            params![variant_credit.id, now],
        )
        .expect("insert track");

        let stats =
            stophammer::db::resolve_artist_identity_for_feed(&mut conn, "feed-admin-review-action")
                .expect("resolve artist identity");
        assert_eq!(stats.pending_reviews, 1, "expected one pending review");
        let review = stophammer::db::list_artist_identity_reviews_for_feed(
            &conn,
            "feed-admin-review-action",
        )
        .expect("list reviews")
        .into_iter()
        .find(|review| review.source == "track_feed_name_variant")
        .expect("track_feed_name_variant review");
        (review.review_id, canonical.artist_id)
    };

    let tmp = tempfile::tempdir().expect("tempdir");
    let signer_path = tmp.path().join("admin-review-resolution.key");
    let state = test_app_state(Arc::clone(&db), &signer_path);
    let app = stophammer::api::build_router(state);

    let body = serde_json::json!({
        "action": "merge",
        "target_artist_id": canonical_artist_id,
        "note": "merge feed/track variant"
    });
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/admin/artist-identity/reviews/{review_id}/resolve"
                ))
                .header("Content-Type", "application/json")
                .header("X-Admin-Token", "test-admin-token")
                .body(Body::from(serde_json::to_vec(&body).expect("serialize")))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), 200);

    let json = body_json(resp).await;
    assert_eq!(json["review"]["review_id"], review_id);
    assert_eq!(json["review"]["status"], "merged");
    assert_eq!(json["review"]["override_type"], "merge");
    assert_eq!(json["review"]["target_artist_id"], canonical_artist_id);
    assert_eq!(json["review"]["note"], "merge feed/track variant");
    assert!(
        json["resolve_stats"]["merges_applied"]
            .as_u64()
            .expect("merges_applied u64")
            >= 1,
        "merge action should cause at least one merge during the follow-up resolver pass"
    );
}

#[tokio::test]
async fn admin_wallet_diagnostics_exposes_claims_peers_and_reviews() {
    let db = common::test_db_arc();
    let now = common::now();
    {
        let conn = db.lock().expect("lock db");

        for (feed_guid, title, address) in [
            (
                "feed-wallet-diagnostics-a",
                "Wallet Diagnostics A",
                "wallet-a@example.com",
            ),
            (
                "feed-wallet-diagnostics-b",
                "Wallet Diagnostics B",
                "wallet-b@example.com",
            ),
        ] {
            let artist =
                stophammer::db::resolve_artist(&conn, title, Some(feed_guid)).expect("artist");
            let credit = stophammer::db::get_or_create_artist_credit(
                &conn,
                &artist.name,
                &[(artist.artist_id.clone(), artist.name.clone(), String::new())],
                Some(feed_guid),
            )
            .expect("credit");
            conn.execute(
                "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                params![
                    feed_guid,
                    format!("https://example.com/{feed_guid}.xml"),
                    title,
                    title.to_lowercase(),
                    credit.id,
                    now
                ],
            )
            .expect("insert feed");
            conn.execute(
                "INSERT INTO feed_payment_routes \
                 (feed_guid, recipient_name, route_type, address, custom_key, custom_value, split, fee) \
                 VALUES (?1, 'Shared Wallet Alias', 'lnaddress', ?2, NULL, NULL, 100, 0)",
                params![feed_guid, address],
            )
            .expect("insert feed route");
        }
    };

    {
        let conn = db.lock().expect("lock db");
        stophammer::db::resolve_wallet_identity_for_feed(&conn, "feed-wallet-diagnostics-a")
            .expect("resolve wallet a");
        stophammer::db::resolve_wallet_identity_for_feed(&conn, "feed-wallet-diagnostics-b")
            .expect("resolve wallet b");
        stophammer::db::backfill_wallet_pass5(&conn).expect("wallet pass5");
    }

    let wallet_id = {
        let conn = db.lock().expect("lock db");
        let wallet_id = stophammer::db::get_wallet_ids_for_feed(&conn, "feed-wallet-diagnostics-a")
            .expect("wallet ids")
            .into_iter()
            .next()
            .expect("wallet id for feed a");
        conn.execute(
            "INSERT INTO wallet_id_redirect (old_wallet_id, new_wallet_id, created_at) VALUES (?1, ?2, ?3)",
            params!["wallet-old-shared-alias", wallet_id, now],
        )
        .expect("insert wallet redirect");
        wallet_id
    };

    let tmp = tempfile::tempdir().expect("tempdir");
    let signer_path = tmp.path().join("admin-wallet-diagnostics.key");
    let state = test_app_state(Arc::clone(&db), &signer_path);
    let app = stophammer::api::build_router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/diagnostics/wallets/{wallet_id}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), 200);

    let json = body_json(resp).await;
    assert_eq!(json["wallet"]["wallet_id"], wallet_id);
    assert_eq!(json["redirected_from"][0], "wallet-old-shared-alias");
    assert_eq!(json["claim_feeds"][0]["routes"][0]["route_scope"], "feed");
    assert_eq!(
        json["alias_peers"][0]["display_name"],
        "Shared Wallet Alias"
    );
    assert_eq!(json["reviews"][0]["source"], "cross_wallet_alias");
    assert_eq!(json["reviews"][0]["evidence_key"], "shared wallet alias");
}
