use std::collections::BTreeSet;
use std::error::Error;
use std::path::PathBuf;

use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use stophammer::db::DEFAULT_DB_PATH;

#[derive(Debug)]
struct Args {
    db_path: PathBuf,
    limit: usize,
    name_filter: Option<String>,
    feed_guid: Option<String>,
    pending_feeds: bool,
    pending_reviews: bool,
    show_review: Option<i64>,
    merge_review: Option<i64>,
    reject_review: Option<i64>,
    target_artist: Option<String>,
    note: Option<String>,
    json: bool,
}

#[derive(Debug, Serialize)]
struct ReviewReport {
    groups: Vec<ArtistNameGroup>,
}

#[derive(Debug, Serialize)]
struct FeedPlanReport {
    plan: stophammer::db::ArtistIdentityFeedPlan,
}

#[derive(Debug, Serialize)]
struct PendingFeedsReport {
    feeds: Vec<stophammer::db::ArtistIdentityPendingFeed>,
}

#[derive(Debug, Serialize)]
struct PendingReviewsReport {
    reviews: Vec<stophammer::db::ArtistIdentityPendingReview>,
}

#[derive(Debug, Serialize)]
struct ReviewItemReport {
    review: stophammer::db::ArtistIdentityReviewItem,
}

#[derive(Debug, Serialize)]
struct ArtistNameGroup {
    name_key: String,
    artists: Vec<ArtistReviewRow>,
}

#[derive(Debug, Serialize)]
struct ArtistReviewRow {
    artist_id: String,
    name: String,
    created_at: i64,
    feed_count: i64,
    release_count: i64,
    external_ids: Vec<String>,
    feeds: Vec<FeedEvidenceRow>,
}

#[derive(Debug, Serialize)]
struct FeedEvidenceRow {
    feed_guid: String,
    title: String,
    feed_url: String,
    canonical_release_id: Option<String>,
    canonical_match_type: Option<String>,
    canonical_confidence: Option<i64>,
    platforms: Vec<String>,
    website_links: Vec<String>,
    npubs: Vec<String>,
    publisher_remote_feed_guids: Vec<String>,
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
    let mut limit = 20usize;
    let mut name_filter = None;
    let mut feed_guid = None;
    let mut pending_feeds = false;
    let mut pending_reviews = false;
    let mut show_review = None;
    let mut merge_review = None;
    let mut reject_review = None;
    let mut target_artist = None;
    let mut note = None;
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
            "--name" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--name requires a value".to_string())?;
                name_filter = Some(value);
            }
            "--feed-guid" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--feed-guid requires a value".to_string())?;
                feed_guid = Some(value);
            }
            "--pending-feeds" => {
                pending_feeds = true;
            }
            "--pending-reviews" => {
                pending_reviews = true;
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
            "--merge-review" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--merge-review requires an integer id".to_string())?;
                merge_review = Some(
                    value
                        .parse::<i64>()
                        .map_err(|_err| format!("invalid --merge-review value: {value}"))?,
                );
            }
            "--reject-review" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--reject-review requires an integer id".to_string())?;
                reject_review = Some(
                    value
                        .parse::<i64>()
                        .map_err(|_err| format!("invalid --reject-review value: {value}"))?,
                );
            }
            "--target-artist" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--target-artist requires an artist id".to_string())?;
                target_artist = Some(value);
            }
            "--note" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--note requires a value".to_string())?;
                note = Some(value);
            }
            "--json" => {
                json = true;
            }
            "--help" | "-h" => {
                println!(
                    "Usage: review_artist_identity [--db PATH] [--limit N] [--name NAME] [--feed-guid GUID] [--pending-feeds] [--pending-reviews] [--show-review ID] [--merge-review ID --target-artist ARTIST_ID] [--reject-review ID] [--note TEXT] [--json]\n\
                     Reports duplicate artist-name groups and the current source evidence\n\
                     behind each candidate so merge decisions can be reviewed safely.\n\
                     Pending and review views include confidence, explanation, and any scored supporting_sources.\n\
                     With --feed-guid, prints the targeted resolver plan for one feed.\n\
                     With --pending-feeds, lists feeds whose targeted plan still has candidate groups.\n\
                     With --pending-reviews or --show-review, inspects stored resolver review items.\n\
                     With --merge-review or --reject-review, stores a durable override."
                );
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    Ok(Args {
        db_path,
        limit,
        name_filter,
        feed_guid,
        pending_feeds,
        pending_reviews,
        show_review,
        merge_review,
        reject_review,
        target_artist,
        note,
        json,
    })
}

