#![warn(clippy::pedantic)]

mod api;
mod db;
mod event;
mod ingest;
mod model;
mod signing;
mod sync;
mod verify;

use verify::{
    CrawlTokenVerifier, ContentHashVerifier, EnclosureTypeVerifier,
    FeedGuidVerifier, MediumMusicVerifier, PaymentRouteSumVerifier,
    VerifierChain,
};

fn build_verifier_chain(crawl_token: String) -> VerifierChain {
    VerifierChain::new(vec![
        Box::new(CrawlTokenVerifier { expected: crawl_token }),
        Box::new(MediumMusicVerifier),
        Box::new(FeedGuidVerifier),
        Box::new(PaymentRouteSumVerifier),
        Box::new(ContentHashVerifier),
        Box::new(EnclosureTypeVerifier),
    ])
}

#[tokio::main]
async fn main() {
    let db_path     = std::env::var("DB_PATH").unwrap_or_else(|_| "stophammer.db".into());
    let key_path    = std::env::var("KEY_PATH").unwrap_or_else(|_| "signing.key".into());
    let crawl_token = std::env::var("CRAWL_TOKEN").expect("CRAWL_TOKEN env var required");
    let bind_addr   = std::env::var("BIND").unwrap_or_else(|_| "0.0.0.0:8008".into());

    let conn    = db::open_db(&db_path);
    let db      = std::sync::Arc::new(std::sync::Mutex::new(conn));
    let signer  = signing::NodeSigner::load_or_create(&key_path).expect("failed to load signing key");
    let pubkey  = signer.pubkey_hex().to_string();
    let chain   = build_verifier_chain(crawl_token);

    let state = std::sync::Arc::new(api::AppState {
        db,
        chain:           std::sync::Arc::new(chain),
        signer:          std::sync::Arc::new(signer),
        node_pubkey_hex: pubkey,
    });

    let router   = api::build_router(state);
    let listener = tokio::net::TcpListener::bind(&bind_addr).await.unwrap();
    println!("stophammer listening on {bind_addr}");
    axum::serve(listener, router).await.unwrap();
}
