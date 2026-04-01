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
    assert!(
        candidate_sources.contains(&"likely_same_artist"),
        "likely_same_artist should appear when multiple same-feed signals agree"
    );
    let wallet_variant_group = json["artist_identity_plan"]["candidate_groups"]
        .as_array()
        .expect("candidate_groups array")
        .iter()
        .find(|group| group["source"].as_str() == Some("wallet_name_variant"))
        .expect("wallet_name_variant candidate group");
    assert_eq!(wallet_variant_group["confidence"], "high_confidence");
    assert!(
        wallet_variant_group["explanation"]
            .as_str()
            .expect("candidate group explanation")
            .contains("wallet alias evidence"),
        "candidate group explanation should mention wallet alias evidence"
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
    assert!(
        review_sources.contains(&"likely_same_artist"),
        "likely_same_artist should appear among stored review items"
    );
    let wallet_variant_review = json["artist_identity_reviews"]
        .as_array()
        .expect("artist_identity_reviews array")
        .iter()
        .find(|review| review["source"].as_str() == Some("wallet_name_variant"))
        .expect("wallet_name_variant review");
    assert_eq!(wallet_variant_review["confidence"], "high_confidence");
    assert!(
        wallet_variant_review["explanation"]
            .as_str()
            .expect("artist review explanation")
            .contains("wallet alias evidence"),
        "artist review explanation should mention wallet alias evidence"
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
    let wallet_variant_review = json["reviews"]
        .as_array()
        .expect("reviews array")
        .iter()
        .find(|review| review["review"]["source"].as_str() == Some("wallet_name_variant"))
        .expect("wallet_name_variant review");
    let review_names = wallet_variant_review["review"]["artist_names"]
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
async fn admin_wallet_review_resolution_endpoint_applies_merge_override() {
    let db = common::test_db_arc();
    let now = common::now();
    let (review_id, canonical_wallet_id, merge_wallet_id) = {
        let conn = db.lock().expect("lock db");
        let ep_a = stophammer::db::get_or_create_endpoint(
            &conn,
            "keysend",
            "wallet-admin-a",
            "",
            "",
            Some("Shared Wallet Alias"),
            now,
        )
        .expect("endpoint a");
        let ep_b = stophammer::db::get_or_create_endpoint(
            &conn,
            "keysend",
            "wallet-admin-b",
            "",
            "",
            Some("Shared Wallet Alias"),
            now,
        )
        .expect("endpoint b");
        let canonical_wallet_id =
            stophammer::db::create_provisional_wallet(&conn, ep_a, now).expect("wallet a");
        let merge_wallet_id =
            stophammer::db::create_provisional_wallet(&conn, ep_b, now).expect("wallet b");
        let review_id: i64 = conn
            .query_row(
                "INSERT INTO wallet_identity_review \
                 (wallet_id, source, evidence_key, wallet_ids_json, endpoint_summary_json, \
                  status, created_at, updated_at) \
                 VALUES (?1, 'cross_wallet_alias', 'shared wallet alias', json_array(?1, ?2), '[]', \
                         'pending', ?3, ?3) \
                 RETURNING id",
                params![merge_wallet_id, canonical_wallet_id, now],
                |row| row.get(0),
            )
            .expect("insert wallet review");
        (review_id, canonical_wallet_id, merge_wallet_id)
    };

    let tmp = tempfile::tempdir().expect("tempdir");
    let signer_path = tmp.path().join("admin-wallet-review-resolution.key");
    let state = test_app_state(Arc::clone(&db), &signer_path);
    let app = stophammer::api::build_router(state);

    let body = serde_json::json!({
        "action": "merge",
        "target_wallet_id": canonical_wallet_id
    });
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/admin/wallet-identity/reviews/{review_id}/resolve"
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
    assert_eq!(json["review"]["id"], review_id);
    assert_eq!(json["review"]["wallet_id"], merge_wallet_id);
    assert_eq!(json["review"]["status"], "merged");
    assert_eq!(json["review"]["source"], "cross_wallet_alias");
    assert_eq!(json["review"]["evidence_key"], "shared wallet alias");

    let conn = db.lock().expect("lock db after request");
    let override_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM wallet_identity_override \
             WHERE wallet_id = ?1 AND override_type = 'merge' AND target_id = ?2",
            params![merge_wallet_id, canonical_wallet_id],
            |row| row.get(0),
        )
        .expect("override count");
    assert_eq!(override_count, 1);
}

#[tokio::test]
async fn admin_pending_review_endpoints_expose_artist_and_wallet_queues() {
    let db = common::test_db_arc();
    let now = common::now();
    {
        let mut conn = db.lock().expect("lock db");

        let canonical =
            stophammer::db::resolve_artist(&conn, "HeyCitizen", Some("feed-admin-pending"))
                .expect("canonical artist");
        let canonical_credit = stophammer::db::get_or_create_artist_credit(
            &conn,
            &canonical.name,
            &[(
                canonical.artist_id.clone(),
                canonical.name.clone(),
                String::new(),
            )],
            Some("feed-admin-pending"),
        )
        .expect("canonical credit");
        conn.execute(
            "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
             VALUES ('feed-admin-pending', 'https://example.com/admin-pending.xml', 'Admin Pending', 'admin pending', ?1, ?2, ?2)",
            params![canonical_credit.id, now],
        )
        .expect("insert feed");

        let variant =
            stophammer::db::resolve_artist(&conn, "Hey Citizen", Some("feed-admin-pending"))
                .expect("variant artist");
        let variant_credit = stophammer::db::get_or_create_artist_credit(
            &conn,
            &variant.name,
            &[(
                variant.artist_id.clone(),
                variant.name.clone(),
                String::new(),
            )],
            Some("feed-admin-pending"),
        )
        .expect("variant credit");
        conn.execute(
            "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, explicit, created_at, updated_at) \
             VALUES ('track-admin-pending', 'feed-admin-pending', ?1, 'Autistic Girl', 'autistic girl', 0, ?2, ?2)",
            params![variant_credit.id, now],
        )
        .expect("insert track");
        stophammer::db::resolve_artist_identity_for_feed(&mut conn, "feed-admin-pending")
            .expect("resolve artist identity");

        let ep_a = stophammer::db::get_or_create_endpoint(
            &conn,
            "keysend",
            "wallet-pending-a",
            "",
            "",
            Some("Shared Wallet Alias"),
            now,
        )
        .expect("endpoint a");
        let ep_b = stophammer::db::get_or_create_endpoint(
            &conn,
            "keysend",
            "wallet-pending-b",
            "",
            "",
            Some("Shared Wallet Alias"),
            now,
        )
        .expect("endpoint b");
        let _wallet_a =
            stophammer::db::create_provisional_wallet(&conn, ep_a, now).expect("wallet a");
        let _wallet_b =
            stophammer::db::create_provisional_wallet(&conn, ep_b, now).expect("wallet b");
        stophammer::db::generate_wallet_review_items(&conn).expect("generate wallet reviews");
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    let signer_path = tmp.path().join("admin-pending-review-queues.key");
    let state = test_app_state(Arc::clone(&db), &signer_path);
    let app = stophammer::api::build_router(state.clone());

    let artist_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/artist-identity/reviews/pending?limit=10")
                .header("X-Admin-Token", "test-admin-token")
                .body(Body::empty())
                .expect("artist request"),
        )
        .await
        .expect("artist response");
    assert_eq!(artist_resp.status(), 200);
    let artist_json = body_json(artist_resp).await;
    assert_eq!(
        artist_json["reviews"][0]["source"],
        "track_feed_name_variant"
    );
    assert_eq!(artist_json["reviews"][0]["confidence"], "review_required");

    let wallet_resp = app
        .oneshot(
            Request::builder()
                .uri("/admin/wallet-identity/reviews/pending?limit=10")
                .header("X-Admin-Token", "test-admin-token")
                .body(Body::empty())
                .expect("wallet request"),
        )
        .await
        .expect("wallet response");
    assert_eq!(wallet_resp.status(), 200);
    let wallet_json = body_json(wallet_resp).await;
    assert_eq!(wallet_json["reviews"][0]["source"], "cross_wallet_alias");
    assert_eq!(wallet_json["reviews"][0]["confidence"], "review_required");
}

#[tokio::test]
async fn admin_stale_review_endpoints_filter_old_artist_and_wallet_items() {
    let db = common::test_db_arc();
    let now = common::now();
    {
        let conn = db.lock().expect("lock db");

        let artist =
            stophammer::db::resolve_artist(&conn, "Stale Artist", Some("feed-stale-reviews"))
                .expect("stale artist");
        let credit = stophammer::db::get_or_create_artist_credit(
            &conn,
            &artist.name,
            &[(artist.artist_id.clone(), artist.name.clone(), String::new())],
            Some("feed-stale-reviews"),
        )
        .expect("stale credit");
        conn.execute(
            "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
             VALUES ('feed-stale-reviews', 'https://example.com/feed-stale-reviews.xml', 'Feed Stale Reviews', 'feed stale reviews', ?1, ?2, ?2)",
            params![credit.id, now],
        )
        .expect("insert feed");
        conn.execute(
            "INSERT INTO artist_identity_review \
             (feed_guid, source, name_key, evidence_key, status, artist_ids_json, artist_names_json, created_at, updated_at) \
             VALUES ('feed-stale-reviews', 'track_feed_name_variant', 'staleartist', 'feed-stale-reviews', 'pending', '[]', '[]', ?1, ?1)",
            params![now - 9 * 24 * 60 * 60],
        )
        .expect("insert stale artist review");
        conn.execute(
            "INSERT INTO artist_identity_review \
             (feed_guid, source, name_key, evidence_key, status, artist_ids_json, artist_names_json, created_at, updated_at) \
             VALUES ('feed-stale-reviews', 'collaboration_credit', 'staleartist', 'artist-stale-collab', 'pending', '[]', '[]', ?1, ?1)",
            params![now - 2 * 24 * 60 * 60],
        )
        .expect("insert fresh artist review");

        let ep = stophammer::db::get_or_create_endpoint(
            &conn,
            "keysend",
            "wallet-stale",
            "",
            "",
            Some("Stale Wallet"),
            now,
        )
        .expect("endpoint");
        let wallet_id = stophammer::db::create_provisional_wallet(&conn, ep, now).expect("wallet");
        conn.execute(
            "INSERT INTO wallet_identity_review \
             (wallet_id, source, evidence_key, wallet_ids_json, endpoint_summary_json, status, created_at, updated_at) \
             VALUES (?1, 'cross_wallet_alias', 'stale wallet', json_array(?1), '[]', 'pending', ?2, ?2)",
            params![wallet_id, now - 8 * 24 * 60 * 60],
        )
        .expect("insert stale wallet review");
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    let signer_path = tmp.path().join("admin-stale-review-queues.key");
    let state = test_app_state(Arc::clone(&db), &signer_path);
    let app = stophammer::api::build_router(state);

    let artist_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/artist-identity/reviews/pending/stale?min_age_days=7")
                .header("X-Admin-Token", "test-admin-token")
                .body(Body::empty())
                .expect("artist stale request"),
        )
        .await
        .expect("artist stale response");
    assert_eq!(artist_resp.status(), 200);
    let artist_json = body_json(artist_resp).await;
    assert_eq!(
        artist_json["reviews"]
            .as_array()
            .expect("artist stale array")
            .len(),
        1
    );
    assert_eq!(
        artist_json["reviews"][0]["source"],
        "track_feed_name_variant"
    );

    let wallet_resp = app
        .oneshot(
            Request::builder()
                .uri("/admin/wallet-identity/reviews/pending/stale?min_age_days=7")
                .header("X-Admin-Token", "test-admin-token")
                .body(Body::empty())
                .expect("wallet stale request"),
        )
        .await
        .expect("wallet stale response");
    assert_eq!(wallet_resp.status(), 200);
    let wallet_json = body_json(wallet_resp).await;
    assert_eq!(
        wallet_json["reviews"]
            .as_array()
            .expect("wallet stale array")
            .len(),
        1
    );
    assert_eq!(wallet_json["reviews"][0]["source"], "cross_wallet_alias");
}

#[tokio::test]
async fn admin_recent_review_endpoints_filter_new_artist_and_wallet_items() {
    let db = common::test_db_arc();
    let now = common::now();
    {
        let conn = db.lock().expect("lock db");

        let artist =
            stophammer::db::resolve_artist(&conn, "Recent Artist", Some("feed-recent-reviews"))
                .expect("artist");
        let credit = stophammer::db::get_or_create_artist_credit(
            &conn,
            &artist.name,
            &[(artist.artist_id.clone(), artist.name.clone(), String::new())],
            Some("feed-recent-reviews"),
        )
        .expect("credit");
        conn.execute(
            "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
             VALUES ('feed-recent-reviews', 'https://example.com/feed-recent-reviews.xml', 'Feed Recent Reviews', 'feed recent reviews', ?1, ?2, ?2)",
            params![credit.id, now],
        )
        .expect("insert feed");
        conn.execute(
            "INSERT INTO artist_identity_review \
             (feed_guid, source, name_key, evidence_key, status, artist_ids_json, artist_names_json, created_at, updated_at) \
             VALUES ('feed-recent-reviews', 'track_feed_name_variant', 'recentartist', 'feed-recent-reviews', 'pending', '[]', '[]', ?1, ?1)",
            params![now - 12 * 60 * 60],
        )
        .expect("insert recent artist review");
        conn.execute(
            "INSERT INTO artist_identity_review \
             (feed_guid, source, name_key, evidence_key, status, artist_ids_json, artist_names_json, created_at, updated_at) \
             VALUES ('feed-recent-reviews', 'collaboration_credit', 'recentartist', 'artist-recent-collab', 'pending', '[]', '[]', ?1, ?1)",
            params![now - 3 * 24 * 60 * 60],
        )
        .expect("insert old artist review");

        let ep_recent = stophammer::db::get_or_create_endpoint(
            &conn,
            "keysend",
            "wallet-recent",
            "",
            "",
            Some("Recent Wallet"),
            now,
        )
        .expect("recent endpoint");
        let wallet_recent = stophammer::db::create_provisional_wallet(&conn, ep_recent, now)
            .expect("recent wallet");
        conn.execute(
            "INSERT INTO wallet_identity_review \
             (wallet_id, source, evidence_key, wallet_ids_json, endpoint_summary_json, status, created_at, updated_at) \
             VALUES (?1, 'cross_wallet_alias', 'recent wallet', json_array(?1), '[]', 'pending', ?2, ?2)",
            params![wallet_recent, now - 6 * 60 * 60],
        )
        .expect("insert recent wallet review");

        let ep_old = stophammer::db::get_or_create_endpoint(
            &conn,
            "keysend",
            "wallet-old",
            "",
            "",
            Some("Old Wallet"),
            now,
        )
        .expect("old endpoint");
        let wallet_old =
            stophammer::db::create_provisional_wallet(&conn, ep_old, now).expect("old wallet");
        conn.execute(
            "INSERT INTO wallet_identity_review \
             (wallet_id, source, evidence_key, wallet_ids_json, endpoint_summary_json, status, created_at, updated_at) \
             VALUES (?1, 'cross_wallet_alias', 'old wallet', json_array(?1), '[]', 'pending', ?2, ?2)",
            params![wallet_old, now - 5 * 24 * 60 * 60],
        )
        .expect("insert old wallet review");
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    let signer_path = tmp.path().join("admin-recent-review-queues.key");
    let state = test_app_state(Arc::clone(&db), &signer_path);
    let app = stophammer::api::build_router(state);

    let artist_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/artist-identity/reviews/pending/recent?max_age_days=1")
                .header("X-Admin-Token", "test-admin-token")
                .body(Body::empty())
                .expect("artist recent request"),
        )
        .await
        .expect("artist recent response");
    assert_eq!(artist_resp.status(), 200);
    let artist_json = body_json(artist_resp).await;
    assert_eq!(
        artist_json["reviews"]
            .as_array()
            .expect("artist recent array")
            .len(),
        1
    );
    assert_eq!(
        artist_json["reviews"][0]["source"],
        "track_feed_name_variant"
    );

    let wallet_resp = app
        .oneshot(
            Request::builder()
                .uri("/admin/wallet-identity/reviews/pending/recent?max_age_days=1")
                .header("X-Admin-Token", "test-admin-token")
                .body(Body::empty())
                .expect("wallet recent request"),
        )
        .await
        .expect("wallet recent response");
    assert_eq!(wallet_resp.status(), 200);
    let wallet_json = body_json(wallet_resp).await;
    assert_eq!(
        wallet_json["reviews"]
            .as_array()
            .expect("wallet recent array")
            .len(),
        1
    );
    assert_eq!(wallet_json["reviews"][0]["evidence_key"], "recent wallet");
}

#[tokio::test]
async fn admin_pending_review_summary_endpoints_group_by_source() {
    let db = common::test_db_arc();
    let now = common::now();
    {
        let mut conn = db.lock().expect("lock db");

        let canonical =
            stophammer::db::resolve_artist(&conn, "HeyCitizen", Some("feed-admin-summary"))
                .expect("canonical artist");
        let canonical_credit = stophammer::db::get_or_create_artist_credit(
            &conn,
            &canonical.name,
            &[(
                canonical.artist_id.clone(),
                canonical.name.clone(),
                String::new(),
            )],
            Some("feed-admin-summary"),
        )
        .expect("canonical credit");
        conn.execute(
            "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
             VALUES ('feed-admin-summary', 'https://example.com/admin-summary.xml', 'Admin Summary', 'admin summary', ?1, ?2, ?2)",
            params![canonical_credit.id, now],
        )
        .expect("insert feed");

        let variant =
            stophammer::db::resolve_artist(&conn, "Hey Citizen", Some("feed-admin-summary"))
                .expect("variant artist");
        let variant_credit = stophammer::db::get_or_create_artist_credit(
            &conn,
            &variant.name,
            &[(
                variant.artist_id.clone(),
                variant.name.clone(),
                String::new(),
            )],
            Some("feed-admin-summary"),
        )
        .expect("variant credit");
        conn.execute(
            "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, explicit, created_at, updated_at) \
             VALUES ('track-admin-summary-a', 'feed-admin-summary', ?1, 'Autistic Girl', 'autistic girl', 0, ?2, ?2)",
            params![variant_credit.id, now],
        )
        .expect("insert variant track");

        let collab = stophammer::db::resolve_artist(
            &conn,
            "HeyCitizen and Fletcher",
            Some("feed-admin-summary"),
        )
        .expect("collab artist");
        let collab_credit = stophammer::db::get_or_create_artist_credit(
            &conn,
            &collab.name,
            &[(collab.artist_id.clone(), collab.name.clone(), String::new())],
            Some("feed-admin-summary"),
        )
        .expect("collab credit");
        conn.execute(
            "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, explicit, created_at, updated_at) \
             VALUES ('track-admin-summary-b', 'feed-admin-summary', ?1, 'Hardware Store Lady (Screw and Bolt Mix)', 'hardware store lady (screw and bolt mix)', 0, ?2, ?2)",
            params![collab_credit.id, now],
        )
        .expect("insert collab track");
        stophammer::db::resolve_artist_identity_for_feed(&mut conn, "feed-admin-summary")
            .expect("resolve artist identity");

        let ep_a = stophammer::db::get_or_create_endpoint(
            &conn,
            "keysend",
            "wallet-summary-a",
            "",
            "",
            Some("Shared Wallet Alias"),
            now,
        )
        .expect("endpoint a");
        let ep_b = stophammer::db::get_or_create_endpoint(
            &conn,
            "keysend",
            "wallet-summary-b",
            "",
            "",
            Some("Shared Wallet Alias"),
            now,
        )
        .expect("endpoint b");
        let _wallet_a =
            stophammer::db::create_provisional_wallet(&conn, ep_a, now).expect("wallet a");
        let _wallet_b =
            stophammer::db::create_provisional_wallet(&conn, ep_b, now).expect("wallet b");
        stophammer::db::generate_wallet_review_items(&conn).expect("generate wallet reviews");
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    let signer_path = tmp.path().join("admin-pending-review-summary.key");
    let state = test_app_state(Arc::clone(&db), &signer_path);
    let app = stophammer::api::build_router(state);

    let artist_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/artist-identity/reviews/pending/summary")
                .header("X-Admin-Token", "test-admin-token")
                .body(Body::empty())
                .expect("artist summary request"),
        )
        .await
        .expect("artist summary response");
    assert_eq!(artist_resp.status(), 200);
    let artist_json = body_json(artist_resp).await;
    let artist_sources = artist_json["summary"]
        .as_array()
        .expect("artist summary array")
        .iter()
        .filter_map(|row| row["source"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(artist_sources.contains("track_feed_name_variant"));
    assert!(artist_sources.contains("collaboration_credit"));

    let wallet_resp = app
        .oneshot(
            Request::builder()
                .uri("/admin/wallet-identity/reviews/pending/summary")
                .header("X-Admin-Token", "test-admin-token")
                .body(Body::empty())
                .expect("wallet summary request"),
        )
        .await
        .expect("wallet summary response");
    assert_eq!(wallet_resp.status(), 200);
    let wallet_json = body_json(wallet_resp).await;
    assert_eq!(wallet_json["summary"][0]["source"], "cross_wallet_alias");
}

#[tokio::test]
async fn admin_pending_review_age_summary_reports_recent_and_stale_counts() {
    let db = common::test_db_arc();
    let now = common::now();
    {
        let conn = db.lock().expect("lock db");

        let artist = stophammer::db::resolve_artist(&conn, "Age Artist", Some("feed-age-a"))
            .expect("age artist");
        let credit_a = stophammer::db::get_or_create_artist_credit(
            &conn,
            &artist.name,
            &[(artist.artist_id.clone(), artist.name.clone(), String::new())],
            Some("feed-age-a"),
        )
        .expect("credit a");
        let credit_b = stophammer::db::get_or_create_artist_credit(
            &conn,
            &artist.name,
            &[(artist.artist_id.clone(), artist.name.clone(), String::new())],
            Some("feed-age-b"),
        )
        .expect("credit b");
        conn.execute(
            "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
             VALUES ('feed-age-a', 'https://example.com/feed-age-a.xml', 'Feed Age A', 'feed age a', ?1, ?2, ?2)",
            params![credit_a.id, now],
        )
        .expect("insert feed age a");
        conn.execute(
            "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
             VALUES ('feed-age-b', 'https://example.com/feed-age-b.xml', 'Feed Age B', 'feed age b', ?1, ?2, ?2)",
            params![credit_b.id, now],
        )
        .expect("insert feed age b");

        conn.execute(
            "INSERT INTO artist_identity_review \
             (feed_guid, source, name_key, evidence_key, status, artist_ids_json, artist_names_json, created_at, updated_at) \
             VALUES ('feed-age-a', 'track_feed_name_variant', 'heycitizen', 'feed-age-a', 'pending', '[]', '[]', ?1, ?1)",
            params![now - 2 * 60 * 60],
        )
        .expect("insert fresh artist review");
        conn.execute(
            "INSERT INTO artist_identity_review \
             (feed_guid, source, name_key, evidence_key, status, artist_ids_json, artist_names_json, created_at, updated_at) \
             VALUES ('feed-age-b', 'collaboration_credit', 'heycitizen', 'artist-collab', 'pending', '[]', '[]', ?1, ?1)",
            params![now - 8 * 24 * 60 * 60],
        )
        .expect("insert stale artist review");

        let ep = stophammer::db::get_or_create_endpoint(
            &conn,
            "keysend",
            "wallet-age",
            "",
            "",
            Some("Age Wallet"),
            now,
        )
        .expect("endpoint");
        let wallet_id = stophammer::db::create_provisional_wallet(&conn, ep, now).expect("wallet");
        conn.execute(
            "INSERT INTO wallet_identity_review \
             (wallet_id, source, evidence_key, wallet_ids_json, endpoint_summary_json, status, created_at, updated_at) \
             VALUES (?1, 'cross_wallet_alias', 'age wallet', json_array(?1), '[]', 'pending', ?2, ?2)",
            params![wallet_id, now - 10 * 24 * 60 * 60],
        )
        .expect("insert wallet review");
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    let signer_path = tmp.path().join("admin-pending-review-age-summary.key");
    let state = test_app_state(Arc::clone(&db), &signer_path);
    let app = stophammer::api::build_router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/admin/reviews/pending/age-summary")
                .header("X-Admin-Token", "test-admin-token")
                .body(Body::empty())
                .expect("age summary request"),
        )
        .await
        .expect("age summary response");
    assert_eq!(resp.status(), 200);

    let json = body_json(resp).await;
    assert_eq!(json["artist_identity"]["total"], 2);
    assert_eq!(json["artist_identity"]["created_last_24h"], 1);
    assert_eq!(json["artist_identity"]["older_than_7d"], 1);
    assert_eq!(json["wallet_identity"]["total"], 1);
    assert_eq!(json["wallet_identity"]["older_than_7d"], 1);
}

#[tokio::test]
async fn admin_pending_review_dashboard_combines_summary_age_and_hotspots() {
    let db = common::test_db_arc();
    let now = common::now();
    {
        let conn = db.lock().expect("lock db");

        let artist =
            stophammer::db::resolve_artist(&conn, "Dashboard Artist", Some("feed-dashboard"))
                .expect("artist");
        let credit = stophammer::db::get_or_create_artist_credit(
            &conn,
            &artist.name,
            &[(artist.artist_id.clone(), artist.name.clone(), String::new())],
            Some("feed-dashboard"),
        )
        .expect("credit");
        conn.execute(
            "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
             VALUES ('feed-dashboard', 'https://example.com/feed-dashboard.xml', 'Feed Dashboard', 'feed dashboard', ?1, ?2, ?2)",
            params![credit.id, now],
        )
        .expect("insert feed");
        conn.execute(
            "INSERT INTO artist_identity_review \
             (feed_guid, source, name_key, evidence_key, status, artist_ids_json, artist_names_json, created_at, updated_at) \
             VALUES ('feed-dashboard', 'track_feed_name_variant', 'dashboardartist', 'feed-dashboard', 'pending', '[]', '[]', ?1, ?1)",
            params![now - 60 * 60],
        )
        .expect("insert artist review");

        let ep_a = stophammer::db::get_or_create_endpoint(
            &conn,
            "keysend",
            "wallet-dashboard-a",
            "",
            "",
            Some("Dashboard Wallet Alias"),
            now,
        )
        .expect("endpoint a");
        let ep_b = stophammer::db::get_or_create_endpoint(
            &conn,
            "keysend",
            "wallet-dashboard-b",
            "",
            "",
            Some("Dashboard Wallet Alias"),
            now,
        )
        .expect("endpoint b");
        let _wallet_a =
            stophammer::db::create_provisional_wallet(&conn, ep_a, now).expect("wallet a");
        let _wallet_b =
            stophammer::db::create_provisional_wallet(&conn, ep_b, now).expect("wallet b");
        stophammer::db::generate_wallet_review_items(&conn).expect("generate wallet reviews");
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    let signer_path = tmp.path().join("admin-pending-review-dashboard.key");
    let state = test_app_state(Arc::clone(&db), &signer_path);
    let app = stophammer::api::build_router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/admin/reviews/dashboard?hotspot_limit=5")
                .header("X-Admin-Token", "test-admin-token")
                .body(Body::empty())
                .expect("dashboard request"),
        )
        .await
        .expect("dashboard response");
    assert_eq!(resp.status(), 200);

    let json = body_json(resp).await;
    assert_eq!(
        json["artist_identity_summary"][0]["source"],
        "track_feed_name_variant"
    );
    assert_eq!(
        json["wallet_identity_summary"][0]["source"],
        "cross_wallet_alias"
    );
    assert_eq!(json["age_summary"]["artist_identity"]["total"], 1);
    assert_eq!(json["feed_hotspots"][0]["feed_guid"], "feed-dashboard");
}

#[tokio::test]
async fn admin_pending_review_feed_hotspots_orders_by_total_load() {
    let db = common::test_db_arc();
    let now = common::now();
    {
        let mut conn = db.lock().expect("lock db");

        let primary_artist =
            stophammer::db::resolve_artist(&conn, "Hot Artist", Some("feed-hotspot-a"))
                .expect("primary artist");
        let credit_a = stophammer::db::get_or_create_artist_credit(
            &conn,
            &primary_artist.name,
            &[(
                primary_artist.artist_id.clone(),
                primary_artist.name.clone(),
                String::new(),
            )],
            Some("feed-hotspot-a"),
        )
        .expect("credit a");
        conn.execute(
            "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
             VALUES ('feed-hotspot-a', 'https://example.com/feed-hotspot-a.xml', 'Feed Hotspot A', 'feed hotspot a', ?1, ?2, ?2)",
            params![credit_a.id, now],
        )
        .expect("insert feed a");

        let variant_artist =
            stophammer::db::resolve_artist(&conn, "Hot Artist and Friend", Some("feed-hotspot-a"))
                .expect("variant artist");
        let variant_credit = stophammer::db::get_or_create_artist_credit(
            &conn,
            &variant_artist.name,
            &[(
                variant_artist.artist_id.clone(),
                variant_artist.name.clone(),
                String::new(),
            )],
            Some("feed-hotspot-a"),
        )
        .expect("variant credit");
        conn.execute(
            "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, explicit, created_at, updated_at) \
             VALUES ('track-hotspot-a', 'feed-hotspot-a', ?1, 'Hotspot Track', 'hotspot track', 0, ?2, ?2)",
            params![variant_credit.id, now],
        )
        .expect("insert hotspot track");
        stophammer::db::resolve_artist_identity_for_feed(&mut conn, "feed-hotspot-a")
            .expect("resolve feed hotspot a");

        let ep_a = stophammer::db::get_or_create_endpoint(
            &conn,
            "keysend",
            "wallet-hotspot-a1",
            "",
            "",
            Some("Shared Wallet Alias"),
            now,
        )
        .expect("endpoint a1");
        let ep_b = stophammer::db::get_or_create_endpoint(
            &conn,
            "keysend",
            "wallet-hotspot-a2",
            "",
            "",
            Some("Shared Wallet Alias"),
            now,
        )
        .expect("endpoint a2");
        let _wallet_a =
            stophammer::db::create_provisional_wallet(&conn, ep_a, now).expect("wallet a");
        let _wallet_b =
            stophammer::db::create_provisional_wallet(&conn, ep_b, now).expect("wallet b");
        conn.execute(
            "INSERT INTO feed_payment_routes (feed_guid, recipient_name, split, fee, route_type, address, custom_key, custom_value) \
             VALUES ('feed-hotspot-a', 'Shared Wallet Alias', 100, 0, 'keysend', 'wallet-hotspot-a1', '', '')",
            [],
        )
        .expect("insert feed route a1");
        let route_id_a1 = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO wallet_feed_route_map (route_id, endpoint_id, created_at) \
             VALUES (?1, ?2, ?3)",
            params![route_id_a1, ep_a, now],
        )
        .expect("map wallet a1");
        conn.execute(
            "INSERT INTO feed_payment_routes (feed_guid, recipient_name, split, fee, route_type, address, custom_key, custom_value) \
             VALUES ('feed-hotspot-a', 'Shared Wallet Alias', 100, 0, 'keysend', 'wallet-hotspot-a2', '', '')",
            [],
        )
        .expect("insert feed route a2");
        let route_id_a2 = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO wallet_feed_route_map (route_id, endpoint_id, created_at) \
             VALUES (?1, ?2, ?3)",
            params![route_id_a2, ep_b, now],
        )
        .expect("map wallet a2");
        stophammer::db::generate_wallet_review_items(&conn).expect("wallet review items");

        let artist_b = stophammer::db::resolve_artist(&conn, "Cool Artist", Some("feed-hotspot-b"))
            .expect("artist b");
        let credit_b = stophammer::db::get_or_create_artist_credit(
            &conn,
            &artist_b.name,
            &[(
                artist_b.artist_id.clone(),
                artist_b.name.clone(),
                String::new(),
            )],
            Some("feed-hotspot-b"),
        )
        .expect("credit b");
        conn.execute(
            "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
             VALUES ('feed-hotspot-b', 'https://example.com/feed-hotspot-b.xml', 'Feed Hotspot B', 'feed hotspot b', ?1, ?2, ?2)",
            params![credit_b.id, now],
        )
        .expect("insert feed b");
        let collab_b =
            stophammer::db::resolve_artist(&conn, "Cool Artist and Friend", Some("feed-hotspot-b"))
                .expect("collab b");
        let collab_credit_b = stophammer::db::get_or_create_artist_credit(
            &conn,
            &collab_b.name,
            &[(
                collab_b.artist_id.clone(),
                collab_b.name.clone(),
                String::new(),
            )],
            Some("feed-hotspot-b"),
        )
        .expect("collab credit b");
        conn.execute(
            "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, explicit, created_at, updated_at) \
             VALUES ('track-hotspot-b', 'feed-hotspot-b', ?1, 'Collab Track', 'collab track', 0, ?2, ?2)",
            params![collab_credit_b.id, now],
        )
        .expect("insert collab track b");
        stophammer::db::resolve_artist_identity_for_feed(&mut conn, "feed-hotspot-b")
            .expect("resolve feed hotspot b");
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    let signer_path = tmp.path().join("admin-pending-review-feed-hotspots.key");
    let state = test_app_state(Arc::clone(&db), &signer_path);
    let app = stophammer::api::build_router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/admin/reviews/feeds/hotspots?limit=10")
                .header("X-Admin-Token", "test-admin-token")
                .body(Body::empty())
                .expect("hotspots request"),
        )
        .await
        .expect("hotspots response");
    assert_eq!(resp.status(), 200);

    let json = body_json(resp).await;
    assert_eq!(json["feeds"][0]["feed_guid"], "feed-hotspot-a");
    assert_eq!(json["feeds"][0]["artist_review_count"], 1);
    assert_eq!(json["feeds"][0]["wallet_review_count"], 2);
    assert_eq!(json["feeds"][0]["total_review_count"], 3);
    assert_eq!(json["feeds"][1]["feed_guid"], "feed-hotspot-b");
    assert_eq!(json["feeds"][1]["artist_review_count"], 1);
    assert_eq!(json["feeds"][1]["wallet_review_count"], 0);
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
    assert_eq!(json["reviews"][0]["confidence"], "review_required");
    assert_eq!(json["reviews"][0]["evidence_key"], "shared wallet alias");
}