fn duplicate_name_keys(
    conn: &Connection,
    limit: usize,
    name_filter: Option<&str>,
) -> Result<Vec<String>, rusqlite::Error> {
    if let Some(name) = name_filter {
        let mut stmt = conn.prepare(
            "SELECT LOWER(name) AS name_key \
             FROM artists \
             WHERE LOWER(name) = LOWER(?1) \
             GROUP BY LOWER(name) \
             HAVING COUNT(*) > 1",
        )?;
        stmt.query_map([name], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()
    } else {
        let limit_i64 = i64::try_from(limit).map_err(|_err| {
            rusqlite::Error::ToSqlConversionFailure(
                "review limit exceeded supported SQLite integer range"
                    .to_string()
                    .into(),
            )
        })?;
        let mut stmt = conn.prepare(
            "SELECT LOWER(name) AS name_key \
             FROM artists \
             GROUP BY LOWER(name) \
             HAVING COUNT(*) > 1 \
             ORDER BY COUNT(*) DESC, name_key \
             LIMIT ?1",
        )?;
        stmt.query_map([limit_i64], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()
    }
}

fn artist_rows_for_name(
    conn: &Connection,
    name_key: &str,
) -> Result<Vec<(String, String, i64)>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT artist_id, name, created_at \
         FROM artists \
         WHERE LOWER(name) = ?1 \
         ORDER BY created_at, artist_id",
    )?;
    stmt.query_map([name_key], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })?
    .collect::<Result<Vec<_>, _>>()
}

fn feed_rows_for_artist(
    conn: &Connection,
    artist_id: &str,
) -> Result<Vec<(String, String, String)>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT f.feed_guid, f.title, f.feed_url \
         FROM artist_credit_name acn \
         JOIN artist_credit ac ON ac.id = acn.artist_credit_id \
         JOIN feeds f ON f.artist_credit_id = ac.id \
         WHERE acn.artist_id = ?1 \
         ORDER BY f.title_lower, f.feed_guid",
    )?;
    stmt.query_map([artist_id], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })?
    .collect::<Result<Vec<_>, _>>()
}

fn external_ids_for_artist(
    conn: &Connection,
    artist_id: &str,
) -> Result<Vec<String>, Box<dyn Error>> {
    Ok(stophammer::db::get_external_ids(conn, "artist", artist_id)?
        .into_iter()
        .map(|row| format!("{}={}", row.scheme, row.value))
        .collect())
}

fn release_count_for_artist(conn: &Connection, artist_id: &str) -> Result<i64, rusqlite::Error> {
    conn.query_row(
        "SELECT COUNT(DISTINCT sfr.release_id) \
         FROM artist_credit_name acn \
         JOIN artist_credit ac ON ac.id = acn.artist_credit_id \
         JOIN feeds f ON f.artist_credit_id = ac.id \
         JOIN source_feed_release_map sfr ON sfr.feed_guid = f.feed_guid \
         WHERE acn.artist_id = ?1",
        [artist_id],
        |row| row.get(0),
    )
}

fn feed_count_for_artist(conn: &Connection, artist_id: &str) -> Result<i64, rusqlite::Error> {
    conn.query_row(
        "SELECT COUNT(DISTINCT f.feed_guid) \
         FROM artist_credit_name acn \
         JOIN artist_credit ac ON ac.id = acn.artist_credit_id \
         JOIN feeds f ON f.artist_credit_id = ac.id \
         WHERE acn.artist_id = ?1",
        [artist_id],
        |row| row.get(0),
    )
}

