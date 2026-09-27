//! ADR 0064 sections 3, 4 and 6: the pure rules for a live item.
//!
//! A live item is `<podcast:liveItem>` from RSS. The ingest calls the rules
//! of section 4 to drop an item that section bans, and to cap the row count
//! of a feed. A read calls the rules of sections 3 and 6 to mark a
//! confirming relay and to place a row in the `now` and `upcoming` views.
//! Each function reads only the values it is given. None of them opens a
//! database connection or reads the clock.

use std::collections::HashSet;
use std::sync::OnceLock;

use crate::ingest::IngestLiveItemData;
use crate::model::LiveEvent;

/// ADR 0064 section 4: a feed keeps at most this many `pending` and `live`
/// rows. The ingest keeps the first ones in RSS order.
pub const MAX_ACTIVE_LIVE_ROWS: usize = 10;

/// ADR 0064 section 4: a feed keeps at most this many `ended` rows. The
/// ingest keeps the ones with the newest `start_at`.
pub const MAX_ENDED_LIVE_ROWS: usize = 10;

/// ADR 0064 section 4: a `live` item with no relay link needs a `start_at`
/// and an `end_at` at most this many seconds apart.
pub const MAX_UNRELAYED_LIVE_SECS: i64 = 6 * 3600;

/// `true` when `item` names a relay link: `live_value_uri` is present and
/// not empty.
#[must_use]
pub fn has_relay_link(item: &IngestLiveItemData) -> bool {
    item.live_value_uri
        .as_deref()
        .is_some_and(|uri| !uri.is_empty())
}

/// `true` when the ADR 0064 section 4 ban applies to `item`.
///
/// The ban keeps out a `live` item with no relay link when it has no
/// `start_at`, no `end_at`, or an `end_at` more than
/// [`MAX_UNRELAYED_LIVE_SECS`] after its `start_at`. An `end_at` exactly
/// [`MAX_UNRELAYED_LIVE_SECS`] after `start_at` passes. The ban does not
/// apply to a `pending` or an `ended` item, and it does not apply to a
/// `live` item that names a relay link.
#[must_use]
pub fn is_banned_live_item(item: &IngestLiveItemData) -> bool {
    if !item.status.eq_ignore_ascii_case("live") || has_relay_link(item) {
        return false;
    }
    match (item.start_at, item.end_at) {
        (Some(start_at), Some(end_at)) => end_at - start_at > MAX_UNRELAYED_LIVE_SECS,
        _ => true,
    }
}

