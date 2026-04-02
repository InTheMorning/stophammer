#![allow(
    clippy::map_err_ignore,
    reason = "CLI argument parsing intentionally returns user-facing strings instead of preserving parse error types"
)]
#![allow(
    clippy::too_many_lines,
    reason = "the CLI entrypoint is a single dispatcher for operator review actions"
)]

use std::error::Error;
use std::path::PathBuf;

use rusqlite::Connection;
use serde::Serialize;
use stophammer::db::{DEFAULT_DB_PATH, WALLET_CLASS_VALUES};

#[derive(Debug)]
struct Args {
    db_path: PathBuf,
    limit: usize,
    high_confidence_only: bool,
    show_review: Option<i64>,
    show_wallet: Option<String>,
    resolve_merge: Option<i64>,
    resolve_reject: Option<i64>,
    resolve_class: Option<i64>,
    resolve_link: Option<i64>,
    resolve_block_link: Option<i64>,
    target_wallet: Option<String>,
    class_value: Option<String>,
    artist_id: Option<String>,
    json: bool,
}

#[derive(Debug, Serialize)]
struct PendingReviewsReport {
    reviews: Vec<stophammer::db::WalletReviewSummary>,
}

#[derive(Debug, Serialize)]
struct WalletDetailReport {
    wallet: stophammer::db::WalletDetail,
}

#[derive(Debug, Serialize)]
struct ReviewDetailReport {
    review: stophammer::db::WalletReviewSummary,
    wallet: stophammer::db::WalletDetail,
}

fn format_score_breakdown(score_breakdown: &[stophammer::db::ReviewScoreComponent]) -> String {
    if score_breakdown.is_empty() {
        "-".to_string()
    } else {
        score_breakdown
            .iter()
            .map(|component| format!("{}:{}", component.source, component.points))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn score_band(score: Option<u16>) -> &'static str {
    match score {
        Some(80..=100) => "80_100",
        Some(60..=79) => "60_79",
        Some(1..=59) => "1_59",
        _ => "unscored",
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "manual CLI parsing keeps the review tool dependency-free"
)]
fn parse_args() -> Result<Args, String> {
    let mut db_path = PathBuf::from(DEFAULT_DB_PATH);
    let mut limit = 50usize;
    let mut high_confidence_only = false;
    let mut show_review = None;
    let mut show_wallet = None;
    let mut resolve_merge = None;
    let mut resolve_reject = None;
    let mut resolve_class = None;
    let mut resolve_link = None;
    let mut resolve_block_link = None;
    let mut target_wallet = None;
    let mut class_value = None;
    let mut artist_id = None;
    let mut json = false;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--db" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--db requires a path".to_string())?;
                db_path = PathBuf::from(value);
            }
            "--limit" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--limit requires a number".to_string())?;
                limit = value
                    .parse::<usize>()
                    .map_err(|_err| format!("invalid --limit value: {value}"))?;
            }
            "--high-confidence-only" => {
                high_confidence_only = true;
            }
            "--show-review" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--show-review requires an integer id".to_string())?;
                show_review = Some(
                    value
                        .parse::<i64>()
                        .map_err(|_err| format!("invalid --show-review value: {value}"))?,
                );
            }
            "--show-wallet" => {
                show_wallet = Some(
                    args.next()
                        .ok_or_else(|| "--show-wallet requires a wallet id".to_string())?,
                );
            }
            "--resolve-merge" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--resolve-merge requires an integer id".to_string())?;
                resolve_merge = Some(
                    value
                        .parse::<i64>()
                        .map_err(|_err| format!("invalid --resolve-merge value: {value}"))?,
                );
            }
            "--resolve-reject" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--resolve-reject requires an integer id".to_string())?;
                resolve_reject = Some(
                    value
                        .parse::<i64>()
                        .map_err(|_err| format!("invalid --resolve-reject value: {value}"))?,
                );
            }
            "--resolve-class" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--resolve-class requires an integer id".to_string())?;
                resolve_class = Some(
                    value
                        .parse::<i64>()
                        .map_err(|_err| format!("invalid --resolve-class value: {value}"))?,
                );
            }
            "--resolve-link" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--resolve-link requires an integer id".to_string())?;
                resolve_link = Some(
                    value
                        .parse::<i64>()
                        .map_err(|_err| format!("invalid --resolve-link value: {value}"))?,
                );
            }
            "--resolve-block-link" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--resolve-block-link requires an integer id".to_string())?;
                resolve_block_link = Some(
                    value
                        .parse::<i64>()
                        .map_err(|_err| format!("invalid --resolve-block-link value: {value}"))?,
                );
            }
            "--target-wallet" => {
                target_wallet = Some(
                    args.next()
                        .ok_or_else(|| "--target-wallet requires a wallet id".to_string())?,
                );
            }
            "--class" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--class requires a value".to_string())?;
                if !WALLET_CLASS_VALUES.contains(&value.as_str()) {
                    return Err(format!(
                        "invalid --class value: {value} (must be one of: {})",
                        WALLET_CLASS_VALUES.join(", ")
                    ));
                }
                class_value = Some(value);
            }
            "--artist" => {
                artist_id = Some(
                    args.next()
                        .ok_or_else(|| "--artist requires an artist id".to_string())?,
                );
            }
            "--json" => {
                json = true;
            }
            "--help" | "-h" => {
                println!(
                    "Usage: review_wallet_identity [OPTIONS]\n\n\
                     Options:\n\
                     --db PATH              Database path (default: ./stophammer.db)\n\
                     --limit N              Limit results (default: 50)\n\
                     --high-confidence-only Filter pending list to confidence=high_confidence\n\
                     --json                 Output JSON\n\n\
                     Display:\n\
                     (default)              List pending wallet reviews with confidence, explanation, and scored supporting_sources\n\
                     --show-review ID       Show review detail with wallet info\n\
                     --show-wallet ID       Show wallet detail\n\n\
                     Resolve:\n\
                     --resolve-merge ID --target-wallet WID   Merge override\n\
                     --resolve-reject ID                      Do-not-merge override\n\
                     --resolve-class ID --class CLASS         Force classification\n\
                     --resolve-link ID --artist AID           Force artist link\n\
                     --resolve-block-link ID --artist AID     Block artist link"
                );
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    Ok(Args {
        db_path,
        limit,
        high_confidence_only,
        show_review,
        show_wallet,
        resolve_merge,
        resolve_reject,
        resolve_class,
        resolve_link,
        resolve_block_link,
        target_wallet,
        class_value,
        artist_id,
        json,
    })
}

