// Issue-PROOF-NAMESPACE — 2026-03-14
//
// Tests for namespace-aware `<podcast:txt>` extraction via roxmltree.
// Covers standard prefixes, non-standard prefixes, single-quoted declarations,
// missing elements, malformed XML, and multiple elements.

use stophammer::proof::extract_podcast_txt_values;

// ---------------------------------------------------------------------------
// 1. Standard `xmlns:podcast="..."` prefix — proof token found
// ---------------------------------------------------------------------------

#[test]
fn standard_podcast_prefix_finds_txt() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:podcast="https://podcastindex.org/namespace/1.0">
  <channel>
    <title>Test Feed</title>
    <podcast:txt>stophammer-proof abc123.hashpart</podcast:txt>
  </channel>
</rss>"#;

    let values = extract_podcast_txt_values(xml);
    assert_eq!(values, vec!["stophammer-proof abc123.hashpart"]);
}

// ---------------------------------------------------------------------------
// 2. Non-standard prefix `xmlns:pc="..."` with `<pc:txt>` — proof token found
// ---------------------------------------------------------------------------

#[test]
fn non_standard_prefix_finds_txt() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:pc="https://podcastindex.org/namespace/1.0">
  <channel>
    <title>Test Feed</title>
    <pc:txt>stophammer-proof token.binding</pc:txt>
  </channel>
</rss>"#;

    let values = extract_podcast_txt_values(xml);
    assert_eq!(values, vec!["stophammer-proof token.binding"]);
}

// ---------------------------------------------------------------------------
// 3. Single-quoted namespace declaration — proof token found
// ---------------------------------------------------------------------------

#[test]
fn single_quoted_namespace_declaration_finds_txt() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:podcast='https://podcastindex.org/namespace/1.0'>
  <channel>
    <title>Test Feed</title>
    <podcast:txt>stophammer-proof single.quote</podcast:txt>
  </channel>
</rss>"#;

    let values = extract_podcast_txt_values(xml);
    assert_eq!(values, vec!["stophammer-proof single.quote"]);
}

// ---------------------------------------------------------------------------
// 4. No `<podcast:txt>` element present — empty result
// ---------------------------------------------------------------------------

#[test]
fn no_podcast_txt_returns_empty() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:podcast="https://podcastindex.org/namespace/1.0">
  <channel>
    <title>Test Feed</title>
    <podcast:guid>some-guid</podcast:guid>
  </channel>
</rss>"#;

    let values = extract_podcast_txt_values(xml);
    assert!(
        values.is_empty(),
        "expected empty result when no <podcast:txt> present"
    );
}

// ---------------------------------------------------------------------------
// 5. Malformed XML — empty result (no panic)
// ---------------------------------------------------------------------------

#[test]
fn malformed_xml_returns_empty() {
    let xml = r#"<rss version="2.0" xmlns:podcast="not closed"#;

    let values = extract_podcast_txt_values(xml);
    assert!(
        values.is_empty(),
        "malformed XML should return empty vec, not panic"
    );
}

// ---------------------------------------------------------------------------
// 6. Multiple `<podcast:txt>` elements — all returned
// ---------------------------------------------------------------------------

#[test]
fn multiple_txt_elements_all_returned() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:podcast="https://podcastindex.org/namespace/1.0">
  <channel>
    <title>Test Feed</title>
    <podcast:txt>stophammer-proof first.token</podcast:txt>
    <podcast:txt>stophammer-proof second.token</podcast:txt>
    <podcast:txt>other-verification xyz</podcast:txt>
  </channel>
</rss>"#;

    let values = extract_podcast_txt_values(xml);
    assert_eq!(
        values.len(),
        3,
        "should return all three <podcast:txt> values"
    );
    assert_eq!(values[0], "stophammer-proof first.token");
    assert_eq!(values[1], "stophammer-proof second.token");
    assert_eq!(values[2], "other-verification xyz");
}