fn feed_evidence_row(
    conn: &Connection,
    feed_guid: &str,
    title: String,
    feed_url: String,
) -> Result<FeedEvidenceRow, Box<dyn Error>> {
    let canonical = conn
        .query_row(
            "SELECT release_id, match_type, confidence \
             FROM source_feed_release_map WHERE feed_guid = ?1",
            [feed_guid],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()?;

    let platforms = stophammer::db::get_source_platform_claims_for_feed(conn, feed_guid)?
        .into_iter()
        .map(|claim| claim.platform_key)
        .filter(|value| !value.trim().is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    let website_links =
        stophammer::db::get_source_entity_links_for_entity(conn, "feed", feed_guid)?
            .into_iter()
            .filter(|claim| claim.link_type == "website")
            .map(|claim| claim.url)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();

    let npubs = stophammer::db::get_source_entity_ids_for_entity(conn, "feed", feed_guid)?
        .into_iter()
        .filter(|claim| claim.scheme == "nostr_npub")
        .map(|claim| claim.value)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    let publisher_remote_feed_guids =
        stophammer::db::get_feed_remote_items_for_feed(conn, feed_guid)?
            .into_iter()
            .filter(|item| item.medium.as_deref() == Some("publisher"))
            .map(|item| item.remote_feed_guid)
            .filter(|value| !value.trim().is_empty())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();

    Ok(FeedEvidenceRow {
        feed_guid: feed_guid.to_string(),
        title,
        feed_url,
        canonical_release_id: canonical.as_ref().map(|row| row.0.clone()),
        canonical_match_type: canonical.as_ref().map(|row| row.1.clone()),
        canonical_confidence: canonical.as_ref().map(|row| row.2),
        platforms,
        website_links,
        npubs,
        publisher_remote_feed_guids,
    })
}

fn build_report(
    conn: &Connection,
    limit: usize,
    name_filter: Option<&str>,
) -> Result<ReviewReport, Box<dyn std::error::Error>> {
    let name_keys = duplicate_name_keys(conn, limit, name_filter)?;
    let mut groups = Vec::new();

    for name_key in name_keys {
        let artist_rows = artist_rows_for_name(conn, &name_key)?;
        let mut artists = Vec::new();

        for (artist_id, name, created_at) in artist_rows {
            let feed_rows = feed_rows_for_artist(conn, &artist_id)?;
            let mut feeds = Vec::new();
            for (feed_guid, title, feed_url) in feed_rows {
                feeds.push(feed_evidence_row(conn, &feed_guid, title, feed_url)?);
            }

            artists.push(ArtistReviewRow {
                artist_id: artist_id.clone(),
                name,
                created_at,
                feed_count: feed_count_for_artist(conn, &artist_id)?,
                release_count: release_count_for_artist(conn, &artist_id)?,
                external_ids: external_ids_for_artist(conn, &artist_id)?,
                feeds,
            });
        }

        groups.push(ArtistNameGroup { name_key, artists });
    }

    Ok(ReviewReport { groups })
}

fn build_feed_plan_report(
    conn: &Connection,
    feed_guid: &str,
) -> Result<FeedPlanReport, Box<dyn Error>> {
    Ok(FeedPlanReport {
        plan: stophammer::db::explain_artist_identity_for_feed(conn, feed_guid)?,
    })
}

fn build_pending_feeds_report(
    conn: &Connection,
    limit: usize,
) -> Result<PendingFeedsReport, Box<dyn Error>> {
    Ok(PendingFeedsReport {
        feeds: stophammer::db::list_pending_artist_identity_feeds(conn, limit)?,
    })
}

fn build_pending_reviews_report(
    conn: &Connection,
    limit: usize,
) -> Result<PendingReviewsReport, Box<dyn Error>> {
    Ok(PendingReviewsReport {
        reviews: stophammer::db::list_pending_artist_identity_reviews(conn, limit)?,
    })
}

fn build_review_item_report(
    conn: &Connection,
    review_id: i64,
) -> Result<ReviewItemReport, Box<dyn Error>> {
    let review = stophammer::db::get_artist_identity_review(conn, review_id)?
        .ok_or_else(|| std::io::Error::other(format!("review not found: {review_id}")))?;
    Ok(ReviewItemReport { review })
}

fn print_text(report: &ReviewReport) {
    if report.groups.is_empty() {
        println!("review_artist_identity: no duplicate-name artist groups found");
        return;
    }

    for group in &report.groups {
        println!("name: {} ({} artists)", group.name_key, group.artists.len());
        for artist in &group.artists {
            println!(
                "  artist {}  feeds={} releases={} created_at={}",
                artist.artist_id, artist.feed_count, artist.release_count, artist.created_at
            );
            if artist.external_ids.is_empty() {
                println!("    external_ids: -");
            } else {
                println!("    external_ids: {}", artist.external_ids.join(", "));
            }
            for feed in &artist.feeds {
                println!(
                    "    feed {}  title={:?}  release={:?}  match={:?}  confidence={:?}",
                    feed.feed_guid,
                    feed.title,
                    feed.canonical_release_id,
                    feed.canonical_match_type,
                    feed.canonical_confidence
                );
                if !feed.platforms.is_empty() {
                    println!("      platforms: {}", feed.platforms.join(", "));
                }
                if !feed.website_links.is_empty() {
                    println!("      websites: {}", feed.website_links.join(", "));
                }
                if !feed.npubs.is_empty() {
                    println!("      npubs: {}", feed.npubs.join(", "));
                }
                if !feed.publisher_remote_feed_guids.is_empty() {
                    println!(
                        "      publisher_remote_feed_guids: {}",
                        feed.publisher_remote_feed_guids.join(", ")
                    );
                }
            }
        }
        println!();
    }
}

fn print_feed_plan_text(report: &FeedPlanReport) {
    println!("feed: {}", report.plan.feed_guid);
    if report.plan.seed_artists.is_empty() {
        println!("  seed_artists: -");
    } else {
        println!("  seed_artists:");
        for artist in &report.plan.seed_artists {
            println!("    {}  {:?}", artist.artist_id, artist.name);
        }
    }

    if report.plan.candidate_groups.is_empty() {
        println!("  candidate_groups: -");
        return;
    }

    println!("  candidate_groups: {}", report.plan.candidate_groups.len());
    for group in &report.plan.candidate_groups {
        println!(
            "    source={}  name_key={:?}  evidence_key={:?}  artists={}",
            group.source,
            group.name_key,
            group.evidence_key,
            group.artist_ids.len()
        );
        if !group.artist_names.is_empty() {
            println!("      names: {}", group.artist_names.join(", "));
        }
        println!("      artist_ids: {}", group.artist_ids.join(", "));
        if !group.supporting_sources.is_empty() {
            println!(
                "      supporting_sources: {}",
                group.supporting_sources.join(", ")
            );
        }
        if let Some(review_id) = group.review_id {
            println!(
                "      review_id={}  status={:?}  confidence={:?}  override={:?}  target={:?}",
                review_id,
                group.review_status,
                group.confidence,
                group.override_type,
                group.target_artist_id
            );
        }
        if let Some(explanation) = &group.explanation {
            println!("      explanation: {explanation}");
        }
        if let Some(note) = &group.note {
            println!("      note: {note}");
        }
    }
}

fn print_pending_feeds_text(report: &PendingFeedsReport) {
    if report.feeds.is_empty() {
        println!("review_artist_identity: no pending feed-scoped artist identity candidates found");
        return;
    }

    for feed in &report.feeds {
        println!(
            "feed {}  title={:?}  seed_artists={}  candidate_groups={}",
            feed.feed_guid, feed.title, feed.seed_artists, feed.candidate_groups
        );
        println!("  url={}", feed.feed_url);
    }
}

fn print_pending_reviews_text(report: &PendingReviewsReport) {
    if report.reviews.is_empty() {
        println!("review_artist_identity: no pending artist identity reviews found");
        return;
    }

    let mut high_confidence = 0usize;
    let mut review_required = 0usize;
    let mut blocked = 0usize;
    for review in &report.reviews {
        match review.confidence.as_str() {
            "high_confidence" => high_confidence += 1,
            "review_required" => review_required += 1,
            "blocked" => blocked += 1,
            _ => {}
        }
    }
    let scored_count = report
        .reviews
        .iter()
        .filter(|review| review.score.is_some())
        .count();
    let top_scored = report
        .reviews
        .iter()
        .filter_map(|review| review.score.map(|score| (score, review.source.as_str())))
        .max_by_key(|(score, _source)| *score);
    let mut score_band_counts = std::collections::BTreeMap::<&str, usize>::new();
    for review in &report.reviews {
        *score_band_counts.entry(score_band(review.score)).or_default() += 1;
    }
    println!(
        "pending summary: HIGH={high_confidence} REVIEW={review_required} BLOCKED={blocked}"
    );
    println!(
        "score summary: SCORED={scored_count}/{}  TOP_SCORE={}  TOP_SOURCE={}",
        report.reviews.len(),
        top_scored
            .map_or_else(|| "-".to_string(), |(score, _source)| score.to_string()),
        top_scored.map_or("-", |(_score, source)| source)
    );
    println!(
        "score bands: {}",
        ["80_100", "60_79", "1_59", "unscored"]
            .into_iter()
            .filter_map(|band| score_band_counts.get(band).map(|count| format!("{band}={count}")))
            .collect::<Vec<_>>()
            .join(" ")
    );
    println!();

    for review in &report.reviews {
        println!(
            "review {}  feed={}  title={:?}  source={}  confidence={}  score={}  name_key={:?}  artists={}",
            review.review_id,
            review.feed_guid,
            review.title,
            review.source,
            review.confidence,
            review
                .score
                .map_or_else(|| "-".to_string(), |score| score.to_string()),
            review.name_key,
            review.artist_count
        );
        println!("  evidence_key={}", review.evidence_key);
        println!("  explanation={}", review.explanation);
        if !review.supporting_sources.is_empty() {
            println!(
                "  supporting_sources={}",
                review.supporting_sources.join(", ")
            );
        }
        if !review.score_breakdown.is_empty() {
            println!(
                "  score_breakdown={}",
                format_score_breakdown(&review.score_breakdown)
            );
        }
    }
}

fn print_review_item_text(report: &ReviewItemReport) {
    let review = &report.review;
    println!(
        "review {}  feed={}  source={}  confidence={}  score={}  status={}",
        review.review_id,
        review.feed_guid,
        review.source,
        review.confidence,
        review
            .score
            .map_or_else(|| "-".to_string(), |score| score.to_string()),
        review.status
    );
    println!("  name_key={}", review.name_key);
    println!("  evidence_key={}", review.evidence_key);
    println!("  explanation={}", review.explanation);
    if !review.supporting_sources.is_empty() {
        println!(
            "  supporting_sources={}",
            review.supporting_sources.join(", ")
        );
    }
    if !review.score_breakdown.is_empty() {
        println!(
            "  score_breakdown={}",
            format_score_breakdown(&review.score_breakdown)
        );
    }
    println!("  artist_ids={}", review.artist_ids.join(", "));
    if !review.artist_names.is_empty() {
        println!("  artist_names={}", review.artist_names.join(", "));
    }
    println!(
        "  override={:?}  target_artist_id={:?}",
        review.override_type, review.target_artist_id
    );
    if let Some(note) = &review.note {
        println!("  note={note}");
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args().map_err(std::io::Error::other)?;
    let mut conn = stophammer::db::open_db(&args.db_path);
    if let Some(review_id) = args.merge_review {
        let target_artist = args
            .target_artist
            .as_deref()
            .ok_or_else(|| std::io::Error::other("--merge-review requires --target-artist"))?;
        stophammer::db::apply_artist_identity_review_action(
            &mut conn,
            review_id,
            "merge",
            Some(target_artist),
            args.note.as_deref(),
        )?;
        let report = build_review_item_report(&conn, review_id)?;
        if args.json {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            print_review_item_text(&report);
        }
    } else if let Some(review_id) = args.reject_review {
        stophammer::db::apply_artist_identity_review_action(
            &mut conn,
            review_id,
            "do_not_merge",
            None,
            args.note.as_deref(),
        )?;
        let report = build_review_item_report(&conn, review_id)?;
        if args.json {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            print_review_item_text(&report);
        }
    } else if let Some(review_id) = args.show_review {
        let report = build_review_item_report(&conn, review_id)?;
        if args.json {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            print_review_item_text(&report);
        }
    } else if let Some(feed_guid) = args.feed_guid.as_deref() {
        let report = build_feed_plan_report(&conn, feed_guid)?;
        if args.json {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            print_feed_plan_text(&report);
        }
    } else if args.pending_reviews {
        let report = build_pending_reviews_report(&conn, args.limit)?;
        if args.json {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            print_pending_reviews_text(&report);
        }
    } else if args.pending_feeds {
        let report = build_pending_feeds_report(&conn, args.limit)?;
        if args.json {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            print_pending_feeds_text(&report);
        }
    } else {
        let report = build_report(&conn, args.limit, args.name_filter.as_deref())?;
        if args.json {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            print_text(&report);
        }
    }

    Ok(())
}