fn print_pending_reviews(reviews: &[stophammer::db::WalletReviewSummary]) {
    if reviews.is_empty() {
        println!("review_wallet_identity: no pending wallet reviews found");
        return;
    }

    let mut high_confidence = 0usize;
    let mut review_required = 0usize;
    let mut blocked = 0usize;
    for review in reviews {
        match review.confidence.as_str() {
            "high_confidence" => high_confidence += 1,
            "review_required" => review_required += 1,
            "blocked" => blocked += 1,
            _ => {}
        }
    }
    let scored_count = reviews
        .iter()
        .filter(|review| review.score.is_some())
        .count();
    let top_scored = reviews
        .iter()
        .filter_map(|review| review.score.map(|score| (score, review.source.as_str())))
        .max_by_key(|(score, _source)| *score);
    let mut score_band_counts = std::collections::BTreeMap::<&str, usize>::new();
    let mut conflict_reason_counts = std::collections::BTreeMap::<&str, usize>::new();
    for review in reviews {
        *score_band_counts
            .entry(score_band(review.score))
            .or_default() += 1;
        for reason in &review.conflict_reasons {
            *conflict_reason_counts.entry(reason).or_default() += 1;
        }
    }
    println!("pending summary: HIGH={high_confidence} REVIEW={review_required} BLOCKED={blocked}");
    println!(
        "score summary: SCORED={scored_count}/{}  TOP_SCORE={}  TOP_SOURCE={}",
        reviews.len(),
        top_scored.map_or_else(|| "-".to_string(), |(score, _source)| score.to_string()),
        top_scored.map_or("-", |(_score, source)| source)
    );
    println!(
        "score bands: {}",
        ["80_100", "60_79", "1_59", "unscored"]
            .into_iter()
            .filter_map(|band| score_band_counts
                .get(band)
                .map(|count| format!("{band}={count}")))
            .collect::<Vec<_>>()
            .join(" ")
    );
    if !conflict_reason_counts.is_empty() {
        println!(
            "conflicts: {}",
            conflict_reason_counts
                .into_iter()
                .map(|(reason, count)| format!("{reason}={count}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
    }
    println!();

    for r in reviews {
        println!(
            "review {}  wallet={}  name={:?}  class={}  class_confidence={}  review_confidence={}  score={}",
            r.id,
            r.wallet_id,
            r.display_name,
            r.wallet_class,
            r.class_confidence,
            r.confidence,
            r.score
                .map_or_else(|| "-".to_string(), |score| score.to_string())
        );
        println!(
            "  source={}  evidence_key={:?}  related_wallets={}",
            r.source,
            r.evidence_key,
            r.wallet_ids.len()
        );
        println!("  explanation={}", r.explanation);
        if !r.supporting_sources.is_empty() {
            println!("  supporting_sources={}", r.supporting_sources.join(", "));
        }
        if !r.conflict_reasons.is_empty() {
            println!("  conflict_reasons={}", r.conflict_reasons.join(", "));
        }
        if !r.score_breakdown.is_empty() {
            println!(
                "  score_breakdown={}",
                format_score_breakdown(&r.score_breakdown)
            );
        }
    }
}

fn print_wallet_detail(w: &stophammer::db::WalletDetail) {
    println!(
        "wallet {}  name={:?}  class={}  confidence={}",
        w.wallet_id, w.display_name, w.wallet_class, w.class_confidence
    );
    println!("  created_at={}  updated_at={}", w.created_at, w.updated_at);

    if w.endpoints.is_empty() {
        println!("  endpoints: -");
    } else {
        println!("  endpoints ({}):", w.endpoints.len());
        for ep in &w.endpoints {
            if ep.custom_key.is_empty() && ep.custom_value.is_empty() {
                println!(
                    "    [{}] {} {}",
                    ep.id, ep.route_type, ep.normalized_address
                );
            } else {
                println!(
                    "    [{}] {} {} key={} val={}",
                    ep.id, ep.route_type, ep.normalized_address, ep.custom_key, ep.custom_value
                );
            }
        }
    }

    if w.aliases.is_empty() {
        println!("  aliases: -");
    } else {
        println!("  aliases ({}):", w.aliases.len());
        for a in &w.aliases {
            println!(
                "    {:?}  first_seen={}  last_seen={}",
                a.alias, a.first_seen_at, a.last_seen_at
            );
        }
    }

    if w.artist_links.is_empty() {
        println!("  artist_links: -");
    } else {
        println!("  artist_links ({}):", w.artist_links.len());
        for link in &w.artist_links {
            println!(
                "    artist={}  confidence={}  evidence={}:{}",
                link.artist_id, link.confidence, link.evidence_entity_type, link.evidence_entity_id
            );
            println!("      why={}", link.evidence_explanation);
        }
    }

    if w.feed_guids.is_empty() {
        println!("  feeds: -");
    } else {
        println!("  feeds ({}):", w.feed_guids.len());
        for fg in &w.feed_guids {
            println!("    {fg}");
        }
    }

    if !w.overrides.is_empty() {
        println!("  overrides ({}):", w.overrides.len());
        for o in &w.overrides {
            println!(
                "    [{}] {}  target={:?}  value={:?}  at={}",
                o.id, o.override_type, o.target_id, o.value, o.created_at
            );
        }
    }
}

fn resolve_action(
    conn: &Connection,
    review_id: i64,
    override_type: &str,
    target_id: Option<&str>,
    value: Option<&str>,
    json: bool,
) -> Result<(), Box<dyn Error>> {
    let outcome = stophammer::db::apply_wallet_identity_review_action(
        conn,
        review_id,
        override_type,
        target_id,
        value,
    )?;
    println!(
        "Resolved review {review_id} with action: {override_type} (status={})",
        outcome.review.status
    );

    // Show the wallet detail after resolution
    let wallet_id: String = conn.query_row(
        "SELECT wallet_id FROM wallet_identity_review WHERE id = ?1",
        rusqlite::params![review_id],
        |r| r.get(0),
    )?;
    if let Some(detail) = stophammer::db::get_wallet_detail(conn, &wallet_id)? {
        if json {
            println!("{}", serde_json::to_string_pretty(&detail)?);
        } else {
            print_wallet_detail(&detail);
        }
    }
    Ok(())
}

fn show_review(
    conn: &rusqlite::Connection,
    review_id: i64,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let wallet_id: String = conn
        .query_row(
            "SELECT wallet_id FROM wallet_identity_review WHERE id = ?1",
            rusqlite::params![review_id],
            |r| r.get(0),
        )
        .map_err(|_err| std::io::Error::other(format!("review not found: {review_id}")))?;
    let detail = stophammer::db::get_wallet_detail(conn, &wallet_id)?
        .ok_or_else(|| std::io::Error::other(format!("wallet not found: {wallet_id}")))?;

    let review_summary = stophammer::db::get_wallet_review_summary(conn, review_id)?
        .ok_or_else(|| std::io::Error::other(format!("review not found: {review_id}")))?;

    if json {
        let report = ReviewDetailReport {
            review: review_summary,
            wallet: detail,
        };
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "review {}  source={}  review_confidence={}  score={}  status=pending  evidence_key={:?}",
            review_summary.id,
            review_summary.source,
            review_summary.confidence,
            review_summary
                .score
                .map_or_else(|| "-".to_string(), |score| score.to_string()),
            review_summary.evidence_key
        );
        println!("  explanation={}", review_summary.explanation);
        if !review_summary.supporting_sources.is_empty() {
            println!(
                "  supporting_sources={}",
                review_summary.supporting_sources.join(", ")
            );
        }
        if !review_summary.conflict_reasons.is_empty() {
            println!(
                "  conflict_reasons={}",
                review_summary.conflict_reasons.join(", ")
            );
        }
        if !review_summary.score_breakdown.is_empty() {
            println!(
                "  score_breakdown={}",
                format_score_breakdown(&review_summary.score_breakdown)
            );
        }
        print_wallet_detail(&detail);
    }
    Ok(())
}

fn show_wallet(
    conn: &rusqlite::Connection,
    wallet_id: &str,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let resolved_id = {
        let mut current = wallet_id.to_string();
        loop {
            let redirect: Option<String> = conn
                .query_row(
                    "SELECT new_wallet_id FROM wallet_id_redirect WHERE old_wallet_id = ?1",
                    rusqlite::params![current],
                    |r| r.get(0),
                )
                .ok();
            match redirect {
                Some(new_id) => current = new_id,
                None => break current,
            }
        }
    };
    let detail = stophammer::db::get_wallet_detail(conn, &resolved_id)?
        .ok_or_else(|| std::io::Error::other(format!("wallet not found: {resolved_id}")))?;
    if json {
        let report = WalletDetailReport { wallet: detail };
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        if resolved_id != wallet_id {
            println!("(redirected from {wallet_id})");
        }
        print_wallet_detail(&detail);
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args().map_err(std::io::Error::other)?;
    let conn = stophammer::db::open_db(&args.db_path);

    if let Some(review_id) = args.resolve_merge {
        let target = args
            .target_wallet
            .as_deref()
            .ok_or_else(|| std::io::Error::other("--resolve-merge requires --target-wallet"))?;
        resolve_action(&conn, review_id, "merge", Some(target), None, args.json)?;
    } else if let Some(review_id) = args.resolve_reject {
        resolve_action(&conn, review_id, "do_not_merge", None, None, args.json)?;
    } else if let Some(review_id) = args.resolve_class {
        let class = args
            .class_value
            .as_deref()
            .ok_or_else(|| std::io::Error::other("--resolve-class requires --class"))?;
        resolve_action(
            &conn,
            review_id,
            "force_class",
            None,
            Some(class),
            args.json,
        )?;
    } else if let Some(review_id) = args.resolve_link {
        let artist = args
            .artist_id
            .as_deref()
            .ok_or_else(|| std::io::Error::other("--resolve-link requires --artist"))?;
        resolve_action(
            &conn,
            review_id,
            "force_artist_link",
            Some(artist),
            None,
            args.json,
        )?;
    } else if let Some(review_id) = args.resolve_block_link {
        let artist = args
            .artist_id
            .as_deref()
            .ok_or_else(|| std::io::Error::other("--resolve-block-link requires --artist"))?;
        resolve_action(
            &conn,
            review_id,
            "block_artist_link",
            Some(artist),
            None,
            args.json,
        )?;
    } else if let Some(review_id) = args.show_review {
        show_review(&conn, review_id, args.json)?;
    } else if let Some(wallet_id) = args.show_wallet.as_deref() {
        show_wallet(&conn, wallet_id, args.json)?;
    } else {
        let mut reviews = stophammer::db::list_pending_wallet_reviews(&conn, args.limit)?;
        if args.high_confidence_only {
            reviews.retain(|review| review.confidence == "high_confidence");
        }
        if args.json {
            let report = PendingReviewsReport { reviews };
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            print_pending_reviews(&reviews);
        }
    }

    Ok(())
}