/// Applies the ADR 0064 section 4 selection rules to the live items of one
/// feed, in the order the feed gave them.
///
/// The steps run in this order:
///
/// 1. Drop an item whose `status` is not `pending`, `live` or `ended`
///    (compared case-insensitively). The parser accepts any non-empty
///    status, but the `live_events` table accepts only these three.
/// 2. Keep the first item of each `live_item_guid`. Drop a later item with
///    the same GUID.
/// 3. Drop each item that [`is_banned_live_item`] bans.
/// 4. Keep the first [`MAX_ACTIVE_LIVE_ROWS`] `pending` and `live` items, in
///    RSS order. Drop the rest.
/// 5. Keep the [`MAX_ENDED_LIVE_ROWS`] `ended` items with the newest
///    `start_at`. An item with no `start_at` sorts last. Drop the rest.
///
/// Each dropped item adds one warning that names the rule. The kept items
/// keep their relative RSS order.
#[must_use]
pub fn select_live_items(items: &[IngestLiveItemData]) -> (Vec<&IngestLiveItemData>, Vec<String>) {
    let mut warnings = Vec::new();

    // Step 1: drop an item whose status is not pending, live or ended. The
    // live_events table has a CHECK constraint on this column, so an
    // unknown status must never reach the insert.
    let mut known_status: Vec<&IngestLiveItemData> = Vec::with_capacity(items.len());
    for item in items {
        if matches!(
            item.status.to_ascii_lowercase().as_str(),
            "pending" | "live" | "ended"
        ) {
            known_status.push(item);
        } else {
            warnings.push(format!(
                "dropped live item {}: unknown status {} (ADR 0064 section 4)",
                item.live_item_guid, item.status
            ));
        }
    }

    // Step 2: the first item of each live_item_guid, in RSS order.
    let mut seen_guids: HashSet<&str> = HashSet::new();
    let mut deduped: Vec<&IngestLiveItemData> = Vec::with_capacity(known_status.len());
    for item in known_status {
        if seen_guids.insert(item.live_item_guid.as_str()) {
            deduped.push(item);
        } else {
            warnings.push(format!(
                "dropped duplicate live_item_guid {} (ADR 0064 section 4)",
                item.live_item_guid
            ));
        }
    }

    // Step 3: drop each banned item.
    let mut survivors: Vec<&IngestLiveItemData> = Vec::with_capacity(deduped.len());
    for item in deduped {
        if is_banned_live_item(item) {
            let hours = MAX_UNRELAYED_LIVE_SECS / 3600;
            warnings.push(format!(
                "dropped live item {}: no relay link and no start/end window of at most \
                 {hours} hours (ADR 0064 section 4)",
                item.live_item_guid
            ));
        } else {
            survivors.push(item);
        }
    }

    // Step 4: the first MAX_ACTIVE_LIVE_ROWS pending/live items, in RSS
    // order. An ended item does not count against this cap.
    let mut active_kept = 0usize;
    let mut kept_guids: HashSet<&str> = HashSet::new();
    for item in &survivors {
        if item.status.eq_ignore_ascii_case("ended") {
            continue;
        }
        if active_kept < MAX_ACTIVE_LIVE_ROWS {
            kept_guids.insert(item.live_item_guid.as_str());
            active_kept += 1;
        } else {
            warnings.push(format!(
                "dropped live item {}: more than {MAX_ACTIVE_LIVE_ROWS} pending or live items \
                 (ADR 0064 section 4)",
                item.live_item_guid
            ));
        }
    }

    // Step 5: the MAX_ENDED_LIVE_ROWS ended items with the newest start_at.
    // An item with no start_at sorts last.
    let mut ended: Vec<&IngestLiveItemData> = survivors
        .iter()
        .copied()
        .filter(|item| item.status.eq_ignore_ascii_case("ended"))
        .collect();
    ended.sort_by(|a, b| match (a.start_at, b.start_at) {
        (Some(a_start), Some(b_start)) => b_start.cmp(&a_start),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
    for (index, item) in ended.iter().enumerate() {
        if index < MAX_ENDED_LIVE_ROWS {
            kept_guids.insert(item.live_item_guid.as_str());
        } else {
            warnings.push(format!(
                "dropped live item {}: more than {MAX_ENDED_LIVE_ROWS} ended items \
                 (ADR 0064 section 4)",
                item.live_item_guid
            ));
        }
    }

    let kept: Vec<&IngestLiveItemData> = survivors
        .into_iter()
        .filter(|item| kept_guids.contains(item.live_item_guid.as_str()))
        .collect();

    (kept, warnings)
}

// ── The relay setting and the reads (ADR 0064 sections 3 and 6) ────────────

/// ADR 0064 section 3: the confirming relay hosts this node accepts.
///
/// `main.rs` calls [`set_confirming_relay_hosts`] once at start, with the
/// value of the `CONFIRMING_RELAY_HOSTS` environment variable. The list is
/// empty until that call runs, and stays empty when the variable is unset.
static CONFIRMING_RELAY_HOSTS: OnceLock<Vec<String>> = OnceLock::new();

/// Sets the confirming relay host list. Only the first call takes effect; a
/// later call is a no-op.
pub fn set_confirming_relay_hosts(hosts: Vec<String>) {
    let _ = CONFIRMING_RELAY_HOSTS.set(hosts);
}

/// Gives the confirming relay host list that [`set_confirming_relay_hosts`]
/// set. Gives an empty list before that call runs.
#[must_use]
pub fn confirming_relay_hosts() -> &'static [String] {
    CONFIRMING_RELAY_HOSTS.get().map_or(&[], Vec::as_slice)
}

/// ADR 0064 section 6: the number of seconds a `live` row with no confirming
/// relay stays in the `now` view after its `scheduled_end` passes.
pub const LIVE_END_MARGIN_SECS: i64 = 3600;

/// ADR 0064 section 6: the number of seconds a `pending` row with no
/// `scheduled_end` stays in the `upcoming` view after its `scheduled_start`
/// passes.
pub const PENDING_START_MARGIN_SECS: i64 = 3600;

/// `true` when `uri` names a confirming relay (ADR 0064 section 3).
///
/// A confirming relay is an `https` URL whose host is in `hosts`, compared
/// with no case difference. A `uri` with no scheme, such as a bare event
/// identifier, gives `false`. So does a `uri` with a scheme other than
/// `https`, or one whose host is not in `hosts`.
#[must_use]
pub fn is_confirming_relay(uri: Option<&str>, hosts: &[String]) -> bool {
    let Some(uri) = uri else {
        return false;
    };
    let Ok(parsed) = url::Url::parse(uri) else {
        return false;
    };
    if parsed.scheme() != "https" {
        return false;
    }
    let Some(host) = parsed.host_str() else {
        return false;
    };
    hosts.iter().any(|listed| listed.eq_ignore_ascii_case(host))
}

/// `true` when `row` belongs in the `now` view at `now` (ADR 0064 section
/// 6).
///
/// A `live` row is in the view when `confirming` is `true`, when it has no
/// `scheduled_end`, or when `now` is less than [`LIVE_END_MARGIN_SECS`]
/// seconds past its `scheduled_end`. A row of any other status is never in
/// the view.
#[must_use]
pub fn in_now_view(row: &LiveEvent, now: i64, confirming: bool) -> bool {
    if !row.status.eq_ignore_ascii_case("live") {
        return false;
    }
    if confirming {
        return true;
    }
    match row.scheduled_end {
        None => true,
        Some(scheduled_end) => now < scheduled_end + LIVE_END_MARGIN_SECS,
    }
}

/// `true` when `row` belongs in the `upcoming` view at `now` (ADR 0064
/// section 6).
///
/// A `pending` row is in the view while `now` is before its
/// `scheduled_end`. With no `scheduled_end`, it stays in the view while
/// `now` is less than [`PENDING_START_MARGIN_SECS`] seconds past its
/// `scheduled_start`. A row with neither time is never in the view, and
/// neither is a row of any other status.
#[must_use]
pub fn in_upcoming_view(row: &LiveEvent, now: i64) -> bool {
    if !row.status.eq_ignore_ascii_case("pending") {
        return false;
    }
    match row.scheduled_end {
        Some(scheduled_end) => now < scheduled_end,
        None => match row.scheduled_start {
            Some(scheduled_start) => now < scheduled_start + PENDING_START_MARGIN_SECS,
            None => false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{
        LIVE_END_MARGIN_SECS, MAX_ACTIVE_LIVE_ROWS, MAX_ENDED_LIVE_ROWS, MAX_UNRELAYED_LIVE_SECS,
        PENDING_START_MARGIN_SECS, has_relay_link, in_now_view, in_upcoming_view,
        is_banned_live_item, is_confirming_relay, select_live_items,
    };
    use crate::ingest::IngestLiveItemData;
    use crate::model::LiveEvent;

    /// Builds a minimal `IngestLiveItemData` for a test. Every field the
    /// rules under test do not read gets an empty default.
    fn item(
        guid: &str,
        status: &str,
        start_at: Option<i64>,
        end_at: Option<i64>,
        live_value_uri: Option<&str>,
    ) -> IngestLiveItemData {
        IngestLiveItemData {
            live_item_guid: guid.to_string(),
            title: format!("Live {guid}"),
            status: status.to_string(),
            start_at,
            end_at,
            content_link: None,
            pub_date: None,
            duration_secs: None,
            image_url: None,
            language: None,
            enclosure_url: None,
            enclosure_type: None,
            enclosure_bytes: None,
            alternate_enclosures: Vec::new(),
            track_number: None,
            season: None,
            explicit: false,
            description: None,
            author_name: None,
            live_value_uri: live_value_uri.map(str::to_string),
            live_value_protocol: None,
            remote_items: Vec::new(),
            persons: Vec::new(),
            entity_ids: Vec::new(),
            links: Vec::new(),
            payment_routes: Vec::new(),
            value_time_splits: Vec::new(),
            transcripts: Vec::new(),
        }
    }

    // -----------------------------------------------------------------
    // has_relay_link
    // -----------------------------------------------------------------

    #[test]
    fn has_relay_link_needs_a_non_empty_uri() {
        assert!(
            !has_relay_link(&item("g1", "live", None, None, None)),
            "no live_value_uri gives no relay link"
        );
        assert!(
            !has_relay_link(&item("g1", "live", None, None, Some(""))),
            "an empty live_value_uri gives no relay link"
        );
        assert!(
            has_relay_link(&item("g1", "live", None, None, Some("event-id"))),
            "a non-empty live_value_uri gives a relay link"
        );
    }

    // -----------------------------------------------------------------
    // is_banned_live_item
    // -----------------------------------------------------------------

    #[test]
    fn is_banned_live_item_does_not_apply_to_pending_or_ended() {
        assert!(
            !is_banned_live_item(&item("g1", "pending", None, None, None)),
            "the ban does not apply to a pending item, even with no start or end"
        );
        assert!(
            !is_banned_live_item(&item("g1", "ended", None, None, None)),
            "the ban does not apply to an ended item, even with no start or end"
        );
    }

    #[test]
    fn is_banned_live_item_exempts_a_relay_link() {
        assert!(
            !is_banned_live_item(&item("g1", "live", None, None, Some("event-id"))),
            "a relay link keeps a live item with no end"
        );
    }

    #[test]
    fn is_banned_live_item_needs_both_a_start_and_an_end() {
        assert!(
            is_banned_live_item(&item("g1", "live", None, Some(100), None)),
            "no start_at bans an unrelayed live item"
        );
        assert!(
            is_banned_live_item(&item("g1", "live", Some(0), None, None)),
            "no end_at bans an unrelayed live item"
        );
        assert!(
            is_banned_live_item(&item("g1", "live", None, None, None)),
            "no start_at and no end_at bans an unrelayed live item"
        );
    }

    #[test]
    fn is_banned_live_item_caps_the_window_at_six_hours() {
        assert!(
            !is_banned_live_item(&item(
                "g1",
                "live",
                Some(0),
                Some(MAX_UNRELAYED_LIVE_SECS),
                None
            )),
            "exactly six hours is kept"
        );
        assert!(
            is_banned_live_item(&item(
                "g1",
                "live",
                Some(0),
                Some(MAX_UNRELAYED_LIVE_SECS + 1),
                None
            )),
            "six hours and one second is banned"
        );
    }

    // -----------------------------------------------------------------
    // select_live_items
    // -----------------------------------------------------------------

    #[test]
    fn select_live_items_drops_an_unknown_status_and_warns() {
        let items = vec![
            item("cancelled-1", "cancelled", None, None, None),
            item("valid-1", "pending", None, None, None),
        ];
        let (kept, warnings) = select_live_items(&items);
        assert_eq!(
            kept.len(),
            1,
            "an unknown status is dropped, a valid one is kept"
        );
        assert_eq!(
            kept[0].live_item_guid, "valid-1",
            "the kept item is the one with a known status"
        );
        assert_eq!(
            warnings.len(),
            1,
            "an unknown status gives exactly one warning"
        );
        assert!(
            warnings[0].contains("cancelled"),
            "the warning names the unknown status value, got {:?}",
            warnings[0]
        );
    }

    #[test]
    fn select_live_items_keeps_the_first_of_a_duplicate_guid() {
        let items = vec![
            item("dup", "live", Some(0), Some(100), Some("uri")),
            item("dup", "pending", None, None, None),
        ];
        let (kept, warnings) = select_live_items(&items);
        assert_eq!(kept.len(), 1, "a duplicate guid keeps only one item");
        assert_eq!(
            kept[0].status, "live",
            "the kept item is the first one seen"
        );
        assert_eq!(warnings.len(), 1, "a duplicate guid gives one warning");
    }

    #[test]
    fn select_live_items_drops_a_banned_item_and_warns() {
        let items = vec![item("banned", "live", None, None, None)];
        let (kept, warnings) = select_live_items(&items);
        assert!(kept.is_empty(), "a banned item is dropped");
        assert_eq!(warnings.len(), 1, "a banned item gives one warning");
    }

    #[test]
    fn select_live_items_keeps_a_pending_item_with_the_same_times() {
        let items = vec![item("g1", "pending", None, None, None)];
        let (kept, warnings) = select_live_items(&items);
        assert_eq!(
            kept.len(),
            1,
            "a pending item with no start or end is not banned"
        );
        assert!(warnings.is_empty(), "a kept item gives no warning");
    }

    #[test]
    fn select_live_items_caps_active_rows_in_rss_order() {
        let items: Vec<IngestLiveItemData> = (0..11)
            .map(|i| {
                item(
                    &format!("active-{i}"),
                    "live",
                    Some(0),
                    Some(100),
                    Some("uri"),
                )
            })
            .collect();
        let (kept, warnings) = select_live_items(&items);
        assert_eq!(
            kept.len(),
            MAX_ACTIVE_LIVE_ROWS,
            "only the first 10 active rows are kept"
        );
        assert_eq!(
            kept[0].live_item_guid, "active-0",
            "the kept rows keep RSS order"
        );
        assert_eq!(
            kept[9].live_item_guid, "active-9",
            "the tenth kept row is the tenth item in RSS order"
        );
        assert_eq!(
            warnings.len(),
            1,
            "the eleventh active row gives one warning"
        );
    }

    #[test]
    fn select_live_items_keeps_the_newest_ended_rows() {
        let mut items: Vec<IngestLiveItemData> = (0..11)
            .map(|i| {
                item(
                    &format!("ended-{i}"),
                    "ended",
                    Some(i64::from(i)),
                    None,
                    None,
                )
            })
            .collect();
        // The oldest row gets no start_at at all, so it must sort last.
        items[0].start_at = None;

        let (kept, warnings) = select_live_items(&items);
        assert_eq!(
            kept.len(),
            MAX_ENDED_LIVE_ROWS,
            "only 10 ended rows are kept"
        );
        assert!(
            !kept.iter().any(|it| it.live_item_guid == "ended-0"),
            "the row with no start_at sorts last and is dropped"
        );
        assert_eq!(warnings.len(), 1, "the dropped ended row gives one warning");
    }

    // -----------------------------------------------------------------
    // is_confirming_relay
    // -----------------------------------------------------------------

    #[test]
    fn is_confirming_relay_true_for_a_listed_https_host() {
        let hosts = vec!["relay.example.com".to_string()];
        assert!(
            is_confirming_relay(Some("https://relay.example.com/events/1"), &hosts),
            "an https URL on a listed host is a confirming relay"
        );
    }

    #[test]
    fn is_confirming_relay_false_for_an_unlisted_host_an_http_url_and_an_identifier() {
        let hosts = vec!["relay.example.com".to_string()];
        assert!(
            !is_confirming_relay(Some("https://other.example.com/events/1"), &hosts),
            "an https URL on an unlisted host is not a confirming relay"
        );
        assert!(
            !is_confirming_relay(Some("http://relay.example.com/events/1"), &hosts),
            "an http URL is not a confirming relay, even on a listed host"
        );
        assert!(
            !is_confirming_relay(Some("event-one"), &hosts),
            "a bare identifier, with no scheme, is not a confirming relay"
        );
    }

    #[test]
    fn is_confirming_relay_host_compare_has_no_case_difference() {
        let hosts = vec!["Relay.Example.COM".to_string()];
        assert!(
            is_confirming_relay(Some("https://relay.example.com/events/1"), &hosts),
            "the host compare must ignore case on both sides"
        );
    }

    // -----------------------------------------------------------------
    // in_now_view and in_upcoming_view
    // -----------------------------------------------------------------

    /// Builds a minimal `LiveEvent` for a view test. Only `status`,
    /// `scheduled_start` and `scheduled_end` matter to the rules under test.
    fn row(status: &str, scheduled_start: Option<i64>, scheduled_end: Option<i64>) -> LiveEvent {
        LiveEvent {
            live_item_guid: "g1".to_string(),
            feed_guid: "f1".to_string(),
            title: "Live Show".to_string(),
            content_link: None,
            status: status.to_string(),
            scheduled_start,
            scheduled_end,
            created_at: 0,
            updated_at: 0,
            live_value_uri: None,
            live_value_protocol: None,
        }
    }

    const NOW: i64 = 1_000_000;

    #[test]
    fn in_now_view_needs_the_live_status() {
        assert!(
            !in_now_view(&row("pending", None, None), NOW, false),
            "a pending row is never in the now view"
        );
        assert!(
            !in_now_view(&row("ended", None, None), NOW, false),
            "an ended row is never in the now view"
        );
    }

    #[test]
    fn in_now_view_l1_a_confirming_relay_stays_past_its_end_and_with_no_end() {
        assert!(
            in_now_view(&row("live", None, Some(NOW - 10_000)), NOW, true),
            "L1: a confirming relay keeps a live row long past its scheduled_end"
        );
        assert!(
            in_now_view(&row("live", None, None), NOW, true),
            "L1: a confirming relay keeps a live row with no scheduled_end"
        );
    }

    #[test]
    fn in_now_view_l3_and_l4_no_confirming_relay_before_the_end_or_with_no_end() {
        assert!(
            in_now_view(&row("live", None, None), NOW, false),
            "L3: a live row with no confirming relay and no end stays in the view"
        );
        assert!(
            in_now_view(&row("live", None, Some(NOW + 100)), NOW, false),
            "L4: a live row with no confirming relay, before its end, stays in the view"
        );
    }

    #[test]
    fn in_now_view_l5_a_live_row_stays_until_one_hour_past_its_end() {
        assert!(
            in_now_view(&row("live", None, Some(NOW - 1_800)), NOW, false),
            "L5: a live row with no confirming relay stays less than one hour past its end"
        );
    }

    #[test]
    fn in_now_view_l6_a_live_row_leaves_the_view_more_than_one_hour_past_its_end() {
        assert!(
            !in_now_view(&row("live", None, Some(NOW - 3_700)), NOW, false),
            "L6: a live row with no confirming relay leaves the view more than one hour past its end"
        );
    }

    #[test]
    fn in_now_view_boundary_at_exactly_one_hour_past_the_end() {
        let scheduled_end = NOW - LIVE_END_MARGIN_SECS;
        assert!(
            !in_now_view(&row("live", None, Some(scheduled_end)), NOW, false),
            "exactly one hour past scheduled_end, with no confirming relay, leaves the view"
        );
        assert!(
            in_now_view(&row("live", None, Some(scheduled_end + 1)), NOW, false),
            "one second short of one hour past scheduled_end stays in the view"
        );
    }

    #[test]
    fn in_upcoming_view_needs_the_pending_status() {
        assert!(
            !in_upcoming_view(&row("live", None, Some(NOW + 100)), NOW),
            "a live row is never in the upcoming view"
        );
        assert!(
            !in_upcoming_view(&row("ended", None, None), NOW),
            "an ended row is never in the upcoming view"
        );
    }

    #[test]
    fn in_upcoming_view_p1_a_pending_row_with_a_future_start_is_shown() {
        assert!(
            in_upcoming_view(&row("pending", Some(NOW + 100), None), NOW),
            "P1: a pending row with a future start, and no end, is in the upcoming view"
        );
    }

    #[test]
    fn in_upcoming_view_p2_a_pending_row_stays_until_its_end() {
        assert!(
            in_upcoming_view(&row("pending", Some(NOW - 100), Some(NOW + 100)), NOW),
            "P2: a pending row with a past start and a future end is in the upcoming view"
        );
    }

    #[test]
    fn in_upcoming_view_p3_a_pending_row_leaves_the_view_after_its_end() {
        assert!(
            !in_upcoming_view(&row("pending", Some(NOW - 200), Some(NOW - 100)), NOW),
            "P3: a pending row with a past end is not in the upcoming view"
        );
    }

    #[test]
    fn in_upcoming_view_p4_a_pending_row_with_no_end_leaves_the_view_after_one_hour() {
        assert!(
            !in_upcoming_view(&row("pending", Some(NOW - 3_601), None), NOW),
            "P4: a pending row with no end, and a start more than one hour in the past, \
             is not in the upcoming view"
        );
    }

    #[test]
    fn in_upcoming_view_no_start_and_no_end_is_never_shown() {
        assert!(
            !in_upcoming_view(&row("pending", None, None), NOW),
            "a pending row with neither a start nor an end is not in the upcoming view"
        );
    }

    #[test]
    fn in_upcoming_view_boundary_at_exactly_one_hour_past_the_start_with_no_end() {
        let scheduled_start = NOW - PENDING_START_MARGIN_SECS;
        assert!(
            !in_upcoming_view(&row("pending", Some(scheduled_start), None), NOW),
            "exactly one hour past scheduled_start, with no end, leaves the view"
        );
        assert!(
            in_upcoming_view(&row("pending", Some(scheduled_start + 1), None), NOW),
            "one second short of one hour past scheduled_start stays in the view"
        );
    }
}
