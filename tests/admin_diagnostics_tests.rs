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
                .uri("/admin/diagnostics/feeds/feed-wallet-variant")
                .header("X-Admin-Token", "test-admin-token")
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
    assert_eq!(
        json["artist_identity_plan"]["candidate_groups"][0]["source"],
        "wallet_name_variant"
    );
    assert_eq!(
        json["artist_identity_reviews"][0]["source"],
        "wallet_name_variant"
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
